//! Rampart · core domain types.
//!
//! Single tenant, multiple users, no multi-region, no SLO budgets, no
//! on-call rotations. The bet is that the target audience (homelabs,
//! indie devs, small teams) wants better reliability and a handful of
//! long-asked features, not a different product class.
//!
//! All types are `Serialize + Deserialize`. Where they map directly to
//! Postgres rows they're also `sqlx::Type`.

pub mod agent;
pub mod api_key;
pub mod cron;
pub mod error;
pub mod error_tracking;
pub mod escalation;
pub mod heartbeat;
pub mod ids;
pub mod incident;
pub mod ingest_token;
pub mod maintenance;
pub mod metric_rule;
pub mod monitor;
pub mod monitor_group;
pub mod monitor_preset;
pub mod notification;
pub mod promtext;
pub mod proxy;
pub mod role;
pub mod scheduled_report;
pub mod status_page;
pub mod tag;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use agent::{Agent, AgentResult, IssuedAgent, NewAgent, UpdateAgent};
pub use api_key::{ApiKey, IssuedApiKey, KeyScope, NewApiKey};
pub use cron::{CronExpr, CronSchedule};
pub use error::{CoreError, Result};
pub use error_tracking::{
    ErrorEvent, ErrorIssue, ErrorProject, Frame, NewErrorProject, ParsedEvent, UpdateErrorProject,
};
pub use escalation::{
    EscalationEpisode, EscalationPolicy, EscalationStep, NewEscalationPolicy,
    UpdateEscalationPolicy,
};
pub use heartbeat::Heartbeat;
pub use ids::{
    AgentId, ApiKeyId, BadgeId, ErrorIssueId, ErrorProjectId, IncidentId, IncidentTemplateId,
    IncidentUpdateId, IngestTokenId, MaintenanceId, MonitorGroupId, MonitorId, MonitorPresetId,
    MonitorTemplateId, NotificationId, NotificationTemplateId, ProxyId, ScheduledReportId,
    SessionId, StatusPageComponentId, StatusPageGroupId, StatusPageId, StatusPageSectionId,
    StatusPageSubscriberId, TagId, UserId,
};
pub use incident::{Incident, IncidentStyle, IncidentTemplate, IncidentUpdate};
pub use ingest_token::{IngestToken, NewIngestToken};
pub use maintenance::{MaintenanceWindow, NewMaintenanceWindow};
pub use metric_rule::{MetricRule, NewMetricRule, RuleOp, RuleTransition, UpdateMetricRule};
pub use monitor::{
    BackoffStrategy, Monitor, MonitorKind, MonitorStatus, NewMonitor, RetryBackoff, UpdateMonitor,
};
pub use monitor_group::{MonitorGroup, NewMonitorGroup, UpdateMonitorGroup};
pub use monitor_preset::{MonitorPreset, MonitorPresetKind, NewMonitorPreset};
pub use notification::{ChannelKind, MonitorNotification, Notification, NotificationTemplate};
pub use promtext::{PromParseOutcome, PromSample};
pub use proxy::{NewProxy, Proxy, ProxyProtocol};
pub use role::Role;
pub use scheduled_report::ScheduledReport;
pub use status_page::{
    MonitorAssignment, NewStatusPage, NewStatusPageSection, PublicIncident, PublicIncidentUpdate,
    PublicMaintenance, PublicSection, PublicStatusMonitor, PublicStatusPage, StatusPage,
    StatusPageSection, UpdateStatusPage, UpdateStatusPageSection,
};
pub use tag::{NewTag, Tag, TagBrief};
