//! MySQL `metric_samples` domain — externally-pushed metric series (the read
//! foundation for metric_rules + slos). Mirrors the PG/SQLite free-fn surface:
//! insert_many / list_series / range_query / baseline / latest /
//! prune_older_than.
//!
//! Dialect: jsonb labels → canonical-JSON TEXT in a `utf8mb4_bin` column so `=`
//! and `GROUP BY` are byte-exact (series identity); double→DOUBLE; ts→BIGINT.
//! `UNNEST` insert → per-row tx (one stamped `now`); `(ts/step)*step` bucket →
//! `(ts DIV step)*step` integer math; `STDDEV_SAMP` computed app-side from
//! `SUM(value)` / `SUM(value*value)` / `COUNT` (identical to SQLite). AVG/SUM of
//! a DOUBLE column return DOUBLE (not DECIMAL), so no `* 1e0` is needed here.

use super::ts;
use crate::metric_samples::{RangePoint, Series};
use crate::DbResult;
use rampart_core::ids::OrgId;
use rampart_core::promtext::PromSample;
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

/// Canonical JSON text for a label set — the storage + match form (the
/// `utf8mb4_bin` column makes TEXT `=` byte-exact semantic equality). We sort
/// object keys explicitly (via `BTreeMap`) rather than relying on serde_json's
/// default Map ordering: a future Mongo/bson backend would pull in serde_json's
/// `preserve_order` feature, flipping it on for ALL serde_json users via
/// feature-unification — at which point `to_string` would emit insertion order
/// and `labels = ?` matching would SILENTLY miss, killing metric reads and
/// metric-rule / SLO evaluation. The explicit sort is correct regardless.
fn labels_text(v: &serde_json::Value) -> String {
    serde_json::to_string(&canonicalize(v)).unwrap_or_else(|_| "{}".into())
}

/// Recursively rebuild a JSON value with object keys in sorted order. `BTreeMap`
/// iterates sorted, so `to_value` re-inserts in that order regardless of
/// serde_json's Map backing — making the serialized form backing-independent.
fn canonicalize(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(m) => {
            let sorted: std::collections::BTreeMap<String, serde_json::Value> = m
                .iter()
                .map(|(k, val)| (k.clone(), canonicalize(val)))
                .collect();
            serde_json::to_value(sorted).unwrap_or_else(|_| serde_json::json!({}))
        }
        serde_json::Value::Array(a) => {
            serde_json::Value::Array(a.iter().map(canonicalize).collect())
        }
        other => other.clone(),
    }
}

/// Bulk-insert one ingest payload, all rows stamped with a single `now`.
pub async fn insert_many(pool: &MySqlPool, samples: &[PromSample], org_id: OrgId) -> DbResult<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let org = org_id.0.to_string();
    let mut tx = pool.begin().await?;
    for batch in samples.chunks(super::insert_chunk(5)) {
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO metric_samples (name, labels, value, ts, org_id) ",
        );
        qb.push_values(batch, |mut b, s| {
            let labels = serde_json::to_value(&s.labels).unwrap_or_else(|_| serde_json::json!({}));
            b.push_bind(&s.name)
                .push_bind(labels_text(&labels))
                .push_bind(s.value)
                .push_bind(now)
                .push_bind(&org);
        });
        qb.build().execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_series(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<Series>> {
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
/// Bucket = `(ts DIV step)*step`, aligned to the epoch, ascending.
pub async fn range_query(
    pool: &MySqlPool,
    name: &str,
    labels: &serde_json::Value,
    from: OffsetDateTime,
    to: OffsetDateTime,
    step_seconds: i64,
    org_id: OrgId,
) -> DbResult<Vec<RangePoint>> {
    let step = step_seconds.max(1);
    let sql = format!(
        "SELECT (ts DIV {step}) * {step} AS bucket,
                AVG(value) AS avg_v, MIN(value) AS min_v, MAX(value) AS max_v, COUNT(*) AS samples
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
            avg: r.get::<f64, _>("avg_v"),
            min: r.get::<f64, _>("min_v"),
            max: r.get::<f64, _>("max_v"),
            samples: r.get::<i64, _>("samples"),
        })
        .collect())
}

/// Rolling mean + sample stddev over the trailing window. `None` when < 2
/// samples. Computed from sum / sum-of-squares / count (no `STDDEV_SAMP`).
pub async fn baseline(
    pool: &MySqlPool,
    name: &str,
    labels: &serde_json::Value,
    window_secs: i64,
    org_id: OrgId,
) -> DbResult<Option<(f64, f64)>> {
    let since = OffsetDateTime::now_utc().unix_timestamp() - window_secs;
    let row = sqlx::query(
        "SELECT COUNT(*) AS n,
                COALESCE(SUM(value), 0.0) AS sum_v,
                COALESCE(SUM(value * value), 0.0) AS sumsq
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
    let sum = row.get::<f64, _>("sum_v");
    let sumsq = row.get::<f64, _>("sumsq");
    let nf = n as f64;
    let mean = sum / nf;
    // Sample variance: (Σx² − (Σx)²/n) / (n−1); clamp to 0 to absorb FP noise.
    let variance = ((sumsq - sum * sum / nf) / (nf - 1.0)).max(0.0);
    Ok(Some((mean, variance.sqrt())))
}

/// Latest sample for a series. Tie-break by `id` (newest INSERT wins) since `ts`
/// is second-granular and a batch shares one `now`.
pub async fn latest(
    pool: &MySqlPool,
    name: &str,
    labels: &serde_json::Value,
    org_id: OrgId,
) -> DbResult<Option<(f64, OffsetDateTime)>> {
    let row = sqlx::query(
        "SELECT value, ts FROM metric_samples
         WHERE name = ? AND labels = ? AND org_id = ? ORDER BY ts DESC, id DESC LIMIT 1",
    )
    .bind(name)
    .bind(labels_text(labels))
    .bind(org_id.0.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get::<f64, _>("value"), ts(r.get::<i64, _>("ts")))))
}

/// Age-based prune. Returns rows deleted.
pub async fn prune_older_than(pool: &MySqlPool, cutoff: OffsetDateTime) -> DbResult<u64> {
    super::chunked_delete_older(pool, "metric_samples", "ts", cutoff.unix_timestamp()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    /// Canonical label text is recursively key-sorted so insert and every
    /// `labels = ?` read agree regardless of key order or serde_json's Map
    /// backing — guards against a future `preserve_order` flip silently breaking
    /// metric/SLO matching.
    #[test]
    fn labels_text_sorts_keys_canonically() {
        let out = labels_text(&serde_json::json!({ "z": "1", "a": "2", "m": { "y": 1, "x": 2 } }));
        assert_eq!(out, r#"{"a":"2","m":{"x":2,"y":1},"z":"1"}"#);
    }

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

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn insert_series_latest_baseline_range(pool: MySqlPool) {
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

        // latest present (same-ts batch → id tie-break picks the newest INSERT).
        let (v, _) = latest(&pool, "q_depth", &lbl, org).await.unwrap().unwrap();
        assert_eq!(v, 30.0);

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

        // case-sensitive series identity: "A" must NOT match "a" (utf8mb4_bin).
        insert_many(
            &pool,
            &[sample(
                "q_depth",
                &[("instance", "A"), ("job", "web")],
                99.0,
            )],
            org,
        )
        .await
        .unwrap();
        assert_eq!(list_series(&pool, org).await.unwrap().len(), 2);
        let (va, _) = latest(
            &pool,
            "q_depth",
            &serde_json::json!({ "instance": "A", "job": "web" }),
            org,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(va, 99.0);

        // range query buckets (wide step → one bucket, avg 20 over the 3 lc rows).
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

        // prune everything older than the future → wipes all series.
        let deleted = prune_older_than(&pool, now + time::Duration::hours(2))
            .await
            .unwrap();
        assert_eq!(deleted, 4);
        assert!(list_series(&pool, org).await.unwrap().is_empty());
    }
}
