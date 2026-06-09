//! Scheduled uptime report queries.
//!
//! CRUD over the `scheduled_reports` table plus the `due` read the
//! scheduler uses to find reports that need sending, and `mark_sent` to
//! stamp `last_sent_at` after a successful send.

use crate::{DbError, DbPool, DbResult};
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

pub async fn list(pool: &DbPool) -> DbResult<Vec<ScheduledReport>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, recipients, cadence, last_sent_at, created_at
        FROM scheduled_reports
        ORDER BY created_at DESC
        "#,
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

pub async fn get(pool: &DbPool, id: ScheduledReportId) -> DbResult<ScheduledReport> {
    let r = sqlx::query!(
        r#"
        SELECT id, name, recipients, cadence, last_sent_at, created_at
        FROM scheduled_reports
        WHERE id = $1
        "#,
        id.0,
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

pub async fn create(pool: &DbPool, input: NewScheduledReport) -> DbResult<ScheduledReport> {
    let id = Uuid::now_v7();
    let r = sqlx::query!(
        r#"
        INSERT INTO scheduled_reports (id, name, recipients, cadence)
        VALUES ($1, $2, $3, $4)
        RETURNING id, name, recipients, cadence, last_sent_at, created_at
        "#,
        id,
        input.name,
        &input.recipients,
        input.cadence,
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
) -> DbResult<ScheduledReport> {
    let cur = get(pool, id).await?;
    let r = sqlx::query!(
        r#"
        UPDATE scheduled_reports
        SET name = $2, recipients = $3, cadence = $4
        WHERE id = $1
        RETURNING id, name, recipients, cadence, last_sent_at, created_at
        "#,
        id.0,
        input.name.unwrap_or(cur.name),
        &input.recipients.unwrap_or(cur.recipients),
        input.cadence.unwrap_or(cur.cadence),
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

pub async fn delete(pool: &DbPool, id: ScheduledReportId) -> DbResult<()> {
    let r = sqlx::query!("DELETE FROM scheduled_reports WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Reports due to be sent: never sent, or `last_sent_at` older than 7
/// days. The scheduler calls this on its slow tick; the 7-day gate covers
/// the only cadence we support today (weekly). A row with no recipients is
/// still returned — the scheduler logs + skips it so a misconfigured
/// report is visible rather than silently never due.
pub async fn due(pool: &DbPool, now: OffsetDateTime) -> DbResult<Vec<ScheduledReport>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, recipients, cadence, last_sent_at, created_at
        FROM scheduled_reports
        WHERE last_sent_at IS NULL
           OR last_sent_at <= $1::timestamptz - interval '7 days'
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
