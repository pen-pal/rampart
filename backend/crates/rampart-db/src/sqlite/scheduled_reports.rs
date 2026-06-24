//! SQLite `scheduled_reports` domain — periodic uptime digests. Mirrors the
//! Postgres `crate::scheduled_reports` free-fn surface: list / get / create /
//! update / delete / due / render / mark_sent. Structs reused from PG /
//! rampart_core.
//!
//! Dialect: uuid→TEXT, `recipients` TEXT[]→JSON array TEXT, timestamps→INTEGER
//! unix-seconds. The PG `due` cadence CASE (`interval` + `date_trunc('month')`)
//! becomes plain unix-second arithmetic + `strftime('%Y-%m', …)`. `render`
//! reuses the already-ported `sqlite::monitors::list_all` +
//! `sqlite::heartbeats::uptime_pct`, and the dialect-neutral
//! `cadence_window_seconds` from the PG module (so the windows can't drift).

use super::{raw_uuid, ts};
use crate::scheduled_reports::{cadence_window_seconds, NewScheduledReport, UpdateScheduledReport};
use crate::{DbError, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::scheduled_report::ScheduledReport;
use rampart_core::ScheduledReportId;
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;
use uuid::Uuid;

fn report_from(r: &sqlx::sqlite::SqliteRow) -> ScheduledReport {
    ScheduledReport {
        id: ScheduledReportId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        recipients: serde_json::from_str(&r.get::<String, _>("recipients")).unwrap_or_default(),
        cadence: r.get("cadence"),
        last_sent_at: r.get::<Option<i64>, _>("last_sent_at").map(ts),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

const COLS: &str = "id, name, recipients, cadence, last_sent_at, created_at";

pub async fn list(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<ScheduledReport>> {
    let sql =
        format!("SELECT {COLS} FROM scheduled_reports WHERE org_id = ? ORDER BY created_at DESC");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(report_from).collect())
}

pub async fn get(
    pool: &SqlitePool,
    id: ScheduledReportId,
    org_id: OrgId,
) -> DbResult<ScheduledReport> {
    let sql = format!("SELECT {COLS} FROM scheduled_reports WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(report_from(&row))
}

pub async fn create(
    pool: &SqlitePool,
    input: NewScheduledReport,
    org_id: OrgId,
) -> DbResult<ScheduledReport> {
    let id = Uuid::now_v7();
    let sql = format!(
        "INSERT INTO scheduled_reports (id, name, recipients, cadence, org_id)
         VALUES (?, ?, ?, ?, ?) RETURNING {COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.to_string())
        .bind(input.name)
        .bind(serde_json::to_string(&input.recipients).unwrap_or_else(|_| "[]".into()))
        .bind(input.cadence)
        .bind(org_id.0.to_string())
        .fetch_one(pool)
        .await?;
    Ok(report_from(&row))
}

pub async fn update(
    pool: &SqlitePool,
    id: ScheduledReportId,
    input: UpdateScheduledReport,
    org_id: OrgId,
) -> DbResult<ScheduledReport> {
    let cur = get(pool, id, org_id).await?;
    let recipients = input.recipients.unwrap_or(cur.recipients);
    let sql = format!(
        "UPDATE scheduled_reports SET name = ?, recipients = ?, cadence = ?
         WHERE id = ? AND org_id = ? RETURNING {COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(input.name.unwrap_or(cur.name))
        .bind(serde_json::to_string(&recipients).unwrap_or_else(|_| "[]".into()))
        .bind(input.cadence.unwrap_or(cur.cadence))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_one(pool)
        .await?;
    Ok(report_from(&row))
}

pub async fn delete(pool: &SqlitePool, id: ScheduledReportId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM scheduled_reports WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Reports due to send (never sent, or older than the cadence window). The PG
/// `interval` / `date_trunc('month')` CASE becomes unix-second math +
/// `strftime('%Y-%m')`. `now` is bound once per `?` occurrence (positional).
pub async fn due(
    pool: &SqlitePool,
    now: OffsetDateTime,
) -> DbResult<Vec<(ScheduledReport, OrgId)>> {
    let now_unix = now.unix_timestamp();
    let sql = format!(
        "SELECT {COLS}, org_id FROM scheduled_reports
         WHERE last_sent_at IS NULL
            OR CASE cadence
                 WHEN 'daily'   THEN last_sent_at <= ? - 86400
                 WHEN 'monthly' THEN strftime('%Y-%m', last_sent_at, 'unixepoch')
                                     < strftime('%Y-%m', ?, 'unixepoch')
                 ELSE                last_sent_at <= ? - 604800
               END"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(now_unix)
        .bind(now_unix)
        .bind(now_unix)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| (report_from(r), super::oid(&r.get::<String, _>("org_id"))))
        .collect())
}

fn cadence_label(cadence: &str) -> &'static str {
    match cadence {
        "daily" => "Daily",
        "monthly" => "Monthly",
        _ => "Weekly",
    }
}

/// Render the uptime digest over the cadence window. Mirrors PG `render` but
/// against the SQLite monitor/heartbeat domains.
pub async fn render(
    pool: &SqlitePool,
    report_name: &str,
    cadence: &str,
    org_id: OrgId,
) -> DbResult<(String, String)> {
    let window_seconds = cadence_window_seconds(cadence);
    // Org-scoped — a report only summarizes its own org's monitors.
    let monitors = super::monitors::list(pool, org_id).await?;
    let subject = format!("{} uptime report — {report_name}", cadence_label(cadence));
    let mut lines = Vec::with_capacity(monitors.len() + 2);
    lines.push(subject.clone());
    let days = window_seconds / (24 * 3600);
    lines.push(format!("Per-monitor uptime over the last {days} day(s):"));
    if monitors.is_empty() {
        lines.push("(no monitors configured)".to_string());
    }
    // One set-based aggregate instead of a per-monitor query (was N+1).
    let ids: Vec<Uuid> = monitors.iter().map(|m| m.id.0).collect();
    let pcts = super::heartbeats::uptime_pct_batch(pool, &ids, window_seconds).await?;
    for m in &monitors {
        match pcts.get(&m.id.0) {
            Some(p) => lines.push(format!("- {}: {:.2}%", m.name, p)),
            None => lines.push(format!("- {}: no data", m.name)),
        }
    }
    Ok((subject, lines.join("\n")))
}

pub async fn mark_sent(pool: &SqlitePool, id: ScheduledReportId) -> DbResult<()> {
    sqlx::query("UPDATE scheduled_reports SET last_sent_at = unixepoch() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_report(name: &str, cadence: &str) -> NewScheduledReport {
        NewScheduledReport {
            name: name.into(),
            recipients: vec!["ops@example.com".into()],
            cadence: cadence.into(),
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn crud_due_and_mark_sent(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let r = create(&pool, new_report("weekly digest", "weekly"), org)
            .await
            .unwrap();
        assert_eq!(r.recipients, vec!["ops@example.com".to_string()]);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert!(r.last_sent_at.is_none());

        // never-sent → due now.
        let now = OffsetDateTime::now_utc();
        assert_eq!(due(&pool, now).await.unwrap().len(), 1);

        // mark_sent → no longer due (within the weekly window).
        mark_sent(&pool, r.id).await.unwrap();
        assert!(get(&pool, r.id, org).await.unwrap().last_sent_at.is_some());
        assert!(due(&pool, now).await.unwrap().is_empty());
        // ...but due again 8 days later.
        let later = now + time::Duration::days(8);
        assert_eq!(due(&pool, later).await.unwrap().len(), 1);

        // update recipients + cadence.
        let u = update(
            &pool,
            r.id,
            UpdateScheduledReport {
                name: None,
                recipients: Some(vec!["a@x".into(), "b@x".into()]),
                cadence: Some("daily".into()),
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(u.recipients.len(), 2);
        assert_eq!(u.cadence, "daily");

        // render produces a subject + a line per monitor (none here).
        let (subject, body) = render(&pool, "weekly digest", "weekly", org).await.unwrap();
        assert!(subject.starts_with("Weekly uptime report"));
        assert!(body.contains("no monitors configured"));

        // cross-org isolation.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, r.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, r.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, r.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
