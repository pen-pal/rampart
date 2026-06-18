//! Self-ingest of Rampart's own HTTP metrics into the metric tier.
//!
//! Rampart exposes its runtime counters at the Prometheus `/metrics` scrape
//! endpoint, but the in-app Metrics *view* only shows metrics pushed into the
//! tier — so without this, an operator sees nothing of Rampart's own live
//! traffic there. This task snapshots the HTTP counters once a minute, deltas
//! them into a request rate + mean latency, and inserts them as
//! `rampart_http_*` series (labelled `service=rampart`) so the Metrics view
//! shows live self-metrics alongside external ones.

use crate::http_metrics::HttpMetrics;
use rampart_core::promtext::PromSample;
use rampart_db::DbPool;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const INTERVAL_SECS: u64 = 60;

pub async fn run(metrics: Arc<HttpMetrics>, pool: DbPool) {
    let mut prev = metrics.snapshot();
    let mut tick = tokio::time::interval(Duration::from_secs(INTERVAL_SECS));
    tick.tick().await; // consume the immediate first tick

    loop {
        tick.tick().await;
        let cur = metrics.snapshot();
        let (rps, avg_ms) = derive(prev, cur, INTERVAL_SECS);
        prev = cur;

        let mut labels = BTreeMap::new();
        labels.insert("service".to_string(), "rampart".to_string());
        let samples = vec![
            PromSample {
                name: "rampart_http_requests_per_sec".to_string(),
                labels: labels.clone(),
                value: rps,
            },
            PromSample {
                name: "rampart_http_latency_ms_avg".to_string(),
                labels,
                value: avg_ms,
            },
        ];
        // Rampart's own internal metrics: the operator/Default org by design —
        // this is a system background task with no tenant credential to resolve.
        if let Err(e) = rampart_db::metric_samples::insert_many(
            &pool,
            &samples,
            rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID),
        )
        .await
        {
            tracing::warn!(error = %e, "self-metrics insert failed");
        }
    }
}

/// Derive `(requests_per_sec, mean_latency_ms)` from two `(count, sum_micros)`
/// snapshots `interval_secs` apart. Counters are monotonic, but use saturating
/// deltas so a process restart (counters reset to 0) yields 0, never a negative
/// or wild value.
fn derive(prev: (u64, u64), cur: (u64, u64), interval_secs: u64) -> (f64, f64) {
    let dcount = cur.0.saturating_sub(prev.0);
    let dsum = cur.1.saturating_sub(prev.1);
    let rps = if interval_secs > 0 { dcount as f64 / interval_secs as f64 } else { 0.0 };
    let avg_ms = if dcount > 0 { (dsum as f64 / dcount as f64) / 1000.0 } else { 0.0 };
    (rps, avg_ms)
}

#[cfg(test)]
mod tests {
    use super::derive;

    #[test]
    fn rate_and_mean_from_deltas() {
        // 120 requests over 60s = 2 rps; 12_000_000 micros / 120 = 100_000 micros
        // = 100ms mean.
        let (rps, avg_ms) = derive((0, 0), (120, 12_000_000), 60);
        assert_eq!(rps, 2.0);
        assert_eq!(avg_ms, 100.0);
    }

    #[test]
    fn no_requests_in_window_is_zero() {
        let (rps, avg_ms) = derive((500, 9_000_000), (500, 9_000_000), 60);
        assert_eq!(rps, 0.0);
        assert_eq!(avg_ms, 0.0); // no division by zero
    }

    #[test]
    fn counter_reset_saturates_to_zero() {
        // Process restarted: cur < prev. Saturating deltas → 0, not negative.
        let (rps, avg_ms) = derive((1000, 50_000_000), (5, 200_000), 60);
        assert_eq!(rps, 0.0);
        assert_eq!(avg_ms, 0.0);
    }
}
