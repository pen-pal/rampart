//! Cassandra / ScyllaDB probe.
//!
//! Opens a CQL session to the listed node, runs `SELECT release_version
//! FROM system.local` so the probe exercises both the protocol-level
//! handshake (STARTUP → SUPPORTED → READY) AND a real query path. Up
//! when the query returns a row; Down on connect / auth / query error.
//!
//! `system.local` is guaranteed to exist on every Cassandra-compatible
//! node — it's the per-node bootstrap table — so the query works
//! against both Cassandra and ScyllaDB out of the box, regardless of
//! how the operator has carved up keyspaces.
//!
//! Config (all optional under `monitor.config`):
//! ```json
//! {
//!   "username": "rampart",   // PLAIN auth; omitted = no auth
//!   "password": "...",
//!   "tls":      true         // *currently rejected* — see below.
//! }
//! ```
//!
//! Hostname + port come off the monitor row; default port 9042 is
//! applied at the call site if the wizard preset took effect.
//! `monitor.timeout_seconds` caps the whole session-plus-query window.
//!
//! ## TLS status — deferred
//!
//! Plaintext CQL only today. Enabling TLS in the upstream `scylla`
//! crate requires the `rustls-023` feature, but that drags
//! `tokio-rustls = "0.26"` in *without* `default-features = false` —
//! Cargo's feature unification then flips `aws_lc_rs` on for the
//! workspace's shared `rustls v0.23`, dragging `aws-lc-rs` + `cmake`
//! into the build graph. That contradicts the pure-Rust / ring crypto
//! invariant the rest of the workspace upholds (see
//! docs/DEPENDENCIES.md). The probe therefore *rejects* a `tls: true`
//! config field with a clear error rather than silently downgrading or
//! pretending the toggle worked. See docs/SECURITY-DEBT.md "Pending
//! TLS gaps" — the entry will move to `## Progress` when upstream
//! scylla either gates `tokio-rustls` behind a `ring`-only feature or
//! lets callers thread their own `rustls::ClientConfig` through
//! `SessionBuilder` without dragging the provider crates.

use crate::{ms_i32, Probe};
use async_trait::async_trait;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use scylla::client::session_builder::SessionBuilder;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::time::timeout;

pub struct CassandraProbe;

impl CassandraProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CassandraProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Probe for CassandraProbe {
    async fn run(&self, monitor: &Monitor) -> Heartbeat {
        let started = Instant::now();
        let ts = OffsetDateTime::now_utc();
        let to = Duration::from_secs(monitor.timeout_seconds as u64);

        let host = match monitor.hostname.as_deref() {
            Some(h) if !h.is_empty() => h,
            _ => {
                return down(
                    monitor,
                    ts,
                    started,
                    "cassandra monitor requires hostname (plus optional port; default 9042)",
                )
            }
        };
        let port = monitor.port.unwrap_or(9042) as u16;
        let node = format!("{host}:{port}");

        // TLS is a deferred capability — fail loudly rather than
        // silently downgrade to plaintext when the operator asks for it.
        // The blocker is upstream (scylla's `rustls-023` feature pulls
        // a non-pure-Rust crypto provider via tokio-rustls 0.26 default
        // features); see the module doc-comment and
        // docs/SECURITY-DEBT.md "Pending TLS gaps".
        if monitor
            .config
            .get("tls")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return down(
                monitor,
                ts,
                started,
                "cassandra TLS is not yet supported — blocked on upstream scylla \
                 crate features (see docs/SECURITY-DEBT.md)",
            );
        }

        let mut builder = SessionBuilder::new().known_node(&node);

        if let (Some(u), Some(p)) = (
            monitor.config.get("username").and_then(|v| v.as_str()),
            monitor.config.get("password").and_then(|v| v.as_str()),
        ) {
            builder = builder.user(u, p);
        }

        let session = match timeout(to, builder.build()).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return down(monitor, ts, started, &format!("session: {e}")),
            Err(_) => return down(monitor, ts, started, "session timed out"),
        };

        let query_fut = session.query_unpaged("SELECT release_version FROM system.local", ());
        match timeout(to, query_fut).await {
            Ok(Ok(_)) => Heartbeat {
                monitor_id: monitor.id,
                ts,
                status: MonitorStatus::Up,
                latency_ms: Some(ms_i32(started.elapsed())),
                status_code: None,
                msg: Some(format!("connected to {node} · system.local queried")),
                retries: 0,
                important: false,
            },
            Ok(Err(e)) => down(monitor, ts, started, &format!("query: {e}")),
            Err(_) => down(monitor, ts, started, "query timed out"),
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

#[cfg(test)]
mod tests {
    use rampart_core::{Monitor, MonitorId, MonitorKind, MonitorStatus};
    use serde_json::json;
    use time::OffsetDateTime;

    /// Test fixture for a Cassandra monitor; mirrors
    /// `rampart_core::testing::sample_monitor` but produces a
    /// `MonitorKind::Cassandra` row with a hostname + port set rather
    /// than a URL. We instantiate it locally instead of pulling the
    /// `testing` feature into `rampart-checker`'s dev-dependencies —
    /// the workspace's `rampart-checker` crate doesn't currently
    /// depend on `rampart-core/testing`, so this keeps the dep graph
    /// untouched.
    fn cassandra_monitor() -> Monitor {
        let now = OffsetDateTime::now_utc();
        Monitor {
            id: MonitorId::new(),
            name: "cassandra.example.com".into(),
            kind: MonitorKind::Cassandra,
            url: None,
            hostname: Some("cassandra.example.com".into()),
            port: Some(9042),
            config: serde_json::Value::Null,
            interval_seconds: 60,
            retry_interval_sec: 60,
            max_retries: 0,
            timeout_seconds: 10,
            resend_interval_sec: 0,
            upside_down: false,
            http_method: "GET".into(),
            http_body: None,
            http_headers: None,
            accepted_statuses: vec![],
            follow_redirect: true,
            ignore_tls: false,
            proxy_id: None,
            push_token: None,
            last_push_at: None,
            active: true,
            current_status: MonitorStatus::Up,
            created_at: now,
            updated_at: now,
            tags: Vec::new(),
            cert_days_left: None,
            cert_subject: None,
            cert_checked_at: None,
            group_id: None,
            slo_target_pct: None,
            slo_window_days: None,
            agent_id: None,
        }
    }

    /// `config.tls = true` must be rejected with a clear error rather
    /// than silently downgrading. This test asserts the
    /// scheme-acceptance contract the module doc-comment promises: a
    /// future operator who flips the toggle gets a heartbeat that says
    /// "not yet supported" pointing them at SECURITY-DEBT.md, instead
    /// of a probe that connects in plaintext while the dashboard shows
    /// a closed padlock.
    #[tokio::test]
    async fn tls_config_field_is_rejected_until_upstream_fix() {
        use crate::Probe;
        let mut m = cassandra_monitor();
        m.config = json!({ "tls": true });
        let hb = super::CassandraProbe::new().run(&m).await;
        assert_eq!(hb.status, MonitorStatus::Down);
        let msg = hb.msg.unwrap_or_default();
        assert!(
            msg.contains("TLS is not yet supported"),
            "expected a TLS-not-supported error, got: {msg}"
        );
    }

    /// Mirror: with `tls` absent the probe proceeds to the real
    /// connect path. We don't have a live cluster in the test harness,
    /// so the heartbeat is Down — but the error message must be the
    /// network-level failure, NOT the "TLS not supported" gate. This
    /// guards against an over-eager refactor that accidentally trips
    /// the gate even when TLS wasn't requested.
    #[tokio::test]
    async fn plaintext_request_does_not_hit_the_tls_gate() {
        use crate::Probe;
        let mut m = cassandra_monitor();
        // Address that won't resolve / connect; we only check the
        // failure mode is "tried to connect", not "rejected up-front".
        m.hostname = Some("127.0.0.1".into());
        m.port = Some(1);
        m.timeout_seconds = 1;
        let hb = super::CassandraProbe::new().run(&m).await;
        assert_eq!(hb.status, MonitorStatus::Down);
        let msg = hb.msg.unwrap_or_default();
        assert!(
            !msg.contains("TLS is not yet supported"),
            "TLS gate fired without a tls=true config: {msg}"
        );
    }
}
