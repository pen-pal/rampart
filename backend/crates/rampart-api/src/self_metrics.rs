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
        let (count, sum_micros) = metrics.snapshot();
        let dcount = count.saturating_sub(prev.0);
        let dsum = sum_micros.saturating_sub(prev.1);
        prev = (count, sum_micros);

        let rps = dcount as f64 / INTERVAL_SECS as f64;
        let avg_ms = if dcount > 0 { (dsum as f64 / dcount as f64) / 1000.0 } else { 0.0 };

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
        if let Err(e) = rampart_db::metric_samples::insert_many(&pool, &samples).await {
            tracing::warn!(error = %e, "self-metrics insert failed");
        }
    }
}
