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
    /// Parent folder. `None` = a root folder. Cycles are rejected at write
    /// time so the folder forest stays acyclic.
    #[serde(default)]
    pub parent_id: Option<MonitorGroupId>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewMonitorGroup {
    #[validate(length(min = 1, max = 80))]
    pub name: String,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub parent_id: Option<MonitorGroupId>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateMonitorGroup {
    #[validate(length(min = 1, max = 80))]
    pub name: Option<String>,
    pub sort_order: Option<i32>,
    /// Send `null` to move the folder to the root; omit to leave unchanged.
    /// Double-option distinguishes "unset" from "explicit null".
    #[serde(default, deserialize_with = "deserialize_optional_parent_id")]
    pub parent_id: Option<Option<MonitorGroupId>>,
}

fn deserialize_optional_parent_id<'de, D>(
    de: D,
) -> Result<Option<Option<MonitorGroupId>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<MonitorGroupId> = Option::deserialize(de)?;
    Ok(Some(v))
}
