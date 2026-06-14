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
use rampart_core::ids::{ErrorIssueId, ErrorProjectId};
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

pub async fn list(pool: &DbPool) -> DbResult<Vec<ErrorProject>> {
    let rows = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT id, name, slug, public_key, platform, retention_days,
               alert_channel_ids AS "alert_channel_ids!", created_at
        FROM error_projects
        ORDER BY created_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &DbPool, id: ErrorProjectId) -> DbResult<ErrorProject> {
    get_opt(pool, id).await?.ok_or(DbError::NotFound)
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
pub async fn find_or_create_by_name(pool: &DbPool, name: &str) -> DbResult<ErrorProject> {
    let existing = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT id, name, slug, public_key, platform, retention_days,
               alert_channel_ids AS "alert_channel_ids!", created_at
        FROM error_projects
        WHERE name = $1
        ORDER BY created_at
        LIMIT 1
        "#,
        name,
    )
    .fetch_optional(pool)
    .await?;
    if let Some(row) = existing {
        return Ok(row.into());
    }
    create(
        pool,
        rampart_core::error_tracking::NewErrorProject {
            name: name.to_string(),
            platform: Some("javascript".to_string()),
            alert_channel_ids: Vec::new(),
        },
    )
    .await
}

pub async fn create(pool: &DbPool, input: NewErrorProject) -> DbResult<ErrorProject> {
    let id = ErrorProjectId::new();
    let slug = slugify(&input.name);
    let public_key = random_token(KEY_LEN);
    let channels =
        serde_json::to_value(&input.alert_channel_ids).unwrap_or_else(|_| serde_json::json!([]));
    sqlx::query!(
        r#"
        INSERT INTO error_projects (id, name, slug, public_key, platform, alert_channel_ids)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id.0,
        input.name,
        slug,
        public_key,
        input.platform,
        channels,
    )
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: ErrorProjectId,
    patch: UpdateErrorProject,
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
        WHERE id = $1
        "#,
        id.0,
        patch.name,
        patch.platform,
        patch.retention_days,
        channels,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id).await
}

pub async fn delete(pool: &DbPool, id: ErrorProjectId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM error_projects WHERE id = $1", id.0)
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
    sqlx::query!(
        r#"
        INSERT INTO error_events
            (id, issue_id, project_id, ts, level, message, exception_type, culprit,
             environment, release, server_name, stacktrace, context)
        VALUES ($1, $2, $3, now(), $4, $5, $6, $7, $8, $9, $10, $11, $12)
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

/// Issues for a project, newest activity first. `status` filters when set.
pub async fn list_issues(
    pool: &DbPool,
    project_id: ErrorProjectId,
    status: Option<&str>,
) -> DbResult<Vec<ErrorIssue>> {
    let rows = sqlx::query_as!(
        IssueRow,
        r#"
        SELECT id, project_id, fingerprint, title, culprit, level, status,
               first_seen, last_seen, times_seen, assignee
        FROM error_issues
        WHERE project_id = $1 AND ($2::text IS NULL OR status = $2)
        ORDER BY last_seen DESC
        LIMIT 200
        "#,
        project_id.0,
        status,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
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
pub async fn prune(pool: &DbPool) -> DbResult<u64> {
    let result = sqlx::query!(
        r#"
        DELETE FROM error_events e
        USING error_projects p
        WHERE e.project_id = p.id
          AND e.ts < now() - make_interval(days => p.retention_days)
        "#,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
