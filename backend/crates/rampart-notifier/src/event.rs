//! Events that trigger notifications.

use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Status changed across a probe boundary (e.g. up → down).
    StatusFlip,
    /// User-initiated "send me a test message" from the channel form.
    Test,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub kind: EventKind,
    pub monitor: Monitor,
    pub heartbeat: Heartbeat,
    pub prev_status: Option<MonitorStatus>,
}

impl Event {
    pub fn status_str(&self) -> &'static str {
        status_label(self.heartbeat.status)
    }
    pub fn prev_status_str(&self) -> &'static str {
        self.prev_status.map(status_label).unwrap_or("unknown")
    }
}

fn status_label(s: MonitorStatus) -> &'static str {
    match s {
        MonitorStatus::Up => "up",
        MonitorStatus::Down => "down",
        MonitorStatus::Warn => "degraded",
        MonitorStatus::Paused => "paused",
        MonitorStatus::Pending => "pending",
        MonitorStatus::Maintenance => "maintenance",
    }
}
