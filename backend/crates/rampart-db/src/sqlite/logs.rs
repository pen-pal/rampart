//! SQLite `logs` domain — log storage + filtered reads (telemetry foundation
//! for detection + telemetry_rules). Mirrors the PG free-fn surface: insert_logs
//! / query_logs / level_counts / histogram / list_services / prune.
//!
//! Dialect: uuid→TEXT, smallint→INTEGER, jsonb attributes→TEXT, ts→INTEGER
//! unix-seconds. PG-isms translated: `UNNEST` insert → per-row tx;
//! `make_interval(hours=>?)`→`unixepoch() - hours*3600`; `date_bin(step, ts,
//! origin)`→`origin + ((ts-origin)/step)*step`; `COUNT(*) FILTER`→`SUM(CASE …)`;
//! row-value keyset `(ts,id) < (subquery)` ports as-is (SQLite row values).
//! **Full-text `body_tsv @@ websearch_to_tsquery` degrades to a `LIKE`
//! substring match** — SQLite has no tsvector; phrase/OR/negation are lost
//! (homelab tier).

use super::{raw_uuid, ts};
use crate::logs::{LogBucket, LogFilter};
use crate::DbResult;
use rampart_core::ids::OrgId;
use rampart_core::log::{coarse_level, ParsedLog};
use rampart_core::LogEntry;
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;
use uuid::Uuid;

fn log_from(r: &sqlx::sqlite::SqliteRow) -> LogEntry {
    let severity = r.get::<i64, _>("severity") as i16;
    LogEntry {
        id: raw_uuid(&r.get::<String, _>("id")),
        ts: ts(r.get::<i64, _>("ts")),
        severity,
        level: coarse_level(severity).to_string(),
        severity_text: r.get("severity_text"),
        service_name: r.get("service_name"),
        body: r.get("body"),
        trace_id: r.get("trace_id"),
        span_id: r.get("span_id"),
        attributes: r
            .get::<Option<String>, _>("attributes")
            .and_then(|s| serde_json::from_str(&s).ok()),
    }
}

/// Bulk-insert log records (one tx; PG uses UNNEST). `ts` from the OTLP event
/// nanos (→ unix seconds), falling back to now. Returns rows written.
pub async fn insert_logs(pool: &SqlitePool, logs: &[ParsedLog], org_id: OrgId) -> DbResult<u64> {
    if logs.is_empty() {
        return Ok(0);
    }
    let org = org_id.0.to_string();
    let mut tx = pool.begin().await?;
    let mut n = 0u64;
    for l in logs {
        let ts_unix = if l.time_ns > 0 {
            l.time_ns / 1_000_000_000
        } else {
            OffsetDateTime::now_utc().unix_timestamp()
        };
        sqlx::query(
            "INSERT INTO logs
                (id, ts, severity, severity_text, service_name, body, trace_id, span_id,
                 attributes, org_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(ts_unix)
        .bind(l.severity)
        .bind(&l.severity_text)
        .bind(&l.service_name)
        .bind(&l.body)
        .bind(&l.trace_id)
        .bind(&l.span_id)
        .bind(serde_json::to_string(&l.attributes).unwrap_or_else(|_| "null".into()))
        .bind(&org)
        .execute(&mut *tx)
        .await?;
        n += 1;
    }
    tx.commit().await?;
    Ok(n)
}

/// Recent logs matching the filter, newest first, org-scoped. Body search is a
/// `LIKE` substring match (the SQLite FTS degradation).
pub async fn query_logs(
    pool: &SqlitePool,
    f: LogFilter<'_>,
    org_id: OrgId,
) -> DbResult<Vec<LogEntry>> {
    let limit = f.limit.clamp(1, 1000);
    let hours_cutoff = f
        .hours
        .map(|h| OffsetDateTime::now_utc().unix_timestamp() - h.clamp(1, 720) as i64 * 3600);
    let before = f.before_id.map(|b| b.to_string());
    let org = org_id.0.to_string();
    let sql = "SELECT id, ts, severity, severity_text, service_name, body, trace_id, span_id,
                      attributes
               FROM logs
               WHERE (? IS NULL OR service_name = ?)
                 AND (? IS NULL OR severity >= ?)
                 AND (? IS NULL OR body LIKE '%' || ? || '%')
                 AND (? IS NULL OR trace_id = ?)
                 AND (? IS NULL OR span_id = ?)
                 AND (? IS NULL OR received_at > ?)
                 AND (? IS NULL OR (ts, id) < (SELECT ts, id FROM logs WHERE id = ? AND org_id = ?))
                 AND org_id = ?
               ORDER BY ts DESC, id DESC
               LIMIT ?";
    let rows = sqlx::query(sql)
        .bind(f.service)
        .bind(f.service)
        .bind(f.min_severity)
        .bind(f.min_severity)
        .bind(f.query)
        .bind(f.query)
        .bind(f.trace_id)
        .bind(f.trace_id)
        .bind(f.span_id)
        .bind(f.span_id)
        .bind(hours_cutoff)
        .bind(hours_cutoff)
        .bind(&before)
        .bind(&before)
        .bind(&org)
        .bind(&org)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(log_from).collect())
}

/// Count per coarse level over the last `hours`, optionally one service.
pub async fn level_counts(
    pool: &SqlitePool,
    service: Option<&str>,
    hours: i32,
    org_id: OrgId,
) -> DbResult<Vec<(String, i64)>> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - hours.clamp(1, 720) as i64 * 3600;
    let rows = sqlx::query(
        "SELECT severity, COUNT(*) AS count FROM logs
         WHERE received_at > ? AND (? IS NULL OR service_name = ?) AND org_id = ?
         GROUP BY severity",
    )
    .bind(cutoff)
    .bind(service)
    .bind(service)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    let mut acc: std::collections::BTreeMap<&'static str, i64> = std::collections::BTreeMap::new();
    for r in &rows {
        *acc.entry(coarse_level(r.get::<i64, _>("severity") as i16))
            .or_default() += r.get::<i64, _>("count");
    }
    Ok(acc.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// Time-bucketed log volume (total + error≥17), oldest first.
pub async fn histogram(
    pool: &SqlitePool,
    service: Option<&str>,
    min_severity: Option<i16>,
    query: Option<&str>,
    hours: i32,
    buckets: i64,
    org_id: OrgId,
) -> DbResult<Vec<LogBucket>> {
    let hours = hours.clamp(1, 720);
    let buckets = buckets.clamp(2, 200);
    let step = ((hours as i64 * 3600) / buckets).max(1);
    let origin = OffsetDateTime::now_utc().unix_timestamp() - hours as i64 * 3600;
    // origin/step are server-computed i64 → safe to inline into the bucket expr.
    let sql = format!(
        "SELECT {origin} + ((ts - {origin}) / {step}) * {step} AS bucket,
                COUNT(*) AS total,
                SUM(CASE WHEN severity >= 17 THEN 1 ELSE 0 END) AS errors
         FROM logs
         WHERE received_at > {origin}
           AND (? IS NULL OR service_name = ?)
           AND (? IS NULL OR severity >= ?)
           AND (? IS NULL OR body LIKE '%' || ? || '%')
           AND org_id = ?
         GROUP BY bucket ORDER BY bucket"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(service)
        .bind(service)
        .bind(min_severity)
        .bind(min_severity)
        .bind(query)
        .bind(query)
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| LogBucket {
            ts: ts(r.get::<i64, _>("bucket")),
            total: r.get::<i64, _>("total"),
            errors: r.get::<i64, _>("errors"),
        })
        .collect())
}

/// Distinct services seen in the last 7 days (filter dropdown).
pub async fn list_services(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<String>> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - 7 * 86400;
    let rows = sqlx::query(
        "SELECT DISTINCT service_name FROM logs
         WHERE received_at > ? AND org_id = ? ORDER BY service_name LIMIT 500",
    )
    .bind(cutoff)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| r.get::<String, _>("service_name"))
        .collect())
}

/// Delete logs older than `days`. Returns rows removed. (PG chunks the delete
/// to avoid locking the high-volume table; SQLite is single-file — a plain
/// delete is fine.)
pub async fn prune(pool: &SqlitePool, days: i32) -> DbResult<u64> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - days.max(0) as i64 * 86400;
    super::chunked_delete_older(pool, "logs", "received_at", cutoff).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn log(service: &str, severity: i16, body: &str, trace: Option<&str>) -> ParsedLog {
        ParsedLog {
            time_ns: 0,
            severity,
            severity_text: None,
            service_name: service.into(),
            body: body.into(),
            trace_id: trace.map(|t| t.into()),
            span_id: None,
            attributes: serde_json::json!({}),
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn insert_query_counts_histogram(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let n = insert_logs(
            &pool,
            &[
                log("api", 9, "user logged in", Some("trace-1")), // info
                log("api", 17, "boom: null deref", None),         // error
                log("worker", 13, "queue slow", None),            // warn
            ],
            org,
        )
        .await
        .unwrap();
        assert_eq!(n, 3);

        // unfiltered recent.
        let all = query_logs(
            &pool,
            LogFilter {
                limit: 100,
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(all.len(), 3);

        // service filter.
        let api = query_logs(
            &pool,
            LogFilter {
                service: Some("api"),
                limit: 100,
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(api.len(), 2);

        // min_severity warn+ (>=13).
        let warn_plus = query_logs(
            &pool,
            LogFilter {
                min_severity: Some(13),
                limit: 100,
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(warn_plus.len(), 2);

        // body LIKE search (FTS degradation).
        let boom = query_logs(
            &pool,
            LogFilter {
                query: Some("boom"),
                limit: 100,
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(boom.len(), 1);
        assert!(boom[0].body.contains("boom"));

        // trace pivot.
        let traced = query_logs(
            &pool,
            LogFilter {
                trace_id: Some("trace-1"),
                limit: 100,
                ..Default::default()
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(traced.len(), 1);

        // level counts: 1 info, 1 warn, 1 error.
        let counts = level_counts(&pool, None, 24, org).await.unwrap();
        let total: i64 = counts.iter().map(|(_, c)| c).sum();
        assert_eq!(total, 3);

        // histogram totals + error split.
        let hist = histogram(&pool, None, None, None, 24, 24, org)
            .await
            .unwrap();
        let h_total: i64 = hist.iter().map(|b| b.total).sum();
        let h_err: i64 = hist.iter().map(|b| b.errors).sum();
        assert_eq!(h_total, 3);
        assert_eq!(h_err, 1);

        assert_eq!(
            list_services(&pool, org).await.unwrap(),
            vec!["api".to_string(), "worker".to_string()]
        );

        // prune(7d): backdate the worker row 10 days → only it is removed.
        sqlx::query(
            "UPDATE logs SET received_at = unixepoch() - 10 * 86400 WHERE service_name = 'worker'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(prune(&pool, 7).await.unwrap(), 1);
        assert_eq!(
            query_logs(
                &pool,
                LogFilter {
                    limit: 100,
                    ..Default::default()
                },
                org
            )
            .await
            .unwrap()
            .len(),
            2
        );
    }
}
