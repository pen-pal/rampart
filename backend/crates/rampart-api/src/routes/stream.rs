//! Server-Sent Events: live heartbeat fan-out.
//!
//! `GET /v1/stream/heartbeats` subscribes the caller to the scheduler's
//! broadcast channel. Each heartbeat the writer persists is emitted as
//! an SSE `data:` line containing the JSON-serialized [`Heartbeat`].
//!
//! The broadcast channel is **process-global** — it carries every org's
//! heartbeats — so the handler filters it to the caller's org before emitting.
//! Without that filter any authenticated session (Readonly included) could read
//! every tenant's live probe results (status, latency, error text).
//!
//! Why SSE and not WebSockets?
//! - One-way fan-out, no client → server traffic.
//! - Works through every HTTP reverse proxy without an Upgrade dance.
//! - Auto-reconnect is built into the browser (`EventSource`), so the
//!   client doesn't need to reimplement backoff.
//!
//! Lagging consumers receive `RecvError::Lagged(n)` instead of stalling
//! the writer. We log + emit a `lag` event so the dashboard can decide
//! whether to silently catch up or trigger a poll-based refresh.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::Router;
use futures::stream::Stream;
use rampart_core::ids::MonitorId;
use rampart_core::Heartbeat;
use std::collections::HashSet;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_stream::StreamExt;
use tracing::warn;

pub fn router() -> Router<AppState> {
    Router::new().route("/heartbeats", get(heartbeats))
}

async fn heartbeats(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let rx = s.subscribe_heartbeats().ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "live heartbeat stream unavailable (scheduler not wired into AppState)"
        ))
    })?;

    // Snapshot the caller's org's monitor ids and emit only their heartbeats —
    // the broadcast carries every tenant's. ponytail: this is a snapshot taken
    // at subscribe; a monitor created afterwards starts streaming on the
    // EventSource's next reconnect (browsers auto-reconnect, and the dashboard
    // re-subscribes when the monitor set changes) — kept a plain set so the
    // per-message hot path is a single hash lookup with no DB hit or lock.
    let allowed: HashSet<MonitorId> = s
        .store()
        .list_monitors(org.org_id)
        .await?
        .into_iter()
        .map(|m| m.id)
        .collect();

    let stream =
        BroadcastStream::new(rx).filter_map(move |msg| filter_event(&allowed, msg).map(Ok));

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

/// Decide what (if anything) a subscriber whose org owns `allowed` should
/// receive for one broadcast message. A heartbeat for a monitor outside the set
/// is dropped (the cross-tenant filter); a lag notice is always surfaced.
/// Factored out so the isolation boundary is unit-testable without standing up
/// the scheduler broadcast.
fn filter_event(
    allowed: &HashSet<MonitorId>,
    msg: Result<Heartbeat, BroadcastStreamRecvError>,
) -> Option<Event> {
    match msg {
        Ok(hb) if allowed.contains(&hb.monitor_id) => Some(
            Event::default()
                .event("heartbeat")
                .json_data(&hb)
                .unwrap_or_else(|e| {
                    warn!(error = %e, "heartbeat serialize failed");
                    Event::default().event("error").data("serialize-failed")
                }),
        ),
        // Another tenant's heartbeat — never emit.
        Ok(_) => None,
        Err(BroadcastStreamRecvError::Lagged(n)) => {
            warn!(skipped = n, "sse subscriber lagged");
            Some(Event::default().event("lag").data(n.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::MonitorStatus;
    use time::OffsetDateTime;

    fn hb(monitor_id: MonitorId) -> Heartbeat {
        Heartbeat {
            monitor_id,
            ts: OffsetDateTime::now_utc(),
            status: MonitorStatus::Up,
            latency_ms: Some(5),
            status_code: Some(200),
            msg: None,
            retries: 0,
            important: false,
        }
    }

    /// SECURITY (audit re-rank #2): a heartbeat for a monitor outside the
    /// subscriber's org is dropped; the subscriber's own is emitted; a lag notice
    /// always passes through.
    #[test]
    fn drops_foreign_org_heartbeats() {
        let mine = MonitorId::new();
        let theirs = MonitorId::new();
        let allowed: HashSet<MonitorId> = [mine].into_iter().collect();

        assert!(
            filter_event(&allowed, Ok(hb(mine))).is_some(),
            "own-org heartbeat is emitted"
        );
        assert!(
            filter_event(&allowed, Ok(hb(theirs))).is_none(),
            "another tenant's heartbeat is never emitted"
        );
        assert!(
            filter_event(&allowed, Err(BroadcastStreamRecvError::Lagged(7))).is_some(),
            "lag notice still surfaced"
        );
    }
}
