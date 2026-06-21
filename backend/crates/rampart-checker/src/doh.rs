//! DNS-over-HTTPS probe (RFC 8484, JSON variant).
//!
//! Sends a GET to the DoH endpoint with `?name=<query>&type=<rtype>`
//! and the `Accept: application/dns-json` header that both major
//! providers (Cloudflare, Google) gate the JSON variant on. The
//! response shape both providers return is documented at
//! <https://developers.cloudflare.com/1.1.1.1/encryption/dns-over-https/make-api-requests/dns-json/>:
//!
//! ```json
//! {
//!   "Status": 0,                 // 0 = NOERROR, 3 = NXDOMAIN, ...
//!   "Answer": [
//!     { "name": "...", "type": 1, "TTL": 300, "data": "1.2.3.4" }
//!   ]
//! }
//! ```
//!
//! Probe is Up when `Status == 0` AND `Answer` is non-empty — i.e. the
//! resolver replied with at least one record. Status != 0 surfaces as
//! a Warn (Down with a soft-fail message would be confusing for a
//! resolver that is technically reachable but returning NXDOMAIN);
//! transport / HTTP errors are Down.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! {
//!   "query": "example.com",   // name to look up; default "example.com"
//!   "rtype": "A"              // record type; default "A"
//! }
//! ```
//!
//! `monitor.url` carries the DoH endpoint URL (e.g.
//! `https://cloudflare-dns.com/dns-query`).
//! `monitor.timeout_seconds` caps the request.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;
use tracing::warn;

#[derive(Deserialize)]
struct DohResponse {
    #[serde(rename = "Status")]
    status: i32,
    #[serde(rename = "Answer", default)]
    answer: Vec<serde_json::Value>,
}

pub struct DohProbe {
    client: once_cell::sync::OnceCell<Client>,
}

impl DohProbe {
    pub fn new() -> Self {
        Self {
            client: once_cell::sync::OnceCell::new(),
        }
    }

    fn client(&self) -> &Client {
        // SSRF-guarded: a DoH endpoint URL is operator-configured, but vet the
        // dialed IP anyway so a misconfig can't reach internal/link-local hosts.
        self.client.get_or_init(|| {
            crate::ssrf::guarded_client_builder()
                .pool_idle_timeout(Duration::from_secs(60))
                .tcp_keepalive(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build with default features")
        })
    }
}

impl Default for DohProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for DohProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let url = match monitor.url.as_deref() {
            Some(u) if !u.is_empty() => u,
            _ => {
                return down(
                    monitor,
                    ts,
                    started,
                    "doh monitor requires url (e.g. https://cloudflare-dns.com/dns-query)",
                )
            }
        };

        let query = monitor
            .config
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("example.com");
        let rtype = monitor
            .config
            .get("rtype")
            .and_then(|v| v.as_str())
            .unwrap_or("A");

        let req = self
            .client()
            .get(url)
            .query(&[("name", query), ("type", rtype)])
            .header("accept", "application/dns-json")
            .timeout(to);

        let outcome = match timeout(to, req.send()).await {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("request: {e}")),
            Err(_) => return down(monitor, ts, started, "request timed out"),
        };

        let http_status = outcome.status().as_u16() as i32;
        if !outcome.status().is_success() {
            return down(monitor, ts, started, &format!("http {http_status}"));
        }

        let body: DohResponse = match outcome.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!(monitor = %monitor.id, error = %e, "doh body parse failed");
                return down(monitor, ts, started, &format!("body parse: {e}"));
            }
        };

        let elapsed = started.elapsed();
        match (body.status, body.answer.is_empty()) {
            (0, false) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(elapsed)),
                status_code: Some(http_status),
                msg: Some(format!(
                    "NOERROR · {query} {rtype} · {} records",
                    body.answer.len()
                )),
                retries: 0,
                important: false,
            },
            (0, true) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                // NOERROR but empty answer = the resolver replied but
                // the name has no records of that type. Operationally
                // interesting (might be intentional; might be a config
                // drift) so emit Warn rather than Up or Down.
                status: MonitorStatus::Warn,
                latency_ms: Some(ms_i32(elapsed)),
                status_code: Some(http_status),
                msg: Some(format!("NOERROR · {query} {rtype} · empty answer")),
                retries: 0,
                important: false,
            },
            (rc, _) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Warn,
                latency_ms: Some(ms_i32(elapsed)),
                status_code: Some(http_status),
                msg: Some(format!("DNS rcode {rc} · {query} {rtype}")),
                retries: 0,
                important: false,
            },
        }
    }
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
