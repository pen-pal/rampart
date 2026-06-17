//! Deploy markers — point-in-time annotations (deploys, config changes,
//! incidents) overlaid on the metric / response-time timelines so an operator
//! can correlate a latency or error spike with "what changed". CI typically
//! POSTs one on each release (via an API key); they can also be added in the UI.

use crate::ids::DeployMarkerId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployMarker {
    pub id: DeployMarkerId,
    /// When the event happened (defaults to creation time).
    pub ts: OffsetDateTime,
    pub title: String,
    pub description: Option<String>,
    /// Optional service/scope label, so a chart can show only the relevant
    /// markers (empty = applies to everything).
    pub service: Option<String>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewDeployMarker {
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(max = 2000))]
    #[serde(default)]
    pub description: Option<String>,
    #[validate(length(max = 200))]
    #[serde(default)]
    pub service: Option<String>,
    /// When the event happened; defaults to now() server-side when omitted.
    #[serde(default)]
    pub ts: Option<OffsetDateTime>,
}
