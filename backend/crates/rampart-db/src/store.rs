//! Object-safe `Store` seam (multi-DB P0 — heartbeats + deploy-markers +
//! ingest-keys + slos + proxies + on-call + recovery-codes + api-keys +
//! escalations + maintenance + ingest-tokens + tags + templates +
//! telemetry-rules + metric-rules + monitor-groups + silences + oidc-state +
//! status-pages + incidents + routing + subscribers + detection + sessions +
//! incident-templates + monitor-presets + monitor-templates + delivery-log +
//! agents + metric-samples + source-maps domains).
//!
//! This module proves the `&dyn Store` super-trait shape is object-safe and
//! that the AppState wiring compiles, ahead of the full ~40-trait extraction.
//! It introduces NO new SQL: the single Postgres impl (`PgStore`) delegates
//! every method straight to the existing domain free functions, so every
//! existing call path is byte-identical.
//!
//! Each sub-trait mirrors the `pub async fn`s of its domain module one-for-one,
//! dropping the leading `pool: &DbPool` parameter in favour of `&self`. Method
//! names carry a per-domain suffix (e.g. `create_deploy_marker`,
//! `create_ingest_key`, `create_slo`) so the shared `PgStore` can implement
//! several domains that each expose a bare `create`/`delete`/`list` without
//! collision.

use crate::delivery_log::{DeliveryEntry, NewDelivery};
use crate::detection::{FindingEvent, PreviewResult};
use crate::error_tracking::{AffectedUser, ErrorBucket, IssueStats, RecordOutcome, TraceErrorRef};
use crate::heartbeats::{BurndownPoint, ErrorBudget, MonitorSummary, MonthlyUptime, MtbfMttr};
use crate::incident_templates::{NewIncidentTemplate, UpdateIncidentTemplate};
use crate::incidents::{NewIncident, UpdateIncident};
use crate::ingest_keys::IngestKey;
use crate::logs::{LogBucket, LogFilter};
use crate::maintenance::MaintenanceTransition;
use crate::metric_rules::RuleEvent as MetricRuleEvent;
use crate::metric_samples::{RangePoint, Series};
use crate::metrics::{IngestGauges, PipelineGauges, TableSize};
use crate::monitor_templates::{MonitorTemplate, NewMonitorTemplate};
use crate::notifications::Notification;
use crate::notifications::{MonitorChannelCount, NewNotification, UpdateNotification};
use crate::oidc_state::Consumed;
use crate::profiles::{NewProfile, ProfileMeta};
use crate::rum::{RumBrowser, RumSample, RumUser};
use crate::scheduled_reports::{NewScheduledReport, UpdateScheduledReport};
use crate::sessions::{Session, SessionInfo};
use crate::silences::{NewSilence, Silence};
use crate::slos::{SloEvent, SloWithSnapshot};
use crate::source_maps::{NewSourceMap, SourceMapMeta};
use crate::subscribers::{ManagedSubscription, Subscriber};
use crate::tags::TagUsage;
use crate::telemetry_rules::RuleEvent as TelemetryRuleEvent;
use crate::templates::{NewTemplate, RenderedTemplate, Template, UpdateTemplate};
use crate::traces::TraceFilter;
use crate::users::{NewUser, User, UserWithHash};
use crate::{DbPool, DbResult};
use rampart_core::agent::{Agent, IssuedAgent, NewAgent, UpdateAgent};
use rampart_core::api_key::{ApiKey, IssuedApiKey, NewApiKey};
use rampart_core::deploy_marker::{DeployMarker, NewDeployMarker};
use rampart_core::detection::{
    DetectionFinding, DetectionRule, NewDetectionRule, UpdateDetectionRule,
};
use rampart_core::error_tracking::{
    ErrorEvent, ErrorIssue, ErrorProject, NewErrorProject, ParsedEvent, UpdateErrorProject,
};
use rampart_core::escalation::{
    EscalationEpisode, EscalationPolicy, NewEscalationPolicy, UpdateEscalationPolicy,
};
use rampart_core::ids::{
    AgentId, ApiKeyId, DeployMarkerId, DetectionFindingId, DetectionRuleId, ErrorIssueId,
    ErrorProjectId, EscalationPolicyId, IncidentId, IncidentTemplateId, IngestTokenId,
    MaintenanceId, MetricRuleId, MonitorGroupId, MonitorPresetId, MonitorTemplateId,
    NotificationId, NotificationTemplateId, OnCallScheduleId, OrgId, ScheduledReportId, SloId,
    StatusPageId, StatusPageSectionId, StatusPageSubscriberId, TagId, TelemetryRuleId,
};
use rampart_core::incident::{Incident, IncidentTemplate, IncidentUpdate};
use rampart_core::ingest_token::{IngestToken, NewIngestToken};
use rampart_core::log::ParsedLog;
use rampart_core::maintenance::{MaintenanceWindow, NewMaintenanceWindow, UpdateMaintenanceWindow};
use rampart_core::metric_rule::{MetricRule, NewMetricRule, UpdateMetricRule};
use rampart_core::monitor_group::{MonitorGroup, NewMonitorGroup, UpdateMonitorGroup};
use rampart_core::monitor_preset::{MonitorPreset, NewMonitorPreset};
use rampart_core::on_call::{
    NewOnCallSchedule, OnCallSchedule, OnCallTarget, UpdateOnCallSchedule,
};
use rampart_core::promtext::PromSample;
use rampart_core::proxy::{NewProxy, Proxy};
use rampart_core::rum::{RumBeacon, RumPage, RumTracedLoad, RumVitals};
use rampart_core::scheduled_report::ScheduledReport;
use rampart_core::slo::{NewSlo, Slo, SloSnapshot, UpdateSlo};
use rampart_core::status_page::PublicMaintenance;
use rampart_core::status_page::{
    NewStatusPage, NewStatusPageSection, PublicStatusPage, StatusPage, StatusPageSection,
    UpdateStatusPage, UpdateStatusPageSection,
};
use rampart_core::tag::{NewTag, Tag, TagBrief, UpdateTag};
use rampart_core::telemetry_rule::{NewTelemetryRule, TelemetryRule, UpdateTelemetryRule};
use rampart_core::trace::{ParsedSpan, ServiceEdge, Span, TraceSummary};
use rampart_core::Role;
use rampart_core::{Heartbeat, MonitorId, ProxyId, UserId};
use rampart_core::{LogEntry, OperationStat};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

/// One method per public `crate::heartbeats` free function. Signatures are
/// mirrored exactly except the leading `pool: &DbPool` is replaced by `&self`.
#[async_trait::async_trait]
pub trait StoreHeartbeats: Send + Sync {
    async fn insert_many(&self, hbs: &[Heartbeat]) -> DbResult<()>;

    async fn recent_for_monitor(&self, monitor: MonitorId, limit: i64) -> DbResult<Vec<Heartbeat>>;

    async fn recent_for_monitor_before(
        &self,
        monitor: MonitorId,
        limit: i64,
        before: Option<time::OffsetDateTime>,
    ) -> DbResult<Vec<Heartbeat>>;

    async fn range_for_monitor(
        &self,
        monitor: MonitorId,
        since: time::OffsetDateTime,
        until: time::OffsetDateTime,
        limit: i64,
    ) -> DbResult<Vec<Heartbeat>>;

    async fn uptime_pct(&self, monitor: MonitorId, window_seconds: i64) -> DbResult<Option<f64>>;

    async fn current_slo_uptime_pct(
        &self,
        monitor: MonitorId,
        window_days: i32,
    ) -> DbResult<Option<f64>>;

    async fn avg_latency_ms(
        &self,
        monitor: MonitorId,
        window_seconds: i64,
    ) -> DbResult<Option<f64>>;

    async fn daily_status(&self, monitor: MonitorId, days: i32) -> DbResult<Vec<u8>>;

    async fn day_hourly_latency(
        &self,
        monitor: MonitorId,
        day: time::Date,
    ) -> DbResult<Vec<(i32, Option<f32>, i32)>>;

    async fn monthly_uptime(&self, monitor: MonitorId, months: i32)
        -> DbResult<Vec<MonthlyUptime>>;

    async fn uptime_pct_batch(
        &self,
        monitor_ids: &[Uuid],
        window_seconds: i64,
    ) -> DbResult<HashMap<Uuid, f64>>;

    async fn avg_latency_ms_batch(
        &self,
        monitor_ids: &[Uuid],
        window_seconds: i64,
    ) -> DbResult<HashMap<Uuid, f64>>;

    async fn daily_status_batch(
        &self,
        monitor_ids: &[Uuid],
        days: i32,
    ) -> DbResult<HashMap<Uuid, Vec<u8>>>;

    async fn monthly_uptime_batch(
        &self,
        monitor_ids: &[Uuid],
        months: i32,
    ) -> DbResult<HashMap<Uuid, Vec<MonthlyUptime>>>;

    async fn summary_window(
        &self,
        window_seconds: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<MonitorSummary>>;

    async fn mtbf_mttr(&self, monitor: MonitorId, window_seconds: i64) -> DbResult<MtbfMttr>;

    async fn error_budget(
        &self,
        monitor: MonitorId,
        window_days: i32,
        target_pct: f64,
    ) -> DbResult<ErrorBudget>;

    async fn error_budget_burndown(
        &self,
        monitor: MonitorId,
        window_days: i32,
        target_pct: f64,
    ) -> DbResult<Vec<BurndownPoint>>;

    async fn recent_per_monitor(&self, per_monitor: i64, org_id: OrgId)
        -> DbResult<Vec<Heartbeat>>;
}

/// One method per public `crate::deploy_markers` free function. Names carry a
/// `_deploy_marker(s)` suffix to disambiguate from the other domains' CRUD fns.
#[async_trait::async_trait]
pub trait StoreDeployMarkers: Send + Sync {
    async fn create_deploy_marker(
        &self,
        input: NewDeployMarker,
        org_id: OrgId,
    ) -> DbResult<DeployMarker>;

    async fn list_deploy_markers_window(
        &self,
        hours: i32,
        service: Option<&str>,
        org_id: OrgId,
    ) -> DbResult<Vec<DeployMarker>>;

    async fn delete_deploy_marker(&self, id: DeployMarkerId, org_id: OrgId) -> DbResult<()>;
}

/// One method per public `crate::ingest_keys` free function, with an
/// `_ingest_key(s)` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreIngestKeys: Send + Sync {
    async fn create_ingest_key(
        &self,
        org_id: OrgId,
        label: &str,
        kind: &str,
        allowed_origins: &[String],
    ) -> DbResult<(IngestKey, String)>;

    async fn find_ingest_key_by_token(
        &self,
        token: &str,
    ) -> DbResult<Option<(Uuid, OrgId, Vec<String>)>>;

    async fn touch_ingest_key_last_used(&self, id: Uuid) -> DbResult<()>;

    async fn list_ingest_keys_for_org(&self, org_id: OrgId) -> DbResult<Vec<IngestKey>>;

    async fn delete_ingest_key(&self, id: Uuid, org_id: OrgId) -> DbResult<bool>;
}

/// One method per public `crate::slos` free function, with `_slo`/`slo_`
/// suffixes so the CRUD names don't collide with the other domains.
#[async_trait::async_trait]
pub trait StoreSlos: Send + Sync {
    async fn list_slos(&self, org_id: OrgId) -> DbResult<Vec<Slo>>;

    async fn list_all_slos(&self) -> DbResult<Vec<Slo>>;

    async fn get_slo(&self, id: SloId, org_id: OrgId) -> DbResult<Slo>;

    async fn get_slo_unscoped(&self, id: SloId) -> DbResult<Slo>;

    async fn create_slo(&self, input: NewSlo, org_id: OrgId) -> DbResult<Slo>;

    async fn update_slo(&self, id: SloId, patch: UpdateSlo, org_id: OrgId) -> DbResult<Slo>;

    async fn delete_slo(&self, id: SloId, org_id: OrgId) -> DbResult<()>;

    async fn compute_slo(&self, slo: &Slo) -> DbResult<SloSnapshot>;

    async fn slo_trend(&self, slo: &Slo, buckets: i64) -> DbResult<Vec<f64>>;

    async fn list_slos_with_snapshots(&self, org_id: OrgId) -> DbResult<Vec<SloWithSnapshot>>;

    async fn evaluate_slos_tick(&self) -> DbResult<Vec<SloEvent>>;
}

/// One method per public `crate::proxies` free function, with a `_prox(y|ies)`
/// suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreProxies: Send + Sync {
    async fn list_proxies(&self, org_id: OrgId) -> DbResult<Vec<Proxy>>;

    async fn get_proxy(&self, id: ProxyId, org_id: OrgId) -> DbResult<Proxy>;

    async fn get_proxy_unscoped(&self, id: ProxyId) -> DbResult<Proxy>;

    async fn create_proxy(&self, input: NewProxy, org_id: OrgId) -> DbResult<Proxy>;

    async fn delete_proxy(&self, id: ProxyId, org_id: OrgId) -> DbResult<()>;

    async fn set_active_proxy(&self, id: ProxyId, active: bool, org_id: OrgId) -> DbResult<()>;
}

/// One method per public `crate::on_call` free function. CRUD names carry an
/// `_on_call` suffix and the resolvers an `oncall_` prefix to disambiguate.
#[async_trait::async_trait]
pub trait StoreOnCall: Send + Sync {
    async fn list_on_call(&self, org_id: OrgId) -> DbResult<Vec<OnCallSchedule>>;

    async fn get_on_call(&self, id: OnCallScheduleId, org_id: OrgId) -> DbResult<OnCallSchedule>;

    async fn get_on_call_unscoped(&self, id: OnCallScheduleId) -> DbResult<OnCallSchedule>;

    async fn create_on_call(
        &self,
        input: NewOnCallSchedule,
        org_id: OrgId,
    ) -> DbResult<OnCallSchedule>;

    async fn update_on_call(
        &self,
        id: OnCallScheduleId,
        patch: UpdateOnCallSchedule,
        org_id: OrgId,
    ) -> DbResult<OnCallSchedule>;

    async fn delete_on_call(&self, id: OnCallScheduleId, org_id: OrgId) -> DbResult<()>;

    async fn oncall_current_channel(
        &self,
        id: OnCallScheduleId,
        at: OffsetDateTime,
    ) -> DbResult<Option<NotificationId>>;

    async fn oncall_current_target(
        &self,
        id: OnCallScheduleId,
        at: OffsetDateTime,
    ) -> DbResult<Option<OnCallTarget>>;
}

/// One method per public `crate::recovery_codes` free function, with a
/// `_recovery_codes` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreRecoveryCodes: Send + Sync {
    async fn issue_recovery_codes(&self, user: UserId, count: usize) -> DbResult<Vec<String>>;

    async fn consume_recovery_code(&self, user: UserId, code: &str) -> DbResult<bool>;

    async fn delete_recovery_codes_for_user(&self, user: UserId) -> DbResult<()>;

    async fn remaining_recovery_codes(&self, user: UserId) -> DbResult<i64>;
}

/// One method per public `crate::api_keys` free function, with an `_api_key(s)`
/// suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreApiKeys: Send + Sync {
    async fn list_api_keys(&self, org_id: OrgId) -> DbResult<Vec<ApiKey>>;

    async fn create_api_key(
        &self,
        input: NewApiKey,
        created_by: UserId,
        org_id: OrgId,
    ) -> DbResult<IssuedApiKey>;

    async fn delete_api_key(&self, id: ApiKeyId, org_id: OrgId) -> DbResult<()>;

    async fn lookup_api_key(&self, token: &str) -> DbResult<(ApiKey, UserId, OrgId)>;

    async fn touch_api_key_last_used(&self, id: ApiKeyId) -> DbResult<()>;
}

/// One method per public `crate::escalations` free function, with an
/// `_escalation_policy`/`_episode` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreEscalations: Send + Sync {
    async fn list_escalation_policies(&self, org_id: OrgId) -> DbResult<Vec<EscalationPolicy>>;

    async fn get_escalation_policy(
        &self,
        id: EscalationPolicyId,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy>;

    async fn get_escalation_policy_unscoped(
        &self,
        id: EscalationPolicyId,
    ) -> DbResult<EscalationPolicy>;

    async fn create_escalation_policy(
        &self,
        input: NewEscalationPolicy,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy>;

    async fn update_escalation_policy(
        &self,
        id: EscalationPolicyId,
        patch: UpdateEscalationPolicy,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy>;

    async fn delete_escalation_policy(&self, id: EscalationPolicyId, org_id: OrgId)
        -> DbResult<()>;

    async fn open_episode(
        &self,
        monitor_id: MonitorId,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>>;

    async fn open_episode_for_subject(
        &self,
        kind: &str,
        subject_ref: &str,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>>;

    async fn resolve_subject(
        &self,
        kind: &str,
        subject_ref: &str,
    ) -> DbResult<Option<EscalationEpisode>>;

    async fn ack_episode(&self, episode_id: Uuid, by: UserId) -> DbResult<EscalationEpisode>;

    async fn list_open_episodes(&self) -> DbResult<Vec<EscalationEpisode>>;

    async fn list_open_episodes_for_org(&self, org_id: OrgId) -> DbResult<Vec<EscalationEpisode>>;

    async fn episode_in_org(&self, episode: Uuid, org_id: OrgId) -> DbResult<()>;

    async fn open_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
    ) -> DbResult<Option<EscalationEpisode>>;

    async fn ack_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
        by: UserId,
    ) -> DbResult<EscalationEpisode>;

    async fn resolve_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
    ) -> DbResult<Option<EscalationEpisode>>;

    async fn advance_episode(
        &self,
        episode_id: Uuid,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>>;

    async fn due_episodes(&self) -> DbResult<Vec<EscalationEpisode>>;
}

/// One method per public `crate::maintenance` free function, with a
/// `_maintenance_window` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreMaintenance: Send + Sync {
    async fn list_maintenance_windows(&self, org_id: OrgId) -> DbResult<Vec<MaintenanceWindow>>;

    async fn get_maintenance_window(
        &self,
        id: MaintenanceId,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow>;

    async fn create_maintenance_window(
        &self,
        input: NewMaintenanceWindow,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow>;

    async fn update_maintenance_window(
        &self,
        id: MaintenanceId,
        patch: UpdateMaintenanceWindow,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow>;

    async fn delete_maintenance_window(&self, id: MaintenanceId, org_id: OrgId) -> DbResult<()>;

    async fn set_active_maintenance(
        &self,
        id: MaintenanceId,
        active: bool,
        org_id: OrgId,
    ) -> DbResult<()>;

    async fn attach_maintenance_monitor(
        &self,
        window: MaintenanceId,
        monitor: MonitorId,
    ) -> DbResult<()>;

    async fn detach_maintenance_monitor(
        &self,
        window: MaintenanceId,
        monitor: MonitorId,
    ) -> DbResult<()>;

    async fn is_in_active_window(&self, monitor: MonitorId) -> DbResult<bool>;

    async fn maintenance_transitions_needing_notification(
        &self,
    ) -> DbResult<Vec<MaintenanceTransition>>;

    async fn mark_maintenance_notified_start(&self, id: MaintenanceId) -> DbResult<()>;

    async fn mark_maintenance_notified_end(&self, id: MaintenanceId) -> DbResult<()>;

    async fn confirmed_subscriber_emails_for_monitors(
        &self,
        monitors: &[MonitorId],
    ) -> DbResult<Vec<String>>;

    async fn public_maintenance_for_status_page(
        &self,
        page: StatusPageId,
    ) -> DbResult<Vec<PublicMaintenance>>;
}

/// One method per public `crate::ingest_tokens` free function, with an
/// `_ingest_token` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreIngestTokens: Send + Sync {
    async fn create_ingest_token(
        &self,
        page: StatusPageId,
        input: NewIngestToken,
    ) -> DbResult<IngestToken>;

    async fn create_ingest_token_with_token(
        &self,
        page: StatusPageId,
        label: &str,
        token: &str,
    ) -> DbResult<IngestToken>;

    async fn set_ingest_token_mapping(
        &self,
        id: IngestTokenId,
        mapping: Option<serde_json::Value>,
        org_id: OrgId,
    ) -> DbResult<IngestToken>;

    async fn list_ingest_tokens_for_page(&self, page: StatusPageId) -> DbResult<Vec<IngestToken>>;

    async fn find_ingest_token_by_token(&self, token: &str) -> DbResult<IngestToken>;

    async fn delete_ingest_token(&self, id: IngestTokenId, org_id: OrgId) -> DbResult<()>;

    async fn touch_ingest_token_last_used(&self, id: IngestTokenId) -> DbResult<()>;
}

/// One method per public `crate::tags` free function, with a `_tag(s)` suffix on
/// the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreTags: Send + Sync {
    async fn list_tags(&self, org_id: OrgId) -> DbResult<Vec<Tag>>;

    async fn get_tag(&self, id: TagId, org_id: OrgId) -> DbResult<Tag>;

    async fn create_tag(&self, input: NewTag, org_id: OrgId) -> DbResult<Tag>;

    async fn update_tag(&self, id: TagId, patch: UpdateTag, org_id: OrgId) -> DbResult<Tag>;

    async fn tag_usage(&self, org_id: OrgId) -> DbResult<Vec<TagUsage>>;

    async fn delete_tag(&self, id: TagId, org_id: OrgId) -> DbResult<()>;

    async fn attach_tag(&self, monitor: MonitorId, tag: TagId) -> DbResult<()>;

    async fn detach_tag(&self, monitor: MonitorId, tag: TagId) -> DbResult<()>;

    async fn list_tags_for_monitor(&self, monitor: MonitorId) -> DbResult<Vec<TagBrief>>;

    async fn hydrate_tags_for_channels(
        &self,
        ids: &[NotificationId],
    ) -> DbResult<HashMap<NotificationId, Vec<TagBrief>>>;

    async fn hydrate_tags_for_monitors(
        &self,
        ids: &[MonitorId],
    ) -> DbResult<HashMap<MonitorId, Vec<TagBrief>>>;
}

/// One method per public `crate::templates` free function, with a `_template`
/// suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreTemplates: Send + Sync {
    async fn list_templates(&self, org_id: OrgId) -> DbResult<Vec<Template>>;

    async fn get_template(&self, id: NotificationTemplateId, org_id: OrgId) -> DbResult<Template>;

    async fn create_template(&self, input: NewTemplate, org_id: OrgId) -> DbResult<Template>;

    async fn update_template(
        &self,
        id: NotificationTemplateId,
        input: UpdateTemplate,
        org_id: OrgId,
    ) -> DbResult<Template>;

    async fn delete_template(&self, id: NotificationTemplateId, org_id: OrgId) -> DbResult<()>;

    async fn get_template_render_strings(
        &self,
        id: NotificationTemplateId,
    ) -> DbResult<RenderedTemplate>;
}

/// One method per public `crate::telemetry_rules` free function, with a
/// `_telemetry_rule(s)` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreTelemetryRules: Send + Sync {
    async fn list_telemetry_rules(&self, org_id: OrgId) -> DbResult<Vec<TelemetryRule>>;

    async fn list_all_telemetry_rules(&self) -> DbResult<Vec<TelemetryRule>>;

    async fn get_telemetry_rule(
        &self,
        id: TelemetryRuleId,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule>;

    async fn get_telemetry_rule_unscoped(&self, id: TelemetryRuleId) -> DbResult<TelemetryRule>;

    async fn create_telemetry_rule(
        &self,
        input: NewTelemetryRule,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule>;

    async fn update_telemetry_rule(
        &self,
        id: TelemetryRuleId,
        patch: UpdateTelemetryRule,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule>;

    async fn delete_telemetry_rule(&self, id: TelemetryRuleId, org_id: OrgId) -> DbResult<()>;

    async fn evaluate_telemetry_rules_tick(&self) -> DbResult<Vec<TelemetryRuleEvent>>;
}

/// One method per public `crate::metric_rules` free function, with a
/// `_metric_rule(s)` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreMetricRules: Send + Sync {
    async fn list_metric_rules(&self, org_id: OrgId) -> DbResult<Vec<MetricRule>>;

    async fn list_all_metric_rules(&self) -> DbResult<Vec<MetricRule>>;

    async fn get_metric_rule(&self, id: MetricRuleId, org_id: OrgId) -> DbResult<MetricRule>;

    async fn get_metric_rule_unscoped(&self, id: MetricRuleId) -> DbResult<MetricRule>;

    async fn create_metric_rule(&self, input: NewMetricRule, org_id: OrgId)
        -> DbResult<MetricRule>;

    async fn update_metric_rule(
        &self,
        id: MetricRuleId,
        patch: UpdateMetricRule,
        org_id: OrgId,
    ) -> DbResult<MetricRule>;

    async fn delete_metric_rule(&self, id: MetricRuleId, org_id: OrgId) -> DbResult<()>;

    async fn evaluate_metric_rules_tick(&self) -> DbResult<Vec<MetricRuleEvent>>;
}

/// One method per public `crate::monitor_groups` free function, with a
/// `_monitor_group`/`_dependency` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreMonitorGroups: Send + Sync {
    async fn monitor_group_in_org(&self, group: MonitorGroupId, org_id: OrgId) -> DbResult<()>;

    async fn list_monitor_groups(&self, org_id: OrgId) -> DbResult<Vec<MonitorGroup>>;

    async fn create_monitor_group(
        &self,
        input: NewMonitorGroup,
        org_id: OrgId,
    ) -> DbResult<MonitorGroup>;

    async fn update_monitor_group(
        &self,
        id: MonitorGroupId,
        patch: UpdateMonitorGroup,
        org_id: OrgId,
    ) -> DbResult<MonitorGroup>;

    async fn would_form_group_cycle(
        &self,
        group: MonitorGroupId,
        new_parent: MonitorGroupId,
    ) -> DbResult<bool>;

    async fn delete_monitor_group(&self, id: MonitorGroupId, org_id: OrgId) -> DbResult<()>;

    async fn parents_of(&self, child: MonitorId) -> DbResult<Vec<MonitorId>>;

    async fn children_of(&self, parent: MonitorId) -> DbResult<Vec<MonitorId>>;

    async fn any_parent_down(&self, child: MonitorId) -> DbResult<bool>;

    async fn attach_dependency(&self, child: MonitorId, parent: MonitorId) -> DbResult<()>;

    async fn detach_dependency(&self, child: MonitorId, parent: MonitorId) -> DbResult<()>;

    async fn would_form_cycle(&self, child: MonitorId, parent: MonitorId) -> DbResult<bool>;
}

/// One method per public `crate::silences` free function, with a `_silence`
/// suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreSilences: Send + Sync {
    async fn is_silenced(&self, monitor: Option<Uuid>) -> DbResult<bool>;

    async fn create_silence(&self, s: NewSilence<'_>, org_id: OrgId) -> DbResult<Uuid>;

    async fn list_active_silences(&self, org_id: OrgId) -> DbResult<Vec<Silence>>;

    async fn delete_silence(&self, id: Uuid, org_id: OrgId) -> DbResult<bool>;
}

/// One method per public `crate::oidc_state` free function, with an
/// `oidc_state`/`stash`/`consume` naming that avoids collision.
#[async_trait::async_trait]
pub trait StoreOidcState: Send + Sync {
    async fn stash_oidc_state(
        &self,
        state: &str,
        pkce_verifier: &str,
        nonce: Option<&str>,
        return_to: Option<&str>,
    ) -> DbResult<()>;

    async fn consume_oidc_state(&self, state: &str) -> DbResult<Option<Consumed>>;

    async fn prune_oidc_state(&self) -> DbResult<u64>;
}

/// One method per public `crate::status_pages` free function, with a
/// `_status_page(s)`/`_section` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreStatusPages: Send + Sync {
    async fn list_status_pages(&self, org_id: OrgId) -> DbResult<Vec<StatusPage>>;

    async fn list_all_status_pages(&self) -> DbResult<Vec<StatusPage>>;

    async fn get_status_page(&self, id: StatusPageId, org_id: OrgId) -> DbResult<StatusPage>;

    async fn get_status_page_by_slug(&self, slug: &str) -> DbResult<StatusPage>;

    async fn get_status_page_unscoped(&self, id: StatusPageId) -> DbResult<StatusPage>;

    async fn find_status_page_by_custom_domain(&self, host: &str) -> DbResult<Option<StatusPage>>;

    async fn create_status_page(&self, input: NewStatusPage, org_id: OrgId)
        -> DbResult<StatusPage>;

    async fn update_status_page(
        &self,
        id: StatusPageId,
        patch: UpdateStatusPage,
        org_id: OrgId,
    ) -> DbResult<StatusPage>;

    async fn delete_status_page(&self, id: StatusPageId, org_id: OrgId) -> DbResult<()>;

    async fn status_page_public_view(&self, slug: &str) -> DbResult<PublicStatusPage>;

    async fn verify_status_page_password(&self, slug: &str, candidate: &str) -> DbResult<bool>;

    async fn list_status_page_sections(
        &self,
        page_id: StatusPageId,
    ) -> DbResult<Vec<StatusPageSection>>;

    async fn create_status_page_section(
        &self,
        page_id: StatusPageId,
        input: NewStatusPageSection,
    ) -> DbResult<StatusPageSection>;

    async fn update_status_page_section(
        &self,
        id: StatusPageSectionId,
        patch: UpdateStatusPageSection,
    ) -> DbResult<StatusPageSection>;

    async fn delete_status_page_section(&self, id: StatusPageSectionId) -> DbResult<()>;

    async fn assign_status_page_monitor_section(
        &self,
        page_id: StatusPageId,
        monitor_id: MonitorId,
        section_id: Option<StatusPageSectionId>,
    ) -> DbResult<()>;
}

/// One method per public `crate::incidents` free function, with an
/// `_incident(s)` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreIncidents: Send + Sync {
    async fn create_incident(
        &self,
        page: StatusPageId,
        author: Option<UserId>,
        input: NewIncident,
    ) -> DbResult<Incident>;

    async fn find_active_incident_by_dedup_key(
        &self,
        page: StatusPageId,
        key: &str,
    ) -> DbResult<Option<Incident>>;

    async fn list_active_incidents(&self, page: StatusPageId) -> DbResult<Vec<Incident>>;

    async fn recent_incidents(&self, limit: i64, org_id: OrgId) -> DbResult<Vec<Incident>>;

    async fn list_resolved_incident_history(
        &self,
        page: StatusPageId,
        limit: i64,
    ) -> DbResult<Vec<Incident>>;

    async fn resolve_incident(&self, id: IncidentId, now: OffsetDateTime) -> DbResult<()>;

    async fn list_all_incidents(&self, page: StatusPageId, limit: i64) -> DbResult<Vec<Incident>>;

    async fn delete_incident(&self, id: IncidentId) -> DbResult<()>;

    async fn update_incident(&self, id: IncidentId, patch: UpdateIncident) -> DbResult<Incident>;

    async fn get_incident(&self, id: IncidentId) -> DbResult<Incident>;

    async fn list_incident_updates(&self, incident: IncidentId) -> DbResult<Vec<IncidentUpdate>>;

    async fn post_incident_update(
        &self,
        incident: IncidentId,
        author: Option<UserId>,
        message: String,
    ) -> DbResult<Uuid>;
}

/// One method per public `crate::routing` free function. These names are
/// already collision-free across the other domains, so they are mirrored
/// verbatim.
#[async_trait::async_trait]
pub trait StoreRouting: Send + Sync {
    async fn resolve_channels_for_monitor(&self, monitor: MonitorId)
        -> DbResult<Vec<Notification>>;

    async fn group_tag_ids(&self, group: MonitorGroupId) -> DbResult<Vec<TagId>>;

    async fn channel_tag_ids(&self, notif: NotificationId) -> DbResult<Vec<TagId>>;

    async fn group_channel_ids(&self, group: MonitorGroupId) -> DbResult<Vec<NotificationId>>;

    async fn monitor_exclude_ids(&self, monitor: MonitorId) -> DbResult<Vec<NotificationId>>;

    async fn tag_group(&self, group: MonitorGroupId, tag: TagId) -> DbResult<()>;

    async fn untag_group(&self, group: MonitorGroupId, tag: TagId) -> DbResult<()>;

    async fn tag_channel(&self, notif: NotificationId, tag: TagId) -> DbResult<()>;

    async fn untag_channel(&self, notif: NotificationId, tag: TagId) -> DbResult<()>;

    async fn attach_group_channel(
        &self,
        group: MonitorGroupId,
        notif: NotificationId,
    ) -> DbResult<()>;

    async fn detach_group_channel(
        &self,
        group: MonitorGroupId,
        notif: NotificationId,
    ) -> DbResult<()>;

    async fn exclude_channel(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()>;

    async fn unexclude_channel(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()>;
}

/// One method per public `crate::subscribers` free function, with a
/// `_subscriber(s)`/`subscriber_` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreSubscribers: Send + Sync {
    async fn subscribe_email(
        &self,
        page: StatusPageId,
        email: &str,
    ) -> DbResult<(Subscriber, String)>;

    async fn list_subscribers_for_page(&self, page: StatusPageId) -> DbResult<Vec<Subscriber>>;

    async fn confirmed_subscriber_emails_for_page(
        &self,
        page: StatusPageId,
    ) -> DbResult<Vec<String>>;

    async fn delete_subscriber(&self, id: StatusPageSubscriberId) -> DbResult<()>;

    async fn unsubscribe_subscriber_by_token(&self, token: &str) -> DbResult<()>;

    async fn subscriber_email_for_token(&self, token: &str) -> DbResult<Option<String>>;

    async fn subscriptions_for_email(&self, email: &str) -> DbResult<Vec<ManagedSubscription>>;

    async fn unsubscribe_all_for_email(&self, email: &str) -> DbResult<u64>;

    async fn unsubscribe_email_from_page(&self, page: StatusPageId, email: &str) -> DbResult<()>;

    async fn subscriber_page_for(
        &self,
        id: StatusPageSubscriberId,
    ) -> DbResult<Option<StatusPageId>>;

    async fn subscriber_token_for(&self, id: Uuid) -> DbResult<Option<String>>;
}

/// One method per public `crate::detection` free function, with a
/// `_detection_rule(s)`/`detection_`/`_detection_finding(s)` suffix on the
/// collision-prone names.
#[async_trait::async_trait]
pub trait StoreDetection: Send + Sync {
    async fn detection_regex_is_valid(&self, pattern: &str) -> DbResult<bool>;

    async fn list_detection_rules(&self, org_id: OrgId) -> DbResult<Vec<DetectionRule>>;

    async fn list_all_detection_rules(&self) -> DbResult<Vec<DetectionRule>>;

    async fn get_detection_rule(
        &self,
        id: DetectionRuleId,
        org_id: OrgId,
    ) -> DbResult<DetectionRule>;

    async fn get_detection_rule_unscoped(&self, id: DetectionRuleId) -> DbResult<DetectionRule>;

    async fn create_detection_rule(
        &self,
        input: NewDetectionRule,
        org_id: OrgId,
    ) -> DbResult<DetectionRule>;

    async fn update_detection_rule(
        &self,
        id: DetectionRuleId,
        patch: UpdateDetectionRule,
        org_id: OrgId,
    ) -> DbResult<DetectionRule>;

    async fn delete_detection_rule(&self, id: DetectionRuleId, org_id: OrgId) -> DbResult<()>;

    #[allow(clippy::too_many_arguments)]
    async fn preview_detection(
        &self,
        service: &str,
        min_level: i16,
        body_regex: &str,
        attr_key: &str,
        attr_val: &str,
        window_seconds: i32,
        org_id: OrgId,
    ) -> DbResult<PreviewResult>;

    async fn has_recent_detection_finding(
        &self,
        rule_id: DetectionRuleId,
        secs: i64,
        entity: Option<&str>,
    ) -> DbResult<bool>;

    async fn list_detection_findings(
        &self,
        limit: i64,
        open_only: bool,
    ) -> DbResult<Vec<DetectionFinding>>;

    async fn list_detection_findings_for_org(
        &self,
        limit: i64,
        open_only: bool,
        org_id: OrgId,
    ) -> DbResult<Vec<DetectionFinding>>;

    async fn detection_finding_in_org(
        &self,
        finding: DetectionFindingId,
        org_id: OrgId,
    ) -> DbResult<()>;

    async fn open_detection_findings_count(&self) -> DbResult<i64>;

    async fn fetch_detection_findings_since(
        &self,
        after: Option<OffsetDateTime>,
        limit: i64,
    ) -> DbResult<Vec<DetectionFinding>>;

    async fn ack_detection_finding(&self, id: DetectionFindingId) -> DbResult<DetectionFinding>;

    async fn evaluate_detection_tick(&self) -> DbResult<Vec<FindingEvent>>;
}

/// One method per public `crate::sessions` free function (the login/session
/// path), with a `_session(s)` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreSessions: Send + Sync {
    async fn create_session(
        &self,
        user_id: UserId,
        ttl_seconds: i64,
        ip: Option<std::net::IpAddr>,
        user_agent: Option<String>,
    ) -> DbResult<Session>;

    async fn lookup_session(&self, id: Uuid) -> DbResult<Session>;

    async fn set_session_active_org(
        &self,
        session_id: Uuid,
        user_id: UserId,
        org_id: Uuid,
    ) -> DbResult<bool>;

    async fn delete_session(&self, id: Uuid) -> DbResult<()>;

    async fn delete_sessions_for_user(&self, user_id: UserId) -> DbResult<u64>;

    async fn list_sessions_for_user(&self, user_id: UserId) -> DbResult<Vec<SessionInfo>>;

    async fn delete_one_session_for_user(&self, user_id: UserId, id: Uuid) -> DbResult<bool>;

    async fn delete_other_sessions(&self, user_id: UserId, keep: Uuid) -> DbResult<u64>;

    async fn cleanup_expired_sessions(&self) -> DbResult<u64>;
}

/// One method per public `crate::notifications` free function, with a
/// `_notification(s)`/`notification_` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreNotifications: Send + Sync {
    async fn list_notifications(&self, org_id: OrgId) -> DbResult<Vec<Notification>>;

    async fn list_all_notifications(&self) -> DbResult<Vec<Notification>>;

    async fn get_notification(&self, id: NotificationId, org_id: OrgId) -> DbResult<Notification>;

    async fn get_notification_unscoped(&self, id: NotificationId) -> DbResult<Notification>;

    async fn create_notification(
        &self,
        input: NewNotification,
        org_id: OrgId,
    ) -> DbResult<Notification>;

    async fn update_notification(
        &self,
        id: NotificationId,
        input: UpdateNotification,
        org_id: OrgId,
    ) -> DbResult<Notification>;

    async fn notification_counts_per_monitor(
        &self,
        org_id: OrgId,
    ) -> DbResult<Vec<MonitorChannelCount>>;

    async fn delete_notification(&self, id: NotificationId, org_id: OrgId) -> DbResult<()>;

    async fn attach_notification(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()>;

    async fn detach_notification(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()>;

    async fn notifications_for_monitor(&self, monitor: MonitorId) -> DbResult<Vec<Notification>>;

    async fn mark_notification_fired(&self, id: NotificationId) -> DbResult<()>;
}

/// One method per public `crate::settings` free function, with a `_setting`
/// suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreSettings: Send + Sync {
    async fn get_setting(&self, key: &str) -> DbResult<Option<serde_json::Value>>;

    async fn put_setting(&self, key: &str, value: &serde_json::Value) -> DbResult<()>;

    async fn delete_setting(&self, key: &str) -> DbResult<()>;
}

/// One method per public `crate::logs` free function, with a `_log(s)`/`log_`
/// suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreLogs: Send + Sync {
    async fn insert_logs(&self, logs: &[ParsedLog], org_id: OrgId) -> DbResult<u64>;

    async fn query_logs(&self, f: LogFilter<'_>, org_id: OrgId) -> DbResult<Vec<LogEntry>>;

    async fn log_level_counts(
        &self,
        service: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<(String, i64)>>;

    async fn log_histogram(
        &self,
        service: Option<&str>,
        min_severity: Option<i16>,
        query: Option<&str>,
        hours: i32,
        buckets: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<LogBucket>>;

    async fn log_services(&self, org_id: OrgId) -> DbResult<Vec<String>>;

    async fn prune_logs(&self, days: i32) -> DbResult<u64>;
}

/// One method per public `crate::traces` free function, with a `_trace`/`trace_`
/// suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreTraces: Send + Sync {
    async fn insert_spans(&self, spans: &[ParsedSpan], org_id: OrgId) -> DbResult<u64>;

    async fn list_traces(&self, f: TraceFilter<'_>, org_id: OrgId) -> DbResult<Vec<TraceSummary>>;

    async fn get_trace_spans(&self, trace_id: &str, org_id: OrgId) -> DbResult<Vec<Span>>;

    async fn trace_service_map(
        &self,
        window_hours: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ServiceEdge>>;

    async fn trace_operation_stats(
        &self,
        service: &str,
        window_hours: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<OperationStat>>;

    async fn trace_operation_trend(
        &self,
        service: &str,
        operation: &str,
        window_hours: i64,
        buckets: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<f64>>;

    async fn prune_spans(&self, days: i32) -> DbResult<u64>;
}

/// One method per public `crate::rum` free function, with a `rum_` prefix /
/// `_rum_event` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreRum: Send + Sync {
    async fn insert_rum_event(&self, b: &RumBeacon, org_id: OrgId) -> DbResult<()>;

    async fn rum_page_samples(
        &self,
        app: Option<&str>,
        url: &str,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RumSample>>;

    async fn rum_recent_traced(
        &self,
        app: Option<&str>,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RumTracedLoad>>;

    async fn rum_summary(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<RumVitals>;

    async fn rum_pages(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumPage>>;

    async fn rum_browser_breakdown(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumBrowser>>;

    async fn rum_user_breakdown(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumUser>>;

    async fn rum_apps(&self, org_id: OrgId) -> DbResult<Vec<String>>;

    async fn prune_rum(&self, days: i32) -> DbResult<u64>;
}

/// One method per public `crate::profiles` free function, with a `profile_` /
/// `_profile(s)` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreProfiles: Send + Sync {
    async fn insert_profile(&self, p: NewProfile<'_>, org_id: OrgId) -> DbResult<i64>;

    async fn list_profiles(
        &self,
        service: Option<&str>,
        profile_type: Option<&str>,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ProfileMeta>>;

    async fn profile_folded_in_window(
        &self,
        service: &str,
        profile_type: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        org_id: OrgId,
    ) -> DbResult<Vec<Vec<u8>>>;

    async fn profile_fetch_folded(
        &self,
        id: i64,
        org_id: OrgId,
    ) -> DbResult<Option<(String, Vec<u8>)>>;

    async fn profile_services(&self, org_id: OrgId) -> DbResult<Vec<String>>;

    async fn profile_types(&self, service: Option<&str>, org_id: OrgId) -> DbResult<Vec<String>>;

    async fn prune_profiles(&self, days: i32) -> DbResult<u64>;
}

/// One method per public `crate::metrics` free function. These are the
/// parameter-free `/metrics` exposition aggregates; names carry a `metric_`/
/// `_metrics` flavour where needed to avoid collision with the rule domains.
#[async_trait::async_trait]
pub trait StoreMetrics: Send + Sync {
    async fn monitors_by_status(&self) -> DbResult<Vec<(String, i64)>>;

    async fn monitors_by_kind(&self) -> DbResult<Vec<(String, i64)>>;

    async fn channels_active(&self) -> DbResult<i64>;

    async fn webpush_subscribers(&self) -> DbResult<i64>;

    async fn heartbeats_recent_by_status(
        &self,
        window_seconds: i64,
    ) -> DbResult<Vec<(String, i64)>>;

    async fn incidents_open(&self) -> DbResult<i64>;

    async fn pipeline_gauges(&self) -> DbResult<PipelineGauges>;

    async fn storage_usage(&self) -> DbResult<Vec<TableSize>>;

    async fn ingest_gauges(&self) -> DbResult<IngestGauges>;
}

/// One method per public `crate::error_tracking` free function, with an
/// `_error_project`/`_error_issue`/`error_` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreErrorTracking: Send + Sync {
    async fn list_error_projects(&self, org_id: OrgId) -> DbResult<Vec<ErrorProject>>;

    async fn error_project_in_org(&self, project: ErrorProjectId, org_id: OrgId) -> DbResult<()>;

    async fn error_issue_in_org(&self, issue: ErrorIssueId, org_id: OrgId) -> DbResult<()>;

    async fn get_error_project(&self, id: ErrorProjectId) -> DbResult<ErrorProject>;

    async fn org_for_error_project(&self, id: ErrorProjectId) -> DbResult<OrgId>;

    async fn get_error_project_opt(&self, id: ErrorProjectId) -> DbResult<Option<ErrorProject>>;

    async fn find_or_create_error_project_by_name(
        &self,
        name: &str,
        org_id: OrgId,
    ) -> DbResult<ErrorProject>;

    async fn create_error_project(
        &self,
        input: NewErrorProject,
        org_id: OrgId,
    ) -> DbResult<ErrorProject>;

    async fn update_error_project(
        &self,
        id: ErrorProjectId,
        patch: UpdateErrorProject,
        org_id: OrgId,
    ) -> DbResult<ErrorProject>;

    async fn delete_error_project(&self, id: ErrorProjectId, org_id: OrgId) -> DbResult<()>;

    async fn record_error_event(
        &self,
        project_id: ErrorProjectId,
        ev: &ParsedEvent,
    ) -> DbResult<RecordOutcome>;

    async fn error_issues_for_trace(
        &self,
        trace_id: &str,
        org_id: OrgId,
    ) -> DbResult<Vec<TraceErrorRef>>;

    async fn list_error_issues(
        &self,
        project_id: ErrorProjectId,
        status: Option<&str>,
        before_id: Option<Uuid>,
        limit: i64,
    ) -> DbResult<Vec<ErrorIssue>>;

    async fn recent_open_error_issues(
        &self,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ErrorIssue>>;

    async fn error_project_event_histogram(
        &self,
        project_id: ErrorProjectId,
        hours: i32,
        buckets: i64,
    ) -> DbResult<Vec<ErrorBucket>>;

    async fn get_error_issue(&self, id: ErrorIssueId) -> DbResult<ErrorIssue>;

    async fn error_issue_affected_users(
        &self,
        id: ErrorIssueId,
        limit: i64,
    ) -> DbResult<Vec<AffectedUser>>;

    async fn error_issue_stats(&self, id: ErrorIssueId) -> DbResult<IssueStats>;

    async fn set_error_issue_status(&self, id: ErrorIssueId, status: &str) -> DbResult<ErrorIssue>;

    async fn assign_error_issue(
        &self,
        id: ErrorIssueId,
        assignee: Option<UserId>,
    ) -> DbResult<ErrorIssue>;

    async fn error_assignable_users(&self) -> DbResult<Vec<crate::error_tracking::AssignableUser>>;

    async fn list_error_events(
        &self,
        issue_id: ErrorIssueId,
        limit: i64,
    ) -> DbResult<Vec<ErrorEvent>>;

    async fn prune_error_events(&self) -> DbResult<u64>;
}

/// One method per public `crate::scheduled_reports` free function, with a
/// `_scheduled_report` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreScheduledReports: Send + Sync {
    async fn list_scheduled_reports(&self, org_id: OrgId) -> DbResult<Vec<ScheduledReport>>;

    async fn get_scheduled_report(
        &self,
        id: ScheduledReportId,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport>;

    async fn create_scheduled_report(
        &self,
        input: NewScheduledReport,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport>;

    async fn update_scheduled_report(
        &self,
        id: ScheduledReportId,
        input: UpdateScheduledReport,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport>;

    async fn delete_scheduled_report(&self, id: ScheduledReportId, org_id: OrgId) -> DbResult<()>;

    async fn due_scheduled_reports(&self, now: OffsetDateTime) -> DbResult<Vec<ScheduledReport>>;

    async fn render_scheduled_report(
        &self,
        report_name: &str,
        cadence: &str,
    ) -> DbResult<(String, String)>;

    async fn mark_scheduled_report_sent(&self, id: ScheduledReportId) -> DbResult<()>;
}

/// One method per public `crate::incident_templates` free function, with an
/// `_incident_template(s)` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreIncidentTemplates: Send + Sync {
    async fn list_incident_templates(&self, org_id: OrgId) -> DbResult<Vec<IncidentTemplate>>;

    async fn get_incident_template(
        &self,
        id: IncidentTemplateId,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate>;

    async fn create_incident_template(
        &self,
        input: NewIncidentTemplate,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate>;

    async fn update_incident_template(
        &self,
        id: IncidentTemplateId,
        input: UpdateIncidentTemplate,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate>;

    async fn delete_incident_template(&self, id: IncidentTemplateId, org_id: OrgId)
        -> DbResult<()>;
}

/// One method per public `crate::monitor_presets` free function, with a
/// `_monitor_preset(s)` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreMonitorPresets: Send + Sync {
    async fn list_monitor_presets(&self, org_id: OrgId) -> DbResult<Vec<MonitorPreset>>;

    async fn get_monitor_preset(
        &self,
        id: MonitorPresetId,
        org_id: OrgId,
    ) -> DbResult<MonitorPreset>;

    async fn create_monitor_preset(
        &self,
        input: NewMonitorPreset,
        org_id: OrgId,
    ) -> DbResult<MonitorPreset>;

    async fn delete_monitor_preset(&self, id: MonitorPresetId, org_id: OrgId) -> DbResult<()>;
}

/// One method per public `crate::monitor_templates` free function, with a
/// `_monitor_template(s)` suffix on the collision-prone CRUD names. Distinct
/// from `StoreTemplates` (notification templates → `crate::templates`).
#[async_trait::async_trait]
pub trait StoreMonitorTemplates: Send + Sync {
    async fn list_monitor_templates(&self, org_id: OrgId) -> DbResult<Vec<MonitorTemplate>>;

    async fn get_monitor_template(
        &self,
        id: MonitorTemplateId,
        org_id: OrgId,
    ) -> DbResult<MonitorTemplate>;

    async fn create_monitor_template(
        &self,
        input: NewMonitorTemplate,
        org_id: OrgId,
    ) -> DbResult<MonitorTemplate>;

    async fn delete_monitor_template(&self, id: MonitorTemplateId, org_id: OrgId) -> DbResult<()>;
}

/// One method per public `crate::delivery_log` free function, with a
/// `_deliver(y|ies)` suffix on the collision-prone names.
#[async_trait::async_trait]
pub trait StoreDeliveryLog: Send + Sync {
    async fn record_delivery(&self, entry: NewDelivery<'_>) -> DbResult<DeliveryEntry>;

    async fn get_delivery(&self, id: i64, org_id: OrgId) -> DbResult<Option<DeliveryEntry>>;

    async fn list_deliveries(
        &self,
        limit: i64,
        before_ts: Option<OffsetDateTime>,
        org_id: OrgId,
    ) -> DbResult<Vec<DeliveryEntry>>;

    async fn list_all_deliveries(&self, limit: i64, org_id: OrgId) -> DbResult<Vec<DeliveryEntry>>;
}

/// One method per public `crate::agents` free function, with an `_agent(s)`
/// suffix on the collision-prone names (distinct from heartbeats / api_keys).
#[async_trait::async_trait]
pub trait StoreAgents: Send + Sync {
    async fn list_agents(&self, org_id: OrgId) -> DbResult<Vec<Agent>>;

    async fn get_agent(&self, id: AgentId, org_id: OrgId) -> DbResult<Agent>;

    async fn create_agent(&self, input: NewAgent, org_id: OrgId) -> DbResult<IssuedAgent>;

    async fn update_agent(&self, id: AgentId, patch: UpdateAgent, org_id: OrgId)
        -> DbResult<Agent>;

    async fn delete_agent(&self, id: AgentId, org_id: OrgId) -> DbResult<()>;

    async fn lookup_agent(&self, token: &str) -> DbResult<Agent>;

    async fn touch_agent_seen(&self, id: AgentId, version: Option<&str>) -> DbResult<()>;
}

/// One method per public `crate::metric_samples` free function, with a
/// `metric_sample(s)` flavour so it doesn't collide with `StoreMetrics` /
/// `StoreMetricRules`. These are the externally-pushed samples, not the
/// `/metrics` exposition aggregates.
#[async_trait::async_trait]
pub trait StoreMetricSamples: Send + Sync {
    async fn insert_metric_samples(&self, samples: &[PromSample], org_id: OrgId) -> DbResult<()>;

    async fn list_metric_sample_series(&self, org_id: OrgId) -> DbResult<Vec<Series>>;

    async fn metric_sample_range_query(
        &self,
        name: &str,
        labels: &serde_json::Value,
        from: OffsetDateTime,
        to: OffsetDateTime,
        step_seconds: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RangePoint>>;

    async fn metric_sample_baseline(
        &self,
        name: &str,
        labels: &serde_json::Value,
        window_secs: i64,
        org_id: OrgId,
    ) -> DbResult<Option<(f64, f64)>>;

    async fn metric_sample_latest(
        &self,
        name: &str,
        labels: &serde_json::Value,
        org_id: OrgId,
    ) -> DbResult<Option<(f64, OffsetDateTime)>>;

    async fn prune_metric_samples_older_than(&self, cutoff: OffsetDateTime) -> DbResult<u64>;
}

/// One method per public `crate::source_maps` free function, with a
/// `_source_map(s)` suffix on the collision-prone CRUD names.
#[async_trait::async_trait]
pub trait StoreSourceMaps: Send + Sync {
    async fn upsert_source_map(&self, m: NewSourceMap<'_>) -> DbResult<i64>;

    async fn get_source_map(
        &self,
        project_id: Uuid,
        release: &str,
        filename: &str,
    ) -> DbResult<Option<serde_json::Value>>;

    async fn list_source_maps(&self, project_id: Uuid) -> DbResult<Vec<SourceMapMeta>>;

    async fn delete_source_map(&self, project_id: Uuid, id: i64) -> DbResult<bool>;
}

/// One method per public `crate::users` free function (the auth-critical
/// account domain: login, session, `/me`, RBAC, org membership, TOTP, GDPR).
/// Names carry a `_user(s)` suffix so the collision-prone CRUD verbs
/// (`count`/`create`/`get`/`list`/`delete`/`set_*`) don't clash with the
/// other ~40 sub-traits.
#[async_trait::async_trait]
pub trait StoreUsers: Send + Sync {
    async fn count_users(&self) -> DbResult<i64>;

    async fn create_user(&self, input: NewUser) -> DbResult<User>;

    async fn get_user_by_email(&self, email: &str) -> DbResult<UserWithHash>;

    async fn user_by_email(&self, email: &str) -> DbResult<Option<User>>;

    async fn get_user(&self, id: UserId) -> DbResult<User>;

    async fn set_user_totp_secret(&self, id: UserId, secret: &str) -> DbResult<()>;

    async fn enable_user_totp(&self, id: UserId) -> DbResult<()>;

    async fn disable_user_totp(&self, id: UserId) -> DbResult<()>;

    async fn mark_user_login(&self, id: UserId) -> DbResult<()>;

    async fn user_totp_locked_until(&self, id: UserId) -> DbResult<Option<OffsetDateTime>>;

    async fn record_user_totp_failure(
        &self,
        id: UserId,
        max_attempts: i32,
        lockout_mins: i32,
    ) -> DbResult<bool>;

    async fn reset_user_totp_failures(&self, id: UserId) -> DbResult<()>;

    async fn list_users(&self) -> DbResult<Vec<User>>;

    async fn set_user_admin(&self, id: UserId, is_admin: bool) -> DbResult<()>;

    async fn set_user_role(&self, id: UserId, role: Role) -> DbResult<()>;

    async fn delete_user(&self, id: UserId) -> DbResult<()>;

    async fn anonymize_user(&self, id: UserId) -> DbResult<()>;

    async fn get_user_prefs(&self, id: UserId) -> DbResult<serde_json::Value>;

    async fn set_user_prefs(&self, id: UserId, prefs: &serde_json::Value) -> DbResult<()>;

    async fn set_user_password(&self, id: UserId, hash: &str) -> DbResult<()>;
}

#[async_trait::async_trait]
pub trait StoreWebpush: Send + Sync {
    async fn list_webpush_subs(
        &self,
        notification: NotificationId,
    ) -> DbResult<Vec<crate::webpush::WebpushSubscription>>;

    async fn upsert_webpush_sub(
        &self,
        notification: NotificationId,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
    ) -> DbResult<()>;

    async fn delete_webpush_sub_by_endpoint(&self, endpoint: &str) -> DbResult<()>;

    async fn delete_webpush_sub(&self, id: Uuid) -> DbResult<()>;

    /// Read the shared VAPID keypair (absent/corrupt → `None`).
    async fn get_vapid_keys(&self) -> DbResult<Option<crate::webpush::VapidKeys>>;

    /// Persist the shared VAPID keypair.
    async fn set_vapid_keys(&self, keys: &crate::webpush::VapidKeys) -> DbResult<()>;
}

#[async_trait::async_trait]
pub trait StoreOrgs: Send + Sync {
    async fn create_org(&self, slug: &str, name: &str) -> DbResult<rampart_core::org::Org>;

    async fn get_org(&self, id: OrgId) -> DbResult<rampart_core::org::Org>;

    async fn orgs_for_user(&self, user_id: UserId) -> DbResult<Vec<rampart_core::org::Org>>;

    /// Add or update a membership (pool-scoped). The generic-executor free fn
    /// `orgs::upsert_member` stays for tx-atomic callers (it can't sit on a
    /// `dyn Store` method — generic `PgExecutor` bound isn't object-safe).
    async fn upsert_org_member(&self, org_id: OrgId, user_id: UserId, role: Role) -> DbResult<()>;

    async fn org_member_role(&self, org_id: OrgId, user_id: UserId) -> DbResult<Option<Role>>;

    async fn list_org_members(&self, org_id: OrgId) -> DbResult<Vec<rampart_core::org::OrgMember>>;

    async fn list_org_members_detailed(
        &self,
        org_id: OrgId,
    ) -> DbResult<Vec<crate::orgs::OrgMemberDetail>>;

    async fn update_org(&self, id: OrgId, name: &str) -> DbResult<rampart_core::org::Org>;

    async fn org_by_slug(&self, slug: &str) -> DbResult<rampart_core::org::Org>;

    async fn remove_org_member(&self, org_id: OrgId, user_id: UserId) -> DbResult<bool>;

    async fn count_org_admins(&self, org_id: OrgId) -> DbResult<i64>;

    async fn create_org_with_owner(
        &self,
        slug: &str,
        name: &str,
        owner: UserId,
    ) -> DbResult<rampart_core::org::Org>;
}

/// Composed store super-trait spanning every extracted domain sub-trait.
pub trait Store:
    StoreHeartbeats
    + StoreDeployMarkers
    + StoreIngestKeys
    + StoreSlos
    + StoreProxies
    + StoreOnCall
    + StoreRecoveryCodes
    + StoreApiKeys
    + StoreEscalations
    + StoreMaintenance
    + StoreIngestTokens
    + StoreTags
    + StoreTemplates
    + StoreTelemetryRules
    + StoreMetricRules
    + StoreMonitorGroups
    + StoreSilences
    + StoreOidcState
    + StoreStatusPages
    + StoreIncidents
    + StoreRouting
    + StoreSubscribers
    + StoreDetection
    + StoreSessions
    + StoreNotifications
    + StoreSettings
    + StoreLogs
    + StoreTraces
    + StoreRum
    + StoreProfiles
    + StoreMetrics
    + StoreErrorTracking
    + StoreScheduledReports
    + StoreIncidentTemplates
    + StoreMonitorPresets
    + StoreMonitorTemplates
    + StoreDeliveryLog
    + StoreAgents
    + StoreMetricSamples
    + StoreSourceMaps
    + StoreUsers
    + StoreWebpush
    + StoreOrgs
    + Send
    + Sync
{
}

/// The single Postgres-backed implementation. Holds a pool and delegates every
/// trait method to the matching `crate::heartbeats` free function.
pub struct PgStore {
    pool: DbPool,
}

impl PgStore {
    /// Construct from a pool. Does no I/O.
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl StoreHeartbeats for PgStore {
    async fn insert_many(&self, hbs: &[Heartbeat]) -> DbResult<()> {
        crate::heartbeats::insert_many(&self.pool, hbs).await
    }

    async fn recent_for_monitor(&self, monitor: MonitorId, limit: i64) -> DbResult<Vec<Heartbeat>> {
        crate::heartbeats::recent_for_monitor(&self.pool, monitor, limit).await
    }

    async fn recent_for_monitor_before(
        &self,
        monitor: MonitorId,
        limit: i64,
        before: Option<time::OffsetDateTime>,
    ) -> DbResult<Vec<Heartbeat>> {
        crate::heartbeats::recent_for_monitor_before(&self.pool, monitor, limit, before).await
    }

    async fn range_for_monitor(
        &self,
        monitor: MonitorId,
        since: time::OffsetDateTime,
        until: time::OffsetDateTime,
        limit: i64,
    ) -> DbResult<Vec<Heartbeat>> {
        crate::heartbeats::range_for_monitor(&self.pool, monitor, since, until, limit).await
    }

    async fn uptime_pct(&self, monitor: MonitorId, window_seconds: i64) -> DbResult<Option<f64>> {
        crate::heartbeats::uptime_pct(&self.pool, monitor, window_seconds).await
    }

    async fn current_slo_uptime_pct(
        &self,
        monitor: MonitorId,
        window_days: i32,
    ) -> DbResult<Option<f64>> {
        crate::heartbeats::current_slo_uptime_pct(&self.pool, monitor, window_days).await
    }

    async fn avg_latency_ms(
        &self,
        monitor: MonitorId,
        window_seconds: i64,
    ) -> DbResult<Option<f64>> {
        crate::heartbeats::avg_latency_ms(&self.pool, monitor, window_seconds).await
    }

    async fn daily_status(&self, monitor: MonitorId, days: i32) -> DbResult<Vec<u8>> {
        crate::heartbeats::daily_status(&self.pool, monitor, days).await
    }

    async fn day_hourly_latency(
        &self,
        monitor: MonitorId,
        day: time::Date,
    ) -> DbResult<Vec<(i32, Option<f32>, i32)>> {
        crate::heartbeats::day_hourly_latency(&self.pool, monitor, day).await
    }

    async fn monthly_uptime(
        &self,
        monitor: MonitorId,
        months: i32,
    ) -> DbResult<Vec<MonthlyUptime>> {
        crate::heartbeats::monthly_uptime(&self.pool, monitor, months).await
    }

    async fn uptime_pct_batch(
        &self,
        monitor_ids: &[Uuid],
        window_seconds: i64,
    ) -> DbResult<HashMap<Uuid, f64>> {
        crate::heartbeats::uptime_pct_batch(&self.pool, monitor_ids, window_seconds).await
    }

    async fn avg_latency_ms_batch(
        &self,
        monitor_ids: &[Uuid],
        window_seconds: i64,
    ) -> DbResult<HashMap<Uuid, f64>> {
        crate::heartbeats::avg_latency_ms_batch(&self.pool, monitor_ids, window_seconds).await
    }

    async fn daily_status_batch(
        &self,
        monitor_ids: &[Uuid],
        days: i32,
    ) -> DbResult<HashMap<Uuid, Vec<u8>>> {
        crate::heartbeats::daily_status_batch(&self.pool, monitor_ids, days).await
    }

    async fn monthly_uptime_batch(
        &self,
        monitor_ids: &[Uuid],
        months: i32,
    ) -> DbResult<HashMap<Uuid, Vec<MonthlyUptime>>> {
        crate::heartbeats::monthly_uptime_batch(&self.pool, monitor_ids, months).await
    }

    async fn summary_window(
        &self,
        window_seconds: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<MonitorSummary>> {
        crate::heartbeats::summary_window(&self.pool, window_seconds, org_id).await
    }

    async fn mtbf_mttr(&self, monitor: MonitorId, window_seconds: i64) -> DbResult<MtbfMttr> {
        crate::heartbeats::mtbf_mttr(&self.pool, monitor, window_seconds).await
    }

    async fn error_budget(
        &self,
        monitor: MonitorId,
        window_days: i32,
        target_pct: f64,
    ) -> DbResult<ErrorBudget> {
        crate::heartbeats::error_budget(&self.pool, monitor, window_days, target_pct).await
    }

    async fn error_budget_burndown(
        &self,
        monitor: MonitorId,
        window_days: i32,
        target_pct: f64,
    ) -> DbResult<Vec<BurndownPoint>> {
        crate::heartbeats::error_budget_burndown(&self.pool, monitor, window_days, target_pct).await
    }

    async fn recent_per_monitor(
        &self,
        per_monitor: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<Heartbeat>> {
        crate::heartbeats::recent_per_monitor(&self.pool, per_monitor, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreDeployMarkers for PgStore {
    async fn create_deploy_marker(
        &self,
        input: NewDeployMarker,
        org_id: OrgId,
    ) -> DbResult<DeployMarker> {
        crate::deploy_markers::create(&self.pool, input, org_id).await
    }

    async fn list_deploy_markers_window(
        &self,
        hours: i32,
        service: Option<&str>,
        org_id: OrgId,
    ) -> DbResult<Vec<DeployMarker>> {
        crate::deploy_markers::list_window(&self.pool, hours, service, org_id).await
    }

    async fn delete_deploy_marker(&self, id: DeployMarkerId, org_id: OrgId) -> DbResult<()> {
        crate::deploy_markers::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreIngestKeys for PgStore {
    async fn create_ingest_key(
        &self,
        org_id: OrgId,
        label: &str,
        kind: &str,
        allowed_origins: &[String],
    ) -> DbResult<(IngestKey, String)> {
        crate::ingest_keys::create(&self.pool, org_id, label, kind, allowed_origins).await
    }

    async fn find_ingest_key_by_token(
        &self,
        token: &str,
    ) -> DbResult<Option<(Uuid, OrgId, Vec<String>)>> {
        crate::ingest_keys::find_by_token(&self.pool, token).await
    }

    async fn touch_ingest_key_last_used(&self, id: Uuid) -> DbResult<()> {
        crate::ingest_keys::touch_last_used(&self.pool, id).await
    }

    async fn list_ingest_keys_for_org(&self, org_id: OrgId) -> DbResult<Vec<IngestKey>> {
        crate::ingest_keys::list_for_org(&self.pool, org_id).await
    }

    async fn delete_ingest_key(&self, id: Uuid, org_id: OrgId) -> DbResult<bool> {
        crate::ingest_keys::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreSlos for PgStore {
    async fn list_slos(&self, org_id: OrgId) -> DbResult<Vec<Slo>> {
        crate::slos::list(&self.pool, org_id).await
    }

    async fn list_all_slos(&self) -> DbResult<Vec<Slo>> {
        crate::slos::list_all(&self.pool).await
    }

    async fn get_slo(&self, id: SloId, org_id: OrgId) -> DbResult<Slo> {
        crate::slos::get(&self.pool, id, org_id).await
    }

    async fn get_slo_unscoped(&self, id: SloId) -> DbResult<Slo> {
        crate::slos::get_unscoped(&self.pool, id).await
    }

    async fn create_slo(&self, input: NewSlo, org_id: OrgId) -> DbResult<Slo> {
        crate::slos::create(&self.pool, input, org_id).await
    }

    async fn update_slo(&self, id: SloId, patch: UpdateSlo, org_id: OrgId) -> DbResult<Slo> {
        crate::slos::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_slo(&self, id: SloId, org_id: OrgId) -> DbResult<()> {
        crate::slos::delete(&self.pool, id, org_id).await
    }

    async fn compute_slo(&self, slo: &Slo) -> DbResult<SloSnapshot> {
        crate::slos::compute(&self.pool, slo).await
    }

    async fn slo_trend(&self, slo: &Slo, buckets: i64) -> DbResult<Vec<f64>> {
        crate::slos::trend(&self.pool, slo, buckets).await
    }

    async fn list_slos_with_snapshots(&self, org_id: OrgId) -> DbResult<Vec<SloWithSnapshot>> {
        crate::slos::list_with_snapshots(&self.pool, org_id).await
    }

    async fn evaluate_slos_tick(&self) -> DbResult<Vec<SloEvent>> {
        crate::slos::evaluate_tick(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreProxies for PgStore {
    async fn list_proxies(&self, org_id: OrgId) -> DbResult<Vec<Proxy>> {
        crate::proxies::list(&self.pool, org_id).await
    }

    async fn get_proxy(&self, id: ProxyId, org_id: OrgId) -> DbResult<Proxy> {
        crate::proxies::get(&self.pool, id, org_id).await
    }

    async fn get_proxy_unscoped(&self, id: ProxyId) -> DbResult<Proxy> {
        crate::proxies::get_unscoped(&self.pool, id).await
    }

    async fn create_proxy(&self, input: NewProxy, org_id: OrgId) -> DbResult<Proxy> {
        crate::proxies::create(&self.pool, input, org_id).await
    }

    async fn delete_proxy(&self, id: ProxyId, org_id: OrgId) -> DbResult<()> {
        crate::proxies::delete(&self.pool, id, org_id).await
    }

    async fn set_active_proxy(&self, id: ProxyId, active: bool, org_id: OrgId) -> DbResult<()> {
        crate::proxies::set_active(&self.pool, id, active, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreOnCall for PgStore {
    async fn list_on_call(&self, org_id: OrgId) -> DbResult<Vec<OnCallSchedule>> {
        crate::on_call::list(&self.pool, org_id).await
    }

    async fn get_on_call(&self, id: OnCallScheduleId, org_id: OrgId) -> DbResult<OnCallSchedule> {
        crate::on_call::get(&self.pool, id, org_id).await
    }

    async fn get_on_call_unscoped(&self, id: OnCallScheduleId) -> DbResult<OnCallSchedule> {
        crate::on_call::get_unscoped(&self.pool, id).await
    }

    async fn create_on_call(
        &self,
        input: NewOnCallSchedule,
        org_id: OrgId,
    ) -> DbResult<OnCallSchedule> {
        crate::on_call::create(&self.pool, input, org_id).await
    }

    async fn update_on_call(
        &self,
        id: OnCallScheduleId,
        patch: UpdateOnCallSchedule,
        org_id: OrgId,
    ) -> DbResult<OnCallSchedule> {
        crate::on_call::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_on_call(&self, id: OnCallScheduleId, org_id: OrgId) -> DbResult<()> {
        crate::on_call::delete(&self.pool, id, org_id).await
    }

    async fn oncall_current_channel(
        &self,
        id: OnCallScheduleId,
        at: OffsetDateTime,
    ) -> DbResult<Option<NotificationId>> {
        crate::on_call::current_channel(&self.pool, id, at).await
    }

    async fn oncall_current_target(
        &self,
        id: OnCallScheduleId,
        at: OffsetDateTime,
    ) -> DbResult<Option<OnCallTarget>> {
        crate::on_call::current_target(&self.pool, id, at).await
    }
}

#[async_trait::async_trait]
impl StoreRecoveryCodes for PgStore {
    async fn issue_recovery_codes(&self, user: UserId, count: usize) -> DbResult<Vec<String>> {
        crate::recovery_codes::issue_batch(&self.pool, user, count).await
    }

    async fn consume_recovery_code(&self, user: UserId, code: &str) -> DbResult<bool> {
        crate::recovery_codes::consume(&self.pool, user, code).await
    }

    async fn delete_recovery_codes_for_user(&self, user: UserId) -> DbResult<()> {
        crate::recovery_codes::delete_for_user(&self.pool, user).await
    }

    async fn remaining_recovery_codes(&self, user: UserId) -> DbResult<i64> {
        crate::recovery_codes::remaining(&self.pool, user).await
    }
}

#[async_trait::async_trait]
impl StoreApiKeys for PgStore {
    async fn list_api_keys(&self, org_id: OrgId) -> DbResult<Vec<ApiKey>> {
        crate::api_keys::list(&self.pool, org_id).await
    }

    async fn create_api_key(
        &self,
        input: NewApiKey,
        created_by: UserId,
        org_id: OrgId,
    ) -> DbResult<IssuedApiKey> {
        crate::api_keys::create(&self.pool, input, created_by, org_id).await
    }

    async fn delete_api_key(&self, id: ApiKeyId, org_id: OrgId) -> DbResult<()> {
        crate::api_keys::delete(&self.pool, id, org_id).await
    }

    async fn lookup_api_key(&self, token: &str) -> DbResult<(ApiKey, UserId, OrgId)> {
        crate::api_keys::lookup(&self.pool, token).await
    }

    async fn touch_api_key_last_used(&self, id: ApiKeyId) -> DbResult<()> {
        crate::api_keys::touch_last_used(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreEscalations for PgStore {
    async fn list_escalation_policies(&self, org_id: OrgId) -> DbResult<Vec<EscalationPolicy>> {
        crate::escalations::list(&self.pool, org_id).await
    }

    async fn get_escalation_policy(
        &self,
        id: EscalationPolicyId,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy> {
        crate::escalations::get(&self.pool, id, org_id).await
    }

    async fn get_escalation_policy_unscoped(
        &self,
        id: EscalationPolicyId,
    ) -> DbResult<EscalationPolicy> {
        crate::escalations::get_unscoped(&self.pool, id).await
    }

    async fn create_escalation_policy(
        &self,
        input: NewEscalationPolicy,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy> {
        crate::escalations::create(&self.pool, input, org_id).await
    }

    async fn update_escalation_policy(
        &self,
        id: EscalationPolicyId,
        patch: UpdateEscalationPolicy,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy> {
        crate::escalations::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_escalation_policy(
        &self,
        id: EscalationPolicyId,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::escalations::delete(&self.pool, id, org_id).await
    }

    async fn open_episode(
        &self,
        monitor_id: MonitorId,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::escalations::open_episode(&self.pool, monitor_id, policy).await
    }

    async fn open_episode_for_subject(
        &self,
        kind: &str,
        subject_ref: &str,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::escalations::open_episode_for_subject(&self.pool, kind, subject_ref, policy).await
    }

    async fn resolve_subject(
        &self,
        kind: &str,
        subject_ref: &str,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::escalations::resolve_subject(&self.pool, kind, subject_ref).await
    }

    async fn ack_episode(&self, episode_id: Uuid, by: UserId) -> DbResult<EscalationEpisode> {
        crate::escalations::ack_episode(&self.pool, episode_id, by).await
    }

    async fn list_open_episodes(&self) -> DbResult<Vec<EscalationEpisode>> {
        crate::escalations::list_open(&self.pool).await
    }

    async fn list_open_episodes_for_org(&self, org_id: OrgId) -> DbResult<Vec<EscalationEpisode>> {
        crate::escalations::list_open_for_org(&self.pool, org_id).await
    }

    async fn episode_in_org(&self, episode: Uuid, org_id: OrgId) -> DbResult<()> {
        crate::escalations::episode_in_org(&self.pool, episode, org_id).await
    }

    async fn open_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::escalations::open_for_monitor(&self.pool, monitor_id).await
    }

    async fn ack_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
        by: UserId,
    ) -> DbResult<EscalationEpisode> {
        crate::escalations::ack(&self.pool, monitor_id, by).await
    }

    async fn resolve_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::escalations::resolve(&self.pool, monitor_id).await
    }

    async fn advance_episode(
        &self,
        episode_id: Uuid,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::escalations::advance(&self.pool, episode_id, policy).await
    }

    async fn due_episodes(&self) -> DbResult<Vec<EscalationEpisode>> {
        crate::escalations::due(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreMaintenance for PgStore {
    async fn list_maintenance_windows(&self, org_id: OrgId) -> DbResult<Vec<MaintenanceWindow>> {
        crate::maintenance::list(&self.pool, org_id).await
    }

    async fn get_maintenance_window(
        &self,
        id: MaintenanceId,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow> {
        crate::maintenance::get(&self.pool, id, org_id).await
    }

    async fn create_maintenance_window(
        &self,
        input: NewMaintenanceWindow,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow> {
        crate::maintenance::create(&self.pool, input, org_id).await
    }

    async fn update_maintenance_window(
        &self,
        id: MaintenanceId,
        patch: UpdateMaintenanceWindow,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow> {
        crate::maintenance::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_maintenance_window(&self, id: MaintenanceId, org_id: OrgId) -> DbResult<()> {
        crate::maintenance::delete(&self.pool, id, org_id).await
    }

    async fn set_active_maintenance(
        &self,
        id: MaintenanceId,
        active: bool,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::maintenance::set_active(&self.pool, id, active, org_id).await
    }

    async fn attach_maintenance_monitor(
        &self,
        window: MaintenanceId,
        monitor: MonitorId,
    ) -> DbResult<()> {
        crate::maintenance::attach(&self.pool, window, monitor).await
    }

    async fn detach_maintenance_monitor(
        &self,
        window: MaintenanceId,
        monitor: MonitorId,
    ) -> DbResult<()> {
        crate::maintenance::detach(&self.pool, window, monitor).await
    }

    async fn is_in_active_window(&self, monitor: MonitorId) -> DbResult<bool> {
        crate::maintenance::is_in_active_window(&self.pool, monitor).await
    }

    async fn maintenance_transitions_needing_notification(
        &self,
    ) -> DbResult<Vec<MaintenanceTransition>> {
        crate::maintenance::transitions_needing_notification(&self.pool).await
    }

    async fn mark_maintenance_notified_start(&self, id: MaintenanceId) -> DbResult<()> {
        crate::maintenance::mark_notified_start(&self.pool, id).await
    }

    async fn mark_maintenance_notified_end(&self, id: MaintenanceId) -> DbResult<()> {
        crate::maintenance::mark_notified_end(&self.pool, id).await
    }

    async fn confirmed_subscriber_emails_for_monitors(
        &self,
        monitors: &[MonitorId],
    ) -> DbResult<Vec<String>> {
        crate::maintenance::confirmed_subscriber_emails_for_monitors(&self.pool, monitors).await
    }

    async fn public_maintenance_for_status_page(
        &self,
        page: StatusPageId,
    ) -> DbResult<Vec<PublicMaintenance>> {
        crate::maintenance::public_for_status_page(&self.pool, page).await
    }
}

#[async_trait::async_trait]
impl StoreIngestTokens for PgStore {
    async fn create_ingest_token(
        &self,
        page: StatusPageId,
        input: NewIngestToken,
    ) -> DbResult<IngestToken> {
        crate::ingest_tokens::create(&self.pool, page, input).await
    }

    async fn create_ingest_token_with_token(
        &self,
        page: StatusPageId,
        label: &str,
        token: &str,
    ) -> DbResult<IngestToken> {
        crate::ingest_tokens::create_with_token(&self.pool, page, label, token).await
    }

    async fn set_ingest_token_mapping(
        &self,
        id: IngestTokenId,
        mapping: Option<serde_json::Value>,
        org_id: OrgId,
    ) -> DbResult<IngestToken> {
        crate::ingest_tokens::set_mapping(&self.pool, id, mapping, org_id).await
    }

    async fn list_ingest_tokens_for_page(&self, page: StatusPageId) -> DbResult<Vec<IngestToken>> {
        crate::ingest_tokens::list_for_page(&self.pool, page).await
    }

    async fn find_ingest_token_by_token(&self, token: &str) -> DbResult<IngestToken> {
        crate::ingest_tokens::find_by_token(&self.pool, token).await
    }

    async fn delete_ingest_token(&self, id: IngestTokenId, org_id: OrgId) -> DbResult<()> {
        crate::ingest_tokens::delete(&self.pool, id, org_id).await
    }

    async fn touch_ingest_token_last_used(&self, id: IngestTokenId) -> DbResult<()> {
        crate::ingest_tokens::touch_last_used(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreTags for PgStore {
    async fn list_tags(&self, org_id: OrgId) -> DbResult<Vec<Tag>> {
        crate::tags::list(&self.pool, org_id).await
    }

    async fn get_tag(&self, id: TagId, org_id: OrgId) -> DbResult<Tag> {
        crate::tags::get(&self.pool, id, org_id).await
    }

    async fn create_tag(&self, input: NewTag, org_id: OrgId) -> DbResult<Tag> {
        crate::tags::create(&self.pool, input, org_id).await
    }

    async fn update_tag(&self, id: TagId, patch: UpdateTag, org_id: OrgId) -> DbResult<Tag> {
        crate::tags::update(&self.pool, id, patch, org_id).await
    }

    async fn tag_usage(&self, org_id: OrgId) -> DbResult<Vec<TagUsage>> {
        crate::tags::usage(&self.pool, org_id).await
    }

    async fn delete_tag(&self, id: TagId, org_id: OrgId) -> DbResult<()> {
        crate::tags::delete(&self.pool, id, org_id).await
    }

    async fn attach_tag(&self, monitor: MonitorId, tag: TagId) -> DbResult<()> {
        crate::tags::attach(&self.pool, monitor, tag).await
    }

    async fn detach_tag(&self, monitor: MonitorId, tag: TagId) -> DbResult<()> {
        crate::tags::detach(&self.pool, monitor, tag).await
    }

    async fn list_tags_for_monitor(&self, monitor: MonitorId) -> DbResult<Vec<TagBrief>> {
        crate::tags::list_for_monitor(&self.pool, monitor).await
    }

    async fn hydrate_tags_for_channels(
        &self,
        ids: &[NotificationId],
    ) -> DbResult<HashMap<NotificationId, Vec<TagBrief>>> {
        crate::tags::hydrate_for_channels(&self.pool, ids).await
    }

    async fn hydrate_tags_for_monitors(
        &self,
        ids: &[MonitorId],
    ) -> DbResult<HashMap<MonitorId, Vec<TagBrief>>> {
        crate::tags::hydrate_for_monitors(&self.pool, ids).await
    }
}

#[async_trait::async_trait]
impl StoreTemplates for PgStore {
    async fn list_templates(&self, org_id: OrgId) -> DbResult<Vec<Template>> {
        crate::templates::list(&self.pool, org_id).await
    }

    async fn get_template(&self, id: NotificationTemplateId, org_id: OrgId) -> DbResult<Template> {
        crate::templates::get(&self.pool, id, org_id).await
    }

    async fn create_template(&self, input: NewTemplate, org_id: OrgId) -> DbResult<Template> {
        crate::templates::create(&self.pool, input, org_id).await
    }

    async fn update_template(
        &self,
        id: NotificationTemplateId,
        input: UpdateTemplate,
        org_id: OrgId,
    ) -> DbResult<Template> {
        crate::templates::update(&self.pool, id, input, org_id).await
    }

    async fn delete_template(&self, id: NotificationTemplateId, org_id: OrgId) -> DbResult<()> {
        crate::templates::delete(&self.pool, id, org_id).await
    }

    async fn get_template_render_strings(
        &self,
        id: NotificationTemplateId,
    ) -> DbResult<RenderedTemplate> {
        crate::templates::get_render_strings(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreTelemetryRules for PgStore {
    async fn list_telemetry_rules(&self, org_id: OrgId) -> DbResult<Vec<TelemetryRule>> {
        crate::telemetry_rules::list(&self.pool, org_id).await
    }

    async fn list_all_telemetry_rules(&self) -> DbResult<Vec<TelemetryRule>> {
        crate::telemetry_rules::list_all(&self.pool).await
    }

    async fn get_telemetry_rule(
        &self,
        id: TelemetryRuleId,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule> {
        crate::telemetry_rules::get(&self.pool, id, org_id).await
    }

    async fn get_telemetry_rule_unscoped(&self, id: TelemetryRuleId) -> DbResult<TelemetryRule> {
        crate::telemetry_rules::get_unscoped(&self.pool, id).await
    }

    async fn create_telemetry_rule(
        &self,
        input: NewTelemetryRule,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule> {
        crate::telemetry_rules::create(&self.pool, input, org_id).await
    }

    async fn update_telemetry_rule(
        &self,
        id: TelemetryRuleId,
        patch: UpdateTelemetryRule,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule> {
        crate::telemetry_rules::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_telemetry_rule(&self, id: TelemetryRuleId, org_id: OrgId) -> DbResult<()> {
        crate::telemetry_rules::delete(&self.pool, id, org_id).await
    }

    async fn evaluate_telemetry_rules_tick(&self) -> DbResult<Vec<TelemetryRuleEvent>> {
        crate::telemetry_rules::evaluate_tick(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreMetricRules for PgStore {
    async fn list_metric_rules(&self, org_id: OrgId) -> DbResult<Vec<MetricRule>> {
        crate::metric_rules::list(&self.pool, org_id).await
    }

    async fn list_all_metric_rules(&self) -> DbResult<Vec<MetricRule>> {
        crate::metric_rules::list_all(&self.pool).await
    }

    async fn get_metric_rule(&self, id: MetricRuleId, org_id: OrgId) -> DbResult<MetricRule> {
        crate::metric_rules::get(&self.pool, id, org_id).await
    }

    async fn get_metric_rule_unscoped(&self, id: MetricRuleId) -> DbResult<MetricRule> {
        crate::metric_rules::get_unscoped(&self.pool, id).await
    }

    async fn create_metric_rule(
        &self,
        input: NewMetricRule,
        org_id: OrgId,
    ) -> DbResult<MetricRule> {
        crate::metric_rules::create(&self.pool, input, org_id).await
    }

    async fn update_metric_rule(
        &self,
        id: MetricRuleId,
        patch: UpdateMetricRule,
        org_id: OrgId,
    ) -> DbResult<MetricRule> {
        crate::metric_rules::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_metric_rule(&self, id: MetricRuleId, org_id: OrgId) -> DbResult<()> {
        crate::metric_rules::delete(&self.pool, id, org_id).await
    }

    async fn evaluate_metric_rules_tick(&self) -> DbResult<Vec<MetricRuleEvent>> {
        crate::metric_rules::evaluate_tick(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreMonitorGroups for PgStore {
    async fn monitor_group_in_org(&self, group: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
        crate::monitor_groups::in_org(&self.pool, group, org_id).await
    }

    async fn list_monitor_groups(&self, org_id: OrgId) -> DbResult<Vec<MonitorGroup>> {
        crate::monitor_groups::list(&self.pool, org_id).await
    }

    async fn create_monitor_group(
        &self,
        input: NewMonitorGroup,
        org_id: OrgId,
    ) -> DbResult<MonitorGroup> {
        crate::monitor_groups::create(&self.pool, input, org_id).await
    }

    async fn update_monitor_group(
        &self,
        id: MonitorGroupId,
        patch: UpdateMonitorGroup,
        org_id: OrgId,
    ) -> DbResult<MonitorGroup> {
        crate::monitor_groups::update(&self.pool, id, patch, org_id).await
    }

    async fn would_form_group_cycle(
        &self,
        group: MonitorGroupId,
        new_parent: MonitorGroupId,
    ) -> DbResult<bool> {
        crate::monitor_groups::would_form_group_cycle(&self.pool, group, new_parent).await
    }

    async fn delete_monitor_group(&self, id: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
        crate::monitor_groups::delete(&self.pool, id, org_id).await
    }

    async fn parents_of(&self, child: MonitorId) -> DbResult<Vec<MonitorId>> {
        crate::monitor_groups::parents_of(&self.pool, child).await
    }

    async fn children_of(&self, parent: MonitorId) -> DbResult<Vec<MonitorId>> {
        crate::monitor_groups::children_of(&self.pool, parent).await
    }

    async fn any_parent_down(&self, child: MonitorId) -> DbResult<bool> {
        crate::monitor_groups::any_parent_down(&self.pool, child).await
    }

    async fn attach_dependency(&self, child: MonitorId, parent: MonitorId) -> DbResult<()> {
        crate::monitor_groups::attach_dependency(&self.pool, child, parent).await
    }

    async fn detach_dependency(&self, child: MonitorId, parent: MonitorId) -> DbResult<()> {
        crate::monitor_groups::detach_dependency(&self.pool, child, parent).await
    }

    async fn would_form_cycle(&self, child: MonitorId, parent: MonitorId) -> DbResult<bool> {
        crate::monitor_groups::would_form_cycle(&self.pool, child, parent).await
    }
}

#[async_trait::async_trait]
impl StoreSilences for PgStore {
    async fn is_silenced(&self, monitor: Option<Uuid>) -> DbResult<bool> {
        crate::silences::is_silenced(&self.pool, monitor).await
    }

    async fn create_silence(&self, s: NewSilence<'_>, org_id: OrgId) -> DbResult<Uuid> {
        crate::silences::create(&self.pool, s, org_id).await
    }

    async fn list_active_silences(&self, org_id: OrgId) -> DbResult<Vec<Silence>> {
        crate::silences::list_active(&self.pool, org_id).await
    }

    async fn delete_silence(&self, id: Uuid, org_id: OrgId) -> DbResult<bool> {
        crate::silences::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreOidcState for PgStore {
    async fn stash_oidc_state(
        &self,
        state: &str,
        pkce_verifier: &str,
        nonce: Option<&str>,
        return_to: Option<&str>,
    ) -> DbResult<()> {
        crate::oidc_state::stash(&self.pool, state, pkce_verifier, nonce, return_to).await
    }

    async fn consume_oidc_state(&self, state: &str) -> DbResult<Option<Consumed>> {
        crate::oidc_state::consume(&self.pool, state).await
    }

    async fn prune_oidc_state(&self) -> DbResult<u64> {
        crate::oidc_state::prune_expired(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreStatusPages for PgStore {
    async fn list_status_pages(&self, org_id: OrgId) -> DbResult<Vec<StatusPage>> {
        crate::status_pages::list(&self.pool, org_id).await
    }

    async fn list_all_status_pages(&self) -> DbResult<Vec<StatusPage>> {
        crate::status_pages::list_all(&self.pool).await
    }

    async fn get_status_page(&self, id: StatusPageId, org_id: OrgId) -> DbResult<StatusPage> {
        crate::status_pages::get(&self.pool, id, org_id).await
    }

    async fn get_status_page_by_slug(&self, slug: &str) -> DbResult<StatusPage> {
        crate::status_pages::get_by_slug(&self.pool, slug).await
    }

    async fn get_status_page_unscoped(&self, id: StatusPageId) -> DbResult<StatusPage> {
        crate::status_pages::get_unscoped(&self.pool, id).await
    }

    async fn find_status_page_by_custom_domain(&self, host: &str) -> DbResult<Option<StatusPage>> {
        crate::status_pages::find_by_custom_domain(&self.pool, host).await
    }

    async fn create_status_page(
        &self,
        input: NewStatusPage,
        org_id: OrgId,
    ) -> DbResult<StatusPage> {
        crate::status_pages::create(&self.pool, input, org_id).await
    }

    async fn update_status_page(
        &self,
        id: StatusPageId,
        patch: UpdateStatusPage,
        org_id: OrgId,
    ) -> DbResult<StatusPage> {
        crate::status_pages::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_status_page(&self, id: StatusPageId, org_id: OrgId) -> DbResult<()> {
        crate::status_pages::delete(&self.pool, id, org_id).await
    }

    async fn status_page_public_view(&self, slug: &str) -> DbResult<PublicStatusPage> {
        crate::status_pages::public_view(&self.pool, slug).await
    }

    async fn verify_status_page_password(&self, slug: &str, candidate: &str) -> DbResult<bool> {
        crate::status_pages::verify_page_password(&self.pool, slug, candidate).await
    }

    async fn list_status_page_sections(
        &self,
        page_id: StatusPageId,
    ) -> DbResult<Vec<StatusPageSection>> {
        crate::status_pages::list_sections(&self.pool, page_id).await
    }

    async fn create_status_page_section(
        &self,
        page_id: StatusPageId,
        input: NewStatusPageSection,
    ) -> DbResult<StatusPageSection> {
        crate::status_pages::create_section(&self.pool, page_id, input).await
    }

    async fn update_status_page_section(
        &self,
        id: StatusPageSectionId,
        patch: UpdateStatusPageSection,
    ) -> DbResult<StatusPageSection> {
        crate::status_pages::update_section(&self.pool, id, patch).await
    }

    async fn delete_status_page_section(&self, id: StatusPageSectionId) -> DbResult<()> {
        crate::status_pages::delete_section(&self.pool, id).await
    }

    async fn assign_status_page_monitor_section(
        &self,
        page_id: StatusPageId,
        monitor_id: MonitorId,
        section_id: Option<StatusPageSectionId>,
    ) -> DbResult<()> {
        crate::status_pages::assign_monitor_section(&self.pool, page_id, monitor_id, section_id)
            .await
    }
}

#[async_trait::async_trait]
impl StoreIncidents for PgStore {
    async fn create_incident(
        &self,
        page: StatusPageId,
        author: Option<UserId>,
        input: NewIncident,
    ) -> DbResult<Incident> {
        crate::incidents::create(&self.pool, page, author, input).await
    }

    async fn find_active_incident_by_dedup_key(
        &self,
        page: StatusPageId,
        key: &str,
    ) -> DbResult<Option<Incident>> {
        crate::incidents::find_active_by_dedup_key(&self.pool, page, key).await
    }

    async fn list_active_incidents(&self, page: StatusPageId) -> DbResult<Vec<Incident>> {
        crate::incidents::list_active(&self.pool, page).await
    }

    async fn recent_incidents(&self, limit: i64, org_id: OrgId) -> DbResult<Vec<Incident>> {
        crate::incidents::recent(&self.pool, limit, org_id).await
    }

    async fn list_resolved_incident_history(
        &self,
        page: StatusPageId,
        limit: i64,
    ) -> DbResult<Vec<Incident>> {
        crate::incidents::list_resolved_history(&self.pool, page, limit).await
    }

    async fn resolve_incident(&self, id: IncidentId, now: OffsetDateTime) -> DbResult<()> {
        crate::incidents::resolve(&self.pool, id, now).await
    }

    async fn list_all_incidents(&self, page: StatusPageId, limit: i64) -> DbResult<Vec<Incident>> {
        crate::incidents::list_all(&self.pool, page, limit).await
    }

    async fn delete_incident(&self, id: IncidentId) -> DbResult<()> {
        crate::incidents::delete(&self.pool, id).await
    }

    async fn update_incident(&self, id: IncidentId, patch: UpdateIncident) -> DbResult<Incident> {
        crate::incidents::update(&self.pool, id, patch).await
    }

    async fn get_incident(&self, id: IncidentId) -> DbResult<Incident> {
        crate::incidents::get(&self.pool, id).await
    }

    async fn list_incident_updates(&self, incident: IncidentId) -> DbResult<Vec<IncidentUpdate>> {
        crate::incidents::list_updates(&self.pool, incident).await
    }

    async fn post_incident_update(
        &self,
        incident: IncidentId,
        author: Option<UserId>,
        message: String,
    ) -> DbResult<Uuid> {
        crate::incidents::post_update(&self.pool, incident, author, message).await
    }
}

#[async_trait::async_trait]
impl StoreRouting for PgStore {
    async fn resolve_channels_for_monitor(
        &self,
        monitor: MonitorId,
    ) -> DbResult<Vec<Notification>> {
        crate::routing::resolve_channels_for_monitor(&self.pool, monitor).await
    }

    async fn group_tag_ids(&self, group: MonitorGroupId) -> DbResult<Vec<TagId>> {
        crate::routing::group_tag_ids(&self.pool, group).await
    }

    async fn channel_tag_ids(&self, notif: NotificationId) -> DbResult<Vec<TagId>> {
        crate::routing::channel_tag_ids(&self.pool, notif).await
    }

    async fn group_channel_ids(&self, group: MonitorGroupId) -> DbResult<Vec<NotificationId>> {
        crate::routing::group_channel_ids(&self.pool, group).await
    }

    async fn monitor_exclude_ids(&self, monitor: MonitorId) -> DbResult<Vec<NotificationId>> {
        crate::routing::monitor_exclude_ids(&self.pool, monitor).await
    }

    async fn tag_group(&self, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
        crate::routing::tag_group(&self.pool, group, tag).await
    }

    async fn untag_group(&self, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
        crate::routing::untag_group(&self.pool, group, tag).await
    }

    async fn tag_channel(&self, notif: NotificationId, tag: TagId) -> DbResult<()> {
        crate::routing::tag_channel(&self.pool, notif, tag).await
    }

    async fn untag_channel(&self, notif: NotificationId, tag: TagId) -> DbResult<()> {
        crate::routing::untag_channel(&self.pool, notif, tag).await
    }

    async fn attach_group_channel(
        &self,
        group: MonitorGroupId,
        notif: NotificationId,
    ) -> DbResult<()> {
        crate::routing::attach_group_channel(&self.pool, group, notif).await
    }

    async fn detach_group_channel(
        &self,
        group: MonitorGroupId,
        notif: NotificationId,
    ) -> DbResult<()> {
        crate::routing::detach_group_channel(&self.pool, group, notif).await
    }

    async fn exclude_channel(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::routing::exclude_channel(&self.pool, monitor, notif).await
    }

    async fn unexclude_channel(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::routing::unexclude_channel(&self.pool, monitor, notif).await
    }
}

#[async_trait::async_trait]
impl StoreSubscribers for PgStore {
    async fn subscribe_email(
        &self,
        page: StatusPageId,
        email: &str,
    ) -> DbResult<(Subscriber, String)> {
        crate::subscribers::subscribe_email(&self.pool, page, email).await
    }

    async fn list_subscribers_for_page(&self, page: StatusPageId) -> DbResult<Vec<Subscriber>> {
        crate::subscribers::list_for_page(&self.pool, page).await
    }

    async fn confirmed_subscriber_emails_for_page(
        &self,
        page: StatusPageId,
    ) -> DbResult<Vec<String>> {
        crate::subscribers::confirmed_emails_for_page(&self.pool, page).await
    }

    async fn delete_subscriber(&self, id: StatusPageSubscriberId) -> DbResult<()> {
        crate::subscribers::delete(&self.pool, id).await
    }

    async fn unsubscribe_subscriber_by_token(&self, token: &str) -> DbResult<()> {
        crate::subscribers::unsubscribe_by_token(&self.pool, token).await
    }

    async fn subscriber_email_for_token(&self, token: &str) -> DbResult<Option<String>> {
        crate::subscribers::email_for_token(&self.pool, token).await
    }

    async fn subscriptions_for_email(&self, email: &str) -> DbResult<Vec<ManagedSubscription>> {
        crate::subscribers::subscriptions_for_email(&self.pool, email).await
    }

    async fn unsubscribe_all_for_email(&self, email: &str) -> DbResult<u64> {
        crate::subscribers::unsubscribe_all_for_email(&self.pool, email).await
    }

    async fn unsubscribe_email_from_page(&self, page: StatusPageId, email: &str) -> DbResult<()> {
        crate::subscribers::unsubscribe_email_from_page(&self.pool, page, email).await
    }

    async fn subscriber_page_for(
        &self,
        id: StatusPageSubscriberId,
    ) -> DbResult<Option<StatusPageId>> {
        crate::subscribers::page_for(&self.pool, id).await
    }

    async fn subscriber_token_for(&self, id: Uuid) -> DbResult<Option<String>> {
        crate::subscribers::token_for(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreDetection for PgStore {
    async fn detection_regex_is_valid(&self, pattern: &str) -> DbResult<bool> {
        crate::detection::regex_is_valid(&self.pool, pattern).await
    }

    async fn list_detection_rules(&self, org_id: OrgId) -> DbResult<Vec<DetectionRule>> {
        crate::detection::list(&self.pool, org_id).await
    }

    async fn list_all_detection_rules(&self) -> DbResult<Vec<DetectionRule>> {
        crate::detection::list_all(&self.pool).await
    }

    async fn get_detection_rule(
        &self,
        id: DetectionRuleId,
        org_id: OrgId,
    ) -> DbResult<DetectionRule> {
        crate::detection::get(&self.pool, id, org_id).await
    }

    async fn get_detection_rule_unscoped(&self, id: DetectionRuleId) -> DbResult<DetectionRule> {
        crate::detection::get_unscoped(&self.pool, id).await
    }

    async fn create_detection_rule(
        &self,
        input: NewDetectionRule,
        org_id: OrgId,
    ) -> DbResult<DetectionRule> {
        crate::detection::create(&self.pool, input, org_id).await
    }

    async fn update_detection_rule(
        &self,
        id: DetectionRuleId,
        patch: UpdateDetectionRule,
        org_id: OrgId,
    ) -> DbResult<DetectionRule> {
        crate::detection::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_detection_rule(&self, id: DetectionRuleId, org_id: OrgId) -> DbResult<()> {
        crate::detection::delete(&self.pool, id, org_id).await
    }

    async fn preview_detection(
        &self,
        service: &str,
        min_level: i16,
        body_regex: &str,
        attr_key: &str,
        attr_val: &str,
        window_seconds: i32,
        org_id: OrgId,
    ) -> DbResult<PreviewResult> {
        crate::detection::preview(
            &self.pool,
            service,
            min_level,
            body_regex,
            attr_key,
            attr_val,
            window_seconds,
            org_id,
        )
        .await
    }

    async fn has_recent_detection_finding(
        &self,
        rule_id: DetectionRuleId,
        secs: i64,
        entity: Option<&str>,
    ) -> DbResult<bool> {
        crate::detection::has_recent_finding(&self.pool, rule_id, secs, entity).await
    }

    async fn list_detection_findings(
        &self,
        limit: i64,
        open_only: bool,
    ) -> DbResult<Vec<DetectionFinding>> {
        crate::detection::list_findings(&self.pool, limit, open_only).await
    }

    async fn list_detection_findings_for_org(
        &self,
        limit: i64,
        open_only: bool,
        org_id: OrgId,
    ) -> DbResult<Vec<DetectionFinding>> {
        crate::detection::list_findings_for_org(&self.pool, limit, open_only, org_id).await
    }

    async fn detection_finding_in_org(
        &self,
        finding: DetectionFindingId,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::detection::finding_in_org(&self.pool, finding, org_id).await
    }

    async fn open_detection_findings_count(&self) -> DbResult<i64> {
        crate::detection::open_count(&self.pool).await
    }

    async fn fetch_detection_findings_since(
        &self,
        after: Option<OffsetDateTime>,
        limit: i64,
    ) -> DbResult<Vec<DetectionFinding>> {
        crate::detection::fetch_since(&self.pool, after, limit).await
    }

    async fn ack_detection_finding(&self, id: DetectionFindingId) -> DbResult<DetectionFinding> {
        crate::detection::ack_finding(&self.pool, id).await
    }

    async fn evaluate_detection_tick(&self) -> DbResult<Vec<FindingEvent>> {
        crate::detection::evaluate_tick(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreSessions for PgStore {
    async fn create_session(
        &self,
        user_id: UserId,
        ttl_seconds: i64,
        ip: Option<std::net::IpAddr>,
        user_agent: Option<String>,
    ) -> DbResult<Session> {
        crate::sessions::create(&self.pool, user_id, ttl_seconds, ip, user_agent).await
    }

    async fn lookup_session(&self, id: Uuid) -> DbResult<Session> {
        crate::sessions::get(&self.pool, id).await
    }

    async fn set_session_active_org(
        &self,
        session_id: Uuid,
        user_id: UserId,
        org_id: Uuid,
    ) -> DbResult<bool> {
        crate::sessions::set_active_org(&self.pool, session_id, user_id, org_id).await
    }

    async fn delete_session(&self, id: Uuid) -> DbResult<()> {
        crate::sessions::delete(&self.pool, id).await
    }

    async fn delete_sessions_for_user(&self, user_id: UserId) -> DbResult<u64> {
        crate::sessions::delete_for_user(&self.pool, user_id).await
    }

    async fn list_sessions_for_user(&self, user_id: UserId) -> DbResult<Vec<SessionInfo>> {
        crate::sessions::list_for_user(&self.pool, user_id).await
    }

    async fn delete_one_session_for_user(&self, user_id: UserId, id: Uuid) -> DbResult<bool> {
        crate::sessions::delete_one_for_user(&self.pool, user_id, id).await
    }

    async fn delete_other_sessions(&self, user_id: UserId, keep: Uuid) -> DbResult<u64> {
        crate::sessions::delete_others(&self.pool, user_id, keep).await
    }

    async fn cleanup_expired_sessions(&self) -> DbResult<u64> {
        crate::sessions::cleanup_expired(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreNotifications for PgStore {
    async fn list_notifications(&self, org_id: OrgId) -> DbResult<Vec<Notification>> {
        crate::notifications::list(&self.pool, org_id).await
    }

    async fn list_all_notifications(&self) -> DbResult<Vec<Notification>> {
        crate::notifications::list_all(&self.pool).await
    }

    async fn get_notification(&self, id: NotificationId, org_id: OrgId) -> DbResult<Notification> {
        crate::notifications::get(&self.pool, id, org_id).await
    }

    async fn get_notification_unscoped(&self, id: NotificationId) -> DbResult<Notification> {
        crate::notifications::get_unscoped(&self.pool, id).await
    }

    async fn create_notification(
        &self,
        input: NewNotification,
        org_id: OrgId,
    ) -> DbResult<Notification> {
        crate::notifications::create(&self.pool, input, org_id).await
    }

    async fn update_notification(
        &self,
        id: NotificationId,
        input: UpdateNotification,
        org_id: OrgId,
    ) -> DbResult<Notification> {
        crate::notifications::update(&self.pool, id, input, org_id).await
    }

    async fn notification_counts_per_monitor(
        &self,
        org_id: OrgId,
    ) -> DbResult<Vec<MonitorChannelCount>> {
        crate::notifications::counts_per_monitor(&self.pool, org_id).await
    }

    async fn delete_notification(&self, id: NotificationId, org_id: OrgId) -> DbResult<()> {
        crate::notifications::delete(&self.pool, id, org_id).await
    }

    async fn attach_notification(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::notifications::attach(&self.pool, monitor, notif).await
    }

    async fn detach_notification(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::notifications::detach(&self.pool, monitor, notif).await
    }

    async fn notifications_for_monitor(&self, monitor: MonitorId) -> DbResult<Vec<Notification>> {
        crate::notifications::for_monitor(&self.pool, monitor).await
    }

    async fn mark_notification_fired(&self, id: NotificationId) -> DbResult<()> {
        crate::notifications::mark_fired(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreSettings for PgStore {
    async fn get_setting(&self, key: &str) -> DbResult<Option<serde_json::Value>> {
        crate::settings::get(&self.pool, key).await
    }

    async fn put_setting(&self, key: &str, value: &serde_json::Value) -> DbResult<()> {
        crate::settings::put(&self.pool, key, value).await
    }

    async fn delete_setting(&self, key: &str) -> DbResult<()> {
        crate::settings::delete(&self.pool, key).await
    }
}

#[async_trait::async_trait]
impl StoreLogs for PgStore {
    async fn insert_logs(&self, logs: &[ParsedLog], org_id: OrgId) -> DbResult<u64> {
        crate::logs::insert_logs(&self.pool, logs, org_id).await
    }

    async fn query_logs(&self, f: LogFilter<'_>, org_id: OrgId) -> DbResult<Vec<LogEntry>> {
        crate::logs::query_logs(&self.pool, f, org_id).await
    }

    async fn log_level_counts(
        &self,
        service: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<(String, i64)>> {
        crate::logs::level_counts(&self.pool, service, hours, org_id).await
    }

    async fn log_histogram(
        &self,
        service: Option<&str>,
        min_severity: Option<i16>,
        query: Option<&str>,
        hours: i32,
        buckets: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<LogBucket>> {
        crate::logs::histogram(
            &self.pool,
            service,
            min_severity,
            query,
            hours,
            buckets,
            org_id,
        )
        .await
    }

    async fn log_services(&self, org_id: OrgId) -> DbResult<Vec<String>> {
        crate::logs::list_services(&self.pool, org_id).await
    }

    async fn prune_logs(&self, days: i32) -> DbResult<u64> {
        crate::logs::prune(&self.pool, days).await
    }
}

#[async_trait::async_trait]
impl StoreTraces for PgStore {
    async fn insert_spans(&self, spans: &[ParsedSpan], org_id: OrgId) -> DbResult<u64> {
        crate::traces::insert_spans(&self.pool, spans, org_id).await
    }

    async fn list_traces(&self, f: TraceFilter<'_>, org_id: OrgId) -> DbResult<Vec<TraceSummary>> {
        crate::traces::list_traces(&self.pool, f, org_id).await
    }

    async fn get_trace_spans(&self, trace_id: &str, org_id: OrgId) -> DbResult<Vec<Span>> {
        crate::traces::get_trace_spans(&self.pool, trace_id, org_id).await
    }

    async fn trace_service_map(
        &self,
        window_hours: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ServiceEdge>> {
        crate::traces::service_map(&self.pool, window_hours, org_id).await
    }

    async fn trace_operation_stats(
        &self,
        service: &str,
        window_hours: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<OperationStat>> {
        crate::traces::operation_stats(&self.pool, service, window_hours, org_id).await
    }

    async fn trace_operation_trend(
        &self,
        service: &str,
        operation: &str,
        window_hours: i64,
        buckets: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<f64>> {
        crate::traces::operation_trend(
            &self.pool,
            service,
            operation,
            window_hours,
            buckets,
            org_id,
        )
        .await
    }

    async fn prune_spans(&self, days: i32) -> DbResult<u64> {
        crate::traces::prune(&self.pool, days).await
    }
}

#[async_trait::async_trait]
impl StoreRum for PgStore {
    async fn insert_rum_event(&self, b: &RumBeacon, org_id: OrgId) -> DbResult<()> {
        crate::rum::insert_event(&self.pool, b, org_id).await
    }

    async fn rum_page_samples(
        &self,
        app: Option<&str>,
        url: &str,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RumSample>> {
        crate::rum::page_samples(&self.pool, app, url, hours, limit, org_id).await
    }

    async fn rum_recent_traced(
        &self,
        app: Option<&str>,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RumTracedLoad>> {
        crate::rum::recent_traced(&self.pool, app, hours, limit, org_id).await
    }

    async fn rum_summary(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<RumVitals> {
        crate::rum::summary(&self.pool, app, hours, org_id).await
    }

    async fn rum_pages(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumPage>> {
        crate::rum::pages(&self.pool, app, hours, org_id).await
    }

    async fn rum_browser_breakdown(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumBrowser>> {
        crate::rum::browser_breakdown(&self.pool, app, hours, org_id).await
    }

    async fn rum_user_breakdown(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumUser>> {
        crate::rum::user_breakdown(&self.pool, app, hours, org_id).await
    }

    async fn rum_apps(&self, org_id: OrgId) -> DbResult<Vec<String>> {
        crate::rum::apps(&self.pool, org_id).await
    }

    async fn prune_rum(&self, days: i32) -> DbResult<u64> {
        crate::rum::prune(&self.pool, days).await
    }
}

#[async_trait::async_trait]
impl StoreProfiles for PgStore {
    async fn insert_profile(&self, p: NewProfile<'_>, org_id: OrgId) -> DbResult<i64> {
        crate::profiles::insert(&self.pool, p, org_id).await
    }

    async fn list_profiles(
        &self,
        service: Option<&str>,
        profile_type: Option<&str>,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ProfileMeta>> {
        crate::profiles::list(&self.pool, service, profile_type, hours, limit, org_id).await
    }

    async fn profile_folded_in_window(
        &self,
        service: &str,
        profile_type: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        org_id: OrgId,
    ) -> DbResult<Vec<Vec<u8>>> {
        crate::profiles::folded_in_window(&self.pool, service, profile_type, from, to, org_id).await
    }

    async fn profile_fetch_folded(
        &self,
        id: i64,
        org_id: OrgId,
    ) -> DbResult<Option<(String, Vec<u8>)>> {
        crate::profiles::fetch_folded(&self.pool, id, org_id).await
    }

    async fn profile_services(&self, org_id: OrgId) -> DbResult<Vec<String>> {
        crate::profiles::services(&self.pool, org_id).await
    }

    async fn profile_types(&self, service: Option<&str>, org_id: OrgId) -> DbResult<Vec<String>> {
        crate::profiles::profile_types(&self.pool, service, org_id).await
    }

    async fn prune_profiles(&self, days: i32) -> DbResult<u64> {
        crate::profiles::prune(&self.pool, days).await
    }
}

#[async_trait::async_trait]
impl StoreMetrics for PgStore {
    async fn monitors_by_status(&self) -> DbResult<Vec<(String, i64)>> {
        crate::metrics::monitors_by_status(&self.pool).await
    }

    async fn monitors_by_kind(&self) -> DbResult<Vec<(String, i64)>> {
        crate::metrics::monitors_by_kind(&self.pool).await
    }

    async fn channels_active(&self) -> DbResult<i64> {
        crate::metrics::channels_active(&self.pool).await
    }

    async fn webpush_subscribers(&self) -> DbResult<i64> {
        crate::metrics::webpush_subscribers(&self.pool).await
    }

    async fn heartbeats_recent_by_status(
        &self,
        window_seconds: i64,
    ) -> DbResult<Vec<(String, i64)>> {
        crate::metrics::heartbeats_recent_by_status(&self.pool, window_seconds).await
    }

    async fn incidents_open(&self) -> DbResult<i64> {
        crate::metrics::incidents_open(&self.pool).await
    }

    async fn pipeline_gauges(&self) -> DbResult<PipelineGauges> {
        crate::metrics::pipeline_gauges(&self.pool).await
    }

    async fn storage_usage(&self) -> DbResult<Vec<TableSize>> {
        crate::metrics::storage_usage(&self.pool).await
    }

    async fn ingest_gauges(&self) -> DbResult<IngestGauges> {
        crate::metrics::ingest_gauges(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreErrorTracking for PgStore {
    async fn list_error_projects(&self, org_id: OrgId) -> DbResult<Vec<ErrorProject>> {
        crate::error_tracking::list(&self.pool, org_id).await
    }

    async fn error_project_in_org(&self, project: ErrorProjectId, org_id: OrgId) -> DbResult<()> {
        crate::error_tracking::project_in_org(&self.pool, project, org_id).await
    }

    async fn error_issue_in_org(&self, issue: ErrorIssueId, org_id: OrgId) -> DbResult<()> {
        crate::error_tracking::issue_in_org(&self.pool, issue, org_id).await
    }

    async fn get_error_project(&self, id: ErrorProjectId) -> DbResult<ErrorProject> {
        crate::error_tracking::get(&self.pool, id).await
    }

    async fn org_for_error_project(&self, id: ErrorProjectId) -> DbResult<OrgId> {
        crate::error_tracking::org_for_project(&self.pool, id).await
    }

    async fn get_error_project_opt(&self, id: ErrorProjectId) -> DbResult<Option<ErrorProject>> {
        crate::error_tracking::get_opt(&self.pool, id).await
    }

    async fn find_or_create_error_project_by_name(
        &self,
        name: &str,
        org_id: OrgId,
    ) -> DbResult<ErrorProject> {
        crate::error_tracking::find_or_create_by_name(&self.pool, name, org_id).await
    }

    async fn create_error_project(
        &self,
        input: NewErrorProject,
        org_id: OrgId,
    ) -> DbResult<ErrorProject> {
        crate::error_tracking::create(&self.pool, input, org_id).await
    }

    async fn update_error_project(
        &self,
        id: ErrorProjectId,
        patch: UpdateErrorProject,
        org_id: OrgId,
    ) -> DbResult<ErrorProject> {
        crate::error_tracking::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_error_project(&self, id: ErrorProjectId, org_id: OrgId) -> DbResult<()> {
        crate::error_tracking::delete(&self.pool, id, org_id).await
    }

    async fn record_error_event(
        &self,
        project_id: ErrorProjectId,
        ev: &ParsedEvent,
    ) -> DbResult<RecordOutcome> {
        crate::error_tracking::record_event(&self.pool, project_id, ev).await
    }

    async fn error_issues_for_trace(
        &self,
        trace_id: &str,
        org_id: OrgId,
    ) -> DbResult<Vec<TraceErrorRef>> {
        crate::error_tracking::issues_for_trace(&self.pool, trace_id, org_id).await
    }

    async fn list_error_issues(
        &self,
        project_id: ErrorProjectId,
        status: Option<&str>,
        before_id: Option<Uuid>,
        limit: i64,
    ) -> DbResult<Vec<ErrorIssue>> {
        crate::error_tracking::list_issues(&self.pool, project_id, status, before_id, limit).await
    }

    async fn recent_open_error_issues(
        &self,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ErrorIssue>> {
        crate::error_tracking::recent_open_issues(&self.pool, limit, org_id).await
    }

    async fn error_project_event_histogram(
        &self,
        project_id: ErrorProjectId,
        hours: i32,
        buckets: i64,
    ) -> DbResult<Vec<ErrorBucket>> {
        crate::error_tracking::project_event_histogram(&self.pool, project_id, hours, buckets).await
    }

    async fn get_error_issue(&self, id: ErrorIssueId) -> DbResult<ErrorIssue> {
        crate::error_tracking::get_issue(&self.pool, id).await
    }

    async fn error_issue_affected_users(
        &self,
        id: ErrorIssueId,
        limit: i64,
    ) -> DbResult<Vec<AffectedUser>> {
        crate::error_tracking::issue_affected_users(&self.pool, id, limit).await
    }

    async fn error_issue_stats(&self, id: ErrorIssueId) -> DbResult<IssueStats> {
        crate::error_tracking::issue_stats(&self.pool, id).await
    }

    async fn set_error_issue_status(&self, id: ErrorIssueId, status: &str) -> DbResult<ErrorIssue> {
        crate::error_tracking::set_issue_status(&self.pool, id, status).await
    }

    async fn assign_error_issue(
        &self,
        id: ErrorIssueId,
        assignee: Option<UserId>,
    ) -> DbResult<ErrorIssue> {
        crate::error_tracking::assign_issue(&self.pool, id, assignee).await
    }

    async fn error_assignable_users(&self) -> DbResult<Vec<crate::error_tracking::AssignableUser>> {
        crate::error_tracking::assignable_users(&self.pool).await
    }

    async fn list_error_events(
        &self,
        issue_id: ErrorIssueId,
        limit: i64,
    ) -> DbResult<Vec<ErrorEvent>> {
        crate::error_tracking::list_events(&self.pool, issue_id, limit).await
    }

    async fn prune_error_events(&self) -> DbResult<u64> {
        crate::error_tracking::prune(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreScheduledReports for PgStore {
    async fn list_scheduled_reports(&self, org_id: OrgId) -> DbResult<Vec<ScheduledReport>> {
        crate::scheduled_reports::list(&self.pool, org_id).await
    }

    async fn get_scheduled_report(
        &self,
        id: ScheduledReportId,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport> {
        crate::scheduled_reports::get(&self.pool, id, org_id).await
    }

    async fn create_scheduled_report(
        &self,
        input: NewScheduledReport,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport> {
        crate::scheduled_reports::create(&self.pool, input, org_id).await
    }

    async fn update_scheduled_report(
        &self,
        id: ScheduledReportId,
        input: UpdateScheduledReport,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport> {
        crate::scheduled_reports::update(&self.pool, id, input, org_id).await
    }

    async fn delete_scheduled_report(&self, id: ScheduledReportId, org_id: OrgId) -> DbResult<()> {
        crate::scheduled_reports::delete(&self.pool, id, org_id).await
    }

    async fn due_scheduled_reports(&self, now: OffsetDateTime) -> DbResult<Vec<ScheduledReport>> {
        crate::scheduled_reports::due(&self.pool, now).await
    }

    async fn render_scheduled_report(
        &self,
        report_name: &str,
        cadence: &str,
    ) -> DbResult<(String, String)> {
        crate::scheduled_reports::render(&self.pool, report_name, cadence).await
    }

    async fn mark_scheduled_report_sent(&self, id: ScheduledReportId) -> DbResult<()> {
        crate::scheduled_reports::mark_sent(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreIncidentTemplates for PgStore {
    async fn list_incident_templates(&self, org_id: OrgId) -> DbResult<Vec<IncidentTemplate>> {
        crate::incident_templates::list(&self.pool, org_id).await
    }

    async fn get_incident_template(
        &self,
        id: IncidentTemplateId,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate> {
        crate::incident_templates::get(&self.pool, id, org_id).await
    }

    async fn create_incident_template(
        &self,
        input: NewIncidentTemplate,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate> {
        crate::incident_templates::create(&self.pool, input, org_id).await
    }

    async fn update_incident_template(
        &self,
        id: IncidentTemplateId,
        input: UpdateIncidentTemplate,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate> {
        crate::incident_templates::update(&self.pool, id, input, org_id).await
    }

    async fn delete_incident_template(
        &self,
        id: IncidentTemplateId,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::incident_templates::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreMonitorPresets for PgStore {
    async fn list_monitor_presets(&self, org_id: OrgId) -> DbResult<Vec<MonitorPreset>> {
        crate::monitor_presets::list(&self.pool, org_id).await
    }

    async fn get_monitor_preset(
        &self,
        id: MonitorPresetId,
        org_id: OrgId,
    ) -> DbResult<MonitorPreset> {
        crate::monitor_presets::get(&self.pool, id, org_id).await
    }

    async fn create_monitor_preset(
        &self,
        input: NewMonitorPreset,
        org_id: OrgId,
    ) -> DbResult<MonitorPreset> {
        crate::monitor_presets::create(&self.pool, input, org_id).await
    }

    async fn delete_monitor_preset(&self, id: MonitorPresetId, org_id: OrgId) -> DbResult<()> {
        crate::monitor_presets::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreMonitorTemplates for PgStore {
    async fn list_monitor_templates(&self, org_id: OrgId) -> DbResult<Vec<MonitorTemplate>> {
        crate::monitor_templates::list(&self.pool, org_id).await
    }

    async fn get_monitor_template(
        &self,
        id: MonitorTemplateId,
        org_id: OrgId,
    ) -> DbResult<MonitorTemplate> {
        crate::monitor_templates::get(&self.pool, id, org_id).await
    }

    async fn create_monitor_template(
        &self,
        input: NewMonitorTemplate,
        org_id: OrgId,
    ) -> DbResult<MonitorTemplate> {
        crate::monitor_templates::create(&self.pool, input, org_id).await
    }

    async fn delete_monitor_template(&self, id: MonitorTemplateId, org_id: OrgId) -> DbResult<()> {
        crate::monitor_templates::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreDeliveryLog for PgStore {
    async fn record_delivery(&self, entry: NewDelivery<'_>) -> DbResult<DeliveryEntry> {
        crate::delivery_log::record(&self.pool, entry).await
    }

    async fn get_delivery(&self, id: i64, org_id: OrgId) -> DbResult<Option<DeliveryEntry>> {
        crate::delivery_log::get(&self.pool, id, org_id).await
    }

    async fn list_deliveries(
        &self,
        limit: i64,
        before_ts: Option<OffsetDateTime>,
        org_id: OrgId,
    ) -> DbResult<Vec<DeliveryEntry>> {
        crate::delivery_log::list(&self.pool, limit, before_ts, org_id).await
    }

    async fn list_all_deliveries(&self, limit: i64, org_id: OrgId) -> DbResult<Vec<DeliveryEntry>> {
        crate::delivery_log::list_all(&self.pool, limit, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreAgents for PgStore {
    async fn list_agents(&self, org_id: OrgId) -> DbResult<Vec<Agent>> {
        crate::agents::list(&self.pool, org_id).await
    }

    async fn get_agent(&self, id: AgentId, org_id: OrgId) -> DbResult<Agent> {
        crate::agents::get(&self.pool, id, org_id).await
    }

    async fn create_agent(&self, input: NewAgent, org_id: OrgId) -> DbResult<IssuedAgent> {
        crate::agents::create(&self.pool, input, org_id).await
    }

    async fn update_agent(
        &self,
        id: AgentId,
        patch: UpdateAgent,
        org_id: OrgId,
    ) -> DbResult<Agent> {
        crate::agents::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_agent(&self, id: AgentId, org_id: OrgId) -> DbResult<()> {
        crate::agents::delete(&self.pool, id, org_id).await
    }

    async fn lookup_agent(&self, token: &str) -> DbResult<Agent> {
        crate::agents::lookup(&self.pool, token).await
    }

    async fn touch_agent_seen(&self, id: AgentId, version: Option<&str>) -> DbResult<()> {
        crate::agents::touch_seen(&self.pool, id, version).await
    }
}

#[async_trait::async_trait]
impl StoreMetricSamples for PgStore {
    async fn insert_metric_samples(&self, samples: &[PromSample], org_id: OrgId) -> DbResult<()> {
        crate::metric_samples::insert_many(&self.pool, samples, org_id).await
    }

    async fn list_metric_sample_series(&self, org_id: OrgId) -> DbResult<Vec<Series>> {
        crate::metric_samples::list_series(&self.pool, org_id).await
    }

    async fn metric_sample_range_query(
        &self,
        name: &str,
        labels: &serde_json::Value,
        from: OffsetDateTime,
        to: OffsetDateTime,
        step_seconds: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RangePoint>> {
        crate::metric_samples::range_query(&self.pool, name, labels, from, to, step_seconds, org_id)
            .await
    }

    async fn metric_sample_baseline(
        &self,
        name: &str,
        labels: &serde_json::Value,
        window_secs: i64,
        org_id: OrgId,
    ) -> DbResult<Option<(f64, f64)>> {
        crate::metric_samples::baseline(&self.pool, name, labels, window_secs, org_id).await
    }

    async fn metric_sample_latest(
        &self,
        name: &str,
        labels: &serde_json::Value,
        org_id: OrgId,
    ) -> DbResult<Option<(f64, OffsetDateTime)>> {
        crate::metric_samples::latest(&self.pool, name, labels, org_id).await
    }

    async fn prune_metric_samples_older_than(&self, cutoff: OffsetDateTime) -> DbResult<u64> {
        crate::metric_samples::prune_older_than(&self.pool, cutoff).await
    }
}

#[async_trait::async_trait]
impl StoreSourceMaps for PgStore {
    async fn upsert_source_map(&self, m: NewSourceMap<'_>) -> DbResult<i64> {
        crate::source_maps::upsert(&self.pool, m).await
    }

    async fn get_source_map(
        &self,
        project_id: Uuid,
        release: &str,
        filename: &str,
    ) -> DbResult<Option<serde_json::Value>> {
        crate::source_maps::get(&self.pool, project_id, release, filename).await
    }

    async fn list_source_maps(&self, project_id: Uuid) -> DbResult<Vec<SourceMapMeta>> {
        crate::source_maps::list(&self.pool, project_id).await
    }

    async fn delete_source_map(&self, project_id: Uuid, id: i64) -> DbResult<bool> {
        crate::source_maps::delete(&self.pool, project_id, id).await
    }
}

#[async_trait::async_trait]
impl StoreUsers for PgStore {
    async fn count_users(&self) -> DbResult<i64> {
        crate::users::count(&self.pool).await
    }

    async fn create_user(&self, input: NewUser) -> DbResult<User> {
        crate::users::create(&self.pool, input).await
    }

    async fn get_user_by_email(&self, email: &str) -> DbResult<UserWithHash> {
        crate::users::get_by_email(&self.pool, email).await
    }

    async fn user_by_email(&self, email: &str) -> DbResult<Option<User>> {
        crate::users::by_email(&self.pool, email).await
    }

    async fn get_user(&self, id: UserId) -> DbResult<User> {
        crate::users::get(&self.pool, id).await
    }

    async fn set_user_totp_secret(&self, id: UserId, secret: &str) -> DbResult<()> {
        crate::users::set_totp_secret(&self.pool, id, secret).await
    }

    async fn enable_user_totp(&self, id: UserId) -> DbResult<()> {
        crate::users::enable_totp(&self.pool, id).await
    }

    async fn disable_user_totp(&self, id: UserId) -> DbResult<()> {
        crate::users::disable_totp(&self.pool, id).await
    }

    async fn mark_user_login(&self, id: UserId) -> DbResult<()> {
        crate::users::mark_login(&self.pool, id).await
    }

    async fn user_totp_locked_until(&self, id: UserId) -> DbResult<Option<OffsetDateTime>> {
        crate::users::totp_locked_until(&self.pool, id).await
    }

    async fn record_user_totp_failure(
        &self,
        id: UserId,
        max_attempts: i32,
        lockout_mins: i32,
    ) -> DbResult<bool> {
        crate::users::record_totp_failure(&self.pool, id, max_attempts, lockout_mins).await
    }

    async fn reset_user_totp_failures(&self, id: UserId) -> DbResult<()> {
        crate::users::reset_totp_failures(&self.pool, id).await
    }

    async fn list_users(&self) -> DbResult<Vec<User>> {
        crate::users::list(&self.pool).await
    }

    async fn set_user_admin(&self, id: UserId, is_admin: bool) -> DbResult<()> {
        crate::users::set_admin(&self.pool, id, is_admin).await
    }

    async fn set_user_role(&self, id: UserId, role: Role) -> DbResult<()> {
        crate::users::set_role(&self.pool, id, role).await
    }

    async fn delete_user(&self, id: UserId) -> DbResult<()> {
        crate::users::delete(&self.pool, id).await
    }

    async fn anonymize_user(&self, id: UserId) -> DbResult<()> {
        crate::users::anonymize(&self.pool, id).await
    }

    async fn get_user_prefs(&self, id: UserId) -> DbResult<serde_json::Value> {
        crate::users::get_prefs(&self.pool, id).await
    }

    async fn set_user_prefs(&self, id: UserId, prefs: &serde_json::Value) -> DbResult<()> {
        crate::users::set_prefs(&self.pool, id, prefs).await
    }

    async fn set_user_password(&self, id: UserId, hash: &str) -> DbResult<()> {
        crate::users::set_password(&self.pool, id, hash).await
    }
}

#[async_trait::async_trait]
impl StoreWebpush for PgStore {
    async fn list_webpush_subs(
        &self,
        notification: NotificationId,
    ) -> DbResult<Vec<crate::webpush::WebpushSubscription>> {
        crate::webpush::list_for_notification(&self.pool, notification).await
    }

    async fn upsert_webpush_sub(
        &self,
        notification: NotificationId,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
    ) -> DbResult<()> {
        crate::webpush::upsert(&self.pool, notification, endpoint, p256dh, auth).await
    }

    async fn delete_webpush_sub_by_endpoint(&self, endpoint: &str) -> DbResult<()> {
        crate::webpush::delete_by_endpoint(&self.pool, endpoint).await
    }

    async fn delete_webpush_sub(&self, id: Uuid) -> DbResult<()> {
        crate::webpush::delete(&self.pool, id).await
    }

    async fn get_vapid_keys(&self) -> DbResult<Option<crate::webpush::VapidKeys>> {
        crate::webpush::get_vapid(&self.pool).await
    }

    async fn set_vapid_keys(&self, keys: &crate::webpush::VapidKeys) -> DbResult<()> {
        crate::webpush::set_vapid(&self.pool, keys).await
    }
}

#[async_trait::async_trait]
impl StoreOrgs for PgStore {
    async fn create_org(&self, slug: &str, name: &str) -> DbResult<rampart_core::org::Org> {
        crate::orgs::create(&self.pool, slug, name).await
    }

    async fn get_org(&self, id: OrgId) -> DbResult<rampart_core::org::Org> {
        crate::orgs::get(&self.pool, id).await
    }

    async fn orgs_for_user(&self, user_id: UserId) -> DbResult<Vec<rampart_core::org::Org>> {
        crate::orgs::list_for_user(&self.pool, user_id).await
    }

    async fn upsert_org_member(&self, org_id: OrgId, user_id: UserId, role: Role) -> DbResult<()> {
        crate::orgs::upsert_member(&self.pool, org_id, user_id, role).await
    }

    async fn org_member_role(&self, org_id: OrgId, user_id: UserId) -> DbResult<Option<Role>> {
        crate::orgs::member_role(&self.pool, org_id, user_id).await
    }

    async fn list_org_members(&self, org_id: OrgId) -> DbResult<Vec<rampart_core::org::OrgMember>> {
        crate::orgs::list_members(&self.pool, org_id).await
    }

    async fn list_org_members_detailed(
        &self,
        org_id: OrgId,
    ) -> DbResult<Vec<crate::orgs::OrgMemberDetail>> {
        crate::orgs::list_members_detailed(&self.pool, org_id).await
    }

    async fn update_org(&self, id: OrgId, name: &str) -> DbResult<rampart_core::org::Org> {
        crate::orgs::update(&self.pool, id, name).await
    }

    async fn org_by_slug(&self, slug: &str) -> DbResult<rampart_core::org::Org> {
        crate::orgs::get_by_slug(&self.pool, slug).await
    }

    async fn remove_org_member(&self, org_id: OrgId, user_id: UserId) -> DbResult<bool> {
        crate::orgs::remove_member(&self.pool, org_id, user_id).await
    }

    async fn count_org_admins(&self, org_id: OrgId) -> DbResult<i64> {
        crate::orgs::count_admins(&self.pool, org_id).await
    }

    async fn create_org_with_owner(
        &self,
        slug: &str,
        name: &str,
        owner: UserId,
    ) -> DbResult<rampart_core::org::Org> {
        crate::orgs::create_with_owner(&self.pool, slug, name, owner).await
    }
}

impl Store for PgStore {}

/// Compile-time proof the super-trait is object-safe: this fn pointer type can
/// only be written if `dyn Store` is a valid (object-safe) type.
const _: fn(&dyn Store) = |_s| {};

#[cfg(test)]
mod tests {
    use super::*;

    /// Object-safety assertion. Compile-only: if `dyn Store` were not
    /// object-safe this function would fail to type-check.
    #[test]
    fn store_is_object_safe() {
        fn _assert(_: &dyn Store) {}
        // Also confirm an Arc<dyn Store> can be formed from a PgStore (the
        // exact shape AppState wires up), without touching the DB.
        fn _assert_arc(_: std::sync::Arc<dyn Store>) {}
    }
}
