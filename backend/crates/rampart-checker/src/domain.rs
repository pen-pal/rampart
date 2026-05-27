//! Domain registration expiry probe.
//!
//! Speaks the WHOIS protocol directly — TCP/43, send `<domain>\r\n`,
//! read until EOF, parse for an expiry date. We avoid a dedicated
//! WHOIS client crate because none of them gracefully handle the
//! IANA-referral two-step that's required for nearly every modern TLD.
//!
//! Flow:
//! 1. Ask `whois.iana.org` which server is authoritative for the TLD.
//! 2. Connect to that server and ask about the actual domain.
//! 3. Pull "Registry Expiry Date" / "Expiration Date" / "Expiry Date"
//!    out of the response with a small set of regex-free string scans.
//!
//! Config keys:
//!
//! ```json
//! { "warn_days": 30 }    // Warn threshold; default 30
//! ```
//!
//! `monitor.hostname` is the bare domain (e.g. `example.com`).
//!
//! Heads up: this is a heuristic probe. Some ccTLDs return non-ISO date
//! formats; when we can't parse, we report Warn with the raw response
//! head rather than Down — the connect succeeded, so the domain at least
//! exists.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::format_description::well_known::Iso8601;
use time::macros::format_description;
use time::{Date, OffsetDateTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub struct DomainProbe;

impl DomainProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DomainProbe {
    fn default() -> Self {
        Self::new()
    }
}

const IANA_WHOIS: &str = "whois.iana.org";

#[async_trait]
impl Probe for DomainProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds.max(15) as u64);

        let Some(domain) = monitor.hostname.as_deref() else {
            return down(monitor, ts, started, "domain monitor requires a hostname (domain)");
        };
        let warn_days = monitor
            .config
            .get("warn_days")
            .and_then(|v| v.as_i64())
            .unwrap_or(30);

        // Step 1: discover authoritative WHOIS server via IANA.
        let tld = match tld_of(domain) {
            Some(t) => t,
            None => return down(monitor, ts, started, "cannot determine TLD"),
        };
        let referral_response = match query_whois(IANA_WHOIS, &tld, to).await {
            Ok(r) => r,
            Err(e) => return down(monitor, ts, started, &format!("iana whois: {e}")),
        };
        let whois_server = match scan_field(&referral_response, &["whois:", "refer:"]) {
            Some(s) => s,
            None => {
                return down(
                    monitor, ts, started,
                    &format!("no WHOIS server returned by IANA for .{tld}"),
                );
            }
        };

        // Step 2: ask the authoritative server about the domain itself.
        let response = match query_whois(&whois_server, domain, to).await {
            Ok(r) => r,
            Err(e) => return down(monitor, ts, started, &format!("whois lookup: {e}")),
        };

        // Step 3: pull expiry out of the response. Field names vary per
        // registry; this list covers gTLDs + the common ccTLDs.
        let expiry_field = scan_field(
            &response,
            &[
                "Registry Expiry Date:",
                "Registrar Registration Expiration Date:",
                "Expiration Date:",
                "Expiry Date:",
                "Expiration Time:",
                "paid-till:",
                "renewal date:",
            ],
        );

        let Some(raw) = expiry_field else {
            // Connect succeeded, but no field we recognise.
            // Surface as Warn rather than Down — the domain probably exists.
            return Heartbeat {
                monitor_id:  monitor.id,
                ts,
                status:      MonitorStatus::Warn,
                latency_ms:  Some(ms_i32(started.elapsed())),
                status_code: None,
                msg:         Some(format!(
                    "domain {domain}: WHOIS responded but no expiry field recognised"
                )),
                retries:     0,
                important:   false,
            };
        };

        let Some(expiry) = parse_expiry(&raw) else {
            return Heartbeat {
                monitor_id:  monitor.id,
                ts,
                status:      MonitorStatus::Warn,
                latency_ms:  Some(ms_i32(started.elapsed())),
                status_code: None,
                msg:         Some(format!(
                    "domain {domain}: could not parse expiry '{raw}'"
                )),
                retries:     0,
                important:   false,
            };
        };

        let now = OffsetDateTime::now_utc();
        let delta_days = (expiry - now).whole_days();
        if delta_days < 0 {
            down(
                monitor, ts, started,
                &format!("domain expired {} days ago", -delta_days),
            )
        } else if delta_days <= warn_days {
            Heartbeat {
                monitor_id:  monitor.id,
                ts,
                status:      MonitorStatus::Warn,
                latency_ms:  Some(ms_i32(started.elapsed())),
                status_code: None,
                msg:         Some(format!("expires in {delta_days}d")),
                retries:     0,
                important:   false,
            }
        } else {
            Heartbeat {
                monitor_id:  monitor.id,
                ts,
                status:      MonitorStatus::Up,
                latency_ms:  Some(ms_i32(started.elapsed())),
                status_code: None,
                msg:         Some(format!("{delta_days}d left")),
                retries:     0,
                important:   false,
            }
        }
    }
}

fn down(monitor: &Monitor, ts: OffsetDateTime, started: Instant, msg: &str) -> Heartbeat {
    Heartbeat {
        monitor_id:  monitor.id,
        ts,
        status:      MonitorStatus::Down,
        latency_ms:  Some(ms_i32(started.elapsed())),
        status_code: None,
        msg:         Some(msg.into()),
        retries:     0,
        important:   false,
    }
}

/// Last dot-segment; lowercased. "Foo.EXAMPLE.com" → Some("com").
fn tld_of(domain: &str) -> Option<String> {
    domain
        .trim_end_matches('.')
        .rsplit('.')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
}

/// One-shot WHOIS conversation. Some servers expect ASCII, some accept
/// UTF-8; we send raw bytes either way.
async fn query_whois(server: &str, query: &str, to: Duration) -> Result<String, String> {
    let addr = format!("{server}:43");
    let mut stream = timeout(to, TcpStream::connect(&addr))
        .await
        .map_err(|_| "connect timed out".to_string())?
        .map_err(|e| format!("connect to {server}: {e}"))?;

    let line = format!("{query}\r\n");
    timeout(to, stream.write_all(line.as_bytes()))
        .await
        .map_err(|_| "write timed out".to_string())?
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = Vec::with_capacity(4096);
    timeout(to, stream.read_to_end(&mut buf))
        .await
        .map_err(|_| "read timed out".to_string())?
        .map_err(|e| format!("read: {e}"))?;

    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Look for any of `keys` (case-insensitively) at the start of a line
/// and return the trimmed value. Returns the *first* match — registry
/// responses typically list the most authoritative field first.
fn scan_field(text: &str, keys: &[&str]) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        for key in keys {
            if trimmed.len() >= key.len()
                && trimmed[..key.len()].eq_ignore_ascii_case(key)
            {
                let rest = trimmed[key.len()..].trim();
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }
        }
    }
    None
}

/// Attempt to parse a WHOIS expiry timestamp. Tries:
/// - ISO 8601 with offset / Z
/// - `YYYY-MM-DD`
/// - `DD-MMM-YYYY` (some ccTLDs)
fn parse_expiry(raw: &str) -> Option<OffsetDateTime> {
    // Strip anything after the first space if it looks like a comment
    // (e.g. "2030-01-15T00:00:00Z (foo)").
    let trimmed = raw.trim();

    if let Ok(dt) = OffsetDateTime::parse(trimmed, &Iso8601::DEFAULT) {
        return Some(dt);
    }

    // Take the first whitespace-separated token for the looser formats.
    let token = trimmed.split_whitespace().next().unwrap_or(trimmed);

    if let Ok(d) = Date::parse(token, format_description!("[year]-[month]-[day]")) {
        return Some(d.midnight().assume_utc());
    }

    if let Ok(d) = Date::parse(token, format_description!("[day]-[month repr:short]-[year]")) {
        return Some(d.midnight().assume_utc());
    }

    None
}
