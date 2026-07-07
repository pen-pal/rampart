//! Error tracking storage (migration 0077).
//!
//! Projects CRUD + the ingest hot path: [`record_event`] groups an incoming
//! event into an issue (insert-or-bump over the (project, fingerprint) unique
//! index, reopening a resolved issue on regression) and stores the event
//! detail. Issues persist; events age out via [`prune`]. The grouping math is
//! pure in `rampart_core::error_tracking`; this module is the SQL.

use crate::{DbError, DbPool, DbResult};
use rampart_core::error_tracking::{
    fingerprint, ErrorEvent, ErrorIssue, ErrorProject, NewErrorProject, ParsedEvent,
    UpdateErrorProject,
};
use rampart_core::ids::{ErrorIssueId, ErrorProjectId, OrgId};
use time::OffsetDateTime;
use uuid::Uuid;

const KEY_LEN: usize = 32;
const KEY_ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

fn random_token(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| KEY_ALPHA[rng.gen_range(0..KEY_ALPHA.len())] as char)
        .collect()
}

/// URL-safe slug from a name, with a short random suffix to guarantee the
/// UNIQUE(slug) constraint without a retry loop.
fn slugify(name: &str) -> String {
    let mut base: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // Collapse repeated dashes and trim them.
    while base.contains("--") {
        base = base.replace("--", "-");
    }
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "project" } else { base };
    format!("{base}-{}", random_token(4).to_lowercase())
}

// ─────────────────────────── projects ───────────────────────────

struct ProjectRow {
    id: Uuid,
    name: String,
    slug: String,
    public_key: String,
    platform: Option<String>,
    retention_days: i32,
    alert_channel_ids: serde_json::Value,
    created_at: OffsetDateTime,
}

impl From<ProjectRow> for ErrorProject {
    fn from(r: ProjectRow) -> Self {
        ErrorProject {
            id: ErrorProjectId::from_uuid(r.id),
            name: r.name,
            slug: r.slug,
            public_key: r.public_key,
            platform: r.platform,
            retention_days: r.retention_days,
            alert_channel_ids: serde_json::from_value(r.alert_channel_ids).unwrap_or_default(),
            created_at: r.created_at,
        }
    }
}

pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<ErrorProject>> {
    let rows = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT id, name, slug, public_key, platform, retention_days,
               alert_channel_ids AS "alert_channel_ids!", created_at
        FROM error_projects
        WHERE org_id = $1
        ORDER BY created_at
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// 404-gate: succeed only when `project` belongs to `org`. Error-tracking
/// children (issues, events, histograms, source maps) carry no `org_id` of
/// their own — they inherit the project's — so the authenticated handlers gate
/// through this before touching a project-keyed row. The ingest hot path
/// ([`get_opt`], [`find_or_create_by_name`], [`record_event`]) has no org
/// context and stays unscoped by design.
pub async fn project_in_org(pool: &DbPool, project: ErrorProjectId, org_id: OrgId) -> DbResult<()> {
    let ok = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM error_projects WHERE id = $1 AND org_id = $2)",
        project.0,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    if ok.unwrap_or(false) {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
}

/// 404-gate: succeed only when `issue`'s owning project belongs to `org`. Used
/// by the top-level `/v1/error-issues/:id` operations, which carry no project
/// in the path.
pub async fn issue_in_org(pool: &DbPool, issue: ErrorIssueId, org_id: OrgId) -> DbResult<()> {
    let ok = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM error_issues i
            JOIN error_projects p ON p.id = i.project_id
            WHERE i.id = $1 AND p.org_id = $2
        )
        "#,
        issue.0,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    if ok.unwrap_or(false) {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
}

pub async fn get(pool: &DbPool, id: ErrorProjectId) -> DbResult<ErrorProject> {
    get_opt(pool, id).await?.ok_or(DbError::NotFound)
}

/// Resolve a project's owning org. Used by the (session-less) Sentry ingest
/// path to bind RLS to the project's tenant — error_events/issues are children
/// that inherit org from the project, so the org lives on `error_projects`.
/// Returns NotFound for an unknown project id.
pub async fn org_for_project(pool: &DbPool, id: ErrorProjectId) -> DbResult<OrgId> {
    let org: Uuid = sqlx::query_scalar!(
        r#"SELECT org_id AS "org_id!" FROM error_projects WHERE id = $1"#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(OrgId::from_uuid(org))
}

pub async fn get_opt(pool: &DbPool, id: ErrorProjectId) -> DbResult<Option<ErrorProject>> {
    let row = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT id, name, slug, public_key, platform, retention_days,
               alert_channel_ids AS "alert_channel_ids!", created_at
        FROM error_projects
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// Find an existing project by exact name, or auto-provision one. Used by the
/// RUM browser-error capture path, which has no DSN: a beacon names its app
/// and we want its JS errors to land in a project named after that app,
/// creating it on first sight. Returns the oldest match if names collide.
pub async fn find_or_create_by_name(
    pool: &DbPool,
    name: &str,
    org_id: OrgId,
) -> DbResult<ErrorProject> {
    // The RUM browser-error beacon is a PUBLIC surface and `name` is its
    // attacker-controllable `app` field. Clamp the length (this path bypasses
    // the `NewErrorProject` DTO validator, so an unbounded `app` would write an
    // unbounded project name).
    let name: String = name
        .chars()
        .take(rampart_core::error_tracking::PROJECT_NAME_MAX)
        .collect();
    // Phase 5-3: the lookup AND the auto-create are scoped to the resolved org,
    // so two orgs can each have an app named e.g. "web" without colliding.
    let existing = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT id, name, slug, public_key, platform, retention_days,
               alert_channel_ids AS "alert_channel_ids!", created_at
        FROM error_projects
        WHERE name = $1 AND org_id = $2
        ORDER BY created_at
        LIMIT 1
        "#,
        name,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?;
    if let Some(row) = existing {
        return Ok(row.into());
    }
    // DoS guard: cap auto-provisioned projects per org so a public beacon
    // spraying distinct `app` names can't amplify into unbounded rows + slug/key
    // generation. At the cap the beacon's error drops (caller maps Err → 204)
    // rather than minting another row; already-provisioned apps still capture.
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!" FROM error_projects WHERE org_id = $1"#,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    if count >= rampart_core::error_tracking::MAX_AUTO_PROJECTS_PER_ORG {
        return Err(DbError::Conflict(
            "error-project auto-provision limit reached for this org".into(),
        ));
    }
    create(
        pool,
        rampart_core::error_tracking::NewErrorProject {
            name,
            platform: Some("javascript".to_string()),
            alert_channel_ids: Vec::new(),
        },
        org_id,
    )
    .await
}

pub async fn create(
    pool: &DbPool,
    input: NewErrorProject,
    org_id: OrgId,
) -> DbResult<ErrorProject> {
    let id = ErrorProjectId::new();
    let slug = slugify(&input.name);
    let public_key = random_token(KEY_LEN);
    let channels =
        serde_json::to_value(&input.alert_channel_ids).unwrap_or_else(|_| serde_json::json!([]));
    sqlx::query!(
        r#"
        INSERT INTO error_projects (id, name, slug, public_key, platform, alert_channel_ids, org_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        id.0,
        input.name,
        slug,
        public_key,
        input.platform,
        channels,
        org_id.0,
    )
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: ErrorProjectId,
    patch: UpdateErrorProject,
    org_id: OrgId,
) -> DbResult<ErrorProject> {
    let channels = patch
        .alert_channel_ids
        .map(|c| serde_json::to_value(&c).unwrap_or_else(|_| serde_json::json!([])));
    let result = sqlx::query!(
        r#"
        UPDATE error_projects SET
            name              = COALESCE($2, name),
            platform          = COALESCE($3, platform),
            retention_days    = COALESCE($4, retention_days),
            alert_channel_ids = COALESCE($5, alert_channel_ids)
        WHERE id = $1 AND org_id = $6
        "#,
        id.0,
        patch.name,
        patch.platform,
        patch.retention_days,
        channels,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id).await
}

pub async fn delete(pool: &DbPool, id: ErrorProjectId, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM error_projects WHERE id = $1 AND org_id = $2",
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

// ─────────────────────────── issues + events ───────────────────────────

struct IssueRow {
    id: Uuid,
    project_id: Uuid,
    fingerprint: String,
    title: String,
    culprit: Option<String>,
    level: String,
    status: String,
    first_seen: OffsetDateTime,
    last_seen: OffsetDateTime,
    times_seen: i64,
    assignee: Option<Uuid>,
}

impl From<IssueRow> for ErrorIssue {
    fn from(r: IssueRow) -> Self {
        ErrorIssue {
            id: ErrorIssueId::from_uuid(r.id),
            project_id: ErrorProjectId::from_uuid(r.project_id),
            fingerprint: r.fingerprint,
            title: r.title,
            culprit: r.culprit,
            level: r.level,
            status: r.status,
            first_seen: r.first_seen,
            last_seen: r.last_seen,
            times_seen: r.times_seen,
            assignee: r.assignee.map(rampart_core::ids::UserId::from_uuid),
        }
    }
}

/// What `record_event` did, so the caller can decide whether to alert.
pub struct RecordOutcome {
    pub issue_id: ErrorIssueId,
    pub event_id: Uuid,
    pub title: String,
    pub is_new: bool,
    pub regressed: bool,
}

/// Group + store one event. Inserts the issue if its fingerprint is new
/// (`is_new`), otherwise bumps the counter and reopens it if it was resolved
/// (`regressed`). Then stores the event detail. Single-statement per step over
/// the (project, fingerprint) unique index — a flapping crash can't double-open
/// an issue (the ON CONFLICT DO NOTHING returns no row, and we fall through to
/// the bump path).
pub async fn record_event(
    pool: &DbPool,
    project_id: ErrorProjectId,
    ev: &ParsedEvent,
) -> DbResult<RecordOutcome> {
    let fp = fingerprint(ev);
    let title = ev.title();
    let culprit = ev.culprit();
    let level = ev.level.clone();

    // Try to claim the fingerprint as a new issue. (query! binds args by
    // reference, so title/culprit/level/fp feed several statements without moving.)
    let inserted = sqlx::query!(
        r#"
        INSERT INTO error_issues (id, project_id, fingerprint, title, culprit, level, times_seen, last_seen)
        VALUES ($1, $2, $3, $4, $5, $6, 1, now())
        ON CONFLICT (project_id, fingerprint) DO NOTHING
        RETURNING id
        "#,
        Uuid::now_v7(),
        project_id.0,
        fp,
        title,
        culprit,
        level,
    )
    .fetch_optional(pool)
    .await?;

    let (issue_id, is_new, regressed) = match inserted {
        Some(r) => (r.id, true, false),
        None => {
            // Existing issue — read its status (to detect regression), then bump.
            let existing = sqlx::query!(
                "SELECT id, status FROM error_issues WHERE project_id = $1 AND fingerprint = $2",
                project_id.0,
                fp,
            )
            .fetch_one(pool)
            .await?;
            let regressed = existing.status == "resolved";
            sqlx::query!(
                r#"
                UPDATE error_issues SET
                    times_seen = times_seen + 1,
                    last_seen  = now(),
                    title      = $2,
                    level      = $3,
                    culprit    = $4,
                    status     = CASE WHEN status = 'resolved' THEN 'unresolved' ELSE status END
                WHERE id = $1
                "#,
                existing.id,
                title,
                level,
                culprit,
            )
            .execute(pool)
            .await?;
            (existing.id, false, regressed)
        }
    };

    // Store the event detail.
    let event_id = parse_event_id(ev.event_id.as_deref());
    let stacktrace = if ev.frames.is_empty() {
        None
    } else {
        serde_json::to_value(&ev.frames).ok()
    };
    // Cross-tier join key: pull contexts.trace.trace_id out of the raw event
    // so a trace can find its errors (indexed column, vs digging the JSON).
    let trace_id = ev
        .raw
        .pointer("/contexts/trace/trace_id")
        .and_then(|v| v.as_str());
    sqlx::query!(
        r#"
        INSERT INTO error_events
            (id, issue_id, project_id, ts, level, message, exception_type, culprit,
             environment, release, server_name, stacktrace, context, trace_id)
        VALUES ($1, $2, $3, now(), $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
        event_id,
        issue_id,
        project_id.0,
        ev.level,
        ev.message,
        ev.exception_type,
        culprit,
        ev.environment,
        ev.release,
        ev.server_name,
        stacktrace,
        ev.raw,
        trace_id,
    )
    .execute(pool)
    .await?;

    Ok(RecordOutcome {
        issue_id: ErrorIssueId::from_uuid(issue_id),
        event_id,
        title,
        is_new,
        regressed,
    })
}

/// Sentry event ids are 32 hex chars (a UUID without dashes). Parse to a
/// Uuid, or mint a fresh one if absent/unparseable.
fn parse_event_id(raw: Option<&str>) -> Uuid {
    raw.and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::now_v7)
}

/// Distinct error issues touched by events carrying `trace_id` — powers the
/// trace → errors link (reverse of the error → trace context link).
#[derive(serde::Serialize)]
pub struct TraceErrorRef {
    pub issue_id: Uuid,
    pub title: String,
    pub level: String,
    pub count: i64,
}

pub async fn issues_for_trace(
    pool: &DbPool,
    trace_id: &str,
    org_id: OrgId,
) -> DbResult<Vec<TraceErrorRef>> {
    let rows = sqlx::query_as!(
        TraceErrorRef,
        r#"
        SELECT ev.issue_id AS "issue_id!", i.title AS "title!", i.level AS "level!",
               COUNT(*) AS "count!"
        FROM error_events ev
        JOIN error_issues i ON i.id = ev.issue_id
        JOIN error_projects p ON p.id = ev.project_id AND p.org_id = $2
        WHERE ev.trace_id = $1
        GROUP BY ev.issue_id, i.title, i.level
        ORDER BY COUNT(*) DESC
        LIMIT 20
        "#,
        trace_id,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Issues for a project, newest activity first. `status` filters when set.
/// `before_id` is a keyset cursor for "load older" — the last issue's id, whose
/// `(last_seen, id)` is resolved server-side. `limit` is clamped.
pub async fn list_issues(
    pool: &DbPool,
    project_id: ErrorProjectId,
    status: Option<&str>,
    before_id: Option<Uuid>,
    limit: i64,
) -> DbResult<Vec<ErrorIssue>> {
    let rows = sqlx::query_as!(
        IssueRow,
        r#"
        SELECT id, project_id, fingerprint, title, culprit, level, status,
               first_seen, last_seen, times_seen, assignee
        FROM error_issues
        WHERE project_id = $1 AND ($2::text IS NULL OR status = $2)
          AND ($3::uuid IS NULL
               OR (last_seen, id) < ((SELECT last_seen FROM error_issues WHERE id = $3), $3))
        ORDER BY last_seen DESC, id DESC
        LIMIT $4
        "#,
        project_id.0,
        status,
        before_id,
        limit.clamp(1, 200),
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Most-recently-seen open issues across every project — the dashboard feed.
pub async fn recent_open_issues(
    pool: &DbPool,
    limit: i64,
    org_id: OrgId,
) -> DbResult<Vec<ErrorIssue>> {
    let rows = sqlx::query_as!(
        IssueRow,
        r#"
        SELECT i.id, i.project_id, i.fingerprint, i.title, i.culprit, i.level, i.status,
               i.first_seen, i.last_seen, i.times_seen, i.assignee
        FROM error_issues i
        JOIN error_projects p ON p.id = i.project_id
        WHERE i.status = 'unresolved' AND p.org_id = $2
        ORDER BY i.last_seen DESC
        LIMIT $1
        "#,
        limit.clamp(1, 50),
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// One bucket of a project's error-event volume over time.
#[derive(Debug, serde::Serialize)]
pub struct ErrorBucket {
    pub ts: time::OffsetDateTime,
    pub count: i64,
}

/// Time-bucketed error-event volume for a project over the window — the
/// Errors-view sparkline. ~`buckets` points, oldest first.
pub async fn project_event_histogram(
    pool: &DbPool,
    project_id: ErrorProjectId,
    hours: i32,
    buckets: i64,
) -> DbResult<Vec<ErrorBucket>> {
    let hours = hours.clamp(1, 720);
    let buckets = buckets.clamp(2, 200);
    let step = ((hours as i64 * 3600) / buckets).max(1);
    let rows = sqlx::query!(
        r#"
        SELECT date_bin(make_interval(secs => $3), ts,
                        now() - make_interval(hours => $2)) AS "bucket!",
               COUNT(*) AS "count!"
        FROM error_events
        WHERE project_id = $1 AND ts > now() - make_interval(hours => $2)
        GROUP BY 1 ORDER BY 1
        "#,
        project_id.0,
        hours,
        step as f64,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ErrorBucket {
            ts: r.bucket,
            count: r.count,
        })
        .collect())
}

pub async fn get_issue(pool: &DbPool, id: ErrorIssueId) -> DbResult<ErrorIssue> {
    let row = sqlx::query_as!(
        IssueRow,
        r#"
        SELECT id, project_id, fingerprint, title, culprit, level, status,
               first_seen, last_seen, times_seen, assignee
        FROM error_issues
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// Aggregate stats for an issue's events: distinct affected users (from the
/// Sentry `user` context — id, else email, else username) and the top releases
/// and environments by event count. Read-side over stored events; no extra
/// columns. The "who/where is this hitting" view next to the stack trace.
#[derive(Debug, serde::Serialize)]
pub struct IssueStats {
    pub users_affected: i64,
    pub by_release: Vec<(String, i64)>,
    pub by_environment: Vec<(String, i64)>,
}

/// One distinct user hit by an issue — the affected-users drill-down.
#[derive(Debug, serde::Serialize)]
pub struct AffectedUser {
    pub ident: String,
    pub events: i64,
    pub last_seen: time::OffsetDateTime,
}

/// Distinct users affected by an issue (from the Sentry `user` context — id,
/// else email, else username), most-recently-seen first.
pub async fn issue_affected_users(
    pool: &DbPool,
    id: ErrorIssueId,
    limit: i64,
) -> DbResult<Vec<AffectedUser>> {
    let rows = sqlx::query!(
        r#"
        SELECT COALESCE(context->'user'->>'id',
                        context->'user'->>'email',
                        context->'user'->>'username') AS "ident!",
               COUNT(*) AS "events!",
               MAX(ts)  AS "last_seen!"
        FROM error_events
        WHERE issue_id = $1 AND context->'user' IS NOT NULL
        GROUP BY 1
        ORDER BY MAX(ts) DESC
        LIMIT $2
        "#,
        id.0,
        limit.clamp(1, 100),
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AffectedUser {
            ident: r.ident,
            events: r.events,
            last_seen: r.last_seen,
        })
        .collect())
}

pub async fn issue_stats(pool: &DbPool, id: ErrorIssueId) -> DbResult<IssueStats> {
    let users = sqlx::query_scalar!(
        r#"
        SELECT COUNT(DISTINCT COALESCE(
                 context->'user'->>'id',
                 context->'user'->>'email',
                 context->'user'->>'username')) AS "n!"
        FROM error_events
        WHERE issue_id = $1 AND context->'user' IS NOT NULL
        "#,
        id.0,
    )
    .fetch_one(pool)
    .await?;

    let releases = sqlx::query!(
        r#"
        SELECT COALESCE(NULLIF(release, ''), '(none)') AS "k!", COUNT(*) AS "n!"
        FROM error_events WHERE issue_id = $1
        GROUP BY 1 ORDER BY 2 DESC LIMIT 10
        "#,
        id.0,
    )
    .fetch_all(pool)
    .await?;

    let envs = sqlx::query!(
        r#"
        SELECT COALESCE(NULLIF(environment, ''), '(none)') AS "k!", COUNT(*) AS "n!"
        FROM error_events WHERE issue_id = $1
        GROUP BY 1 ORDER BY 2 DESC LIMIT 10
        "#,
        id.0,
    )
    .fetch_all(pool)
    .await?;

    Ok(IssueStats {
        users_affected: users,
        by_release: releases.into_iter().map(|r| (r.k, r.n)).collect(),
        by_environment: envs.into_iter().map(|r| (r.k, r.n)).collect(),
    })
}

/// `status` must be one of `unresolved` | `resolved` | `ignored` (validated
/// at the route layer).
pub async fn set_issue_status(
    pool: &DbPool,
    id: ErrorIssueId,
    status: &str,
) -> DbResult<ErrorIssue> {
    let row = sqlx::query_as!(
        IssueRow,
        r#"
        UPDATE error_issues SET status = $2
        WHERE id = $1
        RETURNING id, project_id, fingerprint, title, culprit, level, status,
                  first_seen, last_seen, times_seen, assignee
        "#,
        id.0,
        status,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// Assign (or, with `None`, unassign) an issue to a user. Returns the updated
/// issue. The assignee FK is `ON DELETE SET NULL`, so a deleted user simply
/// clears the assignment.
pub async fn assign_issue(
    pool: &DbPool,
    id: ErrorIssueId,
    assignee: Option<rampart_core::ids::UserId>,
) -> DbResult<ErrorIssue> {
    let aid = assignee.map(|u| u.0);
    let row = sqlx::query_as!(
        IssueRow,
        r#"
        UPDATE error_issues SET assignee = $2
        WHERE id = $1
        RETURNING id, project_id, fingerprint, title, culprit, level, status,
                  first_seen, last_seen, times_seen, assignee
        "#,
        id.0,
        aid,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// Minimal user directory for the assignee picker (editor-visible, unlike the
/// admin `/v1/users` list — only id / name / email, no roles or secrets).
#[derive(serde::Serialize)]
pub struct AssignableUser {
    pub id: Uuid,
    pub name: Option<String>,
    pub email: String,
}

pub async fn assignable_users(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<AssignableUser>> {
    // Members of the caller's org only — an assignee picker must not leak the
    // full cross-tenant user directory (id + name + email PII).
    let rows = sqlx::query_as!(
        AssignableUser,
        r#"SELECT u.id, u.name, u.email::text AS "email!"
           FROM users u
           JOIN org_members m ON m.user_id = u.id
           WHERE m.org_id = $1
           ORDER BY u.name NULLS LAST, u.email"#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

struct EventRow {
    id: Uuid,
    issue_id: Uuid,
    project_id: Uuid,
    ts: OffsetDateTime,
    level: String,
    message: Option<String>,
    exception_type: Option<String>,
    culprit: Option<String>,
    environment: Option<String>,
    release: Option<String>,
    server_name: Option<String>,
    stacktrace: Option<serde_json::Value>,
    context: Option<serde_json::Value>,
}

impl From<EventRow> for ErrorEvent {
    fn from(r: EventRow) -> Self {
        ErrorEvent {
            id: r.id,
            issue_id: ErrorIssueId::from_uuid(r.issue_id),
            project_id: ErrorProjectId::from_uuid(r.project_id),
            ts: r.ts,
            level: r.level,
            message: r.message,
            exception_type: r.exception_type,
            culprit: r.culprit,
            environment: r.environment,
            release: r.release,
            server_name: r.server_name,
            stacktrace: r.stacktrace,
            context: r.context,
        }
    }
}

/// Most recent events for an issue (detail view), newest first.
pub async fn list_events(
    pool: &DbPool,
    issue_id: ErrorIssueId,
    limit: i64,
) -> DbResult<Vec<ErrorEvent>> {
    let rows = sqlx::query_as!(
        EventRow,
        r#"
        SELECT id, issue_id, project_id, ts, level, message, exception_type, culprit,
               environment, release, server_name, stacktrace, context
        FROM error_events
        WHERE issue_id = $1
        ORDER BY ts DESC
        LIMIT $2
        "#,
        issue_id.0,
        limit.clamp(1, 200),
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Delete events older than each project's `retention_days`. Issues (small,
/// long-lived) are kept; only the per-occurrence detail ages out. Returns the
/// number of event rows removed.
///
/// Chunked (mirrors [`crate::prune::batched_delete`]) so a large retention
/// backlog on the high-volume `error_events` table is cleared in many short,
/// cancellable DELETEs instead of one multi-minute table-locking statement that
/// stalls writes and balloons WAL. The retention window is per-project, so this
/// joins `error_projects` rather than taking a single `days` like
/// `batched_delete`. Runtime query (not `query!`) keeps the fixed SQL off the
/// offline `.sqlx` cache.
pub async fn prune(pool: &DbPool) -> DbResult<u64> {
    const SQL: &str = "DELETE FROM error_events WHERE ctid IN (\
         SELECT e.ctid FROM error_events e \
         JOIN error_projects p ON e.project_id = p.id \
         WHERE e.ts < now() - make_interval(days => p.retention_days) \
         LIMIT $1)";
    let mut total = 0u64;
    loop {
        let n = sqlx::query(SQL)
            .bind(crate::prune::PRUNE_BATCH)
            .execute(pool)
            .await?
            .rows_affected();
        total += n;
        if n < crate::prune::PRUNE_BATCH as u64 {
            break;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::error_tracking::{MAX_AUTO_PROJECTS_PER_ORG, PROJECT_NAME_MAX};
    use rampart_core::org::DEFAULT_ORG_ID;
    use sqlx::PgPool;

    fn def_org() -> OrgId {
        OrgId::from_uuid(DEFAULT_ORG_ID)
    }

    /// SECURITY (audit #123): the public RUM browser-error beacon auto-provisions
    /// a project named after its attacker-controllable `app` via this fn. It must
    /// (1) clamp the name length (the DTO validator is bypassed here) and (2) cap
    /// the number of auto-provisioned projects per org so a beacon spraying
    /// distinct app names can't amplify into unbounded rows.
    #[sqlx::test(migrations = "../../migrations")]
    async fn find_or_create_clamps_name_and_caps_auto_provision(pool: PgPool) {
        let org = def_org();

        // (1) An over-long `app` name is clamped to PROJECT_NAME_MAX, not stored raw.
        let long = "a".repeat(PROJECT_NAME_MAX + 500);
        let p = find_or_create_by_name(&pool, &long, org).await.unwrap();
        assert_eq!(p.name.chars().count(), PROJECT_NAME_MAX, "name clamped");
        // Re-sent (still over-long) → same project, not a duplicate.
        let p2 = find_or_create_by_name(&pool, &long, org).await.unwrap();
        assert_eq!(p2.id, p.id);

        // (2) Fill to the per-org cap with distinct app names, then assert the
        //     next distinct app is REJECTED (no new row) — the DoS guard.
        //     `p` already counts as 1; create up to the cap.
        let mut made = 1usize;
        while made < MAX_AUTO_PROJECTS_PER_ORG as usize {
            find_or_create_by_name(&pool, &format!("app-{made}"), org)
                .await
                .unwrap();
            made += 1;
        }
        // At the cap now. An already-provisioned app still resolves (lookup hit)…
        assert_eq!(
            find_or_create_by_name(&pool, "app-1", org)
                .await
                .unwrap()
                .name,
            "app-1"
        );
        // …but a brand-new distinct app is refused rather than minting a row.
        assert!(matches!(
            find_or_create_by_name(&pool, "spray-attacker-new", org).await,
            Err(DbError::Conflict(_))
        ));
    }

    /// SECURITY: the issue-assignee picker must return only members of the
    /// caller's org — it used to `SELECT ... FROM users` with no org filter,
    /// leaking every tenant's user directory (id + name + email PII).
    #[sqlx::test(migrations = "../../migrations")]
    async fn assignable_users_scoped_to_caller_org(pool: PgPool) {
        let default_org = def_org();
        let org_b = crate::orgs::create(&pool, "orgb", "Org B")
            .await
            .unwrap()
            .id;

        // A user created via the normal path is a member of the Default org (the
        // multi-tenancy mirror), NOT of org_b.
        let u = crate::users::create(
            &pool,
            crate::users::NewUser {
                email: "alice@a.test".into(),
                name: Some("Alice".into()),
                password_hash: "x".into(),
                role: rampart_core::Role::Admin,
            },
        )
        .await
        .unwrap();

        // Visible to their own (Default) org…
        let own = assignable_users(&pool, default_org).await.unwrap();
        assert!(
            own.iter().any(|x| x.id == u.id.0),
            "member visible to their own org"
        );
        // …but NOT to an org they don't belong to (no cross-tenant leak).
        let foreign = assignable_users(&pool, org_b).await.unwrap();
        assert!(
            !foreign.iter().any(|x| x.id == u.id.0),
            "must not leak a user to a foreign org"
        );
    }

    /// PERF (audit #123 rank-8): `prune` chunks the high-volume `error_events`
    /// table per project retention window — events past the window age out, the
    /// recent event and the (small, long-lived) issue are kept. Runtime queries
    /// keep the test off the offline `.sqlx` cache.
    #[sqlx::test(migrations = "../../migrations")]
    async fn prune_deletes_old_events_keeps_recent_and_issue(pool: PgPool) {
        let org = def_org();
        // Default retention_days = 30.
        let p = find_or_create_by_name(&pool, "app", org).await.unwrap();
        let issue = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO error_issues (id, project_id, fingerprint, title) VALUES ($1,$2,'fp','t')",
        )
        .bind(issue)
        .bind(p.id.0)
        .execute(&pool)
        .await
        .unwrap();
        // One event well past the 30-day window → pruned; one fresh → kept.
        sqlx::query("INSERT INTO error_events (id, issue_id, project_id, ts) VALUES ($1,$2,$3, now() - interval '400 days')")
            .bind(uuid::Uuid::now_v7())
            .bind(issue)
            .bind(p.id.0)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO error_events (id, issue_id, project_id, ts) VALUES ($1,$2,$3, now())",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(issue)
        .bind(p.id.0)
        .execute(&pool)
        .await
        .unwrap();

        let deleted = prune(&pool).await.unwrap();
        assert_eq!(deleted, 1, "only the 400-day-old event is pruned");

        let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM error_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(events, 1, "the recent event survives");
        let issues: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM error_issues")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            issues, 1,
            "the issue is kept — only per-occurrence detail ages out"
        );
    }
}
