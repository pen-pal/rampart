//! DNS probe — resolve a record and assert the answer is non-empty.
//!
//! Driven by [`hickory-resolver`]. Config keys (all optional) read from
//! `monitor.config`:
//!
//! ```json
//! {
//!   "record_type": "A",         // A | AAAA | CNAME | MX | TXT | NS | SRV | CAA | SOA
//!   "resolver":    "1.1.1.1",   // override system default
//!   "expected":    "1.2.3.4"    // optional substring match against any answer
//! }
//! ```
//!
//! `monitor.hostname` is the name to resolve. `monitor.timeout_seconds`
//! caps the lookup.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use hickory_resolver::config::{NameServerConfigGroup, ResolverConfig, ResolverOpts};
use hickory_resolver::proto::rr::RecordType;
use hickory_resolver::TokioAsyncResolver;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::net::IpAddr;
use std::str::FromStr;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct DnsProbe;

impl DnsProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DnsProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for DnsProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let Some(hostname) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "dns monitor requires a hostname");
        };

        let cfg = monitor.config.as_object();
        let record_type = cfg
            .and_then(|c| c.get("record_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("A");
        let resolver_addr = cfg.and_then(|c| c.get("resolver")).and_then(|v| v.as_str());
        let expected = cfg.and_then(|c| c.get("expected")).and_then(|v| v.as_str());

        let rt = match parse_record_type(record_type) {
            Some(r) => r,
            None => {
                return down(
                    monitor,
                    ts,
                    started,
                    &format!("unsupported record_type {record_type}"),
                );
            }
        };

        let resolver = match build_resolver(resolver_addr) {
            Ok(r) => r,
            Err(e) => return down(monitor, ts, started, &format!("resolver init failed: {e}")),
        };

        let lookup_fut = resolver.lookup(hostname, rt);
        let result = match timeout(to, lookup_fut).await {
            Ok(r) => r,
            Err(_) => return down(monitor, ts, started, "dns lookup timed out"),
        };

        let answers = match result {
            Ok(r) => r,
            Err(e) => return down(monitor, ts, started, &format!("dns lookup failed: {e}")),
        };

        let rendered: Vec<String> = answers.iter().map(|r| r.to_string()).collect();
        if rendered.is_empty() {
            return down(monitor, ts, started, "dns answer was empty");
        }

        if let Some(needle) = expected {
            if !rendered.iter().any(|a| a.contains(needle)) {
                return down(
                    monitor,
                    ts,
                    started,
                    &format!(
                        "expected '{needle}' not found in {} answer(s)",
                        rendered.len()
                    ),
                );
            }
        }

        Heartbeat {
            monitor_id: monitor.id,
            ts,
            status: MonitorStatus::Up,
            latency_ms: Some(ms_i32(started.elapsed())),
            status_code: None,
            msg: Some(rendered.join(", ")),
            retries: 0,
            important: false,
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

fn parse_record_type(s: &str) -> Option<RecordType> {
    match s.to_ascii_uppercase().as_str() {
        "A" => Some(RecordType::A),
        "AAAA" => Some(RecordType::AAAA),
        "CNAME" => Some(RecordType::CNAME),
        "MX" => Some(RecordType::MX),
        "TXT" => Some(RecordType::TXT),
        "NS" => Some(RecordType::NS),
        "SRV" => Some(RecordType::SRV),
        "CAA" => Some(RecordType::CAA),
        "SOA" => Some(RecordType::SOA),
        _ => None,
    }
}

/// Build a resolver. If the user supplied a specific server, we use just
/// that one over UDP/53 — otherwise we fall back to the host's
/// /etc/resolv.conf style defaults. (The `system-config` feature in
/// hickory-resolver pulls in this fallback.)
fn build_resolver(addr: Option<&str>) -> Result<TokioAsyncResolver, String> {
    let opts = ResolverOpts::default();
    match addr {
        None => Ok(TokioAsyncResolver::tokio(ResolverConfig::default(), opts)),
        Some(s) => {
            let ip = IpAddr::from_str(s).map_err(|e| format!("invalid resolver IP: {e}"))?;
            let group = NameServerConfigGroup::from_ips_clear(&[ip], 53, true);
            Ok(TokioAsyncResolver::tokio(
                ResolverConfig::from_parts(None, vec![], group),
                opts,
            ))
        }
    }
}
