//! Rampart · core domain types.
//!
//! Single tenant, multiple users, no multi-region, no SLO budgets, no
//! on-call rotations. The bet is that the target audience (homelabs,
//! indie devs, small teams) wants better reliability and a handful of
//! long-asked features, not a different product class.
//!
//! All types are `Serialize + Deserialize`. Where they map directly to
//! Postgres rows they're also `sqlx::Type`.

pub mod api_key;
pub mod error;
pub mod heartbeat;
pub mod ids;
pub mod incident;
pub mod ingest_token;
pub mod maintenance;
pub mod monitor;
pub mod monitor_group;
pub mod notification;
pub mod proxy;
pub mod role;
pub mod status_page;
pub mod tag;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use api_key::{ApiKey, IssuedApiKey, KeyScope, NewApiKey};
pub use error::{CoreError, Result};
pub use heartbeat::Heartbeat;
pub use ids::{
    ApiKeyId, BadgeId, IncidentId, IncidentTemplateId, IncidentUpdateId, IngestTokenId,
    MaintenanceId, MonitorGroupId, MonitorId, NotificationId, NotificationTemplateId, ProxyId,
    SessionId, StatusPageComponentId, StatusPageGroupId, StatusPageId, StatusPageSubscriberId,
    TagId, UserId,
};
pub use incident::{Incident, IncidentStyle, IncidentTemplate, IncidentUpdate};
pub use ingest_token::{IngestToken, NewIngestToken};
pub use maintenance::{MaintenanceWindow, NewMaintenanceWindow};
pub use monitor::{
    BackoffStrategy, Monitor, MonitorKind, MonitorStatus, NewMonitor, RetryBackoff, UpdateMonitor,
};
pub use monitor_group::{MonitorGroup, NewMonitorGroup, UpdateMonitorGroup};
pub use notification::{ChannelKind, MonitorNotification, Notification, NotificationTemplate};
pub use proxy::{NewProxy, Proxy, ProxyProtocol};
pub use role::Role;
pub use status_page::{
    NewStatusPage, PublicIncident, PublicIncidentUpdate, PublicMaintenance, PublicStatusMonitor,
    PublicStatusPage, StatusPage, UpdateStatusPage,
};
pub use tag::{NewTag, Tag, TagBrief};
