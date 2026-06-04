//! LDAP probe.
//!
//! Connects to a directory server over `ldap://` (plaintext) or
//! `ldaps://` (TLS), then runs a simple bind. Up when the bind
//! response carries `result_code == 0`; Down with the LDAP error
//! string otherwise. Both anonymous and authenticated binds are
//! supported via optional config.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! {
//!   "bind_dn":       "cn=admin,dc=example,dc=com",
//!   "bind_password": "secret"
//! }
//! ```
//!
//! If `bind_dn` is absent the probe runs an anonymous bind (DN ""
//! / password ""), which most directories accept and which is enough
//! to confirm the server is reachable and speaking LDAP. Adding
//! credentials moves the check from "is it up" to "does this account
//! still authenticate".
//!
//! `monitor.url` carries the target. `monitor.timeout_seconds` caps
//! the connect-plus-bind window.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use ldap3::LdapConnAsync;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct LdapProbe;

impl LdapProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LdapProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for LdapProbe {
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
                    "ldap monitor requires url (e.g. ldap://host:389)",
                )
            }
        };

        let bind_dn = monitor
            .config
            .get("bind_dn")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let bind_pw = monitor
            .config
            .get("bind_password")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let connect_fut = LdapConnAsync::new(url);
        let (conn, mut ldap) = match timeout(to, connect_fut).await {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("connect: {e}")),
            Err(_) => return down(monitor, ts, started, "connect timed out"),
        };
        // Drive the connection loop on a background task — `Ldap`
        // operations require it to be polled or they block forever.
        tokio::spawn(async move {
            let _ = conn.drive().await;
        });

        let bind_fut = ldap.simple_bind(bind_dn, bind_pw);
        let result = match timeout(to, bind_fut).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("bind: {e}")),
            Err(_) => return down(monitor, ts, started, "bind timed out"),
        };

        // `success()` consumes the LdapResult and returns Ok only when
        // `rc == 0`; non-zero codes (e.g. 49 Invalid Credentials, 32
        // No Such Object) come back as an Err with the textual reason
        // the server returned, which we surface verbatim in the
        // heartbeat msg for operator triage.
        match result.success() {
            Ok(_) => {
                let _ = ldap.unbind().await;
                let mode = if bind_dn.is_empty() {
                    "anonymous".to_string()
                } else {
                    format!("as {bind_dn}")
                };
                Heartbeat {
                    monitor_id: monitor.id,
                    ts,
                    status: MonitorStatus::Up,
                    latency_ms: Some(ms_i32(started.elapsed())),
                    status_code: None,
                    msg: Some(format!("bound {mode}")),
                    retries: 0,
                    important: false,
                }
            }
            Err(e) => {
                let _ = ldap.unbind().await;
                down(monitor, ts, started, &format!("bind rejected: {e}"))
            }
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
