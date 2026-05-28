//! Fixtures for use in unit + integration tests across the workspace.
//!
//! Available behind the `testing` feature so production binaries don't
//! pay the (tiny) cost of carrying them. Each helper returns a sane,
//! valid value that downstream tests can mutate.

use crate::{Heartbeat, Monitor, MonitorId, MonitorKind, MonitorStatus};
use time::OffsetDateTime;

/// A valid `Monitor` shaped like a basic HTTP check. Override any field
/// the test cares about; the rest are sane defaults.
pub fn sample_monitor() -> Monitor {
    let now = OffsetDateTime::now_utc();
    Monitor {
        id: MonitorId::new(),
        name: "test.example.com".into(),
        kind: MonitorKind::Http,
        url: Some("https://test.example.com/health".into()),
        hostname: None,
        port: None,
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
        accepted_statuses: vec![200],
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
        cert_days_left:  None,
        cert_subject:    None,
        cert_checked_at: None,
        group_id: None,
    }
}

/// A successful HTTP heartbeat for the given monitor.
pub fn sample_heartbeat_up(monitor: &Monitor) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts: OffsetDateTime::now_utc(),
        status: MonitorStatus::Up,
        latency_ms: Some(123),
        status_code: Some(200),
        msg: None,
        retries: 0,
        important: false,
    }
}

/// A failing HTTP heartbeat with a descriptive msg.
pub fn sample_heartbeat_down(monitor: &Monitor) -> Heartbeat {
    Heartbeat {
        monitor_id: monitor.id,
        ts: OffsetDateTime::now_utc(),
        status: MonitorStatus::Down,
        latency_ms: Some(5_000),
        status_code: Some(503),
        msg: Some("upstream timed out".into()),
        retries: 1,
        important: true,
    }
}
