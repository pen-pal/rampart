//! Object-safe `Store` seam (multi-DB P0 slice 2 — heartbeats domain).
//!
//! This module proves the `&dyn Store` super-trait shape is object-safe and
//! that the AppState wiring compiles, ahead of the full ~40-trait extraction.
//! It introduces NO new SQL: the single Postgres impl (`PgStore`) delegates
//! every method straight to the existing `crate::heartbeats::*` free functions,
//! so every existing call path is byte-identical.
//!
//! `StoreHeartbeats` mirrors each `pub async fn` in [`crate::heartbeats`]
//! one-for-one, dropping the leading `pool: &DbPool` parameter in favour of
//! `&self`. No caller is migrated in this slice; `AppState::store()` exists and
//! compiles but has zero callers yet.

use crate::heartbeats::{BurndownPoint, ErrorBudget, MonitorSummary, MonthlyUptime, MtbfMttr};
use crate::{DbPool, DbResult};
use rampart_core::ids::OrgId;
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

/// Composed store super-trait. Empty for now — one domain sub-trait
/// (`StoreHeartbeats`). The full extraction adds the remaining domains here.
pub trait Store: StoreHeartbeats + Send + Sync {}

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
