//! SQLite-backed `Store` implementation (multi-DB P1 capstone).
//!
//! `SqliteStore` satisfies the same object-safe [`crate::store::Store`]
//! super-trait as `PgStore`, so `AppState` can hold `Arc<dyn Store>` over either
//! backend. The 10 domains ported in P1 (settings, orgs, users, sessions,
//! monitors, heartbeats, tags, agents, notifications, delivery_log) delegate to
//! their `crate::sqlite::*` free functions; the remaining domains are
//! `unimplemented!()` stubs that panic if hit — they light up as each domain is
//! forked. This proves the seam is satisfiable by SQLite end-to-end at the type
//! level.
//!
//! NOT YET WIRED INTO BOOT: `AppState` still holds a `PgPool` that the
//! not-yet-seamed callers (scheduler / notifier / seed) use directly, so a true
//! `RAMPART_DB_URL=sqlite` end-to-end boot needs that pool abstracted first
//! (a separate slice). `SqliteStore` + `connect` exist and compile now.

// The 37 not-yet-ported domains are `unimplemented!()` stubs that intentionally
// ignore their args; allow that here rather than `_`-prefixing ~300 parameters.
#![allow(unused_variables)]

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
use crate::monitors::{BulkEditOutcome, BulkEditPatch, MonitorPrior, SloState};
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
use crate::DbResult;
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
use rampart_core::monitor::{Monitor, MonitorStatus, NewMonitor, UpdateMonitor};
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

use crate::store::{
    Store, StoreAgents, StoreApiKeys, StoreAudit, StoreCompliance, StoreDeliveryLog,
    StoreDeployMarkers, StoreDetection, StoreErrorTracking, StoreEscalations, StoreHeartbeats,
    StoreIncidentTemplates, StoreIncidents, StoreIngestKeys, StoreIngestTokens, StoreLogs,
    StoreMaintenance, StoreMetricRules, StoreMetricSamples, StoreMetrics, StoreMonitorGroups,
    StoreMonitorPresets, StoreMonitorTemplates, StoreMonitors, StoreNotifications, StoreOidcState,
    StoreOnCall, StoreOrgs, StoreProfiles, StoreProxies, StoreRecoveryCodes, StoreRouting,
    StoreRum, StoreScheduledReports, StoreSessions, StoreSettings, StoreSilences, StoreSlos,
    StoreSourceMaps, StoreStatusPages, StoreSubscribers, StoreTags, StoreTelemetryRules,
    StoreTemplates, StoreTraces, StoreUsers, StoreWebpush,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

/// SQLite-backed [`Store`]. Holds a `SqlitePool`; delegates the ported domains
/// to `crate::sqlite::*` and stubs the rest.
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Wrap an existing pool (the pool MUST have `foreign_keys(true)` set, like
    /// the one [`connect`](Self::connect) builds).
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Open a SQLite database URL (`sqlite::memory:`,
    /// `sqlite:///var/lib/rampart/rampart.db`), enabling per-connection foreign
    /// keys (off by default on SQLite) and running the SQLite migration set.
    pub async fn connect(url: &str) -> DbResult<Self> {
        let opts = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        sqlx::migrate!("../../migrations-sqlite").run(&pool).await?;
        Ok(Self { pool })
    }
}

#[async_trait::async_trait]
impl StoreHeartbeats for SqliteStore {
    async fn insert_many(&self, hbs: &[Heartbeat]) -> DbResult<()> {
        crate::sqlite::heartbeats::insert_many(&self.pool, hbs).await
    }

    async fn recent_for_monitor(&self, monitor: MonitorId, limit: i64) -> DbResult<Vec<Heartbeat>> {
        crate::sqlite::heartbeats::recent_for_monitor(&self.pool, monitor, limit).await
    }

    async fn recent_for_monitor_before(
        &self,
        monitor: MonitorId,
        limit: i64,
        before: Option<time::OffsetDateTime>,
    ) -> DbResult<Vec<Heartbeat>> {
        crate::sqlite::heartbeats::recent_for_monitor_before(&self.pool, monitor, limit, before)
            .await
    }

    async fn range_for_monitor(
        &self,
        monitor: MonitorId,
        since: time::OffsetDateTime,
        until: time::OffsetDateTime,
        limit: i64,
    ) -> DbResult<Vec<Heartbeat>> {
        crate::sqlite::heartbeats::range_for_monitor(&self.pool, monitor, since, until, limit).await
    }

    async fn uptime_pct(&self, monitor: MonitorId, window_seconds: i64) -> DbResult<Option<f64>> {
        crate::sqlite::heartbeats::uptime_pct(&self.pool, monitor, window_seconds).await
    }

    async fn current_slo_uptime_pct(
        &self,
        monitor: MonitorId,
        window_days: i32,
    ) -> DbResult<Option<f64>> {
        crate::sqlite::heartbeats::current_slo_uptime_pct(&self.pool, monitor, window_days).await
    }

    async fn avg_latency_ms(
        &self,
        monitor: MonitorId,
        window_seconds: i64,
    ) -> DbResult<Option<f64>> {
        crate::sqlite::heartbeats::avg_latency_ms(&self.pool, monitor, window_seconds).await
    }

    async fn daily_status(&self, monitor: MonitorId, days: i32) -> DbResult<Vec<u8>> {
        crate::sqlite::heartbeats::daily_status(&self.pool, monitor, days).await
    }

    async fn day_hourly_latency(
        &self,
        monitor: MonitorId,
        day: time::Date,
    ) -> DbResult<Vec<(i32, Option<f32>, i32)>> {
        crate::sqlite::heartbeats::day_hourly_latency(&self.pool, monitor, day).await
    }

    async fn monthly_uptime(
        &self,
        monitor: MonitorId,
        months: i32,
    ) -> DbResult<Vec<MonthlyUptime>> {
        crate::sqlite::heartbeats::monthly_uptime(&self.pool, monitor, months).await
    }

    async fn uptime_pct_batch(
        &self,
        monitor_ids: &[Uuid],
        window_seconds: i64,
    ) -> DbResult<HashMap<Uuid, f64>> {
        crate::sqlite::heartbeats::uptime_pct_batch(&self.pool, monitor_ids, window_seconds).await
    }

    async fn avg_latency_ms_batch(
        &self,
        monitor_ids: &[Uuid],
        window_seconds: i64,
    ) -> DbResult<HashMap<Uuid, f64>> {
        crate::sqlite::heartbeats::avg_latency_ms_batch(&self.pool, monitor_ids, window_seconds)
            .await
    }

    async fn daily_status_batch(
        &self,
        monitor_ids: &[Uuid],
        days: i32,
    ) -> DbResult<HashMap<Uuid, Vec<u8>>> {
        crate::sqlite::heartbeats::daily_status_batch(&self.pool, monitor_ids, days).await
    }

    async fn monthly_uptime_batch(
        &self,
        monitor_ids: &[Uuid],
        months: i32,
    ) -> DbResult<HashMap<Uuid, Vec<MonthlyUptime>>> {
        crate::sqlite::heartbeats::monthly_uptime_batch(&self.pool, monitor_ids, months).await
    }

    async fn summary_window(
        &self,
        window_seconds: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<MonitorSummary>> {
        crate::sqlite::heartbeats::summary_window(&self.pool, window_seconds, org_id).await
    }

    async fn mtbf_mttr(&self, monitor: MonitorId, window_seconds: i64) -> DbResult<MtbfMttr> {
        crate::sqlite::heartbeats::mtbf_mttr(&self.pool, monitor, window_seconds).await
    }

    async fn error_budget(
        &self,
        monitor: MonitorId,
        window_days: i32,
        target_pct: f64,
    ) -> DbResult<ErrorBudget> {
        crate::sqlite::heartbeats::error_budget(&self.pool, monitor, window_days, target_pct).await
    }

    async fn error_budget_burndown(
        &self,
        monitor: MonitorId,
        window_days: i32,
        target_pct: f64,
    ) -> DbResult<Vec<BurndownPoint>> {
        crate::sqlite::heartbeats::error_budget_burndown(
            &self.pool,
            monitor,
            window_days,
            target_pct,
        )
        .await
    }

    async fn recent_per_monitor(
        &self,
        per_monitor: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<Heartbeat>> {
        crate::sqlite::heartbeats::recent_per_monitor(&self.pool, per_monitor, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreDeployMarkers for SqliteStore {
    async fn create_deploy_marker(
        &self,
        input: NewDeployMarker,
        org_id: OrgId,
    ) -> DbResult<DeployMarker> {
        unimplemented!(
            "SqliteStore::create_deploy_marker: deploy_markers domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_deploy_markers_window(
        &self,
        hours: i32,
        service: Option<&str>,
        org_id: OrgId,
    ) -> DbResult<Vec<DeployMarker>> {
        unimplemented!("SqliteStore::list_deploy_markers_window: deploy_markers domain not yet ported (multi-DB P1)")
    }

    async fn delete_deploy_marker(&self, id: DeployMarkerId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_deploy_marker: deploy_markers domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreIngestKeys for SqliteStore {
    async fn create_ingest_key(
        &self,
        org_id: OrgId,
        label: &str,
        kind: &str,
        allowed_origins: &[String],
    ) -> DbResult<(IngestKey, String)> {
        unimplemented!(
            "SqliteStore::create_ingest_key: ingest_keys domain not yet ported (multi-DB P1)"
        )
    }

    async fn find_ingest_key_by_token(
        &self,
        token: &str,
    ) -> DbResult<Option<(Uuid, OrgId, Vec<String>)>> {
        unimplemented!("SqliteStore::find_ingest_key_by_token: ingest_keys domain not yet ported (multi-DB P1)")
    }

    async fn touch_ingest_key_last_used(&self, id: Uuid) -> DbResult<()> {
        unimplemented!("SqliteStore::touch_ingest_key_last_used: ingest_keys domain not yet ported (multi-DB P1)")
    }

    async fn list_ingest_keys_for_org(&self, org_id: OrgId) -> DbResult<Vec<IngestKey>> {
        unimplemented!("SqliteStore::list_ingest_keys_for_org: ingest_keys domain not yet ported (multi-DB P1)")
    }

    async fn delete_ingest_key(&self, id: Uuid, org_id: OrgId) -> DbResult<bool> {
        unimplemented!(
            "SqliteStore::delete_ingest_key: ingest_keys domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreSlos for SqliteStore {
    async fn list_slos(&self, org_id: OrgId) -> DbResult<Vec<Slo>> {
        unimplemented!("SqliteStore::list_slos: slos domain not yet ported (multi-DB P1)")
    }

    async fn list_all_slos(&self) -> DbResult<Vec<Slo>> {
        unimplemented!("SqliteStore::list_all_slos: slos domain not yet ported (multi-DB P1)")
    }

    async fn get_slo(&self, id: SloId, org_id: OrgId) -> DbResult<Slo> {
        unimplemented!("SqliteStore::get_slo: slos domain not yet ported (multi-DB P1)")
    }

    async fn get_slo_unscoped(&self, id: SloId) -> DbResult<Slo> {
        unimplemented!("SqliteStore::get_slo_unscoped: slos domain not yet ported (multi-DB P1)")
    }

    async fn create_slo(&self, input: NewSlo, org_id: OrgId) -> DbResult<Slo> {
        unimplemented!("SqliteStore::create_slo: slos domain not yet ported (multi-DB P1)")
    }

    async fn update_slo(&self, id: SloId, patch: UpdateSlo, org_id: OrgId) -> DbResult<Slo> {
        unimplemented!("SqliteStore::update_slo: slos domain not yet ported (multi-DB P1)")
    }

    async fn delete_slo(&self, id: SloId, org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_slo: slos domain not yet ported (multi-DB P1)")
    }

    async fn compute_slo(&self, slo: &Slo) -> DbResult<SloSnapshot> {
        unimplemented!("SqliteStore::compute_slo: slos domain not yet ported (multi-DB P1)")
    }

    async fn slo_trend(&self, slo: &Slo, buckets: i64) -> DbResult<Vec<f64>> {
        unimplemented!("SqliteStore::slo_trend: slos domain not yet ported (multi-DB P1)")
    }

    async fn list_slos_with_snapshots(&self, org_id: OrgId) -> DbResult<Vec<SloWithSnapshot>> {
        unimplemented!(
            "SqliteStore::list_slos_with_snapshots: slos domain not yet ported (multi-DB P1)"
        )
    }

    async fn evaluate_slos_tick(&self) -> DbResult<Vec<SloEvent>> {
        unimplemented!("SqliteStore::evaluate_slos_tick: slos domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreProxies for SqliteStore {
    async fn list_proxies(&self, org_id: OrgId) -> DbResult<Vec<Proxy>> {
        crate::sqlite::proxies::list(&self.pool, org_id).await
    }

    async fn get_proxy(&self, id: ProxyId, org_id: OrgId) -> DbResult<Proxy> {
        crate::sqlite::proxies::get(&self.pool, id, org_id).await
    }

    async fn get_proxy_unscoped(&self, id: ProxyId) -> DbResult<Proxy> {
        crate::sqlite::proxies::get_unscoped(&self.pool, id).await
    }

    async fn create_proxy(&self, input: NewProxy, org_id: OrgId) -> DbResult<Proxy> {
        crate::sqlite::proxies::create(&self.pool, input, org_id).await
    }

    async fn delete_proxy(&self, id: ProxyId, org_id: OrgId) -> DbResult<()> {
        crate::sqlite::proxies::delete(&self.pool, id, org_id).await
    }

    async fn set_active_proxy(&self, id: ProxyId, active: bool, org_id: OrgId) -> DbResult<()> {
        crate::sqlite::proxies::set_active(&self.pool, id, active, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreOnCall for SqliteStore {
    async fn list_on_call(&self, org_id: OrgId) -> DbResult<Vec<OnCallSchedule>> {
        unimplemented!("SqliteStore::list_on_call: on_call domain not yet ported (multi-DB P1)")
    }

    async fn get_on_call(&self, id: OnCallScheduleId, org_id: OrgId) -> DbResult<OnCallSchedule> {
        unimplemented!("SqliteStore::get_on_call: on_call domain not yet ported (multi-DB P1)")
    }

    async fn get_on_call_unscoped(&self, id: OnCallScheduleId) -> DbResult<OnCallSchedule> {
        unimplemented!(
            "SqliteStore::get_on_call_unscoped: on_call domain not yet ported (multi-DB P1)"
        )
    }

    async fn create_on_call(
        &self,
        input: NewOnCallSchedule,
        org_id: OrgId,
    ) -> DbResult<OnCallSchedule> {
        unimplemented!("SqliteStore::create_on_call: on_call domain not yet ported (multi-DB P1)")
    }

    async fn update_on_call(
        &self,
        id: OnCallScheduleId,
        patch: UpdateOnCallSchedule,
        org_id: OrgId,
    ) -> DbResult<OnCallSchedule> {
        unimplemented!("SqliteStore::update_on_call: on_call domain not yet ported (multi-DB P1)")
    }

    async fn delete_on_call(&self, id: OnCallScheduleId, org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_on_call: on_call domain not yet ported (multi-DB P1)")
    }

    async fn oncall_current_channel(
        &self,
        id: OnCallScheduleId,
        at: OffsetDateTime,
    ) -> DbResult<Option<NotificationId>> {
        unimplemented!(
            "SqliteStore::oncall_current_channel: on_call domain not yet ported (multi-DB P1)"
        )
    }

    async fn oncall_current_target(
        &self,
        id: OnCallScheduleId,
        at: OffsetDateTime,
    ) -> DbResult<Option<OnCallTarget>> {
        unimplemented!(
            "SqliteStore::oncall_current_target: on_call domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreRecoveryCodes for SqliteStore {
    async fn issue_recovery_codes(&self, user: UserId, count: usize) -> DbResult<Vec<String>> {
        unimplemented!(
            "SqliteStore::issue_recovery_codes: recovery_codes domain not yet ported (multi-DB P1)"
        )
    }

    async fn consume_recovery_code(&self, user: UserId, code: &str) -> DbResult<bool> {
        unimplemented!("SqliteStore::consume_recovery_code: recovery_codes domain not yet ported (multi-DB P1)")
    }

    async fn delete_recovery_codes_for_user(&self, user: UserId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_recovery_codes_for_user: recovery_codes domain not yet ported (multi-DB P1)")
    }

    async fn remaining_recovery_codes(&self, user: UserId) -> DbResult<i64> {
        unimplemented!("SqliteStore::remaining_recovery_codes: recovery_codes domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreApiKeys for SqliteStore {
    async fn list_api_keys(&self, org_id: OrgId) -> DbResult<Vec<ApiKey>> {
        unimplemented!("SqliteStore::list_api_keys: api_keys domain not yet ported (multi-DB P1)")
    }

    async fn create_api_key(
        &self,
        input: NewApiKey,
        created_by: UserId,
        org_id: OrgId,
    ) -> DbResult<IssuedApiKey> {
        unimplemented!("SqliteStore::create_api_key: api_keys domain not yet ported (multi-DB P1)")
    }

    async fn delete_api_key(&self, id: ApiKeyId, org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_api_key: api_keys domain not yet ported (multi-DB P1)")
    }

    async fn lookup_api_key(&self, token: &str) -> DbResult<(ApiKey, UserId, OrgId)> {
        unimplemented!("SqliteStore::lookup_api_key: api_keys domain not yet ported (multi-DB P1)")
    }

    async fn touch_api_key_last_used(&self, id: ApiKeyId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::touch_api_key_last_used: api_keys domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreEscalations for SqliteStore {
    async fn list_escalation_policies(&self, org_id: OrgId) -> DbResult<Vec<EscalationPolicy>> {
        unimplemented!("SqliteStore::list_escalation_policies: escalations domain not yet ported (multi-DB P1)")
    }

    async fn get_escalation_policy(
        &self,
        id: EscalationPolicyId,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy> {
        unimplemented!(
            "SqliteStore::get_escalation_policy: escalations domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_escalation_policy_unscoped(
        &self,
        id: EscalationPolicyId,
    ) -> DbResult<EscalationPolicy> {
        unimplemented!("SqliteStore::get_escalation_policy_unscoped: escalations domain not yet ported (multi-DB P1)")
    }

    async fn create_escalation_policy(
        &self,
        input: NewEscalationPolicy,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy> {
        unimplemented!("SqliteStore::create_escalation_policy: escalations domain not yet ported (multi-DB P1)")
    }

    async fn update_escalation_policy(
        &self,
        id: EscalationPolicyId,
        patch: UpdateEscalationPolicy,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy> {
        unimplemented!("SqliteStore::update_escalation_policy: escalations domain not yet ported (multi-DB P1)")
    }

    async fn delete_escalation_policy(
        &self,
        id: EscalationPolicyId,
        org_id: OrgId,
    ) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_escalation_policy: escalations domain not yet ported (multi-DB P1)")
    }

    async fn open_episode(
        &self,
        monitor_id: MonitorId,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>> {
        unimplemented!("SqliteStore::open_episode: escalations domain not yet ported (multi-DB P1)")
    }

    async fn open_episode_for_subject(
        &self,
        kind: &str,
        subject_ref: &str,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>> {
        unimplemented!("SqliteStore::open_episode_for_subject: escalations domain not yet ported (multi-DB P1)")
    }

    async fn resolve_subject(
        &self,
        kind: &str,
        subject_ref: &str,
    ) -> DbResult<Option<EscalationEpisode>> {
        unimplemented!(
            "SqliteStore::resolve_subject: escalations domain not yet ported (multi-DB P1)"
        )
    }

    async fn ack_episode(&self, episode_id: Uuid, by: UserId) -> DbResult<EscalationEpisode> {
        unimplemented!("SqliteStore::ack_episode: escalations domain not yet ported (multi-DB P1)")
    }

    async fn list_open_episodes(&self) -> DbResult<Vec<EscalationEpisode>> {
        unimplemented!(
            "SqliteStore::list_open_episodes: escalations domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_open_episodes_for_org(&self, org_id: OrgId) -> DbResult<Vec<EscalationEpisode>> {
        unimplemented!("SqliteStore::list_open_episodes_for_org: escalations domain not yet ported (multi-DB P1)")
    }

    async fn episode_in_org(&self, episode: Uuid, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::episode_in_org: escalations domain not yet ported (multi-DB P1)"
        )
    }

    async fn open_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
    ) -> DbResult<Option<EscalationEpisode>> {
        unimplemented!("SqliteStore::open_episode_for_monitor: escalations domain not yet ported (multi-DB P1)")
    }

    async fn ack_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
        by: UserId,
    ) -> DbResult<EscalationEpisode> {
        unimplemented!(
            "SqliteStore::ack_episode_for_monitor: escalations domain not yet ported (multi-DB P1)"
        )
    }

    async fn resolve_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
    ) -> DbResult<Option<EscalationEpisode>> {
        unimplemented!("SqliteStore::resolve_episode_for_monitor: escalations domain not yet ported (multi-DB P1)")
    }

    async fn advance_episode(
        &self,
        episode_id: Uuid,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>> {
        unimplemented!(
            "SqliteStore::advance_episode: escalations domain not yet ported (multi-DB P1)"
        )
    }

    async fn due_episodes(&self) -> DbResult<Vec<EscalationEpisode>> {
        unimplemented!("SqliteStore::due_episodes: escalations domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreMaintenance for SqliteStore {
    async fn list_maintenance_windows(&self, org_id: OrgId) -> DbResult<Vec<MaintenanceWindow>> {
        unimplemented!("SqliteStore::list_maintenance_windows: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn get_maintenance_window(
        &self,
        id: MaintenanceId,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow> {
        unimplemented!(
            "SqliteStore::get_maintenance_window: maintenance domain not yet ported (multi-DB P1)"
        )
    }

    async fn create_maintenance_window(
        &self,
        input: NewMaintenanceWindow,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow> {
        unimplemented!("SqliteStore::create_maintenance_window: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn update_maintenance_window(
        &self,
        id: MaintenanceId,
        patch: UpdateMaintenanceWindow,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow> {
        unimplemented!("SqliteStore::update_maintenance_window: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn delete_maintenance_window(&self, id: MaintenanceId, org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_maintenance_window: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn set_active_maintenance(
        &self,
        id: MaintenanceId,
        active: bool,
        org_id: OrgId,
    ) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::set_active_maintenance: maintenance domain not yet ported (multi-DB P1)"
        )
    }

    async fn attach_maintenance_monitor(
        &self,
        window: MaintenanceId,
        monitor: MonitorId,
    ) -> DbResult<()> {
        unimplemented!("SqliteStore::attach_maintenance_monitor: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn detach_maintenance_monitor(
        &self,
        window: MaintenanceId,
        monitor: MonitorId,
    ) -> DbResult<()> {
        unimplemented!("SqliteStore::detach_maintenance_monitor: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn is_in_active_window(&self, monitor: MonitorId) -> DbResult<bool> {
        unimplemented!(
            "SqliteStore::is_in_active_window: maintenance domain not yet ported (multi-DB P1)"
        )
    }

    async fn maintenance_transitions_needing_notification(
        &self,
    ) -> DbResult<Vec<MaintenanceTransition>> {
        unimplemented!("SqliteStore::maintenance_transitions_needing_notification: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn mark_maintenance_notified_start(&self, id: MaintenanceId) -> DbResult<()> {
        unimplemented!("SqliteStore::mark_maintenance_notified_start: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn mark_maintenance_notified_end(&self, id: MaintenanceId) -> DbResult<()> {
        unimplemented!("SqliteStore::mark_maintenance_notified_end: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn confirmed_subscriber_emails_for_monitors(
        &self,
        monitors: &[MonitorId],
    ) -> DbResult<Vec<String>> {
        unimplemented!("SqliteStore::confirmed_subscriber_emails_for_monitors: maintenance domain not yet ported (multi-DB P1)")
    }

    async fn public_maintenance_for_status_page(
        &self,
        page: StatusPageId,
    ) -> DbResult<Vec<PublicMaintenance>> {
        unimplemented!("SqliteStore::public_maintenance_for_status_page: maintenance domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreIngestTokens for SqliteStore {
    async fn create_ingest_token(
        &self,
        page: StatusPageId,
        input: NewIngestToken,
    ) -> DbResult<IngestToken> {
        unimplemented!(
            "SqliteStore::create_ingest_token: ingest_tokens domain not yet ported (multi-DB P1)"
        )
    }

    async fn create_ingest_token_with_token(
        &self,
        page: StatusPageId,
        label: &str,
        token: &str,
    ) -> DbResult<IngestToken> {
        unimplemented!("SqliteStore::create_ingest_token_with_token: ingest_tokens domain not yet ported (multi-DB P1)")
    }

    async fn set_ingest_token_mapping(
        &self,
        id: IngestTokenId,
        mapping: Option<serde_json::Value>,
        org_id: OrgId,
    ) -> DbResult<IngestToken> {
        unimplemented!("SqliteStore::set_ingest_token_mapping: ingest_tokens domain not yet ported (multi-DB P1)")
    }

    async fn list_ingest_tokens_for_page(&self, page: StatusPageId) -> DbResult<Vec<IngestToken>> {
        unimplemented!("SqliteStore::list_ingest_tokens_for_page: ingest_tokens domain not yet ported (multi-DB P1)")
    }

    async fn find_ingest_token_by_token(&self, token: &str) -> DbResult<IngestToken> {
        unimplemented!("SqliteStore::find_ingest_token_by_token: ingest_tokens domain not yet ported (multi-DB P1)")
    }

    async fn delete_ingest_token(&self, id: IngestTokenId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_ingest_token: ingest_tokens domain not yet ported (multi-DB P1)"
        )
    }

    async fn touch_ingest_token_last_used(&self, id: IngestTokenId) -> DbResult<()> {
        unimplemented!("SqliteStore::touch_ingest_token_last_used: ingest_tokens domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreTags for SqliteStore {
    async fn list_tags(&self, org_id: OrgId) -> DbResult<Vec<Tag>> {
        crate::sqlite::tags::list(&self.pool, org_id).await
    }

    async fn get_tag(&self, id: TagId, org_id: OrgId) -> DbResult<Tag> {
        crate::sqlite::tags::get(&self.pool, id, org_id).await
    }

    async fn create_tag(&self, input: NewTag, org_id: OrgId) -> DbResult<Tag> {
        crate::sqlite::tags::create(&self.pool, input, org_id).await
    }

    async fn update_tag(&self, id: TagId, patch: UpdateTag, org_id: OrgId) -> DbResult<Tag> {
        crate::sqlite::tags::update(&self.pool, id, patch, org_id).await
    }

    async fn tag_usage(&self, org_id: OrgId) -> DbResult<Vec<TagUsage>> {
        crate::sqlite::tags::usage(&self.pool, org_id).await
    }

    async fn delete_tag(&self, id: TagId, org_id: OrgId) -> DbResult<()> {
        crate::sqlite::tags::delete(&self.pool, id, org_id).await
    }

    async fn attach_tag(&self, monitor: MonitorId, tag: TagId) -> DbResult<()> {
        crate::sqlite::tags::attach(&self.pool, monitor, tag).await
    }

    async fn detach_tag(&self, monitor: MonitorId, tag: TagId) -> DbResult<()> {
        crate::sqlite::tags::detach(&self.pool, monitor, tag).await
    }

    async fn list_tags_for_monitor(&self, monitor: MonitorId) -> DbResult<Vec<TagBrief>> {
        crate::sqlite::tags::list_for_monitor(&self.pool, monitor).await
    }

    async fn hydrate_tags_for_channels(
        &self,
        ids: &[NotificationId],
    ) -> DbResult<HashMap<NotificationId, Vec<TagBrief>>> {
        crate::sqlite::tags::hydrate_for_channels(&self.pool, ids).await
    }

    async fn hydrate_tags_for_monitors(
        &self,
        ids: &[MonitorId],
    ) -> DbResult<HashMap<MonitorId, Vec<TagBrief>>> {
        crate::sqlite::tags::hydrate_for_monitors(&self.pool, ids).await
    }
}

#[async_trait::async_trait]
impl StoreTemplates for SqliteStore {
    async fn list_templates(&self, org_id: OrgId) -> DbResult<Vec<Template>> {
        unimplemented!("SqliteStore::list_templates: templates domain not yet ported (multi-DB P1)")
    }

    async fn get_template(&self, id: NotificationTemplateId, org_id: OrgId) -> DbResult<Template> {
        unimplemented!("SqliteStore::get_template: templates domain not yet ported (multi-DB P1)")
    }

    async fn create_template(&self, input: NewTemplate, org_id: OrgId) -> DbResult<Template> {
        unimplemented!(
            "SqliteStore::create_template: templates domain not yet ported (multi-DB P1)"
        )
    }

    async fn update_template(
        &self,
        id: NotificationTemplateId,
        input: UpdateTemplate,
        org_id: OrgId,
    ) -> DbResult<Template> {
        unimplemented!(
            "SqliteStore::update_template: templates domain not yet ported (multi-DB P1)"
        )
    }

    async fn delete_template(&self, id: NotificationTemplateId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_template: templates domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_template_render_strings(
        &self,
        id: NotificationTemplateId,
    ) -> DbResult<RenderedTemplate> {
        unimplemented!("SqliteStore::get_template_render_strings: templates domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreTelemetryRules for SqliteStore {
    async fn list_telemetry_rules(&self, org_id: OrgId) -> DbResult<Vec<TelemetryRule>> {
        unimplemented!("SqliteStore::list_telemetry_rules: telemetry_rules domain not yet ported (multi-DB P1)")
    }

    async fn list_all_telemetry_rules(&self) -> DbResult<Vec<TelemetryRule>> {
        unimplemented!("SqliteStore::list_all_telemetry_rules: telemetry_rules domain not yet ported (multi-DB P1)")
    }

    async fn get_telemetry_rule(
        &self,
        id: TelemetryRuleId,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule> {
        unimplemented!(
            "SqliteStore::get_telemetry_rule: telemetry_rules domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_telemetry_rule_unscoped(&self, id: TelemetryRuleId) -> DbResult<TelemetryRule> {
        unimplemented!("SqliteStore::get_telemetry_rule_unscoped: telemetry_rules domain not yet ported (multi-DB P1)")
    }

    async fn create_telemetry_rule(
        &self,
        input: NewTelemetryRule,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule> {
        unimplemented!("SqliteStore::create_telemetry_rule: telemetry_rules domain not yet ported (multi-DB P1)")
    }

    async fn update_telemetry_rule(
        &self,
        id: TelemetryRuleId,
        patch: UpdateTelemetryRule,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule> {
        unimplemented!("SqliteStore::update_telemetry_rule: telemetry_rules domain not yet ported (multi-DB P1)")
    }

    async fn delete_telemetry_rule(&self, id: TelemetryRuleId, org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_telemetry_rule: telemetry_rules domain not yet ported (multi-DB P1)")
    }

    async fn evaluate_telemetry_rules_tick(&self) -> DbResult<Vec<TelemetryRuleEvent>> {
        unimplemented!("SqliteStore::evaluate_telemetry_rules_tick: telemetry_rules domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreMetricRules for SqliteStore {
    async fn list_metric_rules(&self, org_id: OrgId) -> DbResult<Vec<MetricRule>> {
        unimplemented!(
            "SqliteStore::list_metric_rules: metric_rules domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_all_metric_rules(&self) -> DbResult<Vec<MetricRule>> {
        unimplemented!(
            "SqliteStore::list_all_metric_rules: metric_rules domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_metric_rule(&self, id: MetricRuleId, org_id: OrgId) -> DbResult<MetricRule> {
        unimplemented!(
            "SqliteStore::get_metric_rule: metric_rules domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_metric_rule_unscoped(&self, id: MetricRuleId) -> DbResult<MetricRule> {
        unimplemented!("SqliteStore::get_metric_rule_unscoped: metric_rules domain not yet ported (multi-DB P1)")
    }

    async fn create_metric_rule(
        &self,
        input: NewMetricRule,
        org_id: OrgId,
    ) -> DbResult<MetricRule> {
        unimplemented!(
            "SqliteStore::create_metric_rule: metric_rules domain not yet ported (multi-DB P1)"
        )
    }

    async fn update_metric_rule(
        &self,
        id: MetricRuleId,
        patch: UpdateMetricRule,
        org_id: OrgId,
    ) -> DbResult<MetricRule> {
        unimplemented!(
            "SqliteStore::update_metric_rule: metric_rules domain not yet ported (multi-DB P1)"
        )
    }

    async fn delete_metric_rule(&self, id: MetricRuleId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_metric_rule: metric_rules domain not yet ported (multi-DB P1)"
        )
    }

    async fn evaluate_metric_rules_tick(&self) -> DbResult<Vec<MetricRuleEvent>> {
        unimplemented!("SqliteStore::evaluate_metric_rules_tick: metric_rules domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreMonitorGroups for SqliteStore {
    async fn monitor_group_in_org(&self, group: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::monitor_group_in_org: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_monitor_groups(&self, org_id: OrgId) -> DbResult<Vec<MonitorGroup>> {
        unimplemented!(
            "SqliteStore::list_monitor_groups: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn create_monitor_group(
        &self,
        input: NewMonitorGroup,
        org_id: OrgId,
    ) -> DbResult<MonitorGroup> {
        unimplemented!(
            "SqliteStore::create_monitor_group: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn update_monitor_group(
        &self,
        id: MonitorGroupId,
        patch: UpdateMonitorGroup,
        org_id: OrgId,
    ) -> DbResult<MonitorGroup> {
        unimplemented!(
            "SqliteStore::update_monitor_group: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn would_form_group_cycle(
        &self,
        group: MonitorGroupId,
        new_parent: MonitorGroupId,
    ) -> DbResult<bool> {
        unimplemented!("SqliteStore::would_form_group_cycle: monitor_groups domain not yet ported (multi-DB P1)")
    }

    async fn delete_monitor_group(&self, id: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_monitor_group: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn parents_of(&self, child: MonitorId) -> DbResult<Vec<MonitorId>> {
        unimplemented!(
            "SqliteStore::parents_of: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn children_of(&self, parent: MonitorId) -> DbResult<Vec<MonitorId>> {
        unimplemented!(
            "SqliteStore::children_of: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn any_parent_down(&self, child: MonitorId) -> DbResult<bool> {
        unimplemented!(
            "SqliteStore::any_parent_down: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn attach_dependency(&self, child: MonitorId, parent: MonitorId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::attach_dependency: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn detach_dependency(&self, child: MonitorId, parent: MonitorId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::detach_dependency: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }

    async fn would_form_cycle(&self, child: MonitorId, parent: MonitorId) -> DbResult<bool> {
        unimplemented!(
            "SqliteStore::would_form_cycle: monitor_groups domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreSilences for SqliteStore {
    async fn is_silenced(&self, monitor: Option<Uuid>) -> DbResult<bool> {
        unimplemented!("SqliteStore::is_silenced: silences domain not yet ported (multi-DB P1)")
    }

    async fn create_silence(&self, s: NewSilence<'_>, org_id: OrgId) -> DbResult<Uuid> {
        unimplemented!("SqliteStore::create_silence: silences domain not yet ported (multi-DB P1)")
    }

    async fn list_active_silences(&self, org_id: OrgId) -> DbResult<Vec<Silence>> {
        unimplemented!(
            "SqliteStore::list_active_silences: silences domain not yet ported (multi-DB P1)"
        )
    }

    async fn delete_silence(&self, id: Uuid, org_id: OrgId) -> DbResult<bool> {
        unimplemented!("SqliteStore::delete_silence: silences domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreOidcState for SqliteStore {
    async fn stash_oidc_state(
        &self,
        state: &str,
        pkce_verifier: &str,
        nonce: Option<&str>,
        return_to: Option<&str>,
    ) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::stash_oidc_state: oidc_state domain not yet ported (multi-DB P1)"
        )
    }

    async fn consume_oidc_state(&self, state: &str) -> DbResult<Option<Consumed>> {
        unimplemented!(
            "SqliteStore::consume_oidc_state: oidc_state domain not yet ported (multi-DB P1)"
        )
    }

    async fn prune_oidc_state(&self) -> DbResult<u64> {
        unimplemented!(
            "SqliteStore::prune_oidc_state: oidc_state domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreStatusPages for SqliteStore {
    async fn list_status_pages(&self, org_id: OrgId) -> DbResult<Vec<StatusPage>> {
        unimplemented!(
            "SqliteStore::list_status_pages: status_pages domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_all_status_pages(&self) -> DbResult<Vec<StatusPage>> {
        unimplemented!(
            "SqliteStore::list_all_status_pages: status_pages domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_status_page(&self, id: StatusPageId, org_id: OrgId) -> DbResult<StatusPage> {
        unimplemented!(
            "SqliteStore::get_status_page: status_pages domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_status_page_by_slug(&self, slug: &str) -> DbResult<StatusPage> {
        unimplemented!("SqliteStore::get_status_page_by_slug: status_pages domain not yet ported (multi-DB P1)")
    }

    async fn get_status_page_unscoped(&self, id: StatusPageId) -> DbResult<StatusPage> {
        unimplemented!("SqliteStore::get_status_page_unscoped: status_pages domain not yet ported (multi-DB P1)")
    }

    async fn find_status_page_by_custom_domain(&self, host: &str) -> DbResult<Option<StatusPage>> {
        unimplemented!("SqliteStore::find_status_page_by_custom_domain: status_pages domain not yet ported (multi-DB P1)")
    }

    async fn create_status_page(
        &self,
        input: NewStatusPage,
        org_id: OrgId,
    ) -> DbResult<StatusPage> {
        unimplemented!(
            "SqliteStore::create_status_page: status_pages domain not yet ported (multi-DB P1)"
        )
    }

    async fn update_status_page(
        &self,
        id: StatusPageId,
        patch: UpdateStatusPage,
        org_id: OrgId,
    ) -> DbResult<StatusPage> {
        unimplemented!(
            "SqliteStore::update_status_page: status_pages domain not yet ported (multi-DB P1)"
        )
    }

    async fn delete_status_page(&self, id: StatusPageId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_status_page: status_pages domain not yet ported (multi-DB P1)"
        )
    }

    async fn status_page_public_view(&self, slug: &str) -> DbResult<PublicStatusPage> {
        unimplemented!("SqliteStore::status_page_public_view: status_pages domain not yet ported (multi-DB P1)")
    }

    async fn verify_status_page_password(&self, slug: &str, candidate: &str) -> DbResult<bool> {
        unimplemented!("SqliteStore::verify_status_page_password: status_pages domain not yet ported (multi-DB P1)")
    }

    async fn list_status_page_sections(
        &self,
        page_id: StatusPageId,
    ) -> DbResult<Vec<StatusPageSection>> {
        unimplemented!("SqliteStore::list_status_page_sections: status_pages domain not yet ported (multi-DB P1)")
    }

    async fn create_status_page_section(
        &self,
        page_id: StatusPageId,
        input: NewStatusPageSection,
    ) -> DbResult<StatusPageSection> {
        unimplemented!("SqliteStore::create_status_page_section: status_pages domain not yet ported (multi-DB P1)")
    }

    async fn update_status_page_section(
        &self,
        id: StatusPageSectionId,
        patch: UpdateStatusPageSection,
    ) -> DbResult<StatusPageSection> {
        unimplemented!("SqliteStore::update_status_page_section: status_pages domain not yet ported (multi-DB P1)")
    }

    async fn delete_status_page_section(&self, id: StatusPageSectionId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_status_page_section: status_pages domain not yet ported (multi-DB P1)")
    }

    async fn assign_status_page_monitor_section(
        &self,
        page_id: StatusPageId,
        monitor_id: MonitorId,
        section_id: Option<StatusPageSectionId>,
    ) -> DbResult<()> {
        unimplemented!("SqliteStore::assign_status_page_monitor_section: status_pages domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreIncidents for SqliteStore {
    async fn create_incident(
        &self,
        page: StatusPageId,
        author: Option<UserId>,
        input: NewIncident,
    ) -> DbResult<Incident> {
        unimplemented!(
            "SqliteStore::create_incident: incidents domain not yet ported (multi-DB P1)"
        )
    }

    async fn find_active_incident_by_dedup_key(
        &self,
        page: StatusPageId,
        key: &str,
    ) -> DbResult<Option<Incident>> {
        unimplemented!("SqliteStore::find_active_incident_by_dedup_key: incidents domain not yet ported (multi-DB P1)")
    }

    async fn list_active_incidents(&self, page: StatusPageId) -> DbResult<Vec<Incident>> {
        unimplemented!(
            "SqliteStore::list_active_incidents: incidents domain not yet ported (multi-DB P1)"
        )
    }

    async fn recent_incidents(&self, limit: i64, org_id: OrgId) -> DbResult<Vec<Incident>> {
        unimplemented!(
            "SqliteStore::recent_incidents: incidents domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_resolved_incident_history(
        &self,
        page: StatusPageId,
        limit: i64,
    ) -> DbResult<Vec<Incident>> {
        unimplemented!("SqliteStore::list_resolved_incident_history: incidents domain not yet ported (multi-DB P1)")
    }

    async fn resolve_incident(&self, id: IncidentId, now: OffsetDateTime) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::resolve_incident: incidents domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_all_incidents(&self, page: StatusPageId, limit: i64) -> DbResult<Vec<Incident>> {
        unimplemented!(
            "SqliteStore::list_all_incidents: incidents domain not yet ported (multi-DB P1)"
        )
    }

    async fn delete_incident(&self, id: IncidentId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_incident: incidents domain not yet ported (multi-DB P1)"
        )
    }

    async fn update_incident(&self, id: IncidentId, patch: UpdateIncident) -> DbResult<Incident> {
        unimplemented!(
            "SqliteStore::update_incident: incidents domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_incident(&self, id: IncidentId) -> DbResult<Incident> {
        unimplemented!("SqliteStore::get_incident: incidents domain not yet ported (multi-DB P1)")
    }

    async fn list_incident_updates(&self, incident: IncidentId) -> DbResult<Vec<IncidentUpdate>> {
        unimplemented!(
            "SqliteStore::list_incident_updates: incidents domain not yet ported (multi-DB P1)"
        )
    }

    async fn post_incident_update(
        &self,
        incident: IncidentId,
        author: Option<UserId>,
        message: String,
    ) -> DbResult<Uuid> {
        unimplemented!(
            "SqliteStore::post_incident_update: incidents domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreRouting for SqliteStore {
    async fn resolve_channels_for_monitor(
        &self,
        monitor: MonitorId,
    ) -> DbResult<Vec<Notification>> {
        unimplemented!("SqliteStore::resolve_channels_for_monitor: routing domain not yet ported (multi-DB P1)")
    }

    async fn group_tag_ids(&self, group: MonitorGroupId) -> DbResult<Vec<TagId>> {
        unimplemented!("SqliteStore::group_tag_ids: routing domain not yet ported (multi-DB P1)")
    }

    async fn channel_tag_ids(&self, notif: NotificationId) -> DbResult<Vec<TagId>> {
        unimplemented!("SqliteStore::channel_tag_ids: routing domain not yet ported (multi-DB P1)")
    }

    async fn group_channel_ids(&self, group: MonitorGroupId) -> DbResult<Vec<NotificationId>> {
        unimplemented!(
            "SqliteStore::group_channel_ids: routing domain not yet ported (multi-DB P1)"
        )
    }

    async fn monitor_exclude_ids(&self, monitor: MonitorId) -> DbResult<Vec<NotificationId>> {
        unimplemented!(
            "SqliteStore::monitor_exclude_ids: routing domain not yet ported (multi-DB P1)"
        )
    }

    async fn tag_group(&self, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
        unimplemented!("SqliteStore::tag_group: routing domain not yet ported (multi-DB P1)")
    }

    async fn untag_group(&self, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
        unimplemented!("SqliteStore::untag_group: routing domain not yet ported (multi-DB P1)")
    }

    async fn tag_channel(&self, notif: NotificationId, tag: TagId) -> DbResult<()> {
        unimplemented!("SqliteStore::tag_channel: routing domain not yet ported (multi-DB P1)")
    }

    async fn untag_channel(&self, notif: NotificationId, tag: TagId) -> DbResult<()> {
        unimplemented!("SqliteStore::untag_channel: routing domain not yet ported (multi-DB P1)")
    }

    async fn attach_group_channel(
        &self,
        group: MonitorGroupId,
        notif: NotificationId,
    ) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::attach_group_channel: routing domain not yet ported (multi-DB P1)"
        )
    }

    async fn detach_group_channel(
        &self,
        group: MonitorGroupId,
        notif: NotificationId,
    ) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::detach_group_channel: routing domain not yet ported (multi-DB P1)"
        )
    }

    async fn exclude_channel(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        unimplemented!("SqliteStore::exclude_channel: routing domain not yet ported (multi-DB P1)")
    }

    async fn unexclude_channel(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::unexclude_channel: routing domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreSubscribers for SqliteStore {
    async fn subscribe_email(
        &self,
        page: StatusPageId,
        email: &str,
    ) -> DbResult<(Subscriber, String)> {
        unimplemented!(
            "SqliteStore::subscribe_email: subscribers domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_subscribers_for_page(&self, page: StatusPageId) -> DbResult<Vec<Subscriber>> {
        unimplemented!("SqliteStore::list_subscribers_for_page: subscribers domain not yet ported (multi-DB P1)")
    }

    async fn confirmed_subscriber_emails_for_page(
        &self,
        page: StatusPageId,
    ) -> DbResult<Vec<String>> {
        unimplemented!("SqliteStore::confirmed_subscriber_emails_for_page: subscribers domain not yet ported (multi-DB P1)")
    }

    async fn delete_subscriber(&self, id: StatusPageSubscriberId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_subscriber: subscribers domain not yet ported (multi-DB P1)"
        )
    }

    async fn unsubscribe_subscriber_by_token(&self, token: &str) -> DbResult<()> {
        unimplemented!("SqliteStore::unsubscribe_subscriber_by_token: subscribers domain not yet ported (multi-DB P1)")
    }

    async fn subscriber_email_for_token(&self, token: &str) -> DbResult<Option<String>> {
        unimplemented!("SqliteStore::subscriber_email_for_token: subscribers domain not yet ported (multi-DB P1)")
    }

    async fn subscriptions_for_email(&self, email: &str) -> DbResult<Vec<ManagedSubscription>> {
        unimplemented!(
            "SqliteStore::subscriptions_for_email: subscribers domain not yet ported (multi-DB P1)"
        )
    }

    async fn unsubscribe_all_for_email(&self, email: &str) -> DbResult<u64> {
        unimplemented!("SqliteStore::unsubscribe_all_for_email: subscribers domain not yet ported (multi-DB P1)")
    }

    async fn unsubscribe_email_from_page(&self, page: StatusPageId, email: &str) -> DbResult<()> {
        unimplemented!("SqliteStore::unsubscribe_email_from_page: subscribers domain not yet ported (multi-DB P1)")
    }

    async fn subscriber_page_for(
        &self,
        id: StatusPageSubscriberId,
    ) -> DbResult<Option<StatusPageId>> {
        unimplemented!(
            "SqliteStore::subscriber_page_for: subscribers domain not yet ported (multi-DB P1)"
        )
    }

    async fn subscriber_token_for(&self, id: Uuid) -> DbResult<Option<String>> {
        unimplemented!(
            "SqliteStore::subscriber_token_for: subscribers domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreDetection for SqliteStore {
    async fn detection_regex_is_valid(&self, pattern: &str) -> DbResult<bool> {
        unimplemented!(
            "SqliteStore::detection_regex_is_valid: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_detection_rules(&self, org_id: OrgId) -> DbResult<Vec<DetectionRule>> {
        unimplemented!(
            "SqliteStore::list_detection_rules: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_all_detection_rules(&self) -> DbResult<Vec<DetectionRule>> {
        unimplemented!(
            "SqliteStore::list_all_detection_rules: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_detection_rule(
        &self,
        id: DetectionRuleId,
        org_id: OrgId,
    ) -> DbResult<DetectionRule> {
        unimplemented!(
            "SqliteStore::get_detection_rule: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_detection_rule_unscoped(&self, id: DetectionRuleId) -> DbResult<DetectionRule> {
        unimplemented!("SqliteStore::get_detection_rule_unscoped: detection domain not yet ported (multi-DB P1)")
    }

    async fn create_detection_rule(
        &self,
        input: NewDetectionRule,
        org_id: OrgId,
    ) -> DbResult<DetectionRule> {
        unimplemented!(
            "SqliteStore::create_detection_rule: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn update_detection_rule(
        &self,
        id: DetectionRuleId,
        patch: UpdateDetectionRule,
        org_id: OrgId,
    ) -> DbResult<DetectionRule> {
        unimplemented!(
            "SqliteStore::update_detection_rule: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn delete_detection_rule(&self, id: DetectionRuleId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_detection_rule: detection domain not yet ported (multi-DB P1)"
        )
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
        unimplemented!(
            "SqliteStore::preview_detection: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn has_recent_detection_finding(
        &self,
        rule_id: DetectionRuleId,
        secs: i64,
        entity: Option<&str>,
    ) -> DbResult<bool> {
        unimplemented!("SqliteStore::has_recent_detection_finding: detection domain not yet ported (multi-DB P1)")
    }

    async fn list_detection_findings(
        &self,
        limit: i64,
        open_only: bool,
    ) -> DbResult<Vec<DetectionFinding>> {
        unimplemented!(
            "SqliteStore::list_detection_findings: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_detection_findings_for_org(
        &self,
        limit: i64,
        open_only: bool,
        org_id: OrgId,
    ) -> DbResult<Vec<DetectionFinding>> {
        unimplemented!("SqliteStore::list_detection_findings_for_org: detection domain not yet ported (multi-DB P1)")
    }

    async fn detection_finding_in_org(
        &self,
        finding: DetectionFindingId,
        org_id: OrgId,
    ) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::detection_finding_in_org: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn open_detection_findings_count(&self) -> DbResult<i64> {
        unimplemented!("SqliteStore::open_detection_findings_count: detection domain not yet ported (multi-DB P1)")
    }

    async fn fetch_detection_findings_since(
        &self,
        after: Option<OffsetDateTime>,
        limit: i64,
    ) -> DbResult<Vec<DetectionFinding>> {
        unimplemented!("SqliteStore::fetch_detection_findings_since: detection domain not yet ported (multi-DB P1)")
    }

    async fn ack_detection_finding(&self, id: DetectionFindingId) -> DbResult<DetectionFinding> {
        unimplemented!(
            "SqliteStore::ack_detection_finding: detection domain not yet ported (multi-DB P1)"
        )
    }

    async fn evaluate_detection_tick(&self) -> DbResult<Vec<FindingEvent>> {
        unimplemented!(
            "SqliteStore::evaluate_detection_tick: detection domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreSessions for SqliteStore {
    async fn create_session(
        &self,
        user_id: UserId,
        ttl_seconds: i64,
        ip: Option<std::net::IpAddr>,
        user_agent: Option<String>,
    ) -> DbResult<Session> {
        crate::sqlite::sessions::create(&self.pool, user_id, ttl_seconds, ip, user_agent).await
    }

    async fn lookup_session(&self, id: Uuid) -> DbResult<Session> {
        crate::sqlite::sessions::get(&self.pool, id).await
    }

    async fn set_session_active_org(
        &self,
        session_id: Uuid,
        user_id: UserId,
        org_id: Uuid,
    ) -> DbResult<bool> {
        crate::sqlite::sessions::set_active_org(&self.pool, session_id, user_id, org_id).await
    }

    async fn delete_session(&self, id: Uuid) -> DbResult<()> {
        crate::sqlite::sessions::delete(&self.pool, id).await
    }

    async fn delete_sessions_for_user(&self, user_id: UserId) -> DbResult<u64> {
        crate::sqlite::sessions::delete_for_user(&self.pool, user_id).await
    }

    async fn list_sessions_for_user(&self, user_id: UserId) -> DbResult<Vec<SessionInfo>> {
        crate::sqlite::sessions::list_for_user(&self.pool, user_id).await
    }

    async fn delete_one_session_for_user(&self, user_id: UserId, id: Uuid) -> DbResult<bool> {
        crate::sqlite::sessions::delete_one_for_user(&self.pool, user_id, id).await
    }

    async fn delete_other_sessions(&self, user_id: UserId, keep: Uuid) -> DbResult<u64> {
        crate::sqlite::sessions::delete_others(&self.pool, user_id, keep).await
    }

    async fn cleanup_expired_sessions(&self) -> DbResult<u64> {
        crate::sqlite::sessions::cleanup_expired(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreNotifications for SqliteStore {
    async fn list_notifications(&self, org_id: OrgId) -> DbResult<Vec<Notification>> {
        crate::sqlite::notifications::list(&self.pool, org_id).await
    }

    async fn list_all_notifications(&self) -> DbResult<Vec<Notification>> {
        crate::sqlite::notifications::list_all(&self.pool).await
    }

    async fn get_notification(&self, id: NotificationId, org_id: OrgId) -> DbResult<Notification> {
        crate::sqlite::notifications::get(&self.pool, id, org_id).await
    }

    async fn get_notification_unscoped(&self, id: NotificationId) -> DbResult<Notification> {
        crate::sqlite::notifications::get_unscoped(&self.pool, id).await
    }

    async fn create_notification(
        &self,
        input: NewNotification,
        org_id: OrgId,
    ) -> DbResult<Notification> {
        crate::sqlite::notifications::create(&self.pool, input, org_id).await
    }

    async fn update_notification(
        &self,
        id: NotificationId,
        input: UpdateNotification,
        org_id: OrgId,
    ) -> DbResult<Notification> {
        crate::sqlite::notifications::update(&self.pool, id, input, org_id).await
    }

    async fn notification_counts_per_monitor(
        &self,
        org_id: OrgId,
    ) -> DbResult<Vec<MonitorChannelCount>> {
        crate::sqlite::notifications::counts_per_monitor(&self.pool, org_id).await
    }

    async fn delete_notification(&self, id: NotificationId, org_id: OrgId) -> DbResult<()> {
        crate::sqlite::notifications::delete(&self.pool, id, org_id).await
    }

    async fn attach_notification(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::sqlite::notifications::attach(&self.pool, monitor, notif).await
    }

    async fn detach_notification(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::sqlite::notifications::detach(&self.pool, monitor, notif).await
    }

    async fn notifications_for_monitor(&self, monitor: MonitorId) -> DbResult<Vec<Notification>> {
        crate::sqlite::notifications::for_monitor(&self.pool, monitor).await
    }

    async fn mark_notification_fired(&self, id: NotificationId) -> DbResult<()> {
        crate::sqlite::notifications::mark_fired(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreSettings for SqliteStore {
    async fn get_setting(&self, key: &str) -> DbResult<Option<serde_json::Value>> {
        crate::sqlite::settings::get_setting(&self.pool, key).await
    }

    async fn put_setting(&self, key: &str, value: &serde_json::Value) -> DbResult<()> {
        crate::sqlite::settings::put_setting(&self.pool, key, value).await
    }

    async fn delete_setting(&self, key: &str) -> DbResult<()> {
        crate::sqlite::settings::delete_setting(&self.pool, key).await
    }
}

#[async_trait::async_trait]
impl StoreLogs for SqliteStore {
    async fn insert_logs(&self, logs: &[ParsedLog], org_id: OrgId) -> DbResult<u64> {
        unimplemented!("SqliteStore::insert_logs: logs domain not yet ported (multi-DB P1)")
    }

    async fn query_logs(&self, f: LogFilter<'_>, org_id: OrgId) -> DbResult<Vec<LogEntry>> {
        unimplemented!("SqliteStore::query_logs: logs domain not yet ported (multi-DB P1)")
    }

    async fn log_level_counts(
        &self,
        service: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<(String, i64)>> {
        unimplemented!("SqliteStore::log_level_counts: logs domain not yet ported (multi-DB P1)")
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
        unimplemented!("SqliteStore::log_histogram: logs domain not yet ported (multi-DB P1)")
    }

    async fn log_services(&self, org_id: OrgId) -> DbResult<Vec<String>> {
        unimplemented!("SqliteStore::log_services: logs domain not yet ported (multi-DB P1)")
    }

    async fn prune_logs(&self, days: i32) -> DbResult<u64> {
        unimplemented!("SqliteStore::prune_logs: logs domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreTraces for SqliteStore {
    async fn insert_spans(&self, spans: &[ParsedSpan], org_id: OrgId) -> DbResult<u64> {
        unimplemented!("SqliteStore::insert_spans: traces domain not yet ported (multi-DB P1)")
    }

    async fn list_traces(&self, f: TraceFilter<'_>, org_id: OrgId) -> DbResult<Vec<TraceSummary>> {
        unimplemented!("SqliteStore::list_traces: traces domain not yet ported (multi-DB P1)")
    }

    async fn get_trace_spans(&self, trace_id: &str, org_id: OrgId) -> DbResult<Vec<Span>> {
        unimplemented!("SqliteStore::get_trace_spans: traces domain not yet ported (multi-DB P1)")
    }

    async fn trace_service_map(
        &self,
        window_hours: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ServiceEdge>> {
        unimplemented!("SqliteStore::trace_service_map: traces domain not yet ported (multi-DB P1)")
    }

    async fn trace_operation_stats(
        &self,
        service: &str,
        window_hours: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<OperationStat>> {
        unimplemented!(
            "SqliteStore::trace_operation_stats: traces domain not yet ported (multi-DB P1)"
        )
    }

    async fn trace_operation_trend(
        &self,
        service: &str,
        operation: &str,
        window_hours: i64,
        buckets: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<f64>> {
        unimplemented!(
            "SqliteStore::trace_operation_trend: traces domain not yet ported (multi-DB P1)"
        )
    }

    async fn prune_spans(&self, days: i32) -> DbResult<u64> {
        unimplemented!("SqliteStore::prune_spans: traces domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreRum for SqliteStore {
    async fn insert_rum_event(&self, b: &RumBeacon, org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::insert_rum_event: rum domain not yet ported (multi-DB P1)")
    }

    async fn rum_page_samples(
        &self,
        app: Option<&str>,
        url: &str,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RumSample>> {
        unimplemented!("SqliteStore::rum_page_samples: rum domain not yet ported (multi-DB P1)")
    }

    async fn rum_recent_traced(
        &self,
        app: Option<&str>,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RumTracedLoad>> {
        unimplemented!("SqliteStore::rum_recent_traced: rum domain not yet ported (multi-DB P1)")
    }

    async fn rum_summary(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<RumVitals> {
        unimplemented!("SqliteStore::rum_summary: rum domain not yet ported (multi-DB P1)")
    }

    async fn rum_pages(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumPage>> {
        unimplemented!("SqliteStore::rum_pages: rum domain not yet ported (multi-DB P1)")
    }

    async fn rum_browser_breakdown(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumBrowser>> {
        unimplemented!(
            "SqliteStore::rum_browser_breakdown: rum domain not yet ported (multi-DB P1)"
        )
    }

    async fn rum_user_breakdown(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumUser>> {
        unimplemented!("SqliteStore::rum_user_breakdown: rum domain not yet ported (multi-DB P1)")
    }

    async fn rum_apps(&self, org_id: OrgId) -> DbResult<Vec<String>> {
        unimplemented!("SqliteStore::rum_apps: rum domain not yet ported (multi-DB P1)")
    }

    async fn prune_rum(&self, days: i32) -> DbResult<u64> {
        unimplemented!("SqliteStore::prune_rum: rum domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreProfiles for SqliteStore {
    async fn insert_profile(&self, p: NewProfile<'_>, org_id: OrgId) -> DbResult<i64> {
        unimplemented!("SqliteStore::insert_profile: profiles domain not yet ported (multi-DB P1)")
    }

    async fn list_profiles(
        &self,
        service: Option<&str>,
        profile_type: Option<&str>,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ProfileMeta>> {
        unimplemented!("SqliteStore::list_profiles: profiles domain not yet ported (multi-DB P1)")
    }

    async fn profile_folded_in_window(
        &self,
        service: &str,
        profile_type: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        org_id: OrgId,
    ) -> DbResult<Vec<Vec<u8>>> {
        unimplemented!(
            "SqliteStore::profile_folded_in_window: profiles domain not yet ported (multi-DB P1)"
        )
    }

    async fn profile_fetch_folded(
        &self,
        id: i64,
        org_id: OrgId,
    ) -> DbResult<Option<(String, Vec<u8>)>> {
        unimplemented!(
            "SqliteStore::profile_fetch_folded: profiles domain not yet ported (multi-DB P1)"
        )
    }

    async fn profile_services(&self, org_id: OrgId) -> DbResult<Vec<String>> {
        unimplemented!(
            "SqliteStore::profile_services: profiles domain not yet ported (multi-DB P1)"
        )
    }

    async fn profile_types(&self, service: Option<&str>, org_id: OrgId) -> DbResult<Vec<String>> {
        unimplemented!("SqliteStore::profile_types: profiles domain not yet ported (multi-DB P1)")
    }

    async fn prune_profiles(&self, days: i32) -> DbResult<u64> {
        unimplemented!("SqliteStore::prune_profiles: profiles domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreMetrics for SqliteStore {
    async fn monitors_by_status(&self) -> DbResult<Vec<(String, i64)>> {
        unimplemented!(
            "SqliteStore::monitors_by_status: metrics domain not yet ported (multi-DB P1)"
        )
    }

    async fn monitors_by_kind(&self) -> DbResult<Vec<(String, i64)>> {
        unimplemented!("SqliteStore::monitors_by_kind: metrics domain not yet ported (multi-DB P1)")
    }

    async fn channels_active(&self) -> DbResult<i64> {
        unimplemented!("SqliteStore::channels_active: metrics domain not yet ported (multi-DB P1)")
    }

    async fn webpush_subscribers(&self) -> DbResult<i64> {
        unimplemented!(
            "SqliteStore::webpush_subscribers: metrics domain not yet ported (multi-DB P1)"
        )
    }

    async fn heartbeats_recent_by_status(
        &self,
        window_seconds: i64,
    ) -> DbResult<Vec<(String, i64)>> {
        unimplemented!(
            "SqliteStore::heartbeats_recent_by_status: metrics domain not yet ported (multi-DB P1)"
        )
    }

    async fn incidents_open(&self) -> DbResult<i64> {
        unimplemented!("SqliteStore::incidents_open: metrics domain not yet ported (multi-DB P1)")
    }

    async fn pipeline_gauges(&self) -> DbResult<PipelineGauges> {
        unimplemented!("SqliteStore::pipeline_gauges: metrics domain not yet ported (multi-DB P1)")
    }

    async fn storage_usage(&self) -> DbResult<Vec<TableSize>> {
        unimplemented!("SqliteStore::storage_usage: metrics domain not yet ported (multi-DB P1)")
    }

    async fn ingest_gauges(&self) -> DbResult<IngestGauges> {
        unimplemented!("SqliteStore::ingest_gauges: metrics domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreErrorTracking for SqliteStore {
    async fn list_error_projects(&self, org_id: OrgId) -> DbResult<Vec<ErrorProject>> {
        unimplemented!(
            "SqliteStore::list_error_projects: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn error_project_in_org(&self, project: ErrorProjectId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::error_project_in_org: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn error_issue_in_org(&self, issue: ErrorIssueId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::error_issue_in_org: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_error_project(&self, id: ErrorProjectId) -> DbResult<ErrorProject> {
        unimplemented!(
            "SqliteStore::get_error_project: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn org_for_error_project(&self, id: ErrorProjectId) -> DbResult<OrgId> {
        unimplemented!("SqliteStore::org_for_error_project: error_tracking domain not yet ported (multi-DB P1)")
    }

    async fn get_error_project_opt(&self, id: ErrorProjectId) -> DbResult<Option<ErrorProject>> {
        unimplemented!("SqliteStore::get_error_project_opt: error_tracking domain not yet ported (multi-DB P1)")
    }

    async fn find_or_create_error_project_by_name(
        &self,
        name: &str,
        org_id: OrgId,
    ) -> DbResult<ErrorProject> {
        unimplemented!("SqliteStore::find_or_create_error_project_by_name: error_tracking domain not yet ported (multi-DB P1)")
    }

    async fn create_error_project(
        &self,
        input: NewErrorProject,
        org_id: OrgId,
    ) -> DbResult<ErrorProject> {
        unimplemented!(
            "SqliteStore::create_error_project: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn update_error_project(
        &self,
        id: ErrorProjectId,
        patch: UpdateErrorProject,
        org_id: OrgId,
    ) -> DbResult<ErrorProject> {
        unimplemented!(
            "SqliteStore::update_error_project: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn delete_error_project(&self, id: ErrorProjectId, org_id: OrgId) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_error_project: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn record_error_event(
        &self,
        project_id: ErrorProjectId,
        ev: &ParsedEvent,
    ) -> DbResult<RecordOutcome> {
        unimplemented!(
            "SqliteStore::record_error_event: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn error_issues_for_trace(
        &self,
        trace_id: &str,
        org_id: OrgId,
    ) -> DbResult<Vec<TraceErrorRef>> {
        unimplemented!("SqliteStore::error_issues_for_trace: error_tracking domain not yet ported (multi-DB P1)")
    }

    async fn list_error_issues(
        &self,
        project_id: ErrorProjectId,
        status: Option<&str>,
        before_id: Option<Uuid>,
        limit: i64,
    ) -> DbResult<Vec<ErrorIssue>> {
        unimplemented!(
            "SqliteStore::list_error_issues: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn recent_open_error_issues(
        &self,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ErrorIssue>> {
        unimplemented!("SqliteStore::recent_open_error_issues: error_tracking domain not yet ported (multi-DB P1)")
    }

    async fn error_project_event_histogram(
        &self,
        project_id: ErrorProjectId,
        hours: i32,
        buckets: i64,
    ) -> DbResult<Vec<ErrorBucket>> {
        unimplemented!("SqliteStore::error_project_event_histogram: error_tracking domain not yet ported (multi-DB P1)")
    }

    async fn get_error_issue(&self, id: ErrorIssueId) -> DbResult<ErrorIssue> {
        unimplemented!(
            "SqliteStore::get_error_issue: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn error_issue_affected_users(
        &self,
        id: ErrorIssueId,
        limit: i64,
    ) -> DbResult<Vec<AffectedUser>> {
        unimplemented!("SqliteStore::error_issue_affected_users: error_tracking domain not yet ported (multi-DB P1)")
    }

    async fn error_issue_stats(&self, id: ErrorIssueId) -> DbResult<IssueStats> {
        unimplemented!(
            "SqliteStore::error_issue_stats: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn set_error_issue_status(&self, id: ErrorIssueId, status: &str) -> DbResult<ErrorIssue> {
        unimplemented!("SqliteStore::set_error_issue_status: error_tracking domain not yet ported (multi-DB P1)")
    }

    async fn assign_error_issue(
        &self,
        id: ErrorIssueId,
        assignee: Option<UserId>,
    ) -> DbResult<ErrorIssue> {
        unimplemented!(
            "SqliteStore::assign_error_issue: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn error_assignable_users(&self) -> DbResult<Vec<crate::error_tracking::AssignableUser>> {
        unimplemented!("SqliteStore::error_assignable_users: error_tracking domain not yet ported (multi-DB P1)")
    }

    async fn list_error_events(
        &self,
        issue_id: ErrorIssueId,
        limit: i64,
    ) -> DbResult<Vec<ErrorEvent>> {
        unimplemented!(
            "SqliteStore::list_error_events: error_tracking domain not yet ported (multi-DB P1)"
        )
    }

    async fn prune_error_events(&self) -> DbResult<u64> {
        unimplemented!(
            "SqliteStore::prune_error_events: error_tracking domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreScheduledReports for SqliteStore {
    async fn list_scheduled_reports(&self, org_id: OrgId) -> DbResult<Vec<ScheduledReport>> {
        unimplemented!("SqliteStore::list_scheduled_reports: scheduled_reports domain not yet ported (multi-DB P1)")
    }

    async fn get_scheduled_report(
        &self,
        id: ScheduledReportId,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport> {
        unimplemented!("SqliteStore::get_scheduled_report: scheduled_reports domain not yet ported (multi-DB P1)")
    }

    async fn create_scheduled_report(
        &self,
        input: NewScheduledReport,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport> {
        unimplemented!("SqliteStore::create_scheduled_report: scheduled_reports domain not yet ported (multi-DB P1)")
    }

    async fn update_scheduled_report(
        &self,
        id: ScheduledReportId,
        input: UpdateScheduledReport,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport> {
        unimplemented!("SqliteStore::update_scheduled_report: scheduled_reports domain not yet ported (multi-DB P1)")
    }

    async fn delete_scheduled_report(&self, id: ScheduledReportId, org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_scheduled_report: scheduled_reports domain not yet ported (multi-DB P1)")
    }

    async fn due_scheduled_reports(&self, now: OffsetDateTime) -> DbResult<Vec<ScheduledReport>> {
        unimplemented!("SqliteStore::due_scheduled_reports: scheduled_reports domain not yet ported (multi-DB P1)")
    }

    async fn render_scheduled_report(
        &self,
        report_name: &str,
        cadence: &str,
    ) -> DbResult<(String, String)> {
        unimplemented!("SqliteStore::render_scheduled_report: scheduled_reports domain not yet ported (multi-DB P1)")
    }

    async fn mark_scheduled_report_sent(&self, id: ScheduledReportId) -> DbResult<()> {
        unimplemented!("SqliteStore::mark_scheduled_report_sent: scheduled_reports domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreIncidentTemplates for SqliteStore {
    async fn list_incident_templates(&self, org_id: OrgId) -> DbResult<Vec<IncidentTemplate>> {
        unimplemented!("SqliteStore::list_incident_templates: incident_templates domain not yet ported (multi-DB P1)")
    }

    async fn get_incident_template(
        &self,
        id: IncidentTemplateId,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate> {
        unimplemented!("SqliteStore::get_incident_template: incident_templates domain not yet ported (multi-DB P1)")
    }

    async fn create_incident_template(
        &self,
        input: NewIncidentTemplate,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate> {
        unimplemented!("SqliteStore::create_incident_template: incident_templates domain not yet ported (multi-DB P1)")
    }

    async fn update_incident_template(
        &self,
        id: IncidentTemplateId,
        input: UpdateIncidentTemplate,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate> {
        unimplemented!("SqliteStore::update_incident_template: incident_templates domain not yet ported (multi-DB P1)")
    }

    async fn delete_incident_template(
        &self,
        id: IncidentTemplateId,
        org_id: OrgId,
    ) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_incident_template: incident_templates domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreMonitorPresets for SqliteStore {
    async fn list_monitor_presets(&self, org_id: OrgId) -> DbResult<Vec<MonitorPreset>> {
        unimplemented!("SqliteStore::list_monitor_presets: monitor_presets domain not yet ported (multi-DB P1)")
    }

    async fn get_monitor_preset(
        &self,
        id: MonitorPresetId,
        org_id: OrgId,
    ) -> DbResult<MonitorPreset> {
        unimplemented!(
            "SqliteStore::get_monitor_preset: monitor_presets domain not yet ported (multi-DB P1)"
        )
    }

    async fn create_monitor_preset(
        &self,
        input: NewMonitorPreset,
        org_id: OrgId,
    ) -> DbResult<MonitorPreset> {
        unimplemented!("SqliteStore::create_monitor_preset: monitor_presets domain not yet ported (multi-DB P1)")
    }

    async fn delete_monitor_preset(&self, id: MonitorPresetId, org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_monitor_preset: monitor_presets domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreMonitorTemplates for SqliteStore {
    async fn list_monitor_templates(&self, org_id: OrgId) -> DbResult<Vec<MonitorTemplate>> {
        unimplemented!("SqliteStore::list_monitor_templates: monitor_templates domain not yet ported (multi-DB P1)")
    }

    async fn get_monitor_template(
        &self,
        id: MonitorTemplateId,
        org_id: OrgId,
    ) -> DbResult<MonitorTemplate> {
        unimplemented!("SqliteStore::get_monitor_template: monitor_templates domain not yet ported (multi-DB P1)")
    }

    async fn create_monitor_template(
        &self,
        input: NewMonitorTemplate,
        org_id: OrgId,
    ) -> DbResult<MonitorTemplate> {
        unimplemented!("SqliteStore::create_monitor_template: monitor_templates domain not yet ported (multi-DB P1)")
    }

    async fn delete_monitor_template(&self, id: MonitorTemplateId, org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_monitor_template: monitor_templates domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreDeliveryLog for SqliteStore {
    async fn record_delivery(&self, entry: NewDelivery<'_>) -> DbResult<DeliveryEntry> {
        crate::sqlite::delivery_log::record(&self.pool, entry).await
    }

    async fn get_delivery(&self, id: i64, org_id: OrgId) -> DbResult<Option<DeliveryEntry>> {
        crate::sqlite::delivery_log::get(&self.pool, id, org_id).await
    }

    async fn list_deliveries(
        &self,
        limit: i64,
        before_ts: Option<OffsetDateTime>,
        ok: Option<bool>,
        monitor: Option<Uuid>,
        channel: Option<&str>,
        org_id: OrgId,
    ) -> DbResult<Vec<DeliveryEntry>> {
        crate::sqlite::delivery_log::list(
            &self.pool, limit, before_ts, ok, monitor, channel, org_id,
        )
        .await
    }

    async fn list_all_deliveries(&self, limit: i64, org_id: OrgId) -> DbResult<Vec<DeliveryEntry>> {
        crate::sqlite::delivery_log::list_all(&self.pool, limit, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreAgents for SqliteStore {
    async fn list_agents(&self, org_id: OrgId) -> DbResult<Vec<Agent>> {
        crate::sqlite::agents::list(&self.pool, org_id).await
    }

    async fn get_agent(&self, id: AgentId, org_id: OrgId) -> DbResult<Agent> {
        crate::sqlite::agents::get(&self.pool, id, org_id).await
    }

    async fn create_agent(&self, input: NewAgent, org_id: OrgId) -> DbResult<IssuedAgent> {
        crate::sqlite::agents::create(&self.pool, input, org_id).await
    }

    async fn update_agent(
        &self,
        id: AgentId,
        patch: UpdateAgent,
        org_id: OrgId,
    ) -> DbResult<Agent> {
        crate::sqlite::agents::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_agent(&self, id: AgentId, org_id: OrgId) -> DbResult<()> {
        crate::sqlite::agents::delete(&self.pool, id, org_id).await
    }

    async fn lookup_agent(&self, token: &str) -> DbResult<Agent> {
        crate::sqlite::agents::lookup(&self.pool, token).await
    }

    async fn touch_agent_seen(&self, id: AgentId, version: Option<&str>) -> DbResult<()> {
        crate::sqlite::agents::touch_seen(&self.pool, id, version).await
    }
}

#[async_trait::async_trait]
impl StoreMetricSamples for SqliteStore {
    async fn insert_metric_samples(&self, samples: &[PromSample], org_id: OrgId) -> DbResult<()> {
        unimplemented!("SqliteStore::insert_metric_samples: metric_samples domain not yet ported (multi-DB P1)")
    }

    async fn list_metric_sample_series(&self, org_id: OrgId) -> DbResult<Vec<Series>> {
        unimplemented!("SqliteStore::list_metric_sample_series: metric_samples domain not yet ported (multi-DB P1)")
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
        unimplemented!("SqliteStore::metric_sample_range_query: metric_samples domain not yet ported (multi-DB P1)")
    }

    async fn metric_sample_baseline(
        &self,
        name: &str,
        labels: &serde_json::Value,
        window_secs: i64,
        org_id: OrgId,
    ) -> DbResult<Option<(f64, f64)>> {
        unimplemented!("SqliteStore::metric_sample_baseline: metric_samples domain not yet ported (multi-DB P1)")
    }

    async fn metric_sample_latest(
        &self,
        name: &str,
        labels: &serde_json::Value,
        org_id: OrgId,
    ) -> DbResult<Option<(f64, OffsetDateTime)>> {
        unimplemented!(
            "SqliteStore::metric_sample_latest: metric_samples domain not yet ported (multi-DB P1)"
        )
    }

    async fn prune_metric_samples_older_than(&self, cutoff: OffsetDateTime) -> DbResult<u64> {
        unimplemented!("SqliteStore::prune_metric_samples_older_than: metric_samples domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreSourceMaps for SqliteStore {
    async fn upsert_source_map(&self, m: NewSourceMap<'_>) -> DbResult<i64> {
        unimplemented!(
            "SqliteStore::upsert_source_map: source_maps domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_source_map(
        &self,
        project_id: Uuid,
        release: &str,
        filename: &str,
    ) -> DbResult<Option<serde_json::Value>> {
        unimplemented!(
            "SqliteStore::get_source_map: source_maps domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_source_maps(&self, project_id: Uuid) -> DbResult<Vec<SourceMapMeta>> {
        unimplemented!(
            "SqliteStore::list_source_maps: source_maps domain not yet ported (multi-DB P1)"
        )
    }

    async fn delete_source_map(&self, project_id: Uuid, id: i64) -> DbResult<bool> {
        unimplemented!(
            "SqliteStore::delete_source_map: source_maps domain not yet ported (multi-DB P1)"
        )
    }
}

#[async_trait::async_trait]
impl StoreUsers for SqliteStore {
    async fn count_users(&self) -> DbResult<i64> {
        crate::sqlite::users::count(&self.pool).await
    }

    async fn create_user(&self, input: NewUser) -> DbResult<User> {
        crate::sqlite::users::create(&self.pool, input).await
    }

    async fn get_user_by_email(&self, email: &str) -> DbResult<UserWithHash> {
        crate::sqlite::users::get_by_email(&self.pool, email).await
    }

    async fn user_by_email(&self, email: &str) -> DbResult<Option<User>> {
        crate::sqlite::users::by_email(&self.pool, email).await
    }

    async fn get_user(&self, id: UserId) -> DbResult<User> {
        crate::sqlite::users::get(&self.pool, id).await
    }

    async fn set_user_totp_secret(&self, id: UserId, secret: &str) -> DbResult<()> {
        crate::sqlite::users::set_totp_secret(&self.pool, id, secret).await
    }

    async fn enable_user_totp(&self, id: UserId) -> DbResult<()> {
        crate::sqlite::users::enable_totp(&self.pool, id).await
    }

    async fn disable_user_totp(&self, id: UserId) -> DbResult<()> {
        crate::sqlite::users::disable_totp(&self.pool, id).await
    }

    async fn mark_user_login(&self, id: UserId) -> DbResult<()> {
        crate::sqlite::users::mark_login(&self.pool, id).await
    }

    async fn user_totp_locked_until(&self, id: UserId) -> DbResult<Option<OffsetDateTime>> {
        crate::sqlite::users::totp_locked_until(&self.pool, id).await
    }

    async fn record_user_totp_failure(
        &self,
        id: UserId,
        max_attempts: i32,
        lockout_mins: i32,
    ) -> DbResult<bool> {
        crate::sqlite::users::record_totp_failure(&self.pool, id, max_attempts, lockout_mins).await
    }

    async fn reset_user_totp_failures(&self, id: UserId) -> DbResult<()> {
        crate::sqlite::users::reset_totp_failures(&self.pool, id).await
    }

    async fn list_users(&self) -> DbResult<Vec<User>> {
        crate::sqlite::users::list(&self.pool).await
    }

    async fn set_user_admin(&self, id: UserId, is_admin: bool) -> DbResult<()> {
        crate::sqlite::users::set_admin(&self.pool, id, is_admin).await
    }

    async fn set_user_role(&self, id: UserId, role: Role) -> DbResult<()> {
        crate::sqlite::users::set_role(&self.pool, id, role).await
    }

    async fn delete_user(&self, id: UserId) -> DbResult<()> {
        crate::sqlite::users::delete(&self.pool, id).await
    }

    async fn anonymize_user(&self, id: UserId) -> DbResult<()> {
        crate::sqlite::users::anonymize(&self.pool, id).await
    }

    async fn get_user_prefs(&self, id: UserId) -> DbResult<serde_json::Value> {
        crate::sqlite::users::get_prefs(&self.pool, id).await
    }

    async fn set_user_prefs(&self, id: UserId, prefs: &serde_json::Value) -> DbResult<()> {
        crate::sqlite::users::set_prefs(&self.pool, id, prefs).await
    }

    async fn set_user_password(&self, id: UserId, hash: &str) -> DbResult<()> {
        crate::sqlite::users::set_password(&self.pool, id, hash).await
    }
}

#[async_trait::async_trait]
impl StoreWebpush for SqliteStore {
    async fn list_webpush_subs(
        &self,
        notification: NotificationId,
    ) -> DbResult<Vec<crate::webpush::WebpushSubscription>> {
        unimplemented!(
            "SqliteStore::list_webpush_subs: webpush domain not yet ported (multi-DB P1)"
        )
    }

    async fn upsert_webpush_sub(
        &self,
        notification: NotificationId,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
    ) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::upsert_webpush_sub: webpush domain not yet ported (multi-DB P1)"
        )
    }

    async fn delete_webpush_sub_by_endpoint(&self, endpoint: &str) -> DbResult<()> {
        unimplemented!("SqliteStore::delete_webpush_sub_by_endpoint: webpush domain not yet ported (multi-DB P1)")
    }

    async fn delete_webpush_sub(&self, id: Uuid) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::delete_webpush_sub: webpush domain not yet ported (multi-DB P1)"
        )
    }

    async fn get_vapid_keys(&self) -> DbResult<Option<crate::webpush::VapidKeys>> {
        unimplemented!("SqliteStore::get_vapid_keys: webpush domain not yet ported (multi-DB P1)")
    }

    async fn set_vapid_keys(&self, keys: &crate::webpush::VapidKeys) -> DbResult<()> {
        unimplemented!("SqliteStore::set_vapid_keys: webpush domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreOrgs for SqliteStore {
    async fn create_org(&self, slug: &str, name: &str) -> DbResult<rampart_core::org::Org> {
        crate::sqlite::orgs::create(&self.pool, slug, name).await
    }

    async fn get_org(&self, id: OrgId) -> DbResult<rampart_core::org::Org> {
        crate::sqlite::orgs::get(&self.pool, id).await
    }

    async fn orgs_for_user(&self, user_id: UserId) -> DbResult<Vec<rampart_core::org::Org>> {
        crate::sqlite::orgs::list_for_user(&self.pool, user_id).await
    }

    async fn upsert_org_member(&self, org_id: OrgId, user_id: UserId, role: Role) -> DbResult<()> {
        crate::sqlite::orgs::upsert_member(&self.pool, org_id, user_id, role).await
    }

    async fn org_member_role(&self, org_id: OrgId, user_id: UserId) -> DbResult<Option<Role>> {
        crate::sqlite::orgs::member_role(&self.pool, org_id, user_id).await
    }

    async fn list_org_members(&self, org_id: OrgId) -> DbResult<Vec<rampart_core::org::OrgMember>> {
        crate::sqlite::orgs::list_members(&self.pool, org_id).await
    }

    async fn list_org_members_detailed(
        &self,
        org_id: OrgId,
    ) -> DbResult<Vec<crate::orgs::OrgMemberDetail>> {
        crate::sqlite::orgs::list_members_detailed(&self.pool, org_id).await
    }

    async fn update_org(&self, id: OrgId, name: &str) -> DbResult<rampart_core::org::Org> {
        crate::sqlite::orgs::update(&self.pool, id, name).await
    }

    async fn org_by_slug(&self, slug: &str) -> DbResult<rampart_core::org::Org> {
        crate::sqlite::orgs::get_by_slug(&self.pool, slug).await
    }

    async fn remove_org_member(&self, org_id: OrgId, user_id: UserId) -> DbResult<bool> {
        crate::sqlite::orgs::remove_member(&self.pool, org_id, user_id).await
    }

    async fn count_org_admins(&self, org_id: OrgId) -> DbResult<i64> {
        crate::sqlite::orgs::count_admins(&self.pool, org_id).await
    }

    async fn create_org_with_owner(
        &self,
        slug: &str,
        name: &str,
        owner: UserId,
    ) -> DbResult<rampart_core::org::Org> {
        crate::sqlite::orgs::create_with_owner(&self.pool, slug, name, owner).await
    }
}

#[async_trait::async_trait]
impl StoreMonitors for SqliteStore {
    async fn create_monitor(&self, input: NewMonitor, org_id: OrgId) -> DbResult<Monitor> {
        crate::sqlite::monitors::create(&self.pool, input, org_id).await
    }

    async fn regenerate_monitor_push_token(
        &self,
        id: MonitorId,
        org_id: OrgId,
    ) -> DbResult<String> {
        crate::sqlite::monitors::regenerate_push_token(&self.pool, id, org_id).await
    }

    async fn find_monitor_by_push_token(&self, token: &str) -> DbResult<Option<MonitorId>> {
        crate::sqlite::monitors::find_by_push_token(&self.pool, token).await
    }

    async fn fetch_monitor_last_push_at(&self, id: MonitorId) -> DbResult<Option<OffsetDateTime>> {
        crate::sqlite::monitors::fetch_last_push_at(&self.pool, id).await
    }

    async fn set_monitor_cert_info(
        &self,
        id: MonitorId,
        days_left: i32,
        subject: &str,
    ) -> DbResult<()> {
        crate::sqlite::monitors::set_cert_info(&self.pool, id, days_left, subject).await
    }

    async fn mark_monitor_run_started(&self, id: MonitorId) -> DbResult<()> {
        crate::sqlite::monitors::mark_run_started(&self.pool, id).await
    }

    async fn close_monitor_run(&self, id: MonitorId) -> DbResult<Option<OffsetDateTime>> {
        crate::sqlite::monitors::close_run(&self.pool, id).await
    }

    async fn monitor_push_state(
        &self,
        id: MonitorId,
    ) -> DbResult<(Option<OffsetDateTime>, Option<OffsetDateTime>)> {
        crate::sqlite::monitors::push_state(&self.pool, id).await
    }

    async fn bump_monitor_push_at(&self, id: MonitorId) -> DbResult<()> {
        crate::sqlite::monitors::bump_push_at(&self.pool, id).await
    }

    async fn list_monitors(&self, org_id: OrgId) -> DbResult<Vec<Monitor>> {
        crate::sqlite::monitors::list(&self.pool, org_id).await
    }

    async fn list_all_monitors(&self) -> DbResult<Vec<Monitor>> {
        crate::sqlite::monitors::list_all(&self.pool).await
    }

    async fn list_monitors_for_agent(&self, agent: AgentId) -> DbResult<Vec<Monitor>> {
        crate::sqlite::monitors::list_for_agent(&self.pool, agent).await
    }

    async fn list_stale_agent_monitors(&self) -> DbResult<Vec<(Monitor, String)>> {
        crate::sqlite::monitors::list_stale_agent_monitors(&self.pool).await
    }

    async fn get_monitor(&self, id: MonitorId, org_id: OrgId) -> DbResult<Monitor> {
        crate::sqlite::monitors::get(&self.pool, id, org_id).await
    }

    async fn get_monitor_unscoped(&self, id: MonitorId) -> DbResult<Monitor> {
        crate::sqlite::monitors::get_unscoped(&self.pool, id).await
    }

    async fn monitor_public_fields_batch(
        &self,
        ids: &[Uuid],
    ) -> DbResult<HashMap<Uuid, (String, MonitorStatus)>> {
        crate::sqlite::monitors::public_fields_batch(&self.pool, ids).await
    }

    async fn update_monitor(
        &self,
        id: MonitorId,
        patch: UpdateMonitor,
        org_id: OrgId,
    ) -> DbResult<Monitor> {
        crate::sqlite::monitors::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_monitor(&self, id: MonitorId, org_id: OrgId) -> DbResult<()> {
        crate::sqlite::monitors::delete(&self.pool, id, org_id).await
    }

    async fn set_monitor_active(&self, id: MonitorId, active: bool, org_id: OrgId) -> DbResult<()> {
        crate::sqlite::monitors::set_active(&self.pool, id, active, org_id).await
    }

    async fn set_monitors_active_by_tag(
        &self,
        tag: TagId,
        active: bool,
        org_id: OrgId,
    ) -> DbResult<u64> {
        crate::sqlite::monitors::set_active_by_tag(&self.pool, tag, active, org_id).await
    }

    async fn set_monitor_group(
        &self,
        id: MonitorId,
        group: Option<MonitorGroupId>,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::sqlite::monitors::set_group(&self.pool, id, group, org_id).await
    }

    async fn bulk_edit_monitors_preview(
        &self,
        ids: &[MonitorId],
        want_tags: bool,
        org_id: OrgId,
    ) -> DbResult<(Vec<MonitorPrior>, usize)> {
        crate::sqlite::monitors::bulk_edit_preview(&self.pool, ids, want_tags, org_id).await
    }

    async fn bulk_edit_monitors(
        &self,
        ids: &[MonitorId],
        patch: &BulkEditPatch,
        org_id: OrgId,
    ) -> DbResult<BulkEditOutcome> {
        crate::sqlite::monitors::bulk_edit(&self.pool, ids, patch, org_id).await
    }

    async fn set_monitor_status(&self, id: MonitorId, status: MonitorStatus) -> DbResult<()> {
        crate::sqlite::monitors::set_status(&self.pool, id, status).await
    }

    async fn monitor_slo_state(&self, id: MonitorId) -> DbResult<Option<SloState>> {
        crate::sqlite::monitors::slo_state(&self.pool, id).await
    }

    async fn mark_monitor_slo_breached(&self, id: MonitorId) -> DbResult<()> {
        crate::sqlite::monitors::mark_slo_breached(&self.pool, id).await
    }

    async fn clear_monitor_slo_breached(&self, id: MonitorId) -> DbResult<()> {
        crate::sqlite::monitors::clear_slo_breached(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreAudit for SqliteStore {
    async fn record_audit(&self, entry: crate::audit::NewEntry<'_>) -> DbResult<()> {
        unimplemented!("SqliteStore::record_audit: audit domain not yet ported (multi-DB P1)")
    }

    async fn set_audit_chain_watermark(&self, id: i64, hash: &str) -> DbResult<()> {
        unimplemented!(
            "SqliteStore::set_audit_chain_watermark: audit domain not yet ported (multi-DB P1)"
        )
    }

    async fn verify_audit_chain(&self) -> DbResult<crate::audit::VerifyReport> {
        unimplemented!("SqliteStore::verify_audit_chain: audit domain not yet ported (multi-DB P1)")
    }

    async fn audit_security_insights(
        &self,
        hours: i32,
    ) -> DbResult<crate::audit::SecurityInsights> {
        unimplemented!(
            "SqliteStore::audit_security_insights: audit domain not yet ported (multi-DB P1)"
        )
    }

    async fn list_audit_entries(
        &self,
        limit: i64,
        filter: crate::audit::AuditFilter<'_>,
    ) -> DbResult<Vec<crate::audit::AuditEntry>> {
        unimplemented!("SqliteStore::list_audit_entries: audit domain not yet ported (multi-DB P1)")
    }

    async fn fetch_audit_since(
        &self,
        after_id: i64,
        limit: i64,
    ) -> DbResult<Vec<crate::audit::AuditEntry>> {
        unimplemented!("SqliteStore::fetch_audit_since: audit domain not yet ported (multi-DB P1)")
    }

    async fn export_audit_batch(
        &self,
        before_id: Option<i64>,
        batch: i64,
        filter: crate::audit::ExportFilter,
    ) -> DbResult<Vec<crate::audit::ExportRow>> {
        unimplemented!("SqliteStore::export_audit_batch: audit domain not yet ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreCompliance for SqliteStore {
    async fn access_review(&self) -> DbResult<Vec<crate::access_review::AccessReviewRow>> {
        unimplemented!("SqliteStore::access_review: compliance domain not yet ported (multi-DB P1)")
    }
}

impl Store for SqliteStore {}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::monitor::{MonitorKind, NewMonitor};
    use std::sync::Arc;

    fn new_http(name: &str) -> NewMonitor {
        NewMonitor {
            name: name.into(),
            kind: MonitorKind::Http,
            url: Some("https://x".into()),
            hostname: None,
            port: None,
            config: serde_json::json!({}),
            interval_seconds: 60,
            timeout_seconds: 10,
            max_retries: 0,
            retry_interval_sec: 60,
            resend_interval_sec: 0,
            upside_down: false,
            http_method: "GET".into(),
            http_body: None,
            http_headers: None,
            accepted_statuses: vec![200],
            follow_redirect: true,
            ignore_tls: false,
            proxy_id: None,
            group_id: None,
            check_cert: false,
            cert_expiry_days: 14,
            slo_target_pct: None,
            slo_window_days: None,
            agent_id: None,
            escalation_policy_id: None,
        }
    }

    /// The keystone assertion: `SqliteStore` is usable as `Arc<dyn Store>` (the
    /// super-trait is object-safe over SQLite) and a delegated domain round-trips
    /// through the trait object.
    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn sqlite_store_satisfies_dyn_store(pool: SqlitePool) {
        let store: Arc<dyn Store> = Arc::new(SqliteStore::new(pool));
        let org = OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

        assert!(store.list_monitors(org).await.unwrap().is_empty());
        let m = store.create_monitor(new_http("cap"), org).await.unwrap();
        assert_eq!(store.list_monitors(org).await.unwrap().len(), 1);
        assert_eq!(store.get_monitor(m.id, org).await.unwrap().name, "cap");

        // a second delegated domain (settings) through the same trait object.
        store
            .put_setting("k", &serde_json::json!({ "v": 1 }))
            .await
            .unwrap();
        assert!(store.get_setting("k").await.unwrap().is_some());
    }

    /// `connect` builds a pool with foreign_keys on and runs the migration set.
    /// Shared-cache in-memory so all pool connections see the migrated schema
    /// (a plain `:memory:` gives each connection its own empty database).
    #[tokio::test]
    async fn connect_runs_migrations() {
        let store = SqliteStore::connect("sqlite:file:rampart_capstone?mode=memory&cache=shared")
            .await
            .unwrap();
        // Default org is seeded by 0002_identity.sql → a delegated read works.
        let org = OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);
        assert!(store.list_monitors(org).await.unwrap().is_empty());
    }
}
