//! Monitor groups — a thin, cosmetic bucket for the dashboard.
//!
//! Groups don't change probe scheduling, alerting, or any backend
//! behavior. They only affect how the UI groups + orders monitors.
//! Keep this lean — anything heavier belongs in tags.

use crate::ids::MonitorGroupId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorGroup {
    pub id: MonitorGroupId,
    pub name: String,
    pub sort_order: i32,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewMonitorGroup {
    #[validate(length(min = 1, max = 80))]
    pub name: String,
    #[serde(default)]
    pub sort_order: i32,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateMonitorGroup {
    #[validate(length(min = 1, max = 80))]
    pub name: Option<String>,
    pub sort_order: Option<i32>,
}
