//! MySQL `error_tracking` domain — Sentry-lite projects/issues/events. Ported
//! from PG (the biggest management-API tail domain).
//!
//! Dialect notes:
//! - uuid→CHAR(36), ts→BIGINT, JSONB→JSON (native; the affected-users/stats
//!   reads use `->>'$.path'` JSON extraction), `release`→backticked.
//! - `ON CONFLICT (project_id,fingerprint) DO NOTHING RETURNING id` → app-side
//!   UUID + `INSERT IGNORE`; `rows_affected()==1` means we claimed the
//!   fingerprint (new issue), else we read + bump the existing one.
//! - `date_bin(make_interval(secs=>s), ts, origin)` → `((ts-origin) DIV s)*s +
//!   origin` (integer `DIV`, like the logs/error histograms).
//! - `now() - make_interval(days=>p.retention_days)` prune → a multi-table
//!   `DELETE e FROM … JOIN …` with a Rust-free cutoff per project row.
//! - no FK cascade on the MySQL tier → `delete_project` removes events + issues
//!   in a tx by hand.

use super::{raw_uuid, ts};
use crate::error_tracking::{
    AffectedUser, AssignableUser, ErrorBucket, IssueStats, RecordOutcome, TraceErrorRef,
};
use crate::{DbError, DbResult};
use rampart_core::error_tracking::{
    fingerprint, ErrorEvent, ErrorIssue, ErrorProject, NewErrorProject, ParsedEvent,
    UpdateErrorProject,
};
use rampart_core::ids::{ErrorIssueId, ErrorProjectId, OrgId, UserId};
use sqlx::{MySqlPool, Row};
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

/// URL-safe slug + short random suffix (guarantees UNIQUE(slug) without a retry).
fn slugify(name: &str) -> String {
    let mut base: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while base.contains("--") {
        base = base.replace("--", "-");
    }
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "project" } else { base };
    format!("{base}-{}", random_token(4).to_lowercase())
}

// ─────────────────────────── projects ───────────────────────────

const PROJECT_COLS: &str =
    "id, name, slug, public_key, platform, retention_days, alert_channel_ids, created_at";

fn project_from(r: &sqlx::mysql::MySqlRow) -> ErrorProject {
    ErrorProject {
        id: ErrorProjectId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        slug: r.get("slug"),
        public_key: r.get("public_key"),
        platform: r.get("platform"),
        retention_days: r.get::<i32, _>("retention_days"),
        alert_channel_ids: serde_json::from_str(&r.get::<String, _>("alert_channel_ids"))
            .unwrap_or_default(),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<ErrorProject>> {
    let sql =
        format!("SELECT {PROJECT_COLS} FROM error_projects WHERE org_id = ? ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(project_from).collect())
}

pub async fn project_in_org(
    pool: &MySqlPool,
    project: ErrorProjectId,
    org_id: OrgId,
) -> DbResult<()> {
    let (ok,): (i64,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM error_projects WHERE id = ? AND org_id = ?)")
            .bind(project.0.to_string())
            .bind(org_id.0.to_string())
            .fetch_one(pool)
            .await?;
    if ok != 0 {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
}

pub async fn issue_in_org(pool: &MySqlPool, issue: ErrorIssueId, org_id: OrgId) -> DbResult<()> {
    let (ok,): (i64,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM error_issues i
            JOIN error_projects p ON p.id = i.project_id
            WHERE i.id = ? AND p.org_id = ?)",
    )
    .bind(issue.0.to_string())
    .bind(org_id.0.to_string())
    .fetch_one(pool)
    .await?;
    if ok != 0 {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
}

pub async fn get(pool: &MySqlPool, id: ErrorProjectId) -> DbResult<ErrorProject> {
    get_opt(pool, id).await?.ok_or(DbError::NotFound)
}

pub async fn org_for_project(pool: &MySqlPool, id: ErrorProjectId) -> DbResult<OrgId> {
    let row = sqlx::query("SELECT org_id FROM error_projects WHERE id = ?")
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(OrgId::from_uuid(raw_uuid(&row.get::<String, _>("org_id"))))
}

pub async fn get_opt(pool: &MySqlPool, id: ErrorProjectId) -> DbResult<Option<ErrorProject>> {
    let sql = format!("SELECT {PROJECT_COLS} FROM error_projects WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(project_from))
}

pub async fn find_or_create_by_name(
    pool: &MySqlPool,
    name: &str,
    org_id: OrgId,
) -> DbResult<ErrorProject> {
    let sql = format!(
        "SELECT {PROJECT_COLS} FROM error_projects WHERE name = ? AND org_id = ? ORDER BY created_at LIMIT 1"
    );
    let existing = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(name)
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?;
    if let Some(row) = existing {
        return Ok(project_from(&row));
    }
    create(
        pool,
        NewErrorProject {
            name: name.to_string(),
            platform: Some("javascript".to_string()),
            alert_channel_ids: Vec::new(),
        },
        org_id,
    )
    .await
}

pub async fn create(
    pool: &MySqlPool,
    input: NewErrorProject,
    org_id: OrgId,
) -> DbResult<ErrorProject> {
    let id = ErrorProjectId::new();
    let slug = slugify(&input.name);
    let public_key = random_token(KEY_LEN);
    let channels = serde_json::to_string(&input.alert_channel_ids).unwrap_or_else(|_| "[]".into());
    sqlx::query(
        "INSERT INTO error_projects (id, name, slug, public_key, platform, alert_channel_ids, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(slug)
    .bind(public_key)
    .bind(input.platform)
    .bind(channels)
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn update(
    pool: &MySqlPool,
    id: ErrorProjectId,
    patch: UpdateErrorProject,
    org_id: OrgId,
) -> DbResult<ErrorProject> {
    let channels = patch
        .alert_channel_ids
        .map(|c| serde_json::to_string(&c).unwrap_or_else(|_| "[]".into()));
    sqlx::query(
        "UPDATE error_projects SET
            name              = COALESCE(?, name),
            platform          = COALESCE(?, platform),
            retention_days    = COALESCE(?, retention_days),
            alert_channel_ids = COALESCE(?, alert_channel_ids)
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(patch.platform)
    .bind(patch.retention_days)
    .bind(channels)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    // Re-select through the org gate (no rows_affected gate — MySQL counts
    // CHANGED rows, so an all-None patch would false-404).
    let sql = format!("SELECT {PROJECT_COLS} FROM error_projects WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(project_from(&row))
}

pub async fn delete(pool: &MySqlPool, id: ErrorProjectId, org_id: OrgId) -> DbResult<()> {
    let sid = id.0.to_string();
    let mut tx = pool.begin().await?;
    // No FK cascade → drop children first.
    sqlx::query("DELETE FROM error_events WHERE project_id = ?")
        .bind(&sid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM error_issues WHERE project_id = ?")
        .bind(&sid)
        .execute(&mut *tx)
        .await?;
    let res = sqlx::query("DELETE FROM error_projects WHERE id = ? AND org_id = ?")
        .bind(&sid)
        .bind(org_id.0.to_string())
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(DbError::NotFound);
    }
    tx.commit().await?;
    Ok(())
}

// ─────────────────────────── issues + events ───────────────────────────

const ISSUE_COLS: &str = "id, project_id, fingerprint, title, culprit, level, status, \
                          first_seen, last_seen, times_seen, assignee";

fn issue_from(r: &sqlx::mysql::MySqlRow) -> ErrorIssue {
    ErrorIssue {
        id: ErrorIssueId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        project_id: ErrorProjectId::from_uuid(raw_uuid(&r.get::<String, _>("project_id"))),
        fingerprint: r.get("fingerprint"),
        title: r.get("title"),
        culprit: r.get("culprit"),
        level: r.get("level"),
        status: r.get("status"),
        first_seen: ts(r.get::<i64, _>("first_seen")),
        last_seen: ts(r.get::<i64, _>("last_seen")),
        times_seen: r.get::<i64, _>("times_seen"),
        assignee: r
            .get::<Option<String>, _>("assignee")
            .map(|s| UserId::from_uuid(raw_uuid(&s))),
    }
}

pub async fn record_event(
    pool: &MySqlPool,
    project_id: ErrorProjectId,
    ev: &ParsedEvent,
) -> DbResult<RecordOutcome> {
    let fp = fingerprint(ev);
    let title = ev.title();
    let culprit = ev.culprit();
    let level = ev.level.clone();

    // Claim the fingerprint as a new issue (INSERT IGNORE over the unique index).
    let new_id = Uuid::now_v7();
    let res = sqlx::query(
        "INSERT IGNORE INTO error_issues
            (id, project_id, fingerprint, title, culprit, level, times_seen, last_seen)
         VALUES (?, ?, ?, ?, ?, ?, 1, UNIX_TIMESTAMP())",
    )
    .bind(new_id.to_string())
    .bind(project_id.0.to_string())
    .bind(&fp)
    .bind(&title)
    .bind(&culprit)
    .bind(&level)
    .execute(pool)
    .await?;

    let (issue_id, is_new, regressed) = if res.rows_affected() == 1 {
        (new_id, true, false)
    } else {
        let existing = sqlx::query(
            "SELECT id, status FROM error_issues WHERE project_id = ? AND fingerprint = ?",
        )
        .bind(project_id.0.to_string())
        .bind(&fp)
        .fetch_one(pool)
        .await?;
        let existing_id = raw_uuid(&existing.get::<String, _>("id"));
        let regressed = existing.get::<String, _>("status") == "resolved";
        sqlx::query(
            "UPDATE error_issues SET
                times_seen = times_seen + 1,
                last_seen  = UNIX_TIMESTAMP(),
                title      = ?,
                level      = ?,
                culprit    = ?,
                status     = CASE WHEN status = 'resolved' THEN 'unresolved' ELSE status END
             WHERE id = ?",
        )
        .bind(&title)
        .bind(&level)
        .bind(&culprit)
        .bind(existing_id.to_string())
        .execute(pool)
        .await?;
        (existing_id, false, regressed)
    };

    // Store the event detail.
    let event_id = parse_event_id(ev.event_id.as_deref());
    let stacktrace = if ev.frames.is_empty() {
        None
    } else {
        serde_json::to_value(&ev.frames).ok()
    };
    let trace_id = ev
        .raw
        .pointer("/contexts/trace/trace_id")
        .and_then(|v| v.as_str());
    sqlx::query(
        "INSERT INTO error_events
            (id, issue_id, project_id, ts, level, message, exception_type, culprit,
             environment, `release`, server_name, stacktrace, context, trace_id)
         VALUES (?, ?, ?, UNIX_TIMESTAMP(), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(event_id.to_string())
    .bind(issue_id.to_string())
    .bind(project_id.0.to_string())
    .bind(&ev.level)
    .bind(&ev.message)
    .bind(&ev.exception_type)
    .bind(&culprit)
    .bind(&ev.environment)
    .bind(&ev.release)
    .bind(&ev.server_name)
    .bind(stacktrace)
    .bind(&ev.raw)
    .bind(trace_id)
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

fn parse_event_id(raw: Option<&str>) -> Uuid {
    raw.and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::now_v7)
}

pub async fn issues_for_trace(
    pool: &MySqlPool,
    trace_id: &str,
    org_id: OrgId,
) -> DbResult<Vec<TraceErrorRef>> {
    let rows = sqlx::query(
        "SELECT ev.issue_id, i.title, i.level, COUNT(*) AS cnt
         FROM error_events ev
         JOIN error_issues i ON i.id = ev.issue_id
         JOIN error_projects p ON p.id = ev.project_id AND p.org_id = ?
         WHERE ev.trace_id = ?
         GROUP BY ev.issue_id, i.title, i.level
         ORDER BY COUNT(*) DESC LIMIT 20",
    )
    .bind(org_id.0.to_string())
    .bind(trace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| TraceErrorRef {
            issue_id: raw_uuid(&r.get::<String, _>("issue_id")),
            title: r.get("title"),
            level: r.get("level"),
            count: r.get::<i64, _>("cnt"),
        })
        .collect())
}

pub async fn list_issues(
    pool: &MySqlPool,
    project_id: ErrorProjectId,
    status: Option<&str>,
    before_id: Option<Uuid>,
    limit: i64,
) -> DbResult<Vec<ErrorIssue>> {
    let before = before_id.map(|u| u.to_string());
    let sql = format!(
        "SELECT {ISSUE_COLS} FROM error_issues
         WHERE project_id = ? AND (? IS NULL OR status = ?)
           AND (? IS NULL
                OR (last_seen, id) < ((SELECT last_seen FROM error_issues WHERE id = ?), ?))
         ORDER BY last_seen DESC, id DESC
         LIMIT ?"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(project_id.0.to_string())
        .bind(status)
        .bind(status)
        .bind(&before)
        .bind(&before)
        .bind(&before)
        .bind(limit.clamp(1, 200))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(issue_from).collect())
}

pub async fn recent_open_issues(
    pool: &MySqlPool,
    limit: i64,
    org_id: OrgId,
) -> DbResult<Vec<ErrorIssue>> {
    let cols = ISSUE_COLS
        .split(", ")
        .map(|c| format!("i.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {cols}
         FROM error_issues i
         JOIN error_projects p ON p.id = i.project_id
         WHERE i.status = 'unresolved' AND p.org_id = ?
         ORDER BY i.last_seen DESC
         LIMIT ?"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .bind(limit.clamp(1, 50))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(issue_from).collect())
}

pub async fn project_event_histogram(
    pool: &MySqlPool,
    project_id: ErrorProjectId,
    hours: i32,
    buckets: i64,
) -> DbResult<Vec<ErrorBucket>> {
    let hours = hours.clamp(1, 720);
    let buckets = buckets.clamp(2, 200);
    let step = ((hours as i64 * 3600) / buckets).max(1);
    let origin = OffsetDateTime::now_utc().unix_timestamp() - hours as i64 * 3600;
    let rows = sqlx::query(
        "SELECT (((ts - ?) DIV ?) * ?) + ? AS bucket, COUNT(*) AS cnt
         FROM error_events
         WHERE project_id = ? AND ts > ?
         GROUP BY bucket ORDER BY bucket",
    )
    .bind(origin)
    .bind(step)
    .bind(step)
    .bind(origin)
    .bind(project_id.0.to_string())
    .bind(origin)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| ErrorBucket {
            ts: ts(r.get::<i64, _>("bucket")),
            count: r.get::<i64, _>("cnt"),
        })
        .collect())
}

pub async fn get_issue(pool: &MySqlPool, id: ErrorIssueId) -> DbResult<ErrorIssue> {
    let sql = format!("SELECT {ISSUE_COLS} FROM error_issues WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(issue_from(&row))
}

pub async fn issue_affected_users(
    pool: &MySqlPool,
    id: ErrorIssueId,
    limit: i64,
) -> DbResult<Vec<AffectedUser>> {
    let rows = sqlx::query(
        "SELECT COALESCE(JSON_UNQUOTE(JSON_EXTRACT(context, '$.user.id')),
                         JSON_UNQUOTE(JSON_EXTRACT(context, '$.user.email')),
                         JSON_UNQUOTE(JSON_EXTRACT(context, '$.user.username'))) AS ident,
                COUNT(*) AS events, MAX(ts) AS last_seen
         FROM error_events
         WHERE issue_id = ? AND JSON_EXTRACT(context, '$.user') IS NOT NULL
         GROUP BY ident
         ORDER BY MAX(ts) DESC
         LIMIT ?",
    )
    .bind(id.0.to_string())
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            r.get::<Option<String>, _>("ident")
                .map(|ident| AffectedUser {
                    ident,
                    events: r.get::<i64, _>("events"),
                    last_seen: ts(r.get::<i64, _>("last_seen")),
                })
        })
        .collect())
}

pub async fn issue_stats(pool: &MySqlPool, id: ErrorIssueId) -> DbResult<IssueStats> {
    let (users,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT COALESCE(JSON_UNQUOTE(JSON_EXTRACT(context, '$.user.id')),
                                        JSON_UNQUOTE(JSON_EXTRACT(context, '$.user.email')),
                                        JSON_UNQUOTE(JSON_EXTRACT(context, '$.user.username'))))
         FROM error_events
         WHERE issue_id = ? AND JSON_EXTRACT(context, '$.user') IS NOT NULL",
    )
    .bind(id.0.to_string())
    .fetch_one(pool)
    .await?;

    let kv = |col: &str| {
        // COALESCE(NULLIF(col,''),'(none)') grouped by event count, top 10.
        format!(
            "SELECT COALESCE(NULLIF({col}, ''), '(none)') AS k, COUNT(*) AS n
             FROM error_events WHERE issue_id = ? GROUP BY k ORDER BY n DESC LIMIT 10"
        )
    };
    let releases = sqlx::query(sqlx::AssertSqlSafe(kv("`release`")))
        .bind(id.0.to_string())
        .fetch_all(pool)
        .await?;
    let envs = sqlx::query(sqlx::AssertSqlSafe(kv("environment")))
        .bind(id.0.to_string())
        .fetch_all(pool)
        .await?;

    Ok(IssueStats {
        users_affected: users,
        by_release: releases
            .iter()
            .map(|r| (r.get::<String, _>("k"), r.get::<i64, _>("n")))
            .collect(),
        by_environment: envs
            .iter()
            .map(|r| (r.get::<String, _>("k"), r.get::<i64, _>("n")))
            .collect(),
    })
}

pub async fn set_issue_status(
    pool: &MySqlPool,
    id: ErrorIssueId,
    status: &str,
) -> DbResult<ErrorIssue> {
    sqlx::query("UPDATE error_issues SET status = ? WHERE id = ?")
        .bind(status)
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    get_issue(pool, id).await
}

pub async fn assign_issue(
    pool: &MySqlPool,
    id: ErrorIssueId,
    assignee: Option<UserId>,
) -> DbResult<ErrorIssue> {
    sqlx::query("UPDATE error_issues SET assignee = ? WHERE id = ?")
        .bind(assignee.map(|u| u.0.to_string()))
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    get_issue(pool, id).await
}

pub async fn assignable_users(pool: &MySqlPool) -> DbResult<Vec<AssignableUser>> {
    let rows =
        sqlx::query("SELECT id, name, email FROM users ORDER BY (name IS NULL), name, email")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .iter()
        .map(|r| AssignableUser {
            id: raw_uuid(&r.get::<String, _>("id")),
            name: r.get("name"),
            email: r.get("email"),
        })
        .collect())
}

fn event_from(r: &sqlx::mysql::MySqlRow) -> ErrorEvent {
    ErrorEvent {
        id: raw_uuid(&r.get::<String, _>("id")),
        issue_id: ErrorIssueId::from_uuid(raw_uuid(&r.get::<String, _>("issue_id"))),
        project_id: ErrorProjectId::from_uuid(raw_uuid(&r.get::<String, _>("project_id"))),
        ts: ts(r.get::<i64, _>("ts")),
        level: r.get("level"),
        message: r.get("message"),
        exception_type: r.get("exception_type"),
        culprit: r.get("culprit"),
        environment: r.get("environment"),
        release: r.get("release"),
        server_name: r.get("server_name"),
        stacktrace: r.get::<Option<serde_json::Value>, _>("stacktrace"),
        context: r.get::<Option<serde_json::Value>, _>("context"),
    }
}

pub async fn list_events(
    pool: &MySqlPool,
    issue_id: ErrorIssueId,
    limit: i64,
) -> DbResult<Vec<ErrorEvent>> {
    let rows = sqlx::query(
        "SELECT id, issue_id, project_id, ts, level, message, exception_type, culprit,
                environment, `release` AS `release`, server_name, stacktrace, context
         FROM error_events WHERE issue_id = ? ORDER BY ts DESC LIMIT ?",
    )
    .bind(issue_id.0.to_string())
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(event_from).collect())
}

/// Delete events older than each project's `retention_days`. Issues persist.
pub async fn prune(pool: &MySqlPool) -> DbResult<u64> {
    let res = sqlx::query(
        "DELETE e FROM error_events e
         JOIN error_projects p ON p.id = e.project_id
         WHERE e.ts < UNIX_TIMESTAMP() - p.retention_days * 86400",
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn event(msg: &str, etype: &str, user_id: &str, env: &str, rel: &str) -> ParsedEvent {
        // ParsedEvent is parser-built (not Deserialize) → struct literal. Empty
        // frames means fingerprint() groups on (exception_type, message).
        ParsedEvent {
            event_id: None,
            level: "error".into(),
            platform: Some("python".into()),
            environment: Some(env.into()),
            release: Some(rel.into()),
            server_name: Some("h1".into()),
            transaction: Some("/x".into()),
            message: Some(msg.into()),
            exception_type: Some(etype.into()),
            exception_value: Some(msg.into()),
            frames: Vec::new(),
            fingerprint_override: None,
            raw: serde_json::json!({
                "user": { "id": user_id },
                "contexts": { "trace": { "trace_id": "tr-1" } }
            }),
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn projects_record_group_stats_prune(pool: MySqlPool) {
        let org = super::super::oid(DEF);

        let p = create(
            &pool,
            NewErrorProject {
                name: "Web App".into(),
                platform: Some("python".into()),
                alert_channel_ids: Vec::new(),
            },
            org,
        )
        .await
        .unwrap();
        assert!(p.slug.starts_with("web-app-"));
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        project_in_org(&pool, p.id, org).await.unwrap();
        assert_eq!(org_for_project(&pool, p.id).await.unwrap(), org);

        // find_or_create is idempotent by name within an org.
        let same = find_or_create_by_name(&pool, "Web App", org).await.unwrap();
        assert_eq!(same.id, p.id);

        // record two events with the SAME fingerprint → one issue, times_seen 2.
        let o1 = record_event(
            &pool,
            p.id,
            &event("boom", "ValueError", "u1", "prod", "1.0"),
        )
        .await
        .unwrap();
        assert!(o1.is_new && !o1.regressed);
        let o2 = record_event(
            &pool,
            p.id,
            &event("boom", "ValueError", "u2", "prod", "1.0"),
        )
        .await
        .unwrap();
        assert!(!o2.is_new);
        assert_eq!(o1.issue_id, o2.issue_id);
        let issue = get_issue(&pool, o1.issue_id).await.unwrap();
        assert_eq!(issue.times_seen, 2);
        issue_in_org(&pool, issue.id, org).await.unwrap();

        // a different fingerprint → a second issue.
        record_event(
            &pool,
            p.id,
            &event("kaboom", "KeyError", "u1", "dev", "1.0"),
        )
        .await
        .unwrap();
        assert_eq!(
            list_issues(&pool, p.id, None, None, 50)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(recent_open_issues(&pool, 50, org).await.unwrap().len(), 2);

        // resolve → record again regresses (reopens).
        set_issue_status(&pool, issue.id, "resolved").await.unwrap();
        let reg = record_event(
            &pool,
            p.id,
            &event("boom", "ValueError", "u3", "prod", "1.1"),
        )
        .await
        .unwrap();
        assert!(reg.regressed);
        assert_eq!(
            get_issue(&pool, issue.id).await.unwrap().status,
            "unresolved"
        );

        // stats: 3 distinct users on the boom issue; release/env rollups.
        let stats = issue_stats(&pool, issue.id).await.unwrap();
        assert_eq!(stats.users_affected, 3);
        assert!(stats.by_release.iter().any(|(k, _)| k == "1.0"));
        let users = issue_affected_users(&pool, issue.id, 10).await.unwrap();
        assert_eq!(users.len(), 3);

        // trace cross-link + event list.
        assert_eq!(issues_for_trace(&pool, "tr-1", org).await.unwrap().len(), 2);
        assert_eq!(list_events(&pool, issue.id, 10).await.unwrap().len(), 3);

        // histogram has buckets.
        assert!(!project_event_histogram(&pool, p.id, 24, 24)
            .await
            .unwrap()
            .is_empty());

        // cross-org gate + delete cascade.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            project_in_org(&pool, p.id, other.id).await,
            Err(DbError::NotFound)
        ));
        assert!(matches!(
            delete(&pool, p.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, p.id, org).await.unwrap();
        assert!(list(&pool, org).await.unwrap().is_empty());
        assert!(matches!(
            get_issue(&pool, issue.id).await,
            Err(DbError::NotFound)
        ));

        // prune is a no-op with no aged events.
        assert_eq!(prune(&pool).await.unwrap(), 0);
    }
}
