//! Rampart · core domain types.
//!
//! Single tenant, multiple users, no multi-region, no SLO budgets, no
//! on-call rotations. The bet is that the target audience (homelabs,
//! indie devs, small teams) wants better reliability and a handful of
//! long-asked features, not a different product class.
//!
//! All types are `Serialize + Deserialize`. Where they map directly to
//! Postgres rows they're also `sqlx::Type`.

pub mod error;
pub mod heartbeat;
pub mod ids;
pub mod incident;
pub mod monitor;
pub mod notification;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use error::{CoreError, Result};
pub use heartbeat::Heartbeat;
pub use ids::{
    ApiKeyId, BadgeId, IncidentId, IncidentUpdateId, MaintenanceId, MonitorId, NotificationId,
    NotificationTemplateId, ProxyId, SessionId, StatusPageComponentId, StatusPageGroupId,
    StatusPageId, StatusPageSubscriberId, TagId, UserId,
};
pub use incident::{Incident, IncidentStyle, IncidentUpdate};
pub use monitor::{Monitor, MonitorKind, MonitorStatus, NewMonitor};
pub use notification::{ChannelKind, MonitorNotification, Notification, NotificationTemplate};
