//! SQLite `metric_samples` domain — externally-pushed metric series (the read
//! foundation for metric_rules + slos). Mirrors the PG free-fn surface:
//! insert_many / list_series / range_query / baseline / latest /
//! prune_older_than.
//!
//! Dialect: jsonb labels → canonical-JSON TEXT (serde_json default sorts keys,
//! so TEXT `=` matches PG's semantic jsonb `=`); double→REAL; ts→INTEGER
//! unix-seconds. PG-isms translated: `UNNEST` insert → per-row tx (one stamped
//! `now`), `TO_TIMESTAMP(FLOOR(EXTRACT(EPOCH …)/step)*step)` bucket →
//! `(ts/step)*step` integer math, and `STDDEV_SAMP` (absent on SQLite) →
//! computed app-side from `SUM(value)` / `SUM(value*value)` / `COUNT`.

use super::ts;
use crate::metric_samples::{RangePoint, Series};
use crate::DbResult;
use rampart_core::ids::OrgId;
use rampart_core::promtext::PromSample;
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;

/// Canonical JSON text for a label set — the storage + match form. serde_json's
/// default Map sorts keys, so this is stable and TEXT `=` is semantic equality.
fn labels_text(v: &serde_json::Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "{}".into())
}

/// Bulk-insert one ingest payload, all rows stamped with a single `now`
/// (atomic-in-time, like PG's single NOW()).
pub async fn insert_many(pool: &SqlitePool, samples: &[PromSample], org_id: OrgId) -> DbResult<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let org = org_id.0.to_string();
    let mut tx = pool.begin().await?;
    for s in samples {
        let labels = serde_json::to_value(&s.labels).unwrap_or_else(|_| serde_json::json!({}));
        sqlx::query(
            "INSERT INTO metric_samples (name, labels, value, ts, org_id) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&s.name)
        .bind(labels_text(&labels))
        .bind(s.value)
        .bind(now)
        .bind(&org)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_series(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<Series>> {
    let rows = sqlx::query(
        "SELECT name, labels, MAX(ts) AS last_ts, COUNT(*) AS samples
         FROM metric_samples WHERE org_id = ?
         GROUP BY name, labels ORDER BY MAX(ts) DESC LIMIT 1000",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Series {
            name: r.get("name"),
            labels: serde_json::from_str(&r.get::<String, _>("labels")).unwrap_or_default(),
            last_ts: ts(r.get::<i64, _>("last_ts")),
            samples: r.get::<i64, _>("samples"),
        })
        .collect())
}

/// Bucketed range query. `labels` must equal the stored set (series identity).
/// Bucket = `(ts/step)*step`, aligned to the epoch, ascending.
pub async fn range_query(
    pool: &SqlitePool,
    name: &str,
    labels: &serde_json::Value,
    from: OffsetDateTime,
    to: OffsetDateTime,
    step_seconds: i64,
    org_id: OrgId,
) -> DbResult<Vec<RangePoint>> {
    let step = step_seconds.max(1);
    let sql = format!(
        "SELECT (ts / {step}) * {step} AS bucket,
                AVG(value) AS avg, MIN(value) AS min, MAX(value) AS max, COUNT(*) AS samples
         FROM metric_samples
         WHERE name = ? AND labels = ? AND ts >= ? AND ts <= ? AND org_id = ?
         GROUP BY bucket ORDER BY bucket"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(name)
        .bind(labels_text(labels))
        .bind(from.unix_timestamp())
        .bind(to.unix_timestamp())
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| RangePoint {
            bucket: ts(r.get::<i64, _>("bucket")),
            avg: r.get::<f64, _>("avg"),
            min: r.get::<f64, _>("min"),
            max: r.get::<f64, _>("max"),
            samples: r.get::<i64, _>("samples"),
        })
        .collect())
}

/// Rolling mean + sample stddev over the trailing window. `None` when < 2
/// samples (sample stddev undefined). SQLite has no `STDDEV_SAMP`, so it's
/// computed from the sum / sum-of-squares / count.
pub async fn baseline(
    pool: &SqlitePool,
    name: &str,
    labels: &serde_json::Value,
    window_secs: i64,
    org_id: OrgId,
) -> DbResult<Option<(f64, f64)>> {
    let since = OffsetDateTime::now_utc().unix_timestamp() - window_secs;
    let row = sqlx::query(
        "SELECT COUNT(*) AS n, COALESCE(SUM(value), 0) AS sum, COALESCE(SUM(value * value), 0) AS sumsq
         FROM metric_samples
         WHERE name = ? AND labels = ? AND ts >= ? AND org_id = ?",
    )
    .bind(name)
    .bind(labels_text(labels))
    .bind(since)
    .bind(org_id.0.to_string())
    .fetch_one(pool)
    .await?;
    let n = row.get::<i64, _>("n");
    if n < 2 {
        return Ok(None);
    }
    let sum = row.get::<f64, _>("sum");
    let sumsq = row.get::<f64, _>("sumsq");
    let nf = n as f64;
    let mean = sum / nf;
    // Sample variance: (Σx² − (Σx)²/n) / (n−1); clamp to 0 to absorb FP noise.
    let variance = ((sumsq - sum * sum / nf) / (nf - 1.0)).max(0.0);
    Ok(Some((mean, variance.sqrt())))
}

/// Latest sample for a series, with its timestamp. None when never reported.
pub async fn latest(
    pool: &SqlitePool,
    name: &str,
    labels: &serde_json::Value,
    org_id: OrgId,
) -> DbResult<Option<(f64, OffsetDateTime)>> {
    // Tie-break by rowid: SQLite `ts` is second-granular (PG was microsecond),
    // so same-second samples need the newest INSERT to win to stay "latest".
    let row = sqlx::query(
        "SELECT value, ts FROM metric_samples
         WHERE name = ? AND labels = ? AND org_id = ? ORDER BY ts DESC, rowid DESC LIMIT 1",
    )
    .bind(name)
    .bind(labels_text(labels))
    .bind(org_id.0.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get::<f64, _>("value"), ts(r.get::<i64, _>("ts")))))
}

/// Age-based prune. Returns rows deleted.
pub async fn prune_older_than(pool: &SqlitePool, cutoff: OffsetDateTime) -> DbResult<u64> {
    super::chunked_delete_older(pool, "metric_samples", "ts", cutoff.unix_timestamp()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn sample(name: &str, labels: &[(&str, &str)], value: f64) -> PromSample {
        PromSample {
            name: name.into(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<String, String>>(),
            value,
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn insert_series_latest_baseline_range(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let lbl = serde_json::json!({ "instance": "a", "job": "web" });

        insert_many(
            &pool,
            &[
                sample("q_depth", &[("instance", "a"), ("job", "web")], 10.0),
                sample("q_depth", &[("instance", "a"), ("job", "web")], 20.0),
                sample("q_depth", &[("instance", "a"), ("job", "web")], 30.0),
            ],
            org,
        )
        .await
        .unwrap();

        // one distinct series.
        let series = list_series(&pool, org).await.unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].samples, 3);

        // latest is the last-inserted value (same ts batch → any; assert present).
        let (v, _) = latest(&pool, "q_depth", &lbl, org).await.unwrap().unwrap();
        assert!([10.0, 20.0, 30.0].contains(&v));

        // baseline: mean 20, sample stddev of {10,20,30} = 10.
        let (mean, stddev) = baseline(&pool, "q_depth", &lbl, 3600, org)
            .await
            .unwrap()
            .unwrap();
        assert!((mean - 20.0).abs() < 1e-9, "mean {mean}");
        assert!((stddev - 10.0).abs() < 1e-9, "stddev {stddev}");

        // label mismatch → no data.
        let other_lbl = serde_json::json!({ "instance": "b" });
        assert!(latest(&pool, "q_depth", &other_lbl, org)
            .await
            .unwrap()
            .is_none());
        assert!(baseline(&pool, "q_depth", &other_lbl, 3600, org)
            .await
            .unwrap()
            .is_none());

        // range query buckets (wide step → one bucket, avg 20).
        let now = OffsetDateTime::now_utc();
        let pts = range_query(
            &pool,
            "q_depth",
            &lbl,
            now - time::Duration::hours(1),
            now + time::Duration::hours(1),
            3600,
            org,
        )
        .await
        .unwrap();
        assert_eq!(pts.len(), 1);
        assert!((pts[0].avg - 20.0).abs() < 1e-9);
        assert_eq!(pts[0].samples, 3);

        // prune everything older than the future → wipes the series.
        let deleted = prune_older_than(&pool, now + time::Duration::hours(2))
            .await
            .unwrap();
        assert_eq!(deleted, 3);
        assert!(list_series(&pool, org).await.unwrap().is_empty());
    }
}
