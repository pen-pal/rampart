//! Aggregate queries used by the Prometheus exposition at `/metrics`.
//!
//! Single-purpose: each function returns a small Vec of `(label, count)`
//! pairs cheap enough to compute on every scrape (typically 15-60s).
//! No caching layer — Postgres aggregates over the dashboard's typical
//! ~hundred-row monitor + channel tables in microseconds.
//!
//! These queries are deliberately read-only and parameter-free so the
//! `/metrics` endpoint can be hit by an unauthenticated Prometheus
//! scraper without leaking anything more than the dashboard's own
//! summary panel already exposes.

use crate::DbError;
use sqlx::PgPool;

/// `(monitor_status, count)` pairs across the `monitors` table.
/// Includes every `monitor_status` variant currently observed —
/// rows whose status is `pending` / `up` / `down` / `warn` / `paused`
/// / `maintenance` all surface, so the consumer can compute totals
/// without a separate query.
pub async fn monitors_by_status(pool: &PgPool) -> Result<Vec<(String, i64)>, DbError> {
    let rows = sqlx::query!(
        r#"
        SELECT current_status::text AS "status!", COUNT(*) AS "count!"
        FROM monitors
        GROUP BY current_status
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| (r.status, r.count)).collect())
}

/// `(monitor_kind, count)` pairs across the `monitors` table —
/// "how many of each probe kind is this deployment running".
pub async fn monitors_by_kind(pool: &PgPool) -> Result<Vec<(String, i64)>, DbError> {
    let rows = sqlx::query!(
        r#"
        SELECT kind::text AS "kind!", COUNT(*) AS "count!"
        FROM monitors
        GROUP BY kind
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| (r.kind, r.count)).collect())
}

/// Total active notification channels.
pub async fn channels_active(pool: &PgPool) -> Result<i64, DbError> {
    let row = sqlx::query!(r#"SELECT COUNT(*) AS "count!" FROM notifications WHERE active"#,)
        .fetch_one(pool)
        .await?;
    Ok(row.count)
}

/// Total web-push subscribers across all channels — useful to know
/// before pushing a fan-out notification touches.
pub async fn webpush_subscribers(pool: &PgPool) -> Result<i64, DbError> {
    let row = sqlx::query!(r#"SELECT COUNT(*) AS "count!" FROM webpush_subscriptions"#,)
        .fetch_one(pool)
        .await?;
    Ok(row.count)
}

/// `(monitor_status, count)` of heartbeats observed over the trailing
/// window (seconds before now). 24h is the default Prometheus scrape
/// horizon — the consumer can rate-aggregate as needed.
pub async fn heartbeats_recent_by_status(
    pool: &PgPool,
    window_seconds: i64,
) -> Result<Vec<(String, i64)>, DbError> {
    let rows = sqlx::query!(
        r#"
        SELECT status::text AS "status!", COUNT(*) AS "count!"
        FROM heartbeats
        WHERE ts >= NOW() - make_interval(secs => $1::double precision)
        GROUP BY status
        "#,
        window_seconds as f64,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| (r.status, r.count)).collect())
}

/// Open / unresolved incident count.
pub async fn incidents_open(pool: &PgPool) -> Result<i64, DbError> {
    let row = sqlx::query!(r#"SELECT COUNT(*) AS "count!" FROM incidents WHERE active"#,)
        .fetch_one(pool)
        .await?;
    Ok(row.count)
}

/// Alerting-pipeline gauges in one round-trip: how many alert rules of each
/// kind exist and how many are currently firing, plus open escalation episodes
/// and unresolved error issues. Scraped by `/metrics` so an operator can alert
/// on Rampart's own alerting (e.g. "no rules firing for 10m" or a stuck
/// escalation).
pub struct PipelineGauges {
    pub metric_rules: i64,
    pub metric_rules_firing: i64,
    pub telemetry_rules: i64,
    pub telemetry_rules_firing: i64,
    pub detection_rules_enabled: i64,
    pub detection_findings_open: i64,
    pub escalations_open: i64,
    pub error_issues_unresolved: i64,
}

pub async fn pipeline_gauges(pool: &PgPool) -> Result<PipelineGauges, DbError> {
    let row = sqlx::query!(
        r#"
        SELECT
          (SELECT COUNT(*) FROM metric_rules)                                  AS "metric_rules!",
          (SELECT COUNT(*) FROM metric_rules WHERE firing_at IS NOT NULL)      AS "metric_rules_firing!",
          (SELECT COUNT(*) FROM telemetry_alert_rules)                         AS "telemetry_rules!",
          (SELECT COUNT(*) FROM telemetry_alert_rules WHERE firing_at IS NOT NULL) AS "telemetry_rules_firing!",
          (SELECT COUNT(*) FROM detection_rules WHERE enabled)                 AS "detection_rules_enabled!",
          (SELECT COUNT(*) FROM detection_findings WHERE acknowledged_at IS NULL) AS "detection_findings_open!",
          (SELECT COUNT(*) FROM escalation_episodes WHERE resolved_at IS NULL) AS "escalations_open!",
          (SELECT COUNT(*) FROM error_issues WHERE status = 'unresolved')      AS "error_issues_unresolved!"
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(PipelineGauges {
        metric_rules: row.metric_rules,
        metric_rules_firing: row.metric_rules_firing,
        telemetry_rules: row.telemetry_rules,
        telemetry_rules_firing: row.telemetry_rules_firing,
        detection_rules_enabled: row.detection_rules_enabled,
        detection_findings_open: row.detection_findings_open,
        escalations_open: row.escalations_open,
        error_issues_unresolved: row.error_issues_unresolved,
    })
}

/// Telemetry-ingest volume over the trailing 24h, by tier — a freshness/load
/// signal for the observability pipeline. One round-trip.
pub struct IngestGauges {
    pub logs_24h: i64,
    pub spans_24h: i64,
    pub error_events_24h: i64,
}

/// On-disk size + estimated row count of the high-volume telemetry tables,
/// largest first. `bytes` is `pg_total_relation_size` (heap + indexes + TOAST);
/// `rows` is the planner's `reltuples` estimate (cheap — no full scan). Drives
/// the storage panel + `/metrics`, so an operator can see what's growing and
/// tune retention with data.
#[derive(serde::Serialize)]
pub struct TableSize {
    pub table: String,
    pub bytes: i64,
    pub rows: i64,
}

pub async fn storage_usage(pool: &PgPool) -> Result<Vec<TableSize>, DbError> {
    let tables: Vec<String> = [
        "heartbeats",
        "heartbeat_rollups",
        "spans",
        "logs",
        "error_events",
        "profiles",
        "rum_events",
        "metric_samples",
        "detection_findings",
        "audit_log",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let rows = sqlx::query!(
        r#"
        SELECT c.relname AS "table!",
               pg_total_relation_size(c.oid)::bigint AS "bytes!",
               c.reltuples::bigint AS "rows!"
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public' AND c.relkind = 'r' AND c.relname = ANY($1)
        ORDER BY pg_total_relation_size(c.oid) DESC
        "#,
        &tables,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| TableSize {
            table: r.table,
            bytes: r.bytes,
            rows: r.rows.max(0),
        })
        .collect())
}

pub async fn ingest_gauges(pool: &PgPool) -> Result<IngestGauges, DbError> {
    let row = sqlx::query!(
        r#"
        SELECT
          (SELECT COUNT(*) FROM logs   WHERE ts >= now() - interval '24 hours')          AS "logs_24h!",
          (SELECT COUNT(*) FROM spans  WHERE received_at >= now() - interval '24 hours')  AS "spans_24h!",
          (SELECT COUNT(*) FROM error_events WHERE ts >= now() - interval '24 hours')     AS "error_events_24h!"
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(IngestGauges {
        logs_24h: row.logs_24h,
        spans_24h: row.spans_24h,
        error_events_24h: row.error_events_24h,
    })
}
