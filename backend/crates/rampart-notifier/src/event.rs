//! Events that trigger notifications.

use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Status changed across a probe boundary (e.g. up → down).
    StatusFlip,
    /// User-initiated "send me a test message" from the channel form.
    Test,
    /// Rolling SLO uptime fell below the configured target. Fired once
    /// per breach (de-duped by `monitors.slo_breached_at`).
    SloBreached,
    /// Rolling SLO uptime climbed back at-or-above the configured target
    /// after a prior breach. Fired once per recovery, same de-dup column.
    SloRecovered,
    /// A scheduled maintenance window just became active. Fired once per
    /// window transition (de-duped by `maintenance_windows.notified_start_at`).
    MaintenanceStarted,
    /// A scheduled maintenance window just ended. Fired once per window
    /// transition (de-duped by `maintenance_windows.notified_end_at`).
    MaintenanceEnded,
    /// A metric threshold rule started firing (breach outlasted its
    /// sustain window). De-duped by `metric_rules.firing_at`.
    MetricRuleFired,
    /// A firing metric rule's series came back inside the threshold.
    MetricRuleResolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub kind: EventKind,
    pub monitor: Monitor,
    pub heartbeat: Heartbeat,
    pub prev_status: Option<MonitorStatus>,
    /// Current rolling uptime % at the moment an `SloBreached` /
    /// `SloRecovered` event was emitted. None for non-SLO events.
    /// Surfaced to the template engine as `{{ slo_current_pct }}`.
    pub slo_current_pct: Option<f64>,
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
