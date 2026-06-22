//! Object-safe `Store` seam (multi-DB P0 — heartbeats + deploy-markers +
//! ingest-keys + slos + proxies + on-call + recovery-codes + api-keys +
//! escalations + maintenance + ingest-tokens + tags + templates +
//! telemetry-rules + metric-rules + monitor-groups + silences + oidc-state
//! domains).
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

use crate::heartbeats::{BurndownPoint, ErrorBudget, MonitorSummary, MonthlyUptime, MtbfMttr};
use crate::ingest_keys::IngestKey;
use crate::maintenance::MaintenanceTransition;
use crate::metric_rules::RuleEvent as MetricRuleEvent;
use crate::oidc_state::Consumed;
use crate::silences::{NewSilence, Silence};
use crate::slos::{SloEvent, SloWithSnapshot};
use crate::tags::TagUsage;
use crate::telemetry_rules::RuleEvent as TelemetryRuleEvent;
use crate::templates::{NewTemplate, RenderedTemplate, Template, UpdateTemplate};
use crate::{DbPool, DbResult};
use rampart_core::api_key::{ApiKey, IssuedApiKey, NewApiKey};
use rampart_core::deploy_marker::{DeployMarker, NewDeployMarker};
use rampart_core::escalation::{
    EscalationEpisode, EscalationPolicy, NewEscalationPolicy, UpdateEscalationPolicy,
};
use rampart_core::ids::{
    ApiKeyId, DeployMarkerId, EscalationPolicyId, IngestTokenId, MaintenanceId, MetricRuleId,
    MonitorGroupId, NotificationId, NotificationTemplateId, OnCallScheduleId, OrgId, SloId,
    StatusPageId, TagId, TelemetryRuleId,
};
use rampart_core::ingest_token::{IngestToken, NewIngestToken};
use rampart_core::maintenance::{MaintenanceWindow, NewMaintenanceWindow, UpdateMaintenanceWindow};
use rampart_core::metric_rule::{MetricRule, NewMetricRule, UpdateMetricRule};
use rampart_core::monitor_group::{MonitorGroup, NewMonitorGroup, UpdateMonitorGroup};
use rampart_core::on_call::{
    NewOnCallSchedule, OnCallSchedule, OnCallTarget, UpdateOnCallSchedule,
};
use rampart_core::proxy::{NewProxy, Proxy};
use rampart_core::slo::{NewSlo, Slo, SloSnapshot, UpdateSlo};
use rampart_core::status_page::PublicMaintenance;
use rampart_core::tag::{NewTag, Tag, TagBrief, UpdateTag};
use rampart_core::telemetry_rule::{NewTelemetryRule, TelemetryRule, UpdateTelemetryRule};
use rampart_core::{Heartbeat, MonitorId, ProxyId, UserId};
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
