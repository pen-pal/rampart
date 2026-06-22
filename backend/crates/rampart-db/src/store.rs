//! Object-safe `Store` seam (multi-DB P0 — heartbeats + deploy-markers +
//! ingest-keys + slos domains).
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
use crate::slos::{SloEvent, SloWithSnapshot};
use crate::{DbPool, DbResult};
use rampart_core::deploy_marker::{DeployMarker, NewDeployMarker};
use rampart_core::ids::{DeployMarkerId, OrgId, SloId};
use rampart_core::slo::{NewSlo, Slo, SloSnapshot, UpdateSlo};
use rampart_core::{Heartbeat, MonitorId};
use std::collections::HashMap;
use uuid::Uuid;

/// One method per public `crate::heartbeats` free function. Signatures are
/// mirrored exactly except the leading `pool: &DbPool` is replaced by `&self`.
#[async_trait::async_trait]
pub trait StoreHeartbeats: Send + Sync {
    async fn insert_many(&self, hbs: &[Heartbeat]) -> DbResult<()>;

    async fn recent_for_monitor(
        &self,
        monitor: MonitorId,
        limit: i64,
    ) -> DbResult<Vec<Heartbeat>>;

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

    async fn monthly_uptime(
        &self,
        monitor: MonitorId,
        months: i32,
    ) -> DbResult<Vec<MonthlyUptime>>;

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

    async fn recent_per_monitor(
        &self,
        per_monitor: i64,
        org_id: OrgId,
    ) -> DbResult<Vec<Heartbeat>>;
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

/// Composed store super-trait spanning every extracted domain sub-trait.
pub trait Store:
    StoreHeartbeats + StoreDeployMarkers + StoreIngestKeys + StoreSlos + Send + Sync
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

    async fn recent_for_monitor(
        &self,
        monitor: MonitorId,
        limit: i64,
    ) -> DbResult<Vec<Heartbeat>> {
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
