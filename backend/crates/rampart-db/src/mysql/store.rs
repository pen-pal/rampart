//! MySQL-backed `Store` implementation (multi-DB P2 capstone).
//!
//! `MysqlStore` satisfies the same object-safe [`crate::store::Store`]
//! super-trait as `PgStore`/`SqliteStore`, so `AppState` can hold
//! `Arc<dyn Store>` over any of the three backends. The 20 ported P2 domains
//! (settings, orgs, users, sessions, monitors, tags, heartbeats, proxies,
//! notifications, delivery_log, escalations, scheduled_reports, audit,
//! metric_samples, logs, metric_rules, traces, telemetry_rules, slos, detection)
//! delegate to their `crate::mysql::*` free functions; the remaining domains
//! (agents, maintenance, digest_buffer, templates, silences, routing,
//! monitor_groups, error_tracking, profiles, rum, status_pages, incidents,
//! api_keys, ingest_keys, on_call, …) are `unimplemented!()` stubs that panic if
//! hit. This proves the seam is satisfiable by MySQL end-to-end at the type level.
//!
//! NOT YET WIRED INTO BOOT: a true `DATABASE_URL=mysql://…` end-to-end boot needs
//! the `mysql:` scheme branch in `main.rs` + the `rampart-api` `mysql` feature
//! (a follow-up slice, mirroring the SQLite boot flip). `MysqlStore` + `connect`
//! exist and compile now.

// The not-yet-ported domains are `unimplemented!()` stubs that intentionally
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
    StoreDeployMarkers, StoreDetection, StoreDigestBuffer, StoreErrorTracking, StoreEscalations,
    StoreHeartbeats, StoreIncidentTemplates, StoreIncidents, StoreIngestKeys, StoreIngestTokens,
    StoreLogs, StoreMaintenance, StoreMetricRules, StoreMetricSamples, StoreMetrics,
    StoreMonitorGroups, StoreMonitorPresets, StoreMonitorTemplates, StoreMonitors,
    StoreNotifications, StoreOidcState, StoreOnCall, StoreOrgs, StoreProfiles, StoreProxies,
    StoreRecoveryCodes, StoreRetention, StoreRouting, StoreRum, StoreScheduledReports,
    StoreSessions, StoreSettings, StoreSilences, StoreSlos, StoreSourceMaps, StoreStatusPages,
    StoreSubscribers, StoreTags, StoreTelemetryRules, StoreTemplates, StoreTraces, StoreUsers,
    StoreWebpush,
};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::MySqlPool;
use std::str::FromStr;

/// MySQL-backed [`Store`]. Holds a `MySqlPool`; delegates the ported domains
/// to `crate::mysql::*` and stubs the rest.
pub struct MysqlStore {
    pool: MySqlPool,
}

impl MysqlStore {
    /// Wrap an existing pool. The pool SHOULD set `sql_mode=STRICT_TRANS_TABLES`
    /// per connection (as [`connect`](Self::connect) does) so over-length writes
    /// fail loud — the audit hash chain depends on stored == hashed bytes.
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    /// Open a MySQL/MariaDB URL (`mysql://user:pass@host:3306/db`) and run the
    /// MySQL migration set. Each pooled connection is put into
    /// `STRICT_TRANS_TABLES` (so an over-length VARCHAR write errors instead of
    /// silently truncating — the audit chain + detection rely on it) while
    /// KEEPING MySQL's default backslash-escaping mode (the detection
    /// `BodyContains` `ESCAPE '\\'` clause errors under `NO_BACKSLASH_ESCAPES`).
    pub async fn connect(url: &str) -> DbResult<Self> {
        let opts = MySqlConnectOptions::from_str(url)?;
        let pool = MySqlPoolOptions::new()
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query(
                        "SET SESSION sql_mode = 'STRICT_TRANS_TABLES,NO_ENGINE_SUBSTITUTION'",
                    )
                    .execute(&mut *conn)
                    .await?;
                    Ok(())
                })
            })
            .connect_with(opts)
            .await?;
        sqlx::migrate!("../../migrations-mysql").run(&pool).await?;
        Ok(Self { pool })
    }
}

#[async_trait::async_trait]
impl StoreHeartbeats for MysqlStore {
    async fn insert_many(&self, hbs: &[Heartbeat]) -> DbResult<()> {
        crate::mysql::heartbeats::insert_many(&self.pool, hbs).await
    }

    async fn recent_for_monitor(&self, monitor: MonitorId, limit: i64) -> DbResult<Vec<Heartbeat>> {
        crate::mysql::heartbeats::recent_for_monitor(&self.pool, monitor, limit).await
    }

    async fn recent_for_monitor_before(
        &self,
        monitor: MonitorId,
        limit: i64,
        before: Option<time::OffsetDateTime>,
    ) -> DbResult<Vec<Heartbeat>> {
        crate::mysql::heartbeats::recent_for_monitor_before(&self.pool, monitor, limit, before)
            .await
    }

    async fn range_for_monitor(
        &self,
        monitor: MonitorId,
        since: time::OffsetDateTime,
        until: time::OffsetDateTime,
        limit: i64,
    ) -> DbResult<Vec<Heartbeat>> {
        crate::mysql::heartbeats::range_for_monitor(&self.pool, monitor, since, until, limit).await
    }

    async fn uptime_pct(&self, monitor: MonitorId, window_seconds: i64) -> DbResult<Option<f64>> {
        crate::mysql::heartbeats::uptime_pct(&self.pool, monitor, window_seconds).await
    }

    async fn current_slo_uptime_pct(
        &self,
        monitor: MonitorId,
        window_days: i32,
    ) -> DbResult<Option<f64>> {
        crate::mysql::heartbeats::current_slo_uptime_pct(&self.pool, monitor, window_days).await
    }

    async fn avg_latency_ms(
        &self,
        monitor: MonitorId,
        window_seconds: i64,
    ) -> DbResult<Option<f64>> {
        crate::mysql::heartbeats::avg_latency_ms(&self.pool, monitor, window_seconds).await
    }

    async fn daily_status(&self, monitor: MonitorId, days: i32) -> DbResult<Vec<u8>> {
        crate::mysql::heartbeats::daily_status(&self.pool, monitor, days).await
    }

    async fn day_hourly_latency(
        &self,
        monitor: MonitorId,
        day: time::Date,
    ) -> DbResult<Vec<(i32, Option<f32>, i32)>> {
        crate::mysql::heartbeats::day_hourly_latency(&self.pool, monitor, day).await
    }

    async fn monthly_uptime(
        &self,
        monitor: MonitorId,
        months: i32,
    ) -> DbResult<Vec<MonthlyUptime>> {
        crate::mysql::heartbeats::monthly_uptime(&self.pool, monitor, months).await
    }

    async fn uptime_pct_batch(
        &self,
        monitor_ids: &[Uuid],
        window_seconds: i64,
    ) -> DbResult<HashMap<Uuid, f64>> {
        crate::mysql::heartbeats::uptime_pct_batch(&self.pool, monitor_ids, window_seconds).await
    }

    async fn avg_latency_ms_batch(
        &self,
        monitor_ids: &[Uuid],
        window_seconds: i64,
    ) -> DbResult<HashMap<Uuid, f64>> {
        crate::mysql::heartbeats::avg_latency_ms_batch(&self.pool, monitor_ids, window_seconds)
            .await
    }

    async fn daily_status_batch(
        &self,
        monitor_ids: &[Uuid],
        days: i32,
    ) -> DbResult<HashMap<Uuid, Vec<u8>>> {
        crate::mysql::heartbeats::daily_status_batch(&self.pool, monitor_ids, days).await
    }

    async fn monthly_uptime_batch(
        &self,
        monitor_ids: &[Uuid],
        months: i32,
    ) -> DbResult<HashMap<Uuid, Vec<MonthlyUptime>>> {
        crate::mysql::heartbeats::monthly_uptime_batch(&self.pool, monitor_ids, months).await
    }

    async fn summary_window(
        &self,
        window_seconds: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<MonitorSummary>> {
        crate::mysql::heartbeats::summary_window(&self.pool, window_seconds, org_id).await
    }

    async fn mtbf_mttr(&self, monitor: MonitorId, window_seconds: i64) -> DbResult<MtbfMttr> {
        crate::mysql::heartbeats::mtbf_mttr(&self.pool, monitor, window_seconds).await
    }

    async fn error_budget(
        &self,
        monitor: MonitorId,
        window_days: i32,
        target_pct: f64,
    ) -> DbResult<ErrorBudget> {
        crate::mysql::heartbeats::error_budget(&self.pool, monitor, window_days, target_pct).await
    }

    async fn error_budget_burndown(
        &self,
        monitor: MonitorId,
        window_days: i32,
        target_pct: f64,
    ) -> DbResult<Vec<BurndownPoint>> {
        crate::mysql::heartbeats::error_budget_burndown(
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
        crate::mysql::heartbeats::recent_per_monitor(&self.pool, per_monitor, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreDeployMarkers for MysqlStore {
    async fn create_deploy_marker(
        &self,
        input: NewDeployMarker,
        org_id: OrgId,
    ) -> DbResult<DeployMarker> {
        crate::mysql::deploy_markers::create(&self.pool, input, org_id).await
    }

    async fn list_deploy_markers_window(
        &self,
        hours: i32,
        service: Option<&str>,
        org_id: OrgId,
    ) -> DbResult<Vec<DeployMarker>> {
        crate::mysql::deploy_markers::list_window(&self.pool, hours, service, org_id).await
    }

    async fn delete_deploy_marker(&self, id: DeployMarkerId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::deploy_markers::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreIngestKeys for MysqlStore {
    async fn create_ingest_key(
        &self,
        org_id: OrgId,
        label: &str,
        kind: &str,
        allowed_origins: &[String],
    ) -> DbResult<(IngestKey, String)> {
        crate::mysql::ingest_keys::create(&self.pool, org_id, label, kind, allowed_origins).await
    }

    async fn find_ingest_key_by_token(
        &self,
        token: &str,
    ) -> DbResult<Option<(Uuid, OrgId, String, Vec<String>)>> {
        crate::mysql::ingest_keys::find_by_token(&self.pool, token).await
    }

    async fn touch_ingest_key_last_used(&self, id: Uuid) -> DbResult<()> {
        crate::mysql::ingest_keys::touch_last_used(&self.pool, id).await
    }

    async fn list_ingest_keys_for_org(&self, org_id: OrgId) -> DbResult<Vec<IngestKey>> {
        crate::mysql::ingest_keys::list_for_org(&self.pool, org_id).await
    }

    async fn delete_ingest_key(&self, id: Uuid, org_id: OrgId) -> DbResult<bool> {
        crate::mysql::ingest_keys::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreSlos for MysqlStore {
    async fn list_slos(&self, org_id: OrgId) -> DbResult<Vec<Slo>> {
        crate::mysql::slos::list(&self.pool, org_id).await
    }

    async fn list_all_slos(&self) -> DbResult<Vec<Slo>> {
        crate::mysql::slos::list_all(&self.pool).await
    }

    async fn get_slo(&self, id: SloId, org_id: OrgId) -> DbResult<Slo> {
        crate::mysql::slos::get(&self.pool, id, org_id).await
    }

    async fn get_slo_unscoped(&self, id: SloId) -> DbResult<Slo> {
        crate::mysql::slos::get_unscoped(&self.pool, id).await
    }

    async fn create_slo(&self, input: NewSlo, org_id: OrgId) -> DbResult<Slo> {
        crate::mysql::slos::create(&self.pool, input, org_id).await
    }

    async fn update_slo(&self, id: SloId, patch: UpdateSlo, org_id: OrgId) -> DbResult<Slo> {
        crate::mysql::slos::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_slo(&self, id: SloId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::slos::delete(&self.pool, id, org_id).await
    }

    async fn compute_slo(&self, slo: &Slo) -> DbResult<SloSnapshot> {
        crate::mysql::slos::compute(&self.pool, slo).await
    }

    async fn slo_trend(&self, slo: &Slo, buckets: i64) -> DbResult<Vec<f64>> {
        crate::mysql::slos::trend(&self.pool, slo, buckets).await
    }

    async fn list_slos_with_snapshots(&self, org_id: OrgId) -> DbResult<Vec<SloWithSnapshot>> {
        crate::mysql::slos::list_with_snapshots(&self.pool, org_id).await
    }

    async fn evaluate_slos_tick(&self) -> DbResult<Vec<SloEvent>> {
        crate::mysql::slos::evaluate_tick(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreProxies for MysqlStore {
    async fn list_proxies(&self, org_id: OrgId) -> DbResult<Vec<Proxy>> {
        crate::mysql::proxies::list(&self.pool, org_id).await
    }

    async fn get_proxy(&self, id: ProxyId, org_id: OrgId) -> DbResult<Proxy> {
        crate::mysql::proxies::get(&self.pool, id, org_id).await
    }

    async fn get_proxy_unscoped(&self, id: ProxyId) -> DbResult<Proxy> {
        crate::mysql::proxies::get_unscoped(&self.pool, id).await
    }

    async fn create_proxy(&self, input: NewProxy, org_id: OrgId) -> DbResult<Proxy> {
        crate::mysql::proxies::create(&self.pool, input, org_id).await
    }

    async fn delete_proxy(&self, id: ProxyId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::proxies::delete(&self.pool, id, org_id).await
    }

    async fn set_active_proxy(&self, id: ProxyId, active: bool, org_id: OrgId) -> DbResult<()> {
        crate::mysql::proxies::set_active(&self.pool, id, active, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreOnCall for MysqlStore {
    async fn list_on_call(&self, org_id: OrgId) -> DbResult<Vec<OnCallSchedule>> {
        crate::mysql::on_call::list(&self.pool, org_id).await
    }

    async fn get_on_call(&self, id: OnCallScheduleId, org_id: OrgId) -> DbResult<OnCallSchedule> {
        crate::mysql::on_call::get(&self.pool, id, org_id).await
    }

    async fn get_on_call_unscoped(&self, id: OnCallScheduleId) -> DbResult<OnCallSchedule> {
        crate::mysql::on_call::get_unscoped(&self.pool, id).await
    }

    async fn create_on_call(
        &self,
        input: NewOnCallSchedule,
        org_id: OrgId,
    ) -> DbResult<OnCallSchedule> {
        crate::mysql::on_call::create(&self.pool, input, org_id).await
    }

    async fn update_on_call(
        &self,
        id: OnCallScheduleId,
        patch: UpdateOnCallSchedule,
        org_id: OrgId,
    ) -> DbResult<OnCallSchedule> {
        crate::mysql::on_call::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_on_call(&self, id: OnCallScheduleId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::on_call::delete(&self.pool, id, org_id).await
    }

    async fn oncall_current_channel(
        &self,
        id: OnCallScheduleId,
        at: OffsetDateTime,
    ) -> DbResult<Option<NotificationId>> {
        crate::mysql::on_call::current_channel(&self.pool, id, at).await
    }

    async fn oncall_current_target(
        &self,
        id: OnCallScheduleId,
        at: OffsetDateTime,
    ) -> DbResult<Option<OnCallTarget>> {
        crate::mysql::on_call::current_target(&self.pool, id, at).await
    }
}

#[async_trait::async_trait]
impl StoreRecoveryCodes for MysqlStore {
    async fn issue_recovery_codes(&self, user: UserId, count: usize) -> DbResult<Vec<String>> {
        crate::mysql::recovery_codes::issue_batch(&self.pool, user, count).await
    }

    async fn consume_recovery_code(&self, user: UserId, code: &str) -> DbResult<bool> {
        crate::mysql::recovery_codes::consume(&self.pool, user, code).await
    }

    async fn delete_recovery_codes_for_user(&self, user: UserId) -> DbResult<()> {
        crate::mysql::recovery_codes::delete_for_user(&self.pool, user).await
    }

    async fn remaining_recovery_codes(&self, user: UserId) -> DbResult<i64> {
        crate::mysql::recovery_codes::remaining(&self.pool, user).await
    }
}

#[async_trait::async_trait]
impl StoreApiKeys for MysqlStore {
    async fn list_api_keys(&self, org_id: OrgId) -> DbResult<Vec<ApiKey>> {
        crate::mysql::api_keys::list(&self.pool, org_id).await
    }

    async fn create_api_key(
        &self,
        input: NewApiKey,
        created_by: UserId,
        org_id: OrgId,
    ) -> DbResult<IssuedApiKey> {
        crate::mysql::api_keys::create(&self.pool, input, created_by, org_id).await
    }

    async fn delete_api_key(&self, id: ApiKeyId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::api_keys::delete(&self.pool, id, org_id).await
    }

    async fn lookup_api_key(&self, token: &str) -> DbResult<(ApiKey, UserId, OrgId)> {
        crate::mysql::api_keys::lookup(&self.pool, token).await
    }

    async fn touch_api_key_last_used(&self, id: ApiKeyId) -> DbResult<()> {
        crate::mysql::api_keys::touch_last_used(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreEscalations for MysqlStore {
    async fn list_escalation_policies(&self, org_id: OrgId) -> DbResult<Vec<EscalationPolicy>> {
        crate::mysql::escalations::list(&self.pool, org_id).await
    }

    async fn get_escalation_policy(
        &self,
        id: EscalationPolicyId,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy> {
        crate::mysql::escalations::get(&self.pool, id, org_id).await
    }

    async fn get_escalation_policy_unscoped(
        &self,
        id: EscalationPolicyId,
    ) -> DbResult<EscalationPolicy> {
        crate::mysql::escalations::get_unscoped(&self.pool, id).await
    }

    async fn create_escalation_policy(
        &self,
        input: NewEscalationPolicy,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy> {
        crate::mysql::escalations::create(&self.pool, input, org_id).await
    }

    async fn update_escalation_policy(
        &self,
        id: EscalationPolicyId,
        patch: UpdateEscalationPolicy,
        org_id: OrgId,
    ) -> DbResult<EscalationPolicy> {
        crate::mysql::escalations::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_escalation_policy(
        &self,
        id: EscalationPolicyId,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::mysql::escalations::delete(&self.pool, id, org_id).await
    }

    async fn open_episode(
        &self,
        monitor_id: MonitorId,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::mysql::escalations::open_episode(&self.pool, monitor_id, policy).await
    }

    async fn open_episode_for_subject(
        &self,
        kind: &str,
        subject_ref: &str,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::mysql::escalations::open_episode_for_subject(&self.pool, kind, subject_ref, policy)
            .await
    }

    async fn resolve_subject(
        &self,
        kind: &str,
        subject_ref: &str,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::mysql::escalations::resolve_subject(&self.pool, kind, subject_ref).await
    }

    async fn ack_episode(&self, episode_id: Uuid, by: UserId) -> DbResult<EscalationEpisode> {
        crate::mysql::escalations::ack_episode(&self.pool, episode_id, by).await
    }

    async fn list_open_episodes(&self) -> DbResult<Vec<EscalationEpisode>> {
        crate::mysql::escalations::list_open(&self.pool).await
    }

    async fn list_open_episodes_for_org(&self, org_id: OrgId) -> DbResult<Vec<EscalationEpisode>> {
        crate::mysql::escalations::list_open_for_org(&self.pool, org_id).await
    }

    async fn episode_in_org(&self, episode: Uuid, org_id: OrgId) -> DbResult<()> {
        crate::mysql::escalations::episode_in_org(&self.pool, episode, org_id).await
    }

    async fn open_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::mysql::escalations::open_for_monitor(&self.pool, monitor_id).await
    }

    async fn ack_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
        by: UserId,
    ) -> DbResult<EscalationEpisode> {
        crate::mysql::escalations::ack(&self.pool, monitor_id, by).await
    }

    async fn resolve_episode_for_monitor(
        &self,
        monitor_id: MonitorId,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::mysql::escalations::resolve(&self.pool, monitor_id).await
    }

    async fn advance_episode(
        &self,
        episode_id: Uuid,
        policy: &EscalationPolicy,
    ) -> DbResult<Option<EscalationEpisode>> {
        crate::mysql::escalations::advance(&self.pool, episode_id, policy).await
    }

    async fn due_episodes(&self) -> DbResult<Vec<EscalationEpisode>> {
        crate::mysql::escalations::due(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreMaintenance for MysqlStore {
    async fn list_maintenance_windows(&self, org_id: OrgId) -> DbResult<Vec<MaintenanceWindow>> {
        crate::mysql::maintenance::list(&self.pool, org_id).await
    }

    async fn get_maintenance_window(
        &self,
        id: MaintenanceId,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow> {
        crate::mysql::maintenance::get(&self.pool, id, org_id).await
    }

    async fn create_maintenance_window(
        &self,
        input: NewMaintenanceWindow,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow> {
        crate::mysql::maintenance::create(&self.pool, input, org_id).await
    }

    async fn update_maintenance_window(
        &self,
        id: MaintenanceId,
        patch: UpdateMaintenanceWindow,
        org_id: OrgId,
    ) -> DbResult<MaintenanceWindow> {
        crate::mysql::maintenance::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_maintenance_window(&self, id: MaintenanceId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::maintenance::delete(&self.pool, id, org_id).await
    }

    async fn set_active_maintenance(
        &self,
        id: MaintenanceId,
        active: bool,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::mysql::maintenance::set_active(&self.pool, id, active, org_id).await
    }

    async fn attach_maintenance_monitor(
        &self,
        window: MaintenanceId,
        monitor: MonitorId,
    ) -> DbResult<()> {
        crate::mysql::maintenance::attach(&self.pool, window, monitor).await
    }

    async fn detach_maintenance_monitor(
        &self,
        window: MaintenanceId,
        monitor: MonitorId,
    ) -> DbResult<()> {
        crate::mysql::maintenance::detach(&self.pool, window, monitor).await
    }

    async fn is_in_active_window(&self, monitor: MonitorId) -> DbResult<bool> {
        crate::mysql::maintenance::is_in_active_window(&self.pool, monitor).await
    }

    async fn maintenance_transitions_needing_notification(
        &self,
    ) -> DbResult<Vec<MaintenanceTransition>> {
        crate::mysql::maintenance::transitions_needing_notification(&self.pool).await
    }

    async fn mark_maintenance_notified_start(&self, id: MaintenanceId) -> DbResult<()> {
        crate::mysql::maintenance::mark_notified_start(&self.pool, id).await
    }

    async fn mark_maintenance_notified_end(&self, id: MaintenanceId) -> DbResult<()> {
        crate::mysql::maintenance::mark_notified_end(&self.pool, id).await
    }

    async fn confirmed_subscriber_emails_for_monitors(
        &self,
        monitors: &[MonitorId],
    ) -> DbResult<Vec<String>> {
        crate::mysql::maintenance::confirmed_subscriber_emails_for_monitors(&self.pool, monitors)
            .await
    }

    async fn public_maintenance_for_status_page(
        &self,
        page: StatusPageId,
    ) -> DbResult<Vec<PublicMaintenance>> {
        crate::mysql::maintenance::public_for_status_page(&self.pool, page).await
    }
}

#[async_trait::async_trait]
impl StoreIngestTokens for MysqlStore {
    async fn create_ingest_token(
        &self,
        page: StatusPageId,
        input: NewIngestToken,
    ) -> DbResult<IngestToken> {
        crate::mysql::ingest_tokens::create(&self.pool, page, input).await
    }

    async fn create_ingest_token_with_token(
        &self,
        page: StatusPageId,
        label: &str,
        token: &str,
    ) -> DbResult<IngestToken> {
        crate::mysql::ingest_tokens::create_with_token(&self.pool, page, label, token).await
    }

    async fn set_ingest_token_mapping(
        &self,
        id: IngestTokenId,
        mapping: Option<serde_json::Value>,
        org_id: OrgId,
    ) -> DbResult<IngestToken> {
        crate::mysql::ingest_tokens::set_mapping(&self.pool, id, mapping, org_id).await
    }

    async fn list_ingest_tokens_for_page(&self, page: StatusPageId) -> DbResult<Vec<IngestToken>> {
        crate::mysql::ingest_tokens::list_for_page(&self.pool, page).await
    }

    async fn find_ingest_token_by_token(&self, token: &str) -> DbResult<IngestToken> {
        crate::mysql::ingest_tokens::find_by_token(&self.pool, token).await
    }

    async fn delete_ingest_token(&self, id: IngestTokenId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::ingest_tokens::delete(&self.pool, id, org_id).await
    }

    async fn touch_ingest_token_last_used(&self, id: IngestTokenId) -> DbResult<()> {
        crate::mysql::ingest_tokens::touch_last_used(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreTags for MysqlStore {
    async fn list_tags(&self, org_id: OrgId) -> DbResult<Vec<Tag>> {
        crate::mysql::tags::list(&self.pool, org_id).await
    }

    async fn get_tag(&self, id: TagId, org_id: OrgId) -> DbResult<Tag> {
        crate::mysql::tags::get(&self.pool, id, org_id).await
    }

    async fn create_tag(&self, input: NewTag, org_id: OrgId) -> DbResult<Tag> {
        crate::mysql::tags::create(&self.pool, input, org_id).await
    }

    async fn update_tag(&self, id: TagId, patch: UpdateTag, org_id: OrgId) -> DbResult<Tag> {
        crate::mysql::tags::update(&self.pool, id, patch, org_id).await
    }

    async fn tag_usage(&self, org_id: OrgId) -> DbResult<Vec<TagUsage>> {
        crate::mysql::tags::usage(&self.pool, org_id).await
    }

    async fn delete_tag(&self, id: TagId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::tags::delete(&self.pool, id, org_id).await
    }

    async fn attach_tag(&self, monitor: MonitorId, tag: TagId) -> DbResult<()> {
        crate::mysql::tags::attach(&self.pool, monitor, tag).await
    }

    async fn detach_tag(&self, monitor: MonitorId, tag: TagId) -> DbResult<()> {
        crate::mysql::tags::detach(&self.pool, monitor, tag).await
    }

    async fn list_tags_for_monitor(&self, monitor: MonitorId) -> DbResult<Vec<TagBrief>> {
        crate::mysql::tags::list_for_monitor(&self.pool, monitor).await
    }

    async fn hydrate_tags_for_channels(
        &self,
        ids: &[NotificationId],
    ) -> DbResult<HashMap<NotificationId, Vec<TagBrief>>> {
        crate::mysql::tags::hydrate_for_channels(&self.pool, ids).await
    }

    async fn hydrate_tags_for_monitors(
        &self,
        ids: &[MonitorId],
    ) -> DbResult<HashMap<MonitorId, Vec<TagBrief>>> {
        crate::mysql::tags::hydrate_for_monitors(&self.pool, ids).await
    }
}

#[async_trait::async_trait]
impl StoreTemplates for MysqlStore {
    async fn list_templates(&self, org_id: OrgId) -> DbResult<Vec<Template>> {
        crate::mysql::templates::list(&self.pool, org_id).await
    }

    async fn get_template(&self, id: NotificationTemplateId, org_id: OrgId) -> DbResult<Template> {
        crate::mysql::templates::get(&self.pool, id, org_id).await
    }

    async fn create_template(&self, input: NewTemplate, org_id: OrgId) -> DbResult<Template> {
        crate::mysql::templates::create(&self.pool, input, org_id).await
    }

    async fn update_template(
        &self,
        id: NotificationTemplateId,
        input: UpdateTemplate,
        org_id: OrgId,
    ) -> DbResult<Template> {
        crate::mysql::templates::update(&self.pool, id, input, org_id).await
    }

    async fn delete_template(&self, id: NotificationTemplateId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::templates::delete(&self.pool, id, org_id).await
    }

    async fn get_template_render_strings(
        &self,
        id: NotificationTemplateId,
    ) -> DbResult<RenderedTemplate> {
        crate::mysql::templates::get_render_strings(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreTelemetryRules for MysqlStore {
    async fn list_telemetry_rules(&self, org_id: OrgId) -> DbResult<Vec<TelemetryRule>> {
        crate::mysql::telemetry_rules::list(&self.pool, org_id).await
    }

    async fn list_all_telemetry_rules(&self) -> DbResult<Vec<TelemetryRule>> {
        crate::mysql::telemetry_rules::list_all(&self.pool).await
    }

    async fn get_telemetry_rule(
        &self,
        id: TelemetryRuleId,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule> {
        crate::mysql::telemetry_rules::get(&self.pool, id, org_id).await
    }

    async fn get_telemetry_rule_unscoped(&self, id: TelemetryRuleId) -> DbResult<TelemetryRule> {
        crate::mysql::telemetry_rules::get_unscoped(&self.pool, id).await
    }

    async fn create_telemetry_rule(
        &self,
        input: NewTelemetryRule,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule> {
        crate::mysql::telemetry_rules::create(&self.pool, input, org_id).await
    }

    async fn update_telemetry_rule(
        &self,
        id: TelemetryRuleId,
        patch: UpdateTelemetryRule,
        org_id: OrgId,
    ) -> DbResult<TelemetryRule> {
        crate::mysql::telemetry_rules::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_telemetry_rule(&self, id: TelemetryRuleId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::telemetry_rules::delete(&self.pool, id, org_id).await
    }

    async fn evaluate_telemetry_rules_tick(&self) -> DbResult<Vec<TelemetryRuleEvent>> {
        crate::mysql::telemetry_rules::evaluate_tick(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreMetricRules for MysqlStore {
    async fn list_metric_rules(&self, org_id: OrgId) -> DbResult<Vec<MetricRule>> {
        crate::mysql::metric_rules::list(&self.pool, org_id).await
    }

    async fn list_all_metric_rules(&self) -> DbResult<Vec<MetricRule>> {
        crate::mysql::metric_rules::list_all(&self.pool).await
    }

    async fn get_metric_rule(&self, id: MetricRuleId, org_id: OrgId) -> DbResult<MetricRule> {
        crate::mysql::metric_rules::get(&self.pool, id, org_id).await
    }

    async fn get_metric_rule_unscoped(&self, id: MetricRuleId) -> DbResult<MetricRule> {
        crate::mysql::metric_rules::get_unscoped(&self.pool, id).await
    }

    async fn create_metric_rule(
        &self,
        input: NewMetricRule,
        org_id: OrgId,
    ) -> DbResult<MetricRule> {
        crate::mysql::metric_rules::create(&self.pool, input, org_id).await
    }

    async fn update_metric_rule(
        &self,
        id: MetricRuleId,
        patch: UpdateMetricRule,
        org_id: OrgId,
    ) -> DbResult<MetricRule> {
        crate::mysql::metric_rules::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_metric_rule(&self, id: MetricRuleId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::metric_rules::delete(&self.pool, id, org_id).await
    }

    async fn evaluate_metric_rules_tick(&self) -> DbResult<Vec<MetricRuleEvent>> {
        crate::mysql::metric_rules::evaluate_tick(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreMonitorGroups for MysqlStore {
    async fn monitor_group_in_org(&self, group: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::monitor_groups::in_org(&self.pool, group, org_id).await
    }

    async fn list_monitor_groups(&self, org_id: OrgId) -> DbResult<Vec<MonitorGroup>> {
        crate::mysql::monitor_groups::list(&self.pool, org_id).await
    }

    async fn create_monitor_group(
        &self,
        input: NewMonitorGroup,
        org_id: OrgId,
    ) -> DbResult<MonitorGroup> {
        crate::mysql::monitor_groups::create(&self.pool, input, org_id).await
    }

    async fn update_monitor_group(
        &self,
        id: MonitorGroupId,
        patch: UpdateMonitorGroup,
        org_id: OrgId,
    ) -> DbResult<MonitorGroup> {
        crate::mysql::monitor_groups::update(&self.pool, id, patch, org_id).await
    }

    async fn would_form_group_cycle(
        &self,
        group: MonitorGroupId,
        new_parent: MonitorGroupId,
    ) -> DbResult<bool> {
        crate::mysql::monitor_groups::would_form_group_cycle(&self.pool, group, new_parent).await
    }

    async fn delete_monitor_group(&self, id: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::monitor_groups::delete(&self.pool, id, org_id).await
    }

    async fn parents_of(&self, child: MonitorId) -> DbResult<Vec<MonitorId>> {
        crate::mysql::monitor_groups::parents_of(&self.pool, child).await
    }

    async fn children_of(&self, parent: MonitorId) -> DbResult<Vec<MonitorId>> {
        crate::mysql::monitor_groups::children_of(&self.pool, parent).await
    }

    async fn any_parent_down(&self, child: MonitorId) -> DbResult<bool> {
        crate::mysql::monitor_groups::any_parent_down(&self.pool, child).await
    }

    async fn attach_dependency(&self, child: MonitorId, parent: MonitorId) -> DbResult<()> {
        crate::mysql::monitor_groups::attach_dependency(&self.pool, child, parent).await
    }

    async fn detach_dependency(&self, child: MonitorId, parent: MonitorId) -> DbResult<()> {
        crate::mysql::monitor_groups::detach_dependency(&self.pool, child, parent).await
    }

    async fn would_form_cycle(&self, child: MonitorId, parent: MonitorId) -> DbResult<bool> {
        crate::mysql::monitor_groups::would_form_cycle(&self.pool, child, parent).await
    }
}

#[async_trait::async_trait]
impl StoreSilences for MysqlStore {
    async fn is_silenced(&self, monitor: Option<Uuid>) -> DbResult<bool> {
        crate::mysql::silences::is_silenced(&self.pool, monitor).await
    }

    async fn create_silence(&self, s: NewSilence<'_>, org_id: OrgId) -> DbResult<Uuid> {
        crate::mysql::silences::create(&self.pool, s, org_id).await
    }

    async fn list_active_silences(&self, org_id: OrgId) -> DbResult<Vec<Silence>> {
        crate::mysql::silences::list_active(&self.pool, org_id).await
    }

    async fn delete_silence(&self, id: Uuid, org_id: OrgId) -> DbResult<bool> {
        crate::mysql::silences::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreOidcState for MysqlStore {
    async fn stash_oidc_state(
        &self,
        state: &str,
        pkce_verifier: &str,
        nonce: Option<&str>,
        return_to: Option<&str>,
    ) -> DbResult<()> {
        crate::mysql::oidc_state::stash(&self.pool, state, pkce_verifier, nonce, return_to).await
    }

    async fn consume_oidc_state(&self, state: &str) -> DbResult<Option<Consumed>> {
        crate::mysql::oidc_state::consume(&self.pool, state).await
    }

    async fn prune_oidc_state(&self) -> DbResult<u64> {
        crate::mysql::oidc_state::prune_expired(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreStatusPages for MysqlStore {
    async fn list_status_pages(&self, org_id: OrgId) -> DbResult<Vec<StatusPage>> {
        crate::mysql::status_pages::list(&self.pool, org_id).await
    }

    async fn list_all_status_pages(&self) -> DbResult<Vec<StatusPage>> {
        crate::mysql::status_pages::list_all(&self.pool).await
    }

    async fn get_status_page(&self, id: StatusPageId, org_id: OrgId) -> DbResult<StatusPage> {
        crate::mysql::status_pages::get(&self.pool, id, org_id).await
    }

    async fn get_status_page_by_slug(&self, slug: &str) -> DbResult<StatusPage> {
        crate::mysql::status_pages::get_by_slug(&self.pool, slug).await
    }

    async fn get_status_page_unscoped(&self, id: StatusPageId) -> DbResult<StatusPage> {
        crate::mysql::status_pages::get_unscoped(&self.pool, id).await
    }

    async fn find_status_page_by_custom_domain(&self, host: &str) -> DbResult<Option<StatusPage>> {
        crate::mysql::status_pages::find_by_custom_domain(&self.pool, host).await
    }

    async fn create_status_page(
        &self,
        input: NewStatusPage,
        org_id: OrgId,
    ) -> DbResult<StatusPage> {
        crate::mysql::status_pages::create(&self.pool, input, org_id).await
    }

    async fn update_status_page(
        &self,
        id: StatusPageId,
        patch: UpdateStatusPage,
        org_id: OrgId,
    ) -> DbResult<StatusPage> {
        crate::mysql::status_pages::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_status_page(&self, id: StatusPageId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::status_pages::delete(&self.pool, id, org_id).await
    }

    async fn status_page_public_view(&self, slug: &str) -> DbResult<PublicStatusPage> {
        crate::mysql::status_pages::public_view(&self.pool, slug).await
    }

    async fn verify_status_page_password(&self, slug: &str, candidate: &str) -> DbResult<bool> {
        crate::mysql::status_pages::verify_page_password(&self.pool, slug, candidate).await
    }

    async fn list_status_page_sections(
        &self,
        page_id: StatusPageId,
    ) -> DbResult<Vec<StatusPageSection>> {
        crate::mysql::status_pages::list_sections(&self.pool, page_id).await
    }

    async fn create_status_page_section(
        &self,
        page_id: StatusPageId,
        input: NewStatusPageSection,
    ) -> DbResult<StatusPageSection> {
        crate::mysql::status_pages::create_section(&self.pool, page_id, input).await
    }

    async fn update_status_page_section(
        &self,
        page_id: StatusPageId,
        id: StatusPageSectionId,
        patch: UpdateStatusPageSection,
    ) -> DbResult<StatusPageSection> {
        crate::mysql::status_pages::update_section(&self.pool, page_id, id, patch).await
    }

    async fn delete_status_page_section(
        &self,
        page_id: StatusPageId,
        id: StatusPageSectionId,
    ) -> DbResult<()> {
        crate::mysql::status_pages::delete_section(&self.pool, page_id, id).await
    }

    async fn assign_status_page_monitor_section(
        &self,
        page_id: StatusPageId,
        monitor_id: MonitorId,
        section_id: Option<StatusPageSectionId>,
    ) -> DbResult<()> {
        crate::mysql::status_pages::assign_monitor_section(
            &self.pool, page_id, monitor_id, section_id,
        )
        .await
    }
}

#[async_trait::async_trait]
impl StoreIncidents for MysqlStore {
    async fn create_incident(
        &self,
        page: StatusPageId,
        author: Option<UserId>,
        input: NewIncident,
    ) -> DbResult<Incident> {
        crate::mysql::incidents::create(&self.pool, page, author, input).await
    }

    async fn find_active_incident_by_dedup_key(
        &self,
        page: StatusPageId,
        key: &str,
    ) -> DbResult<Option<Incident>> {
        crate::mysql::incidents::find_active_by_dedup_key(&self.pool, page, key).await
    }

    async fn list_active_incidents(&self, page: StatusPageId) -> DbResult<Vec<Incident>> {
        crate::mysql::incidents::list_active(&self.pool, page).await
    }

    async fn recent_incidents(&self, limit: i64, org_id: OrgId) -> DbResult<Vec<Incident>> {
        crate::mysql::incidents::recent(&self.pool, limit, org_id).await
    }

    async fn list_resolved_incident_history(
        &self,
        page: StatusPageId,
        limit: i64,
    ) -> DbResult<Vec<Incident>> {
        crate::mysql::incidents::list_resolved_history(&self.pool, page, limit).await
    }

    async fn resolve_incident(&self, id: IncidentId, now: OffsetDateTime) -> DbResult<()> {
        crate::mysql::incidents::resolve(&self.pool, id, now).await
    }

    async fn list_all_incidents(&self, page: StatusPageId, limit: i64) -> DbResult<Vec<Incident>> {
        crate::mysql::incidents::list_all(&self.pool, page, limit).await
    }

    async fn delete_incident(&self, id: IncidentId) -> DbResult<()> {
        crate::mysql::incidents::delete(&self.pool, id).await
    }

    async fn update_incident(&self, id: IncidentId, patch: UpdateIncident) -> DbResult<Incident> {
        crate::mysql::incidents::update(&self.pool, id, patch).await
    }

    async fn get_incident(&self, id: IncidentId) -> DbResult<Incident> {
        crate::mysql::incidents::get(&self.pool, id).await
    }

    async fn list_incident_updates(&self, incident: IncidentId) -> DbResult<Vec<IncidentUpdate>> {
        crate::mysql::incidents::list_updates(&self.pool, incident).await
    }

    async fn post_incident_update(
        &self,
        incident: IncidentId,
        author: Option<UserId>,
        message: String,
    ) -> DbResult<Uuid> {
        crate::mysql::incidents::post_update(&self.pool, incident, author, message).await
    }
}

#[async_trait::async_trait]
impl StoreRouting for MysqlStore {
    async fn resolve_channels_for_monitor(
        &self,
        monitor: MonitorId,
    ) -> DbResult<Vec<Notification>> {
        crate::mysql::routing::resolve_channels_for_monitor(&self.pool, monitor).await
    }

    async fn group_tag_ids(&self, group: MonitorGroupId) -> DbResult<Vec<TagId>> {
        crate::mysql::routing::group_tag_ids(&self.pool, group).await
    }

    async fn channel_tag_ids(&self, notif: NotificationId) -> DbResult<Vec<TagId>> {
        crate::mysql::routing::channel_tag_ids(&self.pool, notif).await
    }

    async fn group_channel_ids(&self, group: MonitorGroupId) -> DbResult<Vec<NotificationId>> {
        crate::mysql::routing::group_channel_ids(&self.pool, group).await
    }

    async fn monitor_exclude_ids(&self, monitor: MonitorId) -> DbResult<Vec<NotificationId>> {
        crate::mysql::routing::monitor_exclude_ids(&self.pool, monitor).await
    }

    async fn tag_group(&self, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
        crate::mysql::routing::tag_group(&self.pool, group, tag).await
    }

    async fn untag_group(&self, group: MonitorGroupId, tag: TagId) -> DbResult<()> {
        crate::mysql::routing::untag_group(&self.pool, group, tag).await
    }

    async fn tag_channel(&self, notif: NotificationId, tag: TagId) -> DbResult<()> {
        crate::mysql::routing::tag_channel(&self.pool, notif, tag).await
    }

    async fn untag_channel(&self, notif: NotificationId, tag: TagId) -> DbResult<()> {
        crate::mysql::routing::untag_channel(&self.pool, notif, tag).await
    }

    async fn attach_group_channel(
        &self,
        group: MonitorGroupId,
        notif: NotificationId,
    ) -> DbResult<()> {
        crate::mysql::routing::attach_group_channel(&self.pool, group, notif).await
    }

    async fn detach_group_channel(
        &self,
        group: MonitorGroupId,
        notif: NotificationId,
    ) -> DbResult<()> {
        crate::mysql::routing::detach_group_channel(&self.pool, group, notif).await
    }

    async fn exclude_channel(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::mysql::routing::exclude_channel(&self.pool, monitor, notif).await
    }

    async fn unexclude_channel(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::mysql::routing::unexclude_channel(&self.pool, monitor, notif).await
    }
}

#[async_trait::async_trait]
impl StoreSubscribers for MysqlStore {
    async fn subscribe_email(
        &self,
        page: StatusPageId,
        email: &str,
    ) -> DbResult<(Subscriber, String)> {
        crate::mysql::subscribers::subscribe_email(&self.pool, page, email).await
    }

    async fn list_subscribers_for_page(&self, page: StatusPageId) -> DbResult<Vec<Subscriber>> {
        crate::mysql::subscribers::list_for_page(&self.pool, page).await
    }

    async fn confirmed_subscriber_emails_for_page(
        &self,
        page: StatusPageId,
    ) -> DbResult<Vec<String>> {
        crate::mysql::subscribers::confirmed_emails_for_page(&self.pool, page).await
    }

    async fn delete_subscriber(&self, id: StatusPageSubscriberId) -> DbResult<()> {
        crate::mysql::subscribers::delete(&self.pool, id).await
    }

    async fn unsubscribe_subscriber_by_token(&self, token: &str) -> DbResult<()> {
        crate::mysql::subscribers::unsubscribe_by_token(&self.pool, token).await
    }

    async fn subscriber_email_for_token(&self, token: &str) -> DbResult<Option<String>> {
        crate::mysql::subscribers::email_for_token(&self.pool, token).await
    }

    async fn subscriptions_for_email(&self, email: &str) -> DbResult<Vec<ManagedSubscription>> {
        crate::mysql::subscribers::subscriptions_for_email(&self.pool, email).await
    }

    async fn unsubscribe_all_for_email(&self, email: &str) -> DbResult<u64> {
        crate::mysql::subscribers::unsubscribe_all_for_email(&self.pool, email).await
    }

    async fn unsubscribe_email_from_page(&self, page: StatusPageId, email: &str) -> DbResult<()> {
        crate::mysql::subscribers::unsubscribe_email_from_page(&self.pool, page, email).await
    }

    async fn subscriber_page_for(
        &self,
        id: StatusPageSubscriberId,
    ) -> DbResult<Option<StatusPageId>> {
        crate::mysql::subscribers::page_for(&self.pool, id).await
    }

    async fn subscriber_token_for(&self, id: Uuid) -> DbResult<Option<String>> {
        crate::mysql::subscribers::token_for(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreDetection for MysqlStore {
    async fn detection_regex_is_valid(&self, pattern: &str) -> DbResult<bool> {
        crate::mysql::detection::regex_is_valid(&self.pool, pattern).await
    }

    async fn list_detection_rules(&self, org_id: OrgId) -> DbResult<Vec<DetectionRule>> {
        crate::mysql::detection::list(&self.pool, org_id).await
    }

    async fn list_all_detection_rules(&self) -> DbResult<Vec<DetectionRule>> {
        crate::mysql::detection::list_all(&self.pool).await
    }

    async fn get_detection_rule(
        &self,
        id: DetectionRuleId,
        org_id: OrgId,
    ) -> DbResult<DetectionRule> {
        crate::mysql::detection::get(&self.pool, id, org_id).await
    }

    async fn get_detection_rule_unscoped(&self, id: DetectionRuleId) -> DbResult<DetectionRule> {
        crate::mysql::detection::get_unscoped(&self.pool, id).await
    }

    async fn create_detection_rule(
        &self,
        input: NewDetectionRule,
        org_id: OrgId,
    ) -> DbResult<DetectionRule> {
        crate::mysql::detection::create(&self.pool, input, org_id).await
    }

    async fn update_detection_rule(
        &self,
        id: DetectionRuleId,
        patch: UpdateDetectionRule,
        org_id: OrgId,
    ) -> DbResult<DetectionRule> {
        crate::mysql::detection::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_detection_rule(&self, id: DetectionRuleId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::detection::delete(&self.pool, id, org_id).await
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
        crate::mysql::detection::preview(
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
        crate::mysql::detection::has_recent_finding(&self.pool, rule_id, secs, entity).await
    }

    async fn list_detection_findings(
        &self,
        limit: i64,
        open_only: bool,
    ) -> DbResult<Vec<DetectionFinding>> {
        crate::mysql::detection::list_findings(&self.pool, limit, open_only).await
    }

    async fn list_detection_findings_for_org(
        &self,
        limit: i64,
        open_only: bool,
        org_id: OrgId,
    ) -> DbResult<Vec<DetectionFinding>> {
        crate::mysql::detection::list_findings_for_org(&self.pool, limit, open_only, org_id).await
    }

    async fn detection_finding_in_org(
        &self,
        finding: DetectionFindingId,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::mysql::detection::finding_in_org(&self.pool, finding, org_id).await
    }

    async fn open_detection_findings_count(&self) -> DbResult<i64> {
        crate::mysql::detection::open_count(&self.pool).await
    }

    async fn fetch_detection_findings_since(
        &self,
        after: Option<OffsetDateTime>,
        limit: i64,
    ) -> DbResult<Vec<DetectionFinding>> {
        crate::mysql::detection::fetch_since(&self.pool, after, limit).await
    }

    async fn ack_detection_finding(&self, id: DetectionFindingId) -> DbResult<DetectionFinding> {
        crate::mysql::detection::ack_finding(&self.pool, id).await
    }

    async fn evaluate_detection_tick(&self) -> DbResult<Vec<FindingEvent>> {
        crate::mysql::detection::evaluate_tick(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreSessions for MysqlStore {
    async fn create_session(
        &self,
        user_id: UserId,
        ttl_seconds: i64,
        ip: Option<std::net::IpAddr>,
        user_agent: Option<String>,
    ) -> DbResult<Session> {
        crate::mysql::sessions::create(&self.pool, user_id, ttl_seconds, ip, user_agent).await
    }

    async fn lookup_session(&self, id: Uuid) -> DbResult<Session> {
        crate::mysql::sessions::get(&self.pool, id).await
    }

    async fn set_session_active_org(
        &self,
        session_id: Uuid,
        user_id: UserId,
        org_id: Uuid,
    ) -> DbResult<bool> {
        crate::mysql::sessions::set_active_org(&self.pool, session_id, user_id, org_id).await
    }

    async fn delete_session(&self, id: Uuid) -> DbResult<()> {
        crate::mysql::sessions::delete(&self.pool, id).await
    }

    async fn delete_sessions_for_user(&self, user_id: UserId) -> DbResult<u64> {
        crate::mysql::sessions::delete_for_user(&self.pool, user_id).await
    }

    async fn list_sessions_for_user(&self, user_id: UserId) -> DbResult<Vec<SessionInfo>> {
        crate::mysql::sessions::list_for_user(&self.pool, user_id).await
    }

    async fn delete_one_session_for_user(&self, user_id: UserId, id: Uuid) -> DbResult<bool> {
        crate::mysql::sessions::delete_one_for_user(&self.pool, user_id, id).await
    }

    async fn delete_other_sessions(&self, user_id: UserId, keep: Uuid) -> DbResult<u64> {
        crate::mysql::sessions::delete_others(&self.pool, user_id, keep).await
    }

    async fn cleanup_expired_sessions(&self) -> DbResult<u64> {
        crate::mysql::sessions::cleanup_expired(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreNotifications for MysqlStore {
    async fn list_notifications(&self, org_id: OrgId) -> DbResult<Vec<Notification>> {
        crate::mysql::notifications::list(&self.pool, org_id).await
    }

    async fn list_all_notifications(&self) -> DbResult<Vec<Notification>> {
        crate::mysql::notifications::list_all(&self.pool).await
    }

    async fn get_notification(&self, id: NotificationId, org_id: OrgId) -> DbResult<Notification> {
        crate::mysql::notifications::get(&self.pool, id, org_id).await
    }

    async fn get_notification_unscoped(&self, id: NotificationId) -> DbResult<Notification> {
        crate::mysql::notifications::get_unscoped(&self.pool, id).await
    }

    async fn create_notification(
        &self,
        input: NewNotification,
        org_id: OrgId,
    ) -> DbResult<Notification> {
        crate::mysql::notifications::create(&self.pool, input, org_id).await
    }

    async fn update_notification(
        &self,
        id: NotificationId,
        input: UpdateNotification,
        org_id: OrgId,
    ) -> DbResult<Notification> {
        crate::mysql::notifications::update(&self.pool, id, input, org_id).await
    }

    async fn notification_counts_per_monitor(
        &self,
        org_id: OrgId,
    ) -> DbResult<Vec<MonitorChannelCount>> {
        crate::mysql::notifications::counts_per_monitor(&self.pool, org_id).await
    }

    async fn delete_notification(&self, id: NotificationId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::notifications::delete(&self.pool, id, org_id).await
    }

    async fn attach_notification(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::mysql::notifications::attach(&self.pool, monitor, notif).await
    }

    async fn detach_notification(&self, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
        crate::mysql::notifications::detach(&self.pool, monitor, notif).await
    }

    async fn notifications_for_monitor(&self, monitor: MonitorId) -> DbResult<Vec<Notification>> {
        crate::mysql::notifications::for_monitor(&self.pool, monitor).await
    }

    async fn mark_notification_fired(&self, id: NotificationId) -> DbResult<()> {
        crate::mysql::notifications::mark_fired(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreSettings for MysqlStore {
    async fn get_setting(&self, key: &str) -> DbResult<Option<serde_json::Value>> {
        crate::mysql::settings::get_setting(&self.pool, key).await
    }

    async fn put_setting(&self, key: &str, value: &serde_json::Value) -> DbResult<()> {
        crate::mysql::settings::put_setting(&self.pool, key, value).await
    }

    async fn delete_setting(&self, key: &str) -> DbResult<()> {
        crate::mysql::settings::delete_setting(&self.pool, key).await
    }
}

#[async_trait::async_trait]
impl StoreLogs for MysqlStore {
    async fn insert_logs(&self, logs: &[ParsedLog], org_id: OrgId) -> DbResult<u64> {
        crate::mysql::logs::insert_logs(&self.pool, logs, org_id).await
    }

    async fn query_logs(&self, f: LogFilter<'_>, org_id: OrgId) -> DbResult<Vec<LogEntry>> {
        crate::mysql::logs::query_logs(&self.pool, f, org_id).await
    }

    async fn log_level_counts(
        &self,
        service: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<(String, i64)>> {
        crate::mysql::logs::level_counts(&self.pool, service, hours, org_id).await
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
        crate::mysql::logs::histogram(
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
        crate::mysql::logs::list_services(&self.pool, org_id).await
    }

    async fn prune_logs(&self, days: i32) -> DbResult<u64> {
        crate::mysql::logs::prune(&self.pool, days).await
    }
}

#[async_trait::async_trait]
impl StoreTraces for MysqlStore {
    async fn insert_spans(&self, spans: &[ParsedSpan], org_id: OrgId) -> DbResult<u64> {
        crate::mysql::traces::insert_spans(&self.pool, spans, org_id).await
    }

    async fn list_traces(&self, f: TraceFilter<'_>, org_id: OrgId) -> DbResult<Vec<TraceSummary>> {
        crate::mysql::traces::list_traces(&self.pool, f, org_id).await
    }

    async fn get_trace_spans(&self, trace_id: &str, org_id: OrgId) -> DbResult<Vec<Span>> {
        crate::mysql::traces::get_trace_spans(&self.pool, trace_id, org_id).await
    }

    async fn trace_service_map(
        &self,
        window_hours: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ServiceEdge>> {
        crate::mysql::traces::service_map(&self.pool, window_hours, org_id).await
    }

    async fn trace_operation_stats(
        &self,
        service: &str,
        window_hours: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<OperationStat>> {
        crate::mysql::traces::operation_stats(&self.pool, service, window_hours, org_id).await
    }

    async fn trace_operation_trend(
        &self,
        service: &str,
        operation: &str,
        window_hours: i64,
        buckets: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<f64>> {
        crate::mysql::traces::operation_trend(
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
        crate::mysql::traces::prune(&self.pool, days).await
    }
}

#[async_trait::async_trait]
impl StoreRum for MysqlStore {
    async fn insert_rum_event(&self, b: &RumBeacon, org_id: OrgId) -> DbResult<()> {
        crate::mysql::rum::insert_event(&self.pool, b, org_id).await
    }

    async fn rum_page_samples(
        &self,
        app: Option<&str>,
        url: &str,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RumSample>> {
        crate::mysql::rum::page_samples(&self.pool, app, url, hours, limit, org_id).await
    }

    async fn rum_recent_traced(
        &self,
        app: Option<&str>,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<RumTracedLoad>> {
        crate::mysql::rum::recent_traced(&self.pool, app, hours, limit, org_id).await
    }

    async fn rum_summary(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<RumVitals> {
        crate::mysql::rum::summary(&self.pool, app, hours, org_id).await
    }

    async fn rum_pages(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumPage>> {
        crate::mysql::rum::pages(&self.pool, app, hours, org_id).await
    }

    async fn rum_browser_breakdown(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumBrowser>> {
        crate::mysql::rum::browser_breakdown(&self.pool, app, hours, org_id).await
    }

    async fn rum_user_breakdown(
        &self,
        app: Option<&str>,
        hours: i32,
        org_id: OrgId,
    ) -> DbResult<Vec<RumUser>> {
        crate::mysql::rum::user_breakdown(&self.pool, app, hours, org_id).await
    }

    async fn rum_apps(&self, org_id: OrgId) -> DbResult<Vec<String>> {
        crate::mysql::rum::apps(&self.pool, org_id).await
    }

    async fn prune_rum(&self, days: i32) -> DbResult<u64> {
        crate::mysql::rum::prune(&self.pool, days).await
    }
}

#[async_trait::async_trait]
impl StoreProfiles for MysqlStore {
    async fn insert_profile(&self, p: NewProfile<'_>, org_id: OrgId) -> DbResult<i64> {
        crate::mysql::profiles::insert(&self.pool, p, org_id).await
    }

    async fn list_profiles(
        &self,
        service: Option<&str>,
        profile_type: Option<&str>,
        hours: i32,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ProfileMeta>> {
        crate::mysql::profiles::list(&self.pool, service, profile_type, hours, limit, org_id).await
    }

    async fn profile_folded_in_window(
        &self,
        service: &str,
        profile_type: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        org_id: OrgId,
    ) -> DbResult<Vec<Vec<u8>>> {
        crate::mysql::profiles::folded_in_window(
            &self.pool,
            service,
            profile_type,
            from,
            to,
            org_id,
        )
        .await
    }

    async fn profile_fetch_folded(
        &self,
        id: i64,
        org_id: OrgId,
    ) -> DbResult<Option<(String, Vec<u8>)>> {
        crate::mysql::profiles::fetch_folded(&self.pool, id, org_id).await
    }

    async fn profile_services(&self, org_id: OrgId) -> DbResult<Vec<String>> {
        crate::mysql::profiles::services(&self.pool, org_id).await
    }

    async fn profile_types(&self, service: Option<&str>, org_id: OrgId) -> DbResult<Vec<String>> {
        crate::mysql::profiles::profile_types(&self.pool, service, org_id).await
    }

    async fn prune_profiles(&self, days: i32) -> DbResult<u64> {
        crate::mysql::profiles::prune(&self.pool, days).await
    }
}

#[async_trait::async_trait]
impl StoreMetrics for MysqlStore {
    async fn monitors_by_status(&self) -> DbResult<Vec<(String, i64)>> {
        crate::mysql::metrics::monitors_by_status(&self.pool).await
    }

    async fn monitors_by_kind(&self) -> DbResult<Vec<(String, i64)>> {
        crate::mysql::metrics::monitors_by_kind(&self.pool).await
    }

    async fn channels_active(&self) -> DbResult<i64> {
        crate::mysql::metrics::channels_active(&self.pool).await
    }

    async fn webpush_subscribers(&self) -> DbResult<i64> {
        crate::mysql::metrics::webpush_subscribers(&self.pool).await
    }

    async fn heartbeats_recent_by_status(
        &self,
        window_seconds: i64,
    ) -> DbResult<Vec<(String, i64)>> {
        crate::mysql::metrics::heartbeats_recent_by_status(&self.pool, window_seconds).await
    }

    async fn incidents_open(&self) -> DbResult<i64> {
        crate::mysql::metrics::incidents_open(&self.pool).await
    }

    async fn pipeline_gauges(&self) -> DbResult<PipelineGauges> {
        crate::mysql::metrics::pipeline_gauges(&self.pool).await
    }

    async fn storage_usage(&self) -> DbResult<Vec<TableSize>> {
        crate::mysql::metrics::storage_usage(&self.pool).await
    }

    async fn ingest_gauges(&self) -> DbResult<IngestGauges> {
        crate::mysql::metrics::ingest_gauges(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreErrorTracking for MysqlStore {
    async fn list_error_projects(&self, org_id: OrgId) -> DbResult<Vec<ErrorProject>> {
        crate::mysql::error_tracking::list(&self.pool, org_id).await
    }

    async fn error_project_in_org(&self, project: ErrorProjectId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::error_tracking::project_in_org(&self.pool, project, org_id).await
    }

    async fn error_issue_in_org(&self, issue: ErrorIssueId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::error_tracking::issue_in_org(&self.pool, issue, org_id).await
    }

    async fn get_error_project(&self, id: ErrorProjectId) -> DbResult<ErrorProject> {
        crate::mysql::error_tracking::get(&self.pool, id).await
    }

    async fn org_for_error_project(&self, id: ErrorProjectId) -> DbResult<OrgId> {
        crate::mysql::error_tracking::org_for_project(&self.pool, id).await
    }

    async fn get_error_project_opt(&self, id: ErrorProjectId) -> DbResult<Option<ErrorProject>> {
        crate::mysql::error_tracking::get_opt(&self.pool, id).await
    }

    async fn find_or_create_error_project_by_name(
        &self,
        name: &str,
        org_id: OrgId,
    ) -> DbResult<ErrorProject> {
        crate::mysql::error_tracking::find_or_create_by_name(&self.pool, name, org_id).await
    }

    async fn create_error_project(
        &self,
        input: NewErrorProject,
        org_id: OrgId,
    ) -> DbResult<ErrorProject> {
        crate::mysql::error_tracking::create(&self.pool, input, org_id).await
    }

    async fn update_error_project(
        &self,
        id: ErrorProjectId,
        patch: UpdateErrorProject,
        org_id: OrgId,
    ) -> DbResult<ErrorProject> {
        crate::mysql::error_tracking::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_error_project(&self, id: ErrorProjectId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::error_tracking::delete(&self.pool, id, org_id).await
    }

    async fn record_error_event(
        &self,
        project_id: ErrorProjectId,
        ev: &ParsedEvent,
    ) -> DbResult<RecordOutcome> {
        crate::mysql::error_tracking::record_event(&self.pool, project_id, ev).await
    }

    async fn error_issues_for_trace(
        &self,
        trace_id: &str,
        org_id: OrgId,
    ) -> DbResult<Vec<TraceErrorRef>> {
        crate::mysql::error_tracking::issues_for_trace(&self.pool, trace_id, org_id).await
    }

    async fn list_error_issues(
        &self,
        project_id: ErrorProjectId,
        status: Option<&str>,
        before_id: Option<Uuid>,
        limit: i64,
    ) -> DbResult<Vec<ErrorIssue>> {
        crate::mysql::error_tracking::list_issues(&self.pool, project_id, status, before_id, limit)
            .await
    }

    async fn recent_open_error_issues(
        &self,
        limit: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<ErrorIssue>> {
        crate::mysql::error_tracking::recent_open_issues(&self.pool, limit, org_id).await
    }

    async fn error_project_event_histogram(
        &self,
        project_id: ErrorProjectId,
        hours: i32,
        buckets: i64,
    ) -> DbResult<Vec<ErrorBucket>> {
        crate::mysql::error_tracking::project_event_histogram(
            &self.pool, project_id, hours, buckets,
        )
        .await
    }

    async fn get_error_issue(&self, id: ErrorIssueId) -> DbResult<ErrorIssue> {
        crate::mysql::error_tracking::get_issue(&self.pool, id).await
    }

    async fn error_issue_affected_users(
        &self,
        id: ErrorIssueId,
        limit: i64,
    ) -> DbResult<Vec<AffectedUser>> {
        crate::mysql::error_tracking::issue_affected_users(&self.pool, id, limit).await
    }

    async fn error_issue_stats(&self, id: ErrorIssueId) -> DbResult<IssueStats> {
        crate::mysql::error_tracking::issue_stats(&self.pool, id).await
    }

    async fn set_error_issue_status(&self, id: ErrorIssueId, status: &str) -> DbResult<ErrorIssue> {
        crate::mysql::error_tracking::set_issue_status(&self.pool, id, status).await
    }

    async fn assign_error_issue(
        &self,
        id: ErrorIssueId,
        assignee: Option<UserId>,
    ) -> DbResult<ErrorIssue> {
        crate::mysql::error_tracking::assign_issue(&self.pool, id, assignee).await
    }

    async fn error_assignable_users(
        &self,
        org_id: OrgId,
    ) -> DbResult<Vec<crate::error_tracking::AssignableUser>> {
        crate::mysql::error_tracking::assignable_users(&self.pool, org_id).await
    }

    async fn list_error_events(
        &self,
        issue_id: ErrorIssueId,
        limit: i64,
    ) -> DbResult<Vec<ErrorEvent>> {
        crate::mysql::error_tracking::list_events(&self.pool, issue_id, limit).await
    }

    async fn prune_error_events(&self) -> DbResult<u64> {
        crate::mysql::error_tracking::prune(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreScheduledReports for MysqlStore {
    async fn list_scheduled_reports(&self, org_id: OrgId) -> DbResult<Vec<ScheduledReport>> {
        crate::mysql::scheduled_reports::list(&self.pool, org_id).await
    }

    async fn get_scheduled_report(
        &self,
        id: ScheduledReportId,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport> {
        crate::mysql::scheduled_reports::get(&self.pool, id, org_id).await
    }

    async fn create_scheduled_report(
        &self,
        input: NewScheduledReport,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport> {
        crate::mysql::scheduled_reports::create(&self.pool, input, org_id).await
    }

    async fn update_scheduled_report(
        &self,
        id: ScheduledReportId,
        input: UpdateScheduledReport,
        org_id: OrgId,
    ) -> DbResult<ScheduledReport> {
        crate::mysql::scheduled_reports::update(&self.pool, id, input, org_id).await
    }

    async fn delete_scheduled_report(&self, id: ScheduledReportId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::scheduled_reports::delete(&self.pool, id, org_id).await
    }

    async fn due_scheduled_reports(
        &self,
        now: OffsetDateTime,
    ) -> DbResult<Vec<(ScheduledReport, OrgId)>> {
        crate::mysql::scheduled_reports::due(&self.pool, now).await
    }

    async fn render_scheduled_report(
        &self,
        report_name: &str,
        cadence: &str,
        org_id: OrgId,
    ) -> DbResult<(String, String)> {
        crate::mysql::scheduled_reports::render(&self.pool, report_name, cadence, org_id).await
    }

    async fn mark_scheduled_report_sent(&self, id: ScheduledReportId) -> DbResult<()> {
        crate::mysql::scheduled_reports::mark_sent(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreIncidentTemplates for MysqlStore {
    async fn list_incident_templates(&self, org_id: OrgId) -> DbResult<Vec<IncidentTemplate>> {
        crate::mysql::incident_templates::list(&self.pool, org_id).await
    }

    async fn get_incident_template(
        &self,
        id: IncidentTemplateId,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate> {
        crate::mysql::incident_templates::get(&self.pool, id, org_id).await
    }

    async fn create_incident_template(
        &self,
        input: NewIncidentTemplate,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate> {
        crate::mysql::incident_templates::create(&self.pool, input, org_id).await
    }

    async fn update_incident_template(
        &self,
        id: IncidentTemplateId,
        input: UpdateIncidentTemplate,
        org_id: OrgId,
    ) -> DbResult<IncidentTemplate> {
        crate::mysql::incident_templates::update(&self.pool, id, input, org_id).await
    }

    async fn delete_incident_template(
        &self,
        id: IncidentTemplateId,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::mysql::incident_templates::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreMonitorPresets for MysqlStore {
    async fn list_monitor_presets(&self, org_id: OrgId) -> DbResult<Vec<MonitorPreset>> {
        crate::mysql::monitor_presets::list(&self.pool, org_id).await
    }

    async fn get_monitor_preset(
        &self,
        id: MonitorPresetId,
        org_id: OrgId,
    ) -> DbResult<MonitorPreset> {
        crate::mysql::monitor_presets::get(&self.pool, id, org_id).await
    }

    async fn create_monitor_preset(
        &self,
        input: NewMonitorPreset,
        org_id: OrgId,
    ) -> DbResult<MonitorPreset> {
        crate::mysql::monitor_presets::create(&self.pool, input, org_id).await
    }

    async fn delete_monitor_preset(&self, id: MonitorPresetId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::monitor_presets::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreMonitorTemplates for MysqlStore {
    async fn list_monitor_templates(&self, org_id: OrgId) -> DbResult<Vec<MonitorTemplate>> {
        crate::mysql::monitor_templates::list(&self.pool, org_id).await
    }

    async fn get_monitor_template(
        &self,
        id: MonitorTemplateId,
        org_id: OrgId,
    ) -> DbResult<MonitorTemplate> {
        crate::mysql::monitor_templates::get(&self.pool, id, org_id).await
    }

    async fn create_monitor_template(
        &self,
        input: NewMonitorTemplate,
        org_id: OrgId,
    ) -> DbResult<MonitorTemplate> {
        crate::mysql::monitor_templates::create(&self.pool, input, org_id).await
    }

    async fn delete_monitor_template(&self, id: MonitorTemplateId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::monitor_templates::delete(&self.pool, id, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreDeliveryLog for MysqlStore {
    async fn record_delivery(&self, entry: NewDelivery<'_>) -> DbResult<DeliveryEntry> {
        crate::mysql::delivery_log::record(&self.pool, entry).await
    }

    async fn get_delivery(&self, id: i64, org_id: OrgId) -> DbResult<Option<DeliveryEntry>> {
        crate::mysql::delivery_log::get(&self.pool, id, org_id).await
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
        crate::mysql::delivery_log::list(&self.pool, limit, before_ts, ok, monitor, channel, org_id)
            .await
    }

    async fn list_all_deliveries(&self, limit: i64, org_id: OrgId) -> DbResult<Vec<DeliveryEntry>> {
        crate::mysql::delivery_log::list_all(&self.pool, limit, org_id).await
    }
}

#[async_trait::async_trait]
impl StoreAgents for MysqlStore {
    async fn list_agents(&self, org_id: OrgId) -> DbResult<Vec<Agent>> {
        crate::mysql::agents::list(&self.pool, org_id).await
    }

    async fn get_agent(&self, id: AgentId, org_id: OrgId) -> DbResult<Agent> {
        crate::mysql::agents::get(&self.pool, id, org_id).await
    }

    async fn create_agent(&self, input: NewAgent, org_id: OrgId) -> DbResult<IssuedAgent> {
        crate::mysql::agents::create(&self.pool, input, org_id).await
    }

    async fn update_agent(
        &self,
        id: AgentId,
        patch: UpdateAgent,
        org_id: OrgId,
    ) -> DbResult<Agent> {
        crate::mysql::agents::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_agent(&self, id: AgentId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::agents::delete(&self.pool, id, org_id).await
    }

    async fn lookup_agent(&self, token: &str) -> DbResult<Agent> {
        crate::mysql::agents::lookup(&self.pool, token).await
    }

    async fn touch_agent_seen(&self, id: AgentId, version: Option<&str>) -> DbResult<()> {
        crate::mysql::agents::touch_seen(&self.pool, id, version).await
    }
}

#[async_trait::async_trait]
impl StoreMetricSamples for MysqlStore {
    async fn insert_metric_samples(&self, samples: &[PromSample], org_id: OrgId) -> DbResult<()> {
        crate::mysql::metric_samples::insert_many(&self.pool, samples, org_id).await
    }

    async fn list_metric_sample_series(&self, org_id: OrgId) -> DbResult<Vec<Series>> {
        crate::mysql::metric_samples::list_series(&self.pool, org_id).await
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
        crate::mysql::metric_samples::range_query(
            &self.pool,
            name,
            labels,
            from,
            to,
            step_seconds,
            org_id,
        )
        .await
    }

    async fn metric_sample_baseline(
        &self,
        name: &str,
        labels: &serde_json::Value,
        window_secs: i64,
        org_id: OrgId,
    ) -> DbResult<Option<(f64, f64)>> {
        crate::mysql::metric_samples::baseline(&self.pool, name, labels, window_secs, org_id).await
    }

    async fn metric_sample_latest(
        &self,
        name: &str,
        labels: &serde_json::Value,
        org_id: OrgId,
    ) -> DbResult<Option<(f64, OffsetDateTime)>> {
        crate::mysql::metric_samples::latest(&self.pool, name, labels, org_id).await
    }

    async fn prune_metric_samples_older_than(&self, cutoff: OffsetDateTime) -> DbResult<u64> {
        crate::mysql::metric_samples::prune_older_than(&self.pool, cutoff).await
    }
}

#[async_trait::async_trait]
impl StoreRetention for MysqlStore {
    async fn run_retention_prune(&self) -> DbResult<u64> {
        // Flat age-based prune of every telemetry table MySQL writes. No rollup
        // tier yet (Postgres-only optimization), so this bounds growth without
        // preserving long-range uptime history.
        let cfg = crate::prune::parse_config(
            crate::mysql::settings::get_setting(&self.pool, "retention_days").await?,
        );
        let metrics_cutoff =
            OffsetDateTime::now_utc() - time::Duration::days(cfg.metrics_days.max(0) as i64);
        let mut total = 0u64;
        total += crate::mysql::heartbeats::prune(&self.pool, cfg.heartbeats).await?;
        total += crate::mysql::logs::prune(&self.pool, cfg.logs_days).await?;
        total += crate::mysql::traces::prune(&self.pool, cfg.traces_days).await?;
        total += crate::mysql::metric_samples::prune_older_than(&self.pool, metrics_cutoff).await?;
        total += crate::mysql::rum::prune(&self.pool, cfg.rum_days).await?;
        total += crate::mysql::profiles::prune(&self.pool, cfg.profiles_days).await?;
        total += crate::mysql::error_tracking::prune(&self.pool).await?;
        total += crate::mysql::audit::prune(&self.pool, cfg.audit_log).await?;
        total += crate::mysql::detection::prune(&self.pool, cfg.findings_days).await?;
        total += crate::mysql::oidc_state::prune_expired(&self.pool).await?;
        Ok(total)
    }

    // Uptime-history reads use the Postgres rollup tier, which MySQL doesn't
    // maintain (flat age-based prune only, see above). Not ported — the
    // uptime-history route stays Postgres-only, as it was when it called the
    // PG-only `pool()`.
    async fn retention_config(&self) -> DbResult<crate::prune::RetentionConfig> {
        unimplemented!("MysqlStore::retention_config: rollup tier not ported (multi-DB P1)")
    }

    async fn rollups_for_monitor(
        &self,
        _monitor: Uuid,
        _since: time::OffsetDateTime,
        _until: time::OffsetDateTime,
    ) -> DbResult<Vec<crate::prune::HeartbeatRollup>> {
        unimplemented!("MysqlStore::rollups_for_monitor: rollup tier not ported (multi-DB P1)")
    }

    async fn daily_uptime_from_rollups(
        &self,
        _monitor: Uuid,
        _since: time::OffsetDateTime,
        _until: time::OffsetDateTime,
    ) -> DbResult<Vec<crate::prune::DailyUptimePoint>> {
        unimplemented!(
            "MysqlStore::daily_uptime_from_rollups: rollup tier not ported (multi-DB P1)"
        )
    }

    async fn daily_uptime_from_raw(
        &self,
        _monitor: Uuid,
        _since: time::OffsetDateTime,
        _until: time::OffsetDateTime,
    ) -> DbResult<Vec<crate::prune::DailyUptimePoint>> {
        unimplemented!("MysqlStore::daily_uptime_from_raw: rollup tier not ported (multi-DB P1)")
    }
}

#[async_trait::async_trait]
impl StoreSourceMaps for MysqlStore {
    async fn upsert_source_map(&self, m: NewSourceMap<'_>) -> DbResult<i64> {
        crate::mysql::source_maps::upsert(&self.pool, m).await
    }

    async fn get_source_map(
        &self,
        project_id: Uuid,
        release: &str,
        filename: &str,
    ) -> DbResult<Option<serde_json::Value>> {
        crate::mysql::source_maps::get(&self.pool, project_id, release, filename).await
    }

    async fn list_source_maps(&self, project_id: Uuid) -> DbResult<Vec<SourceMapMeta>> {
        crate::mysql::source_maps::list(&self.pool, project_id).await
    }

    async fn delete_source_map(&self, project_id: Uuid, id: i64) -> DbResult<bool> {
        crate::mysql::source_maps::delete(&self.pool, project_id, id).await
    }
}

#[async_trait::async_trait]
impl StoreUsers for MysqlStore {
    async fn count_users(&self) -> DbResult<i64> {
        crate::mysql::users::count(&self.pool).await
    }

    async fn create_user(&self, input: NewUser) -> DbResult<User> {
        crate::mysql::users::create(&self.pool, input).await
    }

    async fn get_user_by_email(&self, email: &str) -> DbResult<UserWithHash> {
        crate::mysql::users::get_by_email(&self.pool, email).await
    }

    async fn user_by_email(&self, email: &str) -> DbResult<Option<User>> {
        crate::mysql::users::by_email(&self.pool, email).await
    }

    async fn get_user(&self, id: UserId) -> DbResult<User> {
        crate::mysql::users::get(&self.pool, id).await
    }

    async fn set_user_totp_secret(&self, id: UserId, secret: &str) -> DbResult<()> {
        crate::mysql::users::set_totp_secret(&self.pool, id, secret).await
    }

    async fn enable_user_totp(&self, id: UserId) -> DbResult<()> {
        crate::mysql::users::enable_totp(&self.pool, id).await
    }

    async fn disable_user_totp(&self, id: UserId) -> DbResult<()> {
        crate::mysql::users::disable_totp(&self.pool, id).await
    }

    async fn mark_user_login(&self, id: UserId) -> DbResult<()> {
        crate::mysql::users::mark_login(&self.pool, id).await
    }

    async fn user_totp_locked_until(&self, id: UserId) -> DbResult<Option<OffsetDateTime>> {
        crate::mysql::users::totp_locked_until(&self.pool, id).await
    }

    async fn record_user_totp_failure(
        &self,
        id: UserId,
        max_attempts: i32,
        lockout_mins: i32,
    ) -> DbResult<bool> {
        crate::mysql::users::record_totp_failure(&self.pool, id, max_attempts, lockout_mins).await
    }

    async fn reset_user_totp_failures(&self, id: UserId) -> DbResult<()> {
        crate::mysql::users::reset_totp_failures(&self.pool, id).await
    }

    async fn list_users(&self) -> DbResult<Vec<User>> {
        crate::mysql::users::list(&self.pool).await
    }

    async fn set_user_admin(&self, id: UserId, is_admin: bool) -> DbResult<()> {
        crate::mysql::users::set_admin(&self.pool, id, is_admin).await
    }

    async fn set_user_role(&self, id: UserId, role: Role) -> DbResult<()> {
        crate::mysql::users::set_role(&self.pool, id, role).await
    }

    async fn delete_user(&self, id: UserId) -> DbResult<()> {
        crate::mysql::users::delete(&self.pool, id).await
    }

    async fn anonymize_user(&self, id: UserId) -> DbResult<()> {
        crate::mysql::users::anonymize(&self.pool, id).await
    }

    async fn get_user_prefs(&self, id: UserId) -> DbResult<serde_json::Value> {
        crate::mysql::users::get_prefs(&self.pool, id).await
    }

    async fn set_user_prefs(&self, id: UserId, prefs: &serde_json::Value) -> DbResult<()> {
        crate::mysql::users::set_prefs(&self.pool, id, prefs).await
    }

    async fn set_user_password(&self, id: UserId, hash: &str) -> DbResult<()> {
        crate::mysql::users::set_password(&self.pool, id, hash).await
    }
}

#[async_trait::async_trait]
impl StoreWebpush for MysqlStore {
    async fn list_webpush_subs(
        &self,
        notification: NotificationId,
    ) -> DbResult<Vec<crate::webpush::WebpushSubscription>> {
        crate::mysql::webpush::list_for_notification(&self.pool, notification).await
    }

    async fn upsert_webpush_sub(
        &self,
        notification: NotificationId,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
    ) -> DbResult<()> {
        crate::mysql::webpush::upsert(&self.pool, notification, endpoint, p256dh, auth).await
    }

    async fn delete_webpush_sub_by_endpoint(&self, endpoint: &str) -> DbResult<()> {
        crate::mysql::webpush::delete_by_endpoint(&self.pool, endpoint).await
    }

    async fn delete_webpush_sub(&self, id: Uuid) -> DbResult<()> {
        crate::mysql::webpush::delete(&self.pool, id).await
    }

    async fn get_vapid_keys(&self) -> DbResult<Option<crate::webpush::VapidKeys>> {
        crate::mysql::webpush::get_vapid(&self.pool).await
    }

    async fn set_vapid_keys(&self, keys: &crate::webpush::VapidKeys) -> DbResult<()> {
        crate::mysql::webpush::set_vapid(&self.pool, keys).await
    }
}

#[async_trait::async_trait]
impl StoreOrgs for MysqlStore {
    async fn create_org(&self, slug: &str, name: &str) -> DbResult<rampart_core::org::Org> {
        crate::mysql::orgs::create(&self.pool, slug, name).await
    }

    async fn get_org(&self, id: OrgId) -> DbResult<rampart_core::org::Org> {
        crate::mysql::orgs::get(&self.pool, id).await
    }

    async fn orgs_for_user(&self, user_id: UserId) -> DbResult<Vec<rampart_core::org::Org>> {
        crate::mysql::orgs::list_for_user(&self.pool, user_id).await
    }

    async fn upsert_org_member(&self, org_id: OrgId, user_id: UserId, role: Role) -> DbResult<()> {
        crate::mysql::orgs::upsert_member(&self.pool, org_id, user_id, role).await
    }

    async fn org_member_role(&self, org_id: OrgId, user_id: UserId) -> DbResult<Option<Role>> {
        crate::mysql::orgs::member_role(&self.pool, org_id, user_id).await
    }

    async fn list_org_members(&self, org_id: OrgId) -> DbResult<Vec<rampart_core::org::OrgMember>> {
        crate::mysql::orgs::list_members(&self.pool, org_id).await
    }

    async fn list_org_members_detailed(
        &self,
        org_id: OrgId,
    ) -> DbResult<Vec<crate::orgs::OrgMemberDetail>> {
        crate::mysql::orgs::list_members_detailed(&self.pool, org_id).await
    }

    async fn update_org(&self, id: OrgId, name: &str) -> DbResult<rampart_core::org::Org> {
        crate::mysql::orgs::update(&self.pool, id, name).await
    }

    async fn org_by_slug(&self, slug: &str) -> DbResult<rampart_core::org::Org> {
        crate::mysql::orgs::get_by_slug(&self.pool, slug).await
    }

    async fn remove_org_member(&self, org_id: OrgId, user_id: UserId) -> DbResult<bool> {
        crate::mysql::orgs::remove_member(&self.pool, org_id, user_id).await
    }

    async fn count_org_admins(&self, org_id: OrgId) -> DbResult<i64> {
        crate::mysql::orgs::count_admins(&self.pool, org_id).await
    }

    async fn create_org_with_owner(
        &self,
        slug: &str,
        name: &str,
        owner: UserId,
    ) -> DbResult<rampart_core::org::Org> {
        crate::mysql::orgs::create_with_owner(&self.pool, slug, name, owner).await
    }
}

#[async_trait::async_trait]
impl StoreMonitors for MysqlStore {
    async fn create_monitor(&self, input: NewMonitor, org_id: OrgId) -> DbResult<Monitor> {
        crate::mysql::monitors::create(&self.pool, input, org_id).await
    }

    async fn regenerate_monitor_push_token(
        &self,
        id: MonitorId,
        org_id: OrgId,
    ) -> DbResult<String> {
        crate::mysql::monitors::regenerate_push_token(&self.pool, id, org_id).await
    }

    async fn find_monitor_by_push_token(&self, token: &str) -> DbResult<Option<MonitorId>> {
        crate::mysql::monitors::find_by_push_token(&self.pool, token).await
    }

    async fn fetch_monitor_last_push_at(&self, id: MonitorId) -> DbResult<Option<OffsetDateTime>> {
        crate::mysql::monitors::fetch_last_push_at(&self.pool, id).await
    }

    async fn set_monitor_cert_info(
        &self,
        id: MonitorId,
        days_left: i32,
        subject: &str,
    ) -> DbResult<()> {
        crate::mysql::monitors::set_cert_info(&self.pool, id, days_left, subject).await
    }

    async fn mark_monitor_run_started(&self, id: MonitorId) -> DbResult<()> {
        crate::mysql::monitors::mark_run_started(&self.pool, id).await
    }

    async fn close_monitor_run(&self, id: MonitorId) -> DbResult<Option<OffsetDateTime>> {
        crate::mysql::monitors::close_run(&self.pool, id).await
    }

    async fn monitor_push_state(
        &self,
        id: MonitorId,
    ) -> DbResult<(Option<OffsetDateTime>, Option<OffsetDateTime>)> {
        crate::mysql::monitors::push_state(&self.pool, id).await
    }

    async fn bump_monitor_push_at(&self, id: MonitorId) -> DbResult<()> {
        crate::mysql::monitors::bump_push_at(&self.pool, id).await
    }

    async fn list_monitors(&self, org_id: OrgId) -> DbResult<Vec<Monitor>> {
        crate::mysql::monitors::list(&self.pool, org_id).await
    }

    async fn list_all_monitors(&self) -> DbResult<Vec<Monitor>> {
        crate::mysql::monitors::list_all(&self.pool).await
    }

    async fn list_monitors_for_agent(&self, agent: AgentId) -> DbResult<Vec<Monitor>> {
        crate::mysql::monitors::list_for_agent(&self.pool, agent).await
    }

    async fn list_stale_agent_monitors(&self) -> DbResult<Vec<(Monitor, String)>> {
        crate::mysql::monitors::list_stale_agent_monitors(&self.pool).await
    }

    async fn get_monitor(&self, id: MonitorId, org_id: OrgId) -> DbResult<Monitor> {
        crate::mysql::monitors::get(&self.pool, id, org_id).await
    }

    async fn get_monitor_unscoped(&self, id: MonitorId) -> DbResult<Monitor> {
        crate::mysql::monitors::get_unscoped(&self.pool, id).await
    }

    async fn monitor_public_fields_batch(
        &self,
        ids: &[Uuid],
    ) -> DbResult<HashMap<Uuid, (String, MonitorStatus)>> {
        crate::mysql::monitors::public_fields_batch(&self.pool, ids).await
    }

    async fn update_monitor(
        &self,
        id: MonitorId,
        patch: UpdateMonitor,
        org_id: OrgId,
    ) -> DbResult<Monitor> {
        crate::mysql::monitors::update(&self.pool, id, patch, org_id).await
    }

    async fn delete_monitor(&self, id: MonitorId, org_id: OrgId) -> DbResult<()> {
        crate::mysql::monitors::delete(&self.pool, id, org_id).await
    }

    async fn set_monitor_active(&self, id: MonitorId, active: bool, org_id: OrgId) -> DbResult<()> {
        crate::mysql::monitors::set_active(&self.pool, id, active, org_id).await
    }

    async fn set_monitors_active_by_tag(
        &self,
        tag: TagId,
        active: bool,
        org_id: OrgId,
    ) -> DbResult<u64> {
        crate::mysql::monitors::set_active_by_tag(&self.pool, tag, active, org_id).await
    }

    async fn set_monitor_group(
        &self,
        id: MonitorId,
        group: Option<MonitorGroupId>,
        org_id: OrgId,
    ) -> DbResult<()> {
        crate::mysql::monitors::set_group(&self.pool, id, group, org_id).await
    }

    async fn bulk_edit_monitors_preview(
        &self,
        ids: &[MonitorId],
        want_tags: bool,
        org_id: OrgId,
    ) -> DbResult<(Vec<MonitorPrior>, usize)> {
        crate::mysql::monitors::bulk_edit_preview(&self.pool, ids, want_tags, org_id).await
    }

    async fn bulk_edit_monitors(
        &self,
        ids: &[MonitorId],
        patch: &BulkEditPatch,
        org_id: OrgId,
    ) -> DbResult<BulkEditOutcome> {
        crate::mysql::monitors::bulk_edit(&self.pool, ids, patch, org_id).await
    }

    async fn set_monitor_status(&self, id: MonitorId, status: MonitorStatus) -> DbResult<()> {
        crate::mysql::monitors::set_status(&self.pool, id, status).await
    }

    async fn monitor_slo_state(&self, id: MonitorId) -> DbResult<Option<SloState>> {
        crate::mysql::monitors::slo_state(&self.pool, id).await
    }

    async fn mark_monitor_slo_breached(&self, id: MonitorId) -> DbResult<()> {
        crate::mysql::monitors::mark_slo_breached(&self.pool, id).await
    }

    async fn clear_monitor_slo_breached(&self, id: MonitorId) -> DbResult<()> {
        crate::mysql::monitors::clear_slo_breached(&self.pool, id).await
    }
}

#[async_trait::async_trait]
impl StoreAudit for MysqlStore {
    async fn record_audit(&self, entry: crate::audit::NewEntry<'_>) -> DbResult<()> {
        crate::mysql::audit::insert(&self.pool, entry).await
    }

    async fn set_audit_chain_watermark(&self, id: i64, hash: &str) -> DbResult<()> {
        crate::mysql::audit::set_chain_watermark(&self.pool, id, hash).await
    }

    async fn verify_audit_chain(&self) -> DbResult<crate::audit::VerifyReport> {
        crate::mysql::audit::verify_chain(&self.pool).await
    }

    async fn audit_security_insights(
        &self,
        hours: i32,
    ) -> DbResult<crate::audit::SecurityInsights> {
        crate::mysql::audit::security_insights(&self.pool, hours).await
    }

    async fn list_audit_entries(
        &self,
        limit: i64,
        filter: crate::audit::AuditFilter<'_>,
    ) -> DbResult<Vec<crate::audit::AuditEntry>> {
        crate::mysql::audit::list(&self.pool, limit, filter).await
    }

    async fn fetch_audit_since(
        &self,
        after_id: i64,
        limit: i64,
    ) -> DbResult<Vec<crate::audit::AuditEntry>> {
        crate::mysql::audit::fetch_since(&self.pool, after_id, limit).await
    }

    async fn export_audit_batch(
        &self,
        before_id: Option<i64>,
        batch: i64,
        filter: crate::audit::ExportFilter,
    ) -> DbResult<Vec<crate::audit::ExportRow>> {
        crate::mysql::audit::export_batch(&self.pool, before_id, batch, filter).await
    }
}

#[async_trait::async_trait]
impl StoreCompliance for MysqlStore {
    async fn access_review(&self) -> DbResult<Vec<crate::access_review::AccessReviewRow>> {
        crate::mysql::access_review::list(&self.pool).await
    }
}

#[async_trait::async_trait]
impl StoreDigestBuffer for MysqlStore {
    async fn enqueue_digest(
        &self,
        notification_id: NotificationId,
        event_json: &serde_json::Value,
    ) -> DbResult<()> {
        crate::mysql::digest_buffer::enqueue(&self.pool, notification_id, event_json).await
    }

    async fn drain_due_digests(
        &self,
        now: OffsetDateTime,
    ) -> DbResult<Vec<crate::digest_buffer::DueChannel>> {
        crate::mysql::digest_buffer::drain_due(&self.pool, now).await
    }

    async fn take_digest_for_channel(
        &self,
        notification_id: NotificationId,
    ) -> DbResult<Vec<crate::digest_buffer::BufferedEvent>> {
        crate::mysql::digest_buffer::take_for_channel(&self.pool, notification_id).await
    }

    async fn delete_digest_by_ids(&self, ids: &[Uuid]) -> DbResult<()> {
        crate::mysql::digest_buffer::delete_by_ids(&self.pool, ids).await
    }
}

impl Store for MysqlStore {}

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

    /// The keystone assertion: `MysqlStore` is usable as `Arc<dyn Store>` (the
    /// super-trait is object-safe over MySQL) and delegated domains round-trip
    /// through the trait object. `#[sqlx::test(migrations=…)]` also proves the
    /// MySQL migration set applies cleanly, exercising `connect`'s migrate path.
    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn mysql_store_satisfies_dyn_store(pool: MySqlPool) {
        let store: Arc<dyn Store> = Arc::new(MysqlStore::new(pool));
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
    // (No standalone `connect()` test: it needs a live MySQL server URL, unlike
    // SQLite's in-memory DB. The migrate path is covered by the `#[sqlx::test]`
    // above and smoke-tested at boot once the `mysql:` main.rs branch lands.)
}
