//! Maintenance windows.
//!
//! A window is a `[start_at, end_at)` range. While "now" sits inside the
//! window AND the window is `active`, any monitor attached to it is
//! reported as Maintenance instead of Up/Down — the scheduler suppresses
//! both the probe and any notification fan-out.
//!
//! v1 is single-shot only (no recurrence). The schema and types are
//! shaped so that a `recurrence` field can be added later without a
//! breaking change.

use crate::ids::{MaintenanceId, MonitorId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    pub id:          MaintenanceId,
    pub name:        String,
    pub description: Option<String>,
    pub start_at:    OffsetDateTime,
    pub end_at:      OffsetDateTime,
    pub active:      bool,
    pub created_at:  OffsetDateTime,
    /// Monitors covered by this window. Populated on detail reads;
    /// list endpoints may leave this empty for performance.
    #[serde(default)]
    pub monitor_ids: Vec<MonitorId>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewMaintenanceWindow {
    #[validate(length(min = 1, max = 120))]
    pub name:        String,

    #[serde(default)]
    pub description: Option<String>,

    pub start_at:    OffsetDateTime,
    pub end_at:      OffsetDateTime,

    /// Monitors to attach when creating the window. Can be empty —
    /// callers may attach later via the detail route.
    #[serde(default)]
    pub monitor_ids: Vec<MonitorId>,
}
