//! External metric sample store (migration 0072).
//!
//! Distinct from `metrics.rs`, which aggregates Rampart's OWN state for
//! the `/metrics` Prometheus exposition — this module stores samples
//! pushed in from outside via POST /v1/metrics/ingest.

use crate::DbResult;
use rampart_core::promtext::PromSample;
use sqlx::PgPool;
use time::OffsetDateTime;

/// Bulk-insert one ingest payload, server-stamped with a single NOW() so
/// a batch is atomic in time as well as in transaction.
pub async fn insert_many(
    pool: &PgPool,
    samples: &[PromSample],
    org_id: rampart_core::ids::OrgId,
) -> DbResult<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = samples.iter().map(|s| s.name.as_str()).collect();
    let labels: Vec<serde_json::Value> = samples
        .iter()
        .map(|s| serde_json::to_value(&s.labels).unwrap_or_else(|_| serde_json::json!({})))
        .collect();
    let values: Vec<f64> = samples.iter().map(|s| s.value).collect();

    sqlx::query!(
        r#"
        INSERT INTO metric_samples (name, labels, value, ts, org_id)
        SELECT * FROM UNNEST(
            $1::text[], $2::jsonb[], $3::float8[],
            ARRAY_FILL(NOW(), ARRAY[$4::int]),
            ARRAY_FILL($5::uuid, ARRAY[$4::int])
        )
        "#,
        &names as &[&str],
        &labels,
        &values,
        samples.len() as i32,
        org_id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// One known series: a name + distinct label set, with freshness and a
/// rough sample count to size the explorer UI.
pub struct Series {
    pub name: String,
    pub labels: serde_json::Value,
    pub last_ts: OffsetDateTime,
    pub samples: i64,
}

/// Every distinct (name, labels) series, newest-activity first. Bounded:
/// the GROUP BY runs over the name+ts index and homelab cardinality.
pub async fn list_series(pool: &PgPool) -> DbResult<Vec<Series>> {
    let rows = sqlx::query!(
        r#"
        SELECT name, labels AS "labels!", MAX(ts) AS "last_ts!", COUNT(*) AS "samples!"
        FROM metric_samples
        GROUP BY name, labels
        ORDER BY MAX(ts) DESC
        LIMIT 1000
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Series {
            name: r.name,
            labels: r.labels,
            last_ts: r.last_ts,
            samples: r.samples,
        })
        .collect())
}

/// One bucket of a range query.
pub struct RangePoint {
    pub bucket: OffsetDateTime,
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    pub samples: i64,
}

/// Bucketed range query for one series. `labels` must equal the stored
/// label set exactly (series identity, not a matcher). Buckets are
/// `step_seconds`-wide, aligned to the epoch, ascending.
pub async fn range_query(
    pool: &PgPool,
    name: &str,
    labels: &serde_json::Value,
    from: OffsetDateTime,
    to: OffsetDateTime,
    step_seconds: i64,
) -> DbResult<Vec<RangePoint>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            TO_TIMESTAMP(FLOOR(EXTRACT(EPOCH FROM ts) / $5) * $5) AS "bucket!",
            AVG(value)  AS "avg!",
            MIN(value)  AS "min!",
            MAX(value)  AS "max!",
            COUNT(*)    AS "samples!"
        FROM metric_samples
        WHERE name = $1 AND labels = $2 AND ts >= $3 AND ts <= $4
        GROUP BY 1
        ORDER BY 1
        "#,
        name,
        labels,
        from,
        to,
        step_seconds as f64,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| RangePoint {
            bucket: r.bucket,
            avg: r.avg,
            min: r.min,
            max: r.max,
            samples: r.samples,
        })
        .collect())
}

/// Rolling baseline (mean + sample stddev) for a series over the trailing
/// `window_secs` — the anomaly op's read. None when the window has fewer than
/// two samples (sample stddev is undefined).
pub async fn baseline(
    pool: &PgPool,
    name: &str,
    labels: &serde_json::Value,
    window_secs: i64,
) -> DbResult<Option<(f64, f64)>> {
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs);
    let row = sqlx::query!(
        r#"
        SELECT AVG(value) AS "mean", STDDEV_SAMP(value) AS "stddev", COUNT(*) AS "n!"
        FROM metric_samples
        WHERE name = $1 AND labels = $2 AND ts >= $3
        "#,
        name,
        labels,
        since,
    )
    .fetch_one(pool)
    .await?;
    match (row.n, row.mean, row.stddev) {
        (n, Some(mean), Some(stddev)) if n >= 2 => Ok(Some((mean, stddev))),
        _ => Ok(None),
    }
}

/// Latest sample for one series, with its age — the threshold-rule
/// evaluator's read. None when the series has never reported.
pub async fn latest(
    pool: &PgPool,
    name: &str,
    labels: &serde_json::Value,
) -> DbResult<Option<(f64, OffsetDateTime)>> {
    let row = sqlx::query!(
        r#"
        SELECT value, ts
        FROM metric_samples
        WHERE name = $1 AND labels = $2
        ORDER BY ts DESC
        LIMIT 1
        "#,
        name,
        labels,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.value, r.ts)))
}

/// Age-based prune, mirroring heartbeat retention. Returns rows deleted.
pub async fn prune_older_than(pool: &PgPool, cutoff: OffsetDateTime) -> DbResult<u64> {
    let res = sqlx::query!("DELETE FROM metric_samples WHERE ts < $1", cutoff)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}
