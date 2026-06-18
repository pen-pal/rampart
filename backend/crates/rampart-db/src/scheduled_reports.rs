//! Scheduled uptime report queries.
//!
//! CRUD over the `scheduled_reports` table plus the `due` read the
//! scheduler uses to find reports that need sending, and `mark_sent` to
//! stamp `last_sent_at` after a successful send.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::scheduled_report::ScheduledReport;
use rampart_core::ScheduledReportId;
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct NewScheduledReport {
    pub name: String,
    #[serde(default)]
    pub recipients: Vec<String>,
    #[serde(default = "default_cadence")]
    pub cadence: String,
}

fn default_cadence() -> String {
    "weekly".to_string()
}

#[derive(Debug, Deserialize)]
pub struct UpdateScheduledReport {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub recipients: Option<Vec<String>>,
    #[serde(default)]
    pub cadence: Option<String>,
}

pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<ScheduledReport>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, recipients, cadence, last_sent_at, created_at
        FROM scheduled_reports
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ScheduledReport {
            id: ScheduledReportId::from_uuid(r.id),
            name: r.name,
            recipients: r.recipients,
            cadence: r.cadence,
            last_sent_at: r.last_sent_at,
            created_at: r.created_at,
        })
        .collect())
}

pub async fn get(pool: &DbPool, id: ScheduledReportId, org_id: OrgId) -> DbResult<ScheduledReport> {
    let r = sqlx::query!(
        r#"
        SELECT id, name, recipients, cadence, last_sent_at, created_at
        FROM scheduled_reports
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(ScheduledReport {
        id: ScheduledReportId::from_uuid(r.id),
        name: r.name,
        recipients: r.recipients,
        cadence: r.cadence,
        last_sent_at: r.last_sent_at,
        created_at: r.created_at,
    })
}

pub async fn create(
    pool: &DbPool,
    input: NewScheduledReport,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<ScheduledReport> {
    let id = Uuid::now_v7();
    let r = sqlx::query!(
        r#"
        INSERT INTO scheduled_reports (id, name, recipients, cadence, org_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, recipients, cadence, last_sent_at, created_at
        "#,
        id,
        input.name,
        &input.recipients,
        input.cadence,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(ScheduledReport {
        id: ScheduledReportId::from_uuid(r.id),
        name: r.name,
        recipients: r.recipients,
        cadence: r.cadence,
        last_sent_at: r.last_sent_at,
        created_at: r.created_at,
    })
}

pub async fn update(
    pool: &DbPool,
    id: ScheduledReportId,
    input: UpdateScheduledReport,
    org_id: OrgId,
) -> DbResult<ScheduledReport> {
    let cur = get(pool, id, org_id).await?;
    let r = sqlx::query!(
        r#"
        UPDATE scheduled_reports
        SET name = $2, recipients = $3, cadence = $4
        WHERE id = $1 AND org_id = $5
        RETURNING id, name, recipients, cadence, last_sent_at, created_at
        "#,
        id.0,
        input.name.unwrap_or(cur.name),
        &input.recipients.unwrap_or(cur.recipients),
        input.cadence.unwrap_or(cur.cadence),
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(ScheduledReport {
        id: ScheduledReportId::from_uuid(r.id),
        name: r.name,
        recipients: r.recipients,
        cadence: r.cadence,
        last_sent_at: r.last_sent_at,
        created_at: r.created_at,
    })
}

pub async fn delete(pool: &DbPool, id: ScheduledReportId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query!(
        "DELETE FROM scheduled_reports WHERE id = $1 AND org_id = $2",
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Reports due to be sent: never sent, or `last_sent_at` older than the
/// row's cadence window. The scheduler calls this on its slow tick.
///
/// Due-ness per cadence:
/// - `daily` — `last_sent_at` more than ~24h ago.
/// - `weekly` — more than ~7d ago (the historical behaviour, preserved).
/// - `monthly` — sent in an earlier calendar month than `now`.
///
/// Any unrecognised cadence falls back to the weekly window so a typo can
/// never wedge a report into "never due". A row with no recipients is still
/// returned — the scheduler logs + skips it so a misconfigured report is
/// visible rather than silently never due.
pub async fn due(pool: &DbPool, now: OffsetDateTime) -> DbResult<Vec<ScheduledReport>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, recipients, cadence, last_sent_at, created_at
        FROM scheduled_reports
        WHERE last_sent_at IS NULL
           OR CASE cadence
                WHEN 'daily'   THEN last_sent_at <= $1::timestamptz - interval '1 day'
                WHEN 'monthly' THEN date_trunc('month', last_sent_at)
                                    < date_trunc('month', $1::timestamptz)
                ELSE                last_sent_at <= $1::timestamptz - interval '7 days'
              END
        "#,
        now,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ScheduledReport {
            id: ScheduledReportId::from_uuid(r.id),
            name: r.name,
            recipients: r.recipients,
            cadence: r.cadence,
            last_sent_at: r.last_sent_at,
            created_at: r.created_at,
        })
        .collect())
}

/// Lookback window, in seconds, for a cadence string. Matches the due-ness
/// gate: daily → 1 day, monthly → 30 days, everything else (incl. weekly and
/// any unknown value) → 7 days. The digest body reports uptime over exactly
/// this window so "Daily uptime report" covers the last day, etc.
pub fn cadence_window_seconds(cadence: &str) -> i64 {
    match cadence {
        "daily" => 24 * 3600,
        "monthly" => 30 * 24 * 3600,
        _ => 7 * 24 * 3600,
    }
}

/// Human label for a cadence, used in the report subject line.
fn cadence_label(cadence: &str) -> &'static str {
    match cadence {
        "daily" => "Daily",
        "monthly" => "Monthly",
        _ => "Weekly",
    }
}

/// Render an uptime digest for a report: one line per monitor with its
/// uptime percentage over the cadence's lookback window (or "no data" when
/// the window holds no heartbeats). Plain text — the system email sender
/// ships text/plain. Returns `(subject, body)`. Shared by the scheduler's
/// due-report check and the API's send-now endpoint so the two never drift.
pub async fn render(pool: &DbPool, report_name: &str, cadence: &str) -> DbResult<(String, String)> {
    let window_seconds = cadence_window_seconds(cadence);
    // System-side report generation over the whole fleet (scheduled reports
    // become per-org in a later multi-tenancy phase).
    let monitors = crate::monitors::list_all(pool).await?;

    let subject = format!("{} uptime report — {report_name}", cadence_label(cadence));
    let mut lines = Vec::with_capacity(monitors.len() + 2);
    lines.push(subject.clone());
    let days = window_seconds / (24 * 3600);
    lines.push(format!("Per-monitor uptime over the last {days} day(s):"));
    if monitors.is_empty() {
        lines.push("(no monitors configured)".to_string());
    }
    for m in &monitors {
        let pct = crate::heartbeats::uptime_pct(pool, m.id, window_seconds).await?;
        match pct {
            Some(p) => lines.push(format!("- {}: {:.2}%", m.name, p)),
            None => lines.push(format!("- {}: no data", m.name)),
        }
    }
    Ok((subject, lines.join("\n")))
}

/// Stamp `last_sent_at = NOW()` after a successful send so the report
/// isn't re-sent until the next cadence window.
pub async fn mark_sent(pool: &DbPool, id: ScheduledReportId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE scheduled_reports SET last_sent_at = NOW() WHERE id = $1",
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}
