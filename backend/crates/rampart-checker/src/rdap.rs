//! RDAP probe (RFC 7480 + 9082).
//!
//! Modern HTTP-based successor to WHOIS. Each TLD registry runs an RDAP
//! server returning machine-readable JSON; `rdap.org` is a public
//! redirector that routes queries to the right registry by TLD. The
//! probe issues `GET <url>/<domain>` with `Accept: application/rdap+
//! json`, asserts a 200 response and a parseable RDAP payload, then
//! looks for an `eventAction = "expiration"` entry to surface days-
//! until-expiry.
//!
//! Three-state outcome:
//!
//! - **Up**   : 200, valid RDAP JSON, expiry (if present) is more than
//!   the configured warning window away.
//! - **Warn** : 200 + valid JSON, but the domain is inside the warning
//!   window (default 30 days) — the registration is real but about to
//!   lapse.
//! - **Down** : HTTP error, parse failure, or the registry returned a
//!   404 / 4xx for an unknown domain.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! {
//!   "domain":         "example.com",   // lookup target
//!   "warn_days":      30                // days-before-expiry → Warn
//! }
//! ```
//!
//! `monitor.url` carries the RDAP base path. Most users will want
//! `https://rdap.org/domain`, which 302s to the right per-TLD registry.
//! Operators querying a private RDAP service can point at it directly.
//!
//! `monitor.timeout_seconds` caps the HTTP request.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use reqwest::{redirect::Policy, Client};
use serde::Deserialize;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;
use tracing::warn;

#[derive(Deserialize)]
struct RdapResponse {
    #[serde(default, rename = "events")]
    events: Vec<RdapEvent>,
}

#[derive(Deserialize)]
struct RdapEvent {
    #[serde(default, rename = "eventAction")]
    action: String,
    #[serde(default, rename = "eventDate")]
    date: String,
}

pub struct RdapProbe {
    client: once_cell::sync::OnceCell<Client>,
}

impl RdapProbe {
    pub fn new() -> Self {
        Self {
            client: once_cell::sync::OnceCell::new(),
        }
    }

    fn client(&self) -> &Client {
        // `rdap.org` 302s to per-TLD registries; follow up to a few
        // hops so the probe lands on the authoritative server without
        // letting a misconfigured registry chain redirects forever.
        // Built via the SSRF-guarded resolver: the redirect target is
        // server-controlled, so a hostile/compromised registry could 302 to
        // 169.254.169.254 or an internal host — the guard vets each hop's
        // dialed IP.
        self.client.get_or_init(|| {
            crate::ssrf::guarded_client_builder()
                .redirect(Policy::limited(5))
                .pool_idle_timeout(Duration::from_secs(60))
                .tcp_keepalive(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build with default features")
        })
    }
}

impl Default for RdapProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for RdapProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let base = match monitor.url.as_deref() {
            Some(u) if !u.is_empty() => u.trim_end_matches('/'),
            _ => {
                return down(
                    monitor,
                    ts,
                    started,
                    "rdap monitor requires url (e.g. https://rdap.org/domain)",
                )
            }
        };

        let domain = monitor
            .config
            .get("domain")
            .and_then(|v| v.as_str())
            .unwrap_or("example.com");
        let warn_days = monitor
            .config
            .get("warn_days")
            .and_then(|v| v.as_i64())
            .unwrap_or(30);

        let url = format!("{base}/{domain}");

        let req = self
            .client()
            .get(&url)
            .header("accept", "application/rdap+json")
            .timeout(to);

        let resp = match timeout(to, req.send()).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("request: {e}")),
            Err(_) => return down(monitor, ts, started, "request timed out"),
        };

        let http_status = resp.status().as_u16() as i32;
        if !resp.status().is_success() {
            return down(monitor, ts, started, &format!("http {http_status}"));
        }

        let body: RdapResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!(monitor = %monitor.id, error = %e, "rdap body parse failed");
                return down(monitor, ts, started, &format!("body parse: {e}"));
            }
        };

        let elapsed = started.elapsed();
        let expiry = body
            .events
            .iter()
            .find(|e| e.action.eq_ignore_ascii_case("expiration"))
            .and_then(|e| parse_rfc3339(&e.date));

        match expiry {
            Some(when) => {
                let secs = (when - OffsetDateTime::now_utc()).whole_seconds();
                // Already past the expiration event → the domain is expired,
                // which is Down — not a soft Warn. (Previously `.max(0)` clamped
                // this to 0 days, so expired domains stayed Warn forever.)
                if secs <= 0 {
                    return down(monitor, ts, started, &format!("{domain} has expired"));
                }
                let days = secs / 86_400;
                if days <= warn_days {
                    Heartbeat {
                        monitor_id: monitor.id,
                        ts,
                        status: MonitorStatus::Warn,
                        latency_ms: Some(ms_i32(elapsed)),
                        status_code: Some(http_status),
                        msg: Some(format!("{domain} expires in {days}d (≤ {warn_days}d warn)")),
                        retries: 0,
                        important: false,
                    }
                } else {
                    Heartbeat {
                        monitor_id: monitor.id,
                        ts,
                        status: MonitorStatus::Up,
                        latency_ms: Some(ms_i32(elapsed)),
                        status_code: Some(http_status),
                        msg: Some(format!("{domain} expires in {days}d")),
                        retries: 0,
                        important: false,
                    }
                }
            }
            None => Heartbeat {
                // 200 + valid RDAP JSON but no expiration event surfaced
                // — some registries omit it. Treat as Up (the registry
                // responded with a real record) but flag in the msg.
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(elapsed)),
                status_code: Some(http_status),
                msg: Some(format!("{domain} · no expiry event in RDAP payload")),
                retries: 0,
                important: false,
            },
        }
    }
}

fn parse_rfc3339(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}

fn down(monitor: &Monitor, ts: OffsetDateTime, started: Instant, msg: &str) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts,
        status: MonitorStatus::Down,
        latency_ms: Some(ms_i32(started.elapsed())),
        status_code: None,
        msg: Some(msg.into()),
        retries: 0,
        important: false,
    }
}
