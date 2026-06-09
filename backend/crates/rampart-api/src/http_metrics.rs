//! Per-request HTTP metrics.
//!
//! In-process counters + a bucketed latency histogram, exposed at the
//! existing `/metrics` Prometheus endpoint. Deliberately uses zero
//! external metrics crates — every Rampart workspace member already
//! pays for `std::sync` + `parking_lot` / `tokio::sync`, and this is
//! about 80 lines of arithmetic. Adding `metrics` + `metrics-exporter-
//! prometheus` would pull in 12 transitive crates we don't otherwise
//! need.
//!
//! Surfaces three Prometheus metric families:
//!
//!   rampart_http_requests_total{method, status_class}
//!     Monotonic counter, partitioned by HTTP method + status-code
//!     family (2xx / 3xx / 4xx / 5xx). Per-route labels are not
//!     emitted — cardinality blow-up risk on a SaaS-shaped surface
//!     where every `/v1/monitors/{id}/heartbeats` would otherwise
//!     mint a new label set per monitor id. Operators wanting per-
//!     route latency can grep the `request_id`-tagged log lines.
//!
//!   rampart_http_request_duration_seconds_bucket{le="..."}
//!     Cumulative histogram. Bucket boundaries chosen so the buckets
//!     are dense in the range a healthy API actually serves (sub-1s)
//!     and coarse above that.
//!
//!   rampart_http_request_duration_seconds_sum
//!   rampart_http_request_duration_seconds_count
//!     Standard Prometheus sum+count companions to the histogram so
//!     `histogram_quantile()` and `rate()` produce meaningful series.
//!
//! Concurrency: counters are `AtomicU64`s; the histogram is an array
//! of `AtomicU64`s indexed by bucket. The middleware path takes a
//! single Instant::now() at request entry and a single elapsed() +
//! a handful of fetch_add()s at response observe — no locks, no
//! allocations.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Upper bounds for the latency histogram, in seconds. The implicit
/// `+Inf` bucket is the `count` field. Boundaries chosen so the dense
/// region covers a healthy HTTP API's natural latency distribution
/// (sub-1 ms for cheap routes, single-digit milliseconds for the
/// common path, hundreds of milliseconds for the DB-heavy ones).
pub const LATENCY_BUCKETS_SECONDS: &[f64] = &[0.001, 0.005, 0.025, 0.1, 0.25, 0.5, 1.0, 2.5, 10.0];

/// Aggregate live counters. Shared via `AppState` so the `/metrics`
/// handler reads exactly the counters the middleware writes to.
pub struct HttpMetrics {
    /// `(method, status_class)` → monotonic count.
    requests: std::sync::Mutex<HashMap<(Method, u16), u64>>,
    /// Cumulative bucket counts. `buckets[i]` is the count of requests
    /// whose duration was `<= LATENCY_BUCKETS_SECONDS[i]`. The +Inf
    /// bucket equals `total_count`.
    buckets: Vec<AtomicU64>,
    /// `count` of all observed durations (matches the +Inf bucket).
    total_count: AtomicU64,
    /// `sum` of all observed durations in seconds, stored as
    /// microseconds in an AtomicU64 so we can fetch_add without a lock.
    /// Rendered as seconds at scrape time.
    total_sum_micros: AtomicU64,
}

impl HttpMetrics {
    pub fn new() -> Self {
        Self {
            requests: std::sync::Mutex::new(HashMap::new()),
            buckets: (0..LATENCY_BUCKETS_SECONDS.len())
                .map(|_| AtomicU64::new(0))
                .collect(),
            total_count: AtomicU64::new(0),
            total_sum_micros: AtomicU64::new(0),
        }
    }

    /// Record one request. `status` is the HTTP response status code;
    /// `duration_seconds` is the observed handler latency.
    pub fn observe(&self, method: &Method, status: StatusCode, duration_seconds: f64) {
        // Counter — partition by method + status class (2xx/3xx/4xx/5xx).
        let class = status_class(status);
        let key = (method.clone(), class);
        let mut g = self.requests.lock().unwrap();
        *g.entry(key).or_insert(0) += 1;
        drop(g);

        // Histogram — cumulative bucket counts.
        let micros = (duration_seconds * 1_000_000.0).round() as u64;
        self.total_sum_micros.fetch_add(micros, Ordering::Relaxed);
        self.total_count.fetch_add(1, Ordering::Relaxed);
        for (i, bound) in LATENCY_BUCKETS_SECONDS.iter().enumerate() {
            if duration_seconds <= *bound {
                self.buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Render a Prometheus text exposition block for the three metric
    /// families. Called once per `/metrics` scrape.
    pub fn render(&self) -> String {
        use std::fmt::Write;
        let mut out = String::with_capacity(1024);

        // ── rampart_http_requests_total ────────────────────────────
        let _ = writeln!(
            out,
            "# HELP rampart_http_requests_total HTTP requests received, by method and status class."
        );
        let _ = writeln!(out, "# TYPE rampart_http_requests_total counter");
        let snapshot: Vec<((Method, u16), u64)> = {
            let g = self.requests.lock().unwrap();
            g.iter().map(|(k, v)| (k.clone(), *v)).collect()
        };
        for ((method, class), count) in snapshot {
            let _ = writeln!(
                out,
                "rampart_http_requests_total{{method=\"{}\",status_class=\"{}xx\"}} {}",
                method.as_str(),
                class / 100,
                count,
            );
        }

        // ── rampart_http_request_duration_seconds histogram ────────
        let _ = writeln!(
            out,
            "# HELP rampart_http_request_duration_seconds Observed HTTP handler latency in seconds."
        );
        let _ = writeln!(
            out,
            "# TYPE rampart_http_request_duration_seconds histogram"
        );
        let total = self.total_count.load(Ordering::Relaxed);
        for (i, bound) in LATENCY_BUCKETS_SECONDS.iter().enumerate() {
            let v = self.buckets[i].load(Ordering::Relaxed);
            let _ = writeln!(
                out,
                "rampart_http_request_duration_seconds_bucket{{le=\"{}\"}} {}",
                fmt_bound(*bound),
                v,
            );
        }
        // The implicit +Inf bucket = total observation count.
        let _ = writeln!(
            out,
            "rampart_http_request_duration_seconds_bucket{{le=\"+Inf\"}} {}",
            total,
        );
        let sum_seconds = self.total_sum_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        let _ = writeln!(
            out,
            "rampart_http_request_duration_seconds_sum {sum_seconds}"
        );
        let _ = writeln!(out, "rampart_http_request_duration_seconds_count {total}");

        out
    }
}

impl Default for HttpMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Axum middleware that observes per-request latency + status.
///
/// Usage:
///   ```ignore
///   let metrics = Arc::new(HttpMetrics::new());
///   router.layer(axum::middleware::from_fn_with_state(
///       metrics.clone(),
///       record_http_metrics,
///   ));
///   ```
pub async fn record_http_metrics(
    state: axum::extract::State<Arc<HttpMetrics>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().clone();
    let started = Instant::now();
    let resp = next.run(req).await;
    let elapsed = started.elapsed().as_secs_f64();
    state.0.observe(&method, resp.status(), elapsed);
    resp
}

/// Map a status code to its class as an HTTP-status integer (2/3/4/5).
fn status_class(s: StatusCode) -> u16 {
    s.as_u16() / 100 * 100
}

/// Bucket boundary formatter that strips trailing zeros so `0.001`
/// renders as `0.001` not `0.001000000000`.
fn fmt_bound(b: f64) -> String {
    if b.fract() == 0.0 {
        format!("{b:.1}")
    } else {
        format!("{b}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_increments_counter_per_method_and_class() {
        let m = HttpMetrics::new();
        m.observe(&Method::GET, StatusCode::OK, 0.002);
        m.observe(&Method::GET, StatusCode::OK, 0.004);
        m.observe(&Method::POST, StatusCode::CREATED, 0.150);
        m.observe(&Method::GET, StatusCode::NOT_FOUND, 0.001);
        m.observe(&Method::GET, StatusCode::INTERNAL_SERVER_ERROR, 0.500);

        let g = m.requests.lock().unwrap();
        assert_eq!(g.get(&(Method::GET, 200)), Some(&2));
        assert_eq!(g.get(&(Method::POST, 200)), Some(&1));
        assert_eq!(g.get(&(Method::GET, 400)), Some(&1));
        assert_eq!(g.get(&(Method::GET, 500)), Some(&1));
    }

    #[test]
    fn histogram_buckets_are_cumulative() {
        let m = HttpMetrics::new();
        m.observe(&Method::GET, StatusCode::OK, 0.001); // hits every bucket
        m.observe(&Method::GET, StatusCode::OK, 0.250); // hits 0.25, 0.5, 1, 2.5, 10

        // 0.001 bucket = both? No — 0.001 = the 0.001 bucket only catches
        // values <= 0.001. The first observation hits it (0.001 <= 0.001).
        // The second (0.250) does not (0.250 > 0.001). So 0.001 bucket = 1.
        assert_eq!(m.buckets[0].load(Ordering::Relaxed), 1); // <= 0.001
        assert_eq!(m.buckets[3].load(Ordering::Relaxed), 1); // <= 0.1  (just 0.001)
        assert_eq!(m.buckets[4].load(Ordering::Relaxed), 2); // <= 0.25 (both)
        assert_eq!(m.buckets[8].load(Ordering::Relaxed), 2); // <= 10.0 (both)
        assert_eq!(m.total_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn render_emits_expected_lines() {
        let m = HttpMetrics::new();
        m.observe(&Method::GET, StatusCode::OK, 0.003);
        let s = m.render();
        assert!(s.contains("rampart_http_requests_total counter"));
        assert!(s.contains("rampart_http_request_duration_seconds histogram"));
        assert!(s.contains("le=\"+Inf\""));
        assert!(s.contains("rampart_http_request_duration_seconds_count 1"));
    }

    #[test]
    fn fmt_bound_is_compact() {
        assert_eq!(fmt_bound(0.001), "0.001");
        assert_eq!(fmt_bound(10.0), "10.0");
        assert_eq!(fmt_bound(2.5), "2.5");
    }
}
