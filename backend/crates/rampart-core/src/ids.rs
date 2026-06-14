//! Typed IDs.
//!
//! Each entity gets its own newtype around `Uuid`. Cheap (one macro per
//! type) and makes signatures unambiguous:
//!
//! ```ignore
//! fn ack(_: IncidentId) {}
//! ack(monitor_id); // <- won't compile
//! ```
//!
//! IDs are time-ordered v7 UUIDs so they sort naturally in the database
//! and indexes don't fragment.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
        #[serde(transparent)]
        #[sqlx(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            #[inline]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
            #[inline]
            pub const fn from_uuid(u: Uuid) -> Self {
                Self(u)
            }
            #[inline]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }
        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

// Core entities. Kept tight — no WorkspaceId, no RotationId, no RuleId
// (those were enterprise-scope and got dropped in the v1 → v2 slim).
id!(UserId);
id!(SessionId);
id!(MonitorId);
id!(TagId);
id!(NotificationId);
id!(NotificationTemplateId);
id!(MaintenanceId);
id!(MonitorGroupId);
id!(StatusPageId);
id!(StatusPageSectionId);
id!(StatusPageGroupId);
id!(StatusPageComponentId);
id!(StatusPageSubscriberId);
id!(IncidentId);
id!(IncidentUpdateId);
id!(IncidentTemplateId);
id!(ApiKeyId);
id!(ProxyId);
id!(BadgeId);
id!(IngestTokenId);
id!(MonitorPresetId);
id!(MonitorTemplateId);
id!(ScheduledReportId);
id!(AgentId);
id!(MetricRuleId);
id!(EscalationPolicyId);
id!(ErrorProjectId);
id!(ErrorIssueId);
id!(OnCallScheduleId);
id!(TelemetryRuleId);
id!(DetectionRuleId);
id!(DetectionFindingId);
