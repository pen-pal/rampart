//! Shared ingestion for externally-produced heartbeats — agent reports
//! and push-monitor pings. One place owns the semantics a local probe
//! tick gets for free from the scheduler:
//!
//! - maintenance suppression (recorded as Maintenance, never alerts)
//! - flip detection against `monitors.current_status` (no local probe
//!   task holds in-memory state for these monitors)
//! - notifier fan-out on user-visible flips
//! - delivery through the scheduler's writer pipeline (batched insert,
//!   SSE broadcast, current_status bounce, SLO checks, result webhooks)
//!
//! Test harnesses build `AppState` without a scheduler; that path falls
//! back to a direct insert so the contract stays testable (no SSE / SLO
//! / webhook side-effects, mirroring the pre-agent push route).

use crate::error::ApiError;
use crate::state::AppState;
use rampart_core::{Heartbeat, Monitor, MonitorStatus};
use time::OffsetDateTime;

pub struct ExternalResult {
    pub status: MonitorStatus,
    pub latency_ms: Option<i32>,
    pub status_code: Option<i32>,
    pub msg: Option<String>,
    pub retries: i32,
}

/// Apply one externally-reported result. The server stamps receive time —
/// remote clocks are never trusted.
pub async fn ingest(
    state: &AppState,
    monitor: &Monitor,
    r: ExternalResult,
) -> Result<(), ApiError> {
    let in_maintenance = state
        .store()
        .is_in_active_window(monitor.id)
        .await
        .unwrap_or(false);

    let mut hb = if in_maintenance {
        Heartbeat {
            monitor_id: monitor.id,
            ts: OffsetDateTime::now_utc(),
            status: MonitorStatus::Maintenance,
            latency_ms: None,
            status_code: None,
            msg: Some("in maintenance".into()),
            retries: 0,
            important: false,
        }
    } else {
        Heartbeat {
            monitor_id: monitor.id,
            ts: OffsetDateTime::now_utc(),
            status: r.status,
            latency_ms: r.latency_ms,
            status_code: r.status_code,
            msg: r.msg,
            retries: r.retries,
            important: false,
        }
    };

    let prev = monitor.current_status;
    if prev != hb.status {
        hb.important = true;
        // Same visibility rule as the scheduler: initialisation
        // (Pending→Up) and admin-driven maintenance transitions don't
        // page anyone.
        let user_visible = !(prev == MonitorStatus::Pending && hb.status == MonitorStatus::Up)
            && hb.status != MonitorStatus::Maintenance
            && prev != MonitorStatus::Maintenance;
        if user_visible {
            if let Some(sched) = state.scheduler() {
                sched.notify_status_flip(monitor.clone(), hb.clone(), prev);
            }
        }
    }

    match state.scheduler() {
        Some(sched) => {
            if !sched.submit_external(hb).await {
                return Err(ApiError::Conflict("heartbeat writer unavailable".into()));
            }
        }
        None => {
            rampart_db::heartbeats::insert_many(state.pool(), std::slice::from_ref(&hb)).await?;
            if hb.important {
                state
                    .store()
                    .set_monitor_status(monitor.id, hb.status)
                    .await?;
            }
        }
    }
    Ok(())
}
