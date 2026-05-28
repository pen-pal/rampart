//! Server-Sent Events: live heartbeat fan-out.
//!
//! `GET /v1/stream/heartbeats` subscribes the caller to the scheduler's
//! broadcast channel. Each heartbeat the writer persists is emitted as
//! an SSE `data:` line containing the JSON-serialized [`Heartbeat`].
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

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::Router;
use futures::stream::Stream;
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
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let rx = s.subscribe_heartbeats().ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "live heartbeat stream unavailable (scheduler not wired into AppState)"
        ))
    })?;

    let stream = BroadcastStream::new(rx).map(|msg| {
        let ev = match msg {
            Ok(hb) => Event::default()
                .event("heartbeat")
                .json_data(&hb)
                .unwrap_or_else(|e| {
                    warn!(error = %e, "heartbeat serialize failed");
                    Event::default().event("error").data("serialize-failed")
                }),
            Err(BroadcastStreamRecvError::Lagged(n)) => {
                warn!(skipped = n, "sse subscriber lagged");
                Event::default().event("lag").data(n.to_string())
            }
        };
        Ok(ev)
    });

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}
