//! Real User Monitoring — browser beacon model + read aggregates.
//!
//! The RUM snippet (see `docs/design/RUM.md`) sends one [`RumBeacon`] per page
//! view: the URL, an optional app/session, and the Core Web Vitals + load
//! timings it collected. The read side reports p75 per metric (the standard
//! Web Vitals statistic). Beacon parsing is `serde`-driven; [`RumBeacon::clean`]
//! bounds the string fields. Pure + unit-tested.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const MAX_URL: usize = 2048;
const MAX_APP: usize = 120;
const MAX_UA: usize = 400;
const MAX_SESSION: usize = 120;

/// Core Web Vitals + navigation timings for one page view (any subset; `None`
/// when the browser didn't measure it). Milliseconds, except `cls` (unitless).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct RumMetrics {
    #[serde(default)]
    pub lcp: Option<f64>,
    #[serde(default)]
    pub fcp: Option<f64>,
    #[serde(default)]
    pub cls: Option<f64>,
    #[serde(default)]
    pub inp: Option<f64>,
    #[serde(default)]
    pub ttfb: Option<f64>,
    #[serde(default)]
    pub load: Option<f64>,
}

/// A page-view beacon from the browser snippet.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RumBeacon {
    #[serde(default = "default_app")]
    pub app: String,
    pub url: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub ua: Option<String>,
    /// Active backend trace id at page load, if the page propagated one — lets
    /// the RUM view deep-link to the trace waterfall.
    #[serde(default)]
    pub trace_id: Option<String>,
    /// Application user identifier (best-effort, set by the page after login) —
    /// answers "who experienced this".
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub metrics: RumMetrics,
}

fn default_app() -> String {
    "web".to_string()
}

impl RumBeacon {
    /// Validate + bound the beacon. Returns `None` for a useless beacon (no
    /// URL and no metric at all). String fields are truncated; a blank `app`
    /// falls back to "web".
    pub fn clean(mut self) -> Option<RumBeacon> {
        self.url = self.url.trim().chars().take(MAX_URL).collect();
        if self.url.is_empty() {
            return None;
        }
        self.app = {
            let a: String = self.app.trim().chars().take(MAX_APP).collect();
            if a.is_empty() {
                default_app()
            } else {
                a
            }
        };
        self.ua = self.ua.map(|s| s.chars().take(MAX_UA).collect());
        self.session = self.session.map(|s| s.chars().take(MAX_SESSION).collect());
        // Trace ids are 32 hex chars; bound generously and drop blanks.
        self.trace_id = self.trace_id.and_then(|s| {
            let t: String = s.trim().chars().take(64).collect();
            (!t.is_empty()).then_some(t)
        });
        self.user_id = self.user_id.and_then(|s| {
            let u: String = s.trim().chars().take(MAX_SESSION).collect();
            (!u.is_empty()).then_some(u)
        });
        // Drop a beacon with no usable signal at all.
        let m = &self.metrics;
        let has_metric = [m.lcp, m.fcp, m.cls, m.inp, m.ttfb, m.load]
            .iter()
            .any(Option::is_some);
        if !has_metric {
            return None;
        }
        Some(self)
    }
}

/// p75 of each metric over a window (the Web Vitals reporting statistic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RumVitals {
    pub views: i64,
    pub lcp_p75: Option<f64>,
    pub fcp_p75: Option<f64>,
    pub cls_p75: Option<f64>,
    pub inp_p75: Option<f64>,
    pub ttfb_p75: Option<f64>,
    pub load_p75: Option<f64>,
}

/// Per-URL rollup for the pages table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RumPage {
    pub url: String,
    pub views: i64,
    pub lcp_p75: Option<f64>,
    pub inp_p75: Option<f64>,
    pub cls_p75: Option<f64>,
    pub last_seen: OffsetDateTime,
}

/// A recent page-load that carried a backend trace id — the RUM → trace pivot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RumTracedLoad {
    pub url: String,
    pub trace_id: String,
    pub load_ms: Option<f64>,
    pub ts: OffsetDateTime,
}

/// Core Web Vitals "good" thresholds (p75 ≤ value is good). Exposed so the
/// dashboard and any alerting agree on the boundaries.
pub fn cwv_good_threshold(metric: &str) -> Option<f64> {
    match metric {
        "lcp" => Some(2500.0),
        "inp" => Some(200.0),
        "cls" => Some(0.1),
        "fcp" => Some(1800.0),
        "ttfb" => Some(800.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: serde_json::Value) -> Option<RumBeacon> {
        serde_json::from_value::<RumBeacon>(v)
            .ok()
            .and_then(RumBeacon::clean)
    }

    #[test]
    fn parses_and_cleans_beacon() {
        let b = parse(json!({
            "app": "storefront", "url": "/checkout", "session": "s1",
            "metrics": { "lcp": 2400.0, "cls": 0.05, "inp": 180.0, "ttfb": 300.0 }
        }))
        .expect("valid");
        assert_eq!(b.app, "storefront");
        assert_eq!(b.url, "/checkout");
        assert_eq!(b.metrics.lcp, Some(2400.0));
        assert_eq!(b.metrics.load, None);
    }

    #[test]
    fn defaults_app_to_web() {
        let b = parse(json!({ "url": "/", "metrics": { "lcp": 1000.0 } })).unwrap();
        assert_eq!(b.app, "web");
    }

    #[test]
    fn rejects_useless_beacons() {
        // No URL.
        assert!(parse(json!({ "url": "", "metrics": { "lcp": 1.0 } })).is_none());
        // No metrics at all.
        assert!(parse(json!({ "url": "/x" })).is_none());
        assert!(parse(json!({ "url": "/x", "metrics": {} })).is_none());
    }

    #[test]
    fn truncates_overlong_url() {
        let long = "/".to_string() + &"a".repeat(5000);
        let b = parse(json!({ "url": long, "metrics": { "lcp": 1.0 } })).unwrap();
        assert!(b.url.chars().count() <= MAX_URL);
    }

    #[test]
    fn cwv_thresholds() {
        assert_eq!(cwv_good_threshold("lcp"), Some(2500.0));
        assert_eq!(cwv_good_threshold("cls"), Some(0.1));
        assert_eq!(cwv_good_threshold("nope"), None);
    }
}
