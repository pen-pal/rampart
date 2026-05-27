//! A single check result — "heartbeat" in monitoring parlance.
//!
//! Renamed from `check.rs`. Dropped the `region` field since this is a
//! single-instance install (no multi-region probing).

use crate::ids::MonitorId;
use crate::monitor::MonitorStatus;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub monitor_id: MonitorId,
    pub ts: OffsetDateTime,
    pub status: MonitorStatus,
    pub latency_ms: Option<i32>,
    pub status_code: Option<i32>,
    pub msg: Option<String>,
    pub retries: i32,
    /// True at the exact point the monitor's status flipped. Used to
    /// scan recent state changes without aggregating the whole series.
    pub important: bool,
}
