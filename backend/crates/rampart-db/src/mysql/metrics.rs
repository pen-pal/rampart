//! MySQL `metrics` domain — the parameter-free `/metrics` Prometheus
//! exposition aggregates. Ported from PG. Read-only, cheap-per-scrape counts.
//!
//! Dialect: enum→TEXT columns group as-is; `make_interval(secs=>?)` →
//! Rust-computed cutoff; the 8-subquery `pipeline_gauges` + 3-subquery
//! `ingest_gauges` port verbatim (scalar subqueries are standard SQL);
//! `storage_usage` swaps PG's `pg_total_relation_size`/`reltuples` for
//! `information_schema.TABLES` (`DATA_LENGTH+INDEX_LENGTH`, `TABLE_ROWS`),
//! scoped to the connection's database. bool→`= 1`.

use super::in_placeholders;
use crate::metrics::{IngestGauges, PipelineGauges, TableSize};
use crate::DbResult;
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

async fn label_counts(pool: &MySqlPool, sql: &'static str) -> DbResult<Vec<(String, i64)>> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| (r.get::<String, _>(0), r.get::<i64, _>(1)))
        .collect())
}

pub async fn monitors_by_status(pool: &MySqlPool) -> DbResult<Vec<(String, i64)>> {
    label_counts(
        pool,
        "SELECT current_status, COUNT(*) FROM monitors GROUP BY current_status",
    )
    .await
}

pub async fn monitors_by_kind(pool: &MySqlPool) -> DbResult<Vec<(String, i64)>> {
    label_counts(pool, "SELECT kind, COUNT(*) FROM monitors GROUP BY kind").await
}

async fn count(pool: &MySqlPool, sql: &'static str) -> DbResult<i64> {
    let (n,): (i64,) = sqlx::query_as(sql).fetch_one(pool).await?;
    Ok(n)
}

pub async fn channels_active(pool: &MySqlPool) -> DbResult<i64> {
    count(pool, "SELECT COUNT(*) FROM notifications WHERE active = 1").await
}

pub async fn webpush_subscribers(pool: &MySqlPool) -> DbResult<i64> {
    count(pool, "SELECT COUNT(*) FROM webpush_subscriptions").await
}

pub async fn heartbeats_recent_by_status(
    pool: &MySqlPool,
    window_seconds: i64,
) -> DbResult<Vec<(String, i64)>> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - window_seconds.max(0);
    let rows = sqlx::query("SELECT status, COUNT(*) FROM heartbeats WHERE ts >= ? GROUP BY status")
        .bind(cutoff)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get::<String, _>(0), r.get::<i64, _>(1)))
        .collect())
}

pub async fn incidents_open(pool: &MySqlPool) -> DbResult<i64> {
    count(pool, "SELECT COUNT(*) FROM incidents WHERE active = 1").await
}

pub async fn pipeline_gauges(pool: &MySqlPool) -> DbResult<PipelineGauges> {
    let r = sqlx::query(
        "SELECT
           (SELECT COUNT(*) FROM metric_rules)                                      AS metric_rules,
           (SELECT COUNT(*) FROM metric_rules WHERE firing_at IS NOT NULL)          AS metric_rules_firing,
           (SELECT COUNT(*) FROM telemetry_alert_rules)                             AS telemetry_rules,
           (SELECT COUNT(*) FROM telemetry_alert_rules WHERE firing_at IS NOT NULL) AS telemetry_rules_firing,
           (SELECT COUNT(*) FROM detection_rules WHERE enabled = 1)                 AS detection_rules_enabled,
           (SELECT COUNT(*) FROM detection_findings WHERE acknowledged_at IS NULL)  AS detection_findings_open,
           (SELECT COUNT(*) FROM escalation_episodes WHERE resolved_at IS NULL)     AS escalations_open,
           (SELECT COUNT(*) FROM error_issues WHERE status = 'unresolved')          AS error_issues_unresolved",
    )
    .fetch_one(pool)
    .await?;
    Ok(PipelineGauges {
        metric_rules: r.get::<i64, _>("metric_rules"),
        metric_rules_firing: r.get::<i64, _>("metric_rules_firing"),
        telemetry_rules: r.get::<i64, _>("telemetry_rules"),
        telemetry_rules_firing: r.get::<i64, _>("telemetry_rules_firing"),
        detection_rules_enabled: r.get::<i64, _>("detection_rules_enabled"),
        detection_findings_open: r.get::<i64, _>("detection_findings_open"),
        escalations_open: r.get::<i64, _>("escalations_open"),
        error_issues_unresolved: r.get::<i64, _>("error_issues_unresolved"),
    })
}

pub async fn ingest_gauges(pool: &MySqlPool) -> DbResult<IngestGauges> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - 24 * 3600;
    let r = sqlx::query(
        "SELECT
           (SELECT COUNT(*) FROM logs WHERE ts >= ?)              AS logs_24h,
           (SELECT COUNT(*) FROM spans WHERE received_at >= ?)    AS spans_24h,
           (SELECT COUNT(*) FROM error_events WHERE ts >= ?)      AS error_events_24h",
    )
    .bind(cutoff)
    .bind(cutoff)
    .bind(cutoff)
    .fetch_one(pool)
    .await?;
    Ok(IngestGauges {
        logs_24h: r.get::<i64, _>("logs_24h"),
        spans_24h: r.get::<i64, _>("spans_24h"),
        error_events_24h: r.get::<i64, _>("error_events_24h"),
    })
}

pub async fn storage_usage(pool: &MySqlPool) -> DbResult<Vec<TableSize>> {
    let tables = [
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
    ];
    // PG's pg_total_relation_size/reltuples → information_schema, scoped to the
    // connection's database. CAST to SIGNED so the UNSIGNED size columns decode
    // as i64. TABLE_ROWS is an estimate (like reltuples) — fine for the panel.
    let sql = format!(
        "SELECT TABLE_NAME AS tbl,
                CAST(COALESCE(DATA_LENGTH, 0) + COALESCE(INDEX_LENGTH, 0) AS SIGNED) AS bytes,
                CAST(COALESCE(TABLE_ROWS, 0) AS SIGNED) AS rows_est
         FROM information_schema.TABLES
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME IN ({})
         ORDER BY bytes DESC",
        in_placeholders(tables.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for t in tables {
        q = q.bind(t);
    }
    let rows = q.fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| TableSize {
            table: r.get::<String, _>("tbl"),
            bytes: r.get::<i64, _>("bytes"),
            rows: r.get::<i64, _>("rows_est").max(0),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn scrape_gauges(pool: MySqlPool) {
        let org = super::super::oid(DEF);

        // empty DB: every gauge is a clean zero/empty (the boot-scrape case).
        assert!(monitors_by_status(&pool).await.unwrap().is_empty());
        assert_eq!(channels_active(&pool).await.unwrap(), 0);
        assert_eq!(webpush_subscribers(&pool).await.unwrap(), 0);
        assert_eq!(incidents_open(&pool).await.unwrap(), 0);
        assert!(heartbeats_recent_by_status(&pool, 86_400)
            .await
            .unwrap()
            .is_empty());
        let pg = pipeline_gauges(&pool).await.unwrap();
        assert_eq!(pg.metric_rules, 0);
        assert_eq!(pg.error_issues_unresolved, 0);
        let ig = ingest_gauges(&pool).await.unwrap();
        assert_eq!(ig.logs_24h, 0);
        // storage_usage lists the telemetry tables that exist (all empty here).
        assert!(!storage_usage(&pool).await.unwrap().is_empty());

        // one monitor + one active channel move the needle.
        super::super::monitors::create(
            &pool,
            serde_json::from_value(serde_json::json!({
                "name": "m", "kind": "http", "url": "https://x",
                "interval_seconds": 60, "timeout_seconds": 10, "max_retries": 0,
                "retry_interval_sec": 60, "resend_interval_sec": 0, "upside_down": false,
                "http_method": "GET", "accepted_statuses": [200],
                "follow_redirect": true, "ignore_tls": false, "check_cert": false,
                "cert_expiry_days": 14
            }))
            .unwrap(),
            org,
        )
        .await
        .unwrap();
        super::super::notifications::create(
            &pool,
            serde_json::from_value(serde_json::json!({
                "kind": "webhook", "name": "c", "config": {"url": "https://e.com/h"}
            }))
            .unwrap(),
            org,
        )
        .await
        .unwrap();
        assert_eq!(
            monitors_by_status(&pool).await.unwrap(),
            vec![("pending".to_string(), 1)]
        );
        assert_eq!(
            monitors_by_kind(&pool).await.unwrap(),
            vec![("http".to_string(), 1)]
        );
        assert_eq!(channels_active(&pool).await.unwrap(), 1);
    }
}
