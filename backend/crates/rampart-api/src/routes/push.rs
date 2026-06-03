//! `POST /push/:token` — public endpoint for push monitors.
//!
//! Push monitors invert the usual probe direction: the external job
//! calls *us* on a heartbeat. The token in the URL identifies which
//! monitor to credit; it's a 24-character random string assigned when
//! the monitor is created (unique-indexed in Postgres).
//!
//! Optional query params:
//!   ?status=up|down   default "up"
//!   ?msg=Hello        free-text written into the heartbeat
//!   ?ping=42          latency in ms recorded with the heartbeat
//!
//! Returns 200 with a one-line confirmation, or 404 if the token doesn't
//! match a monitor (we don't leak which one is which via timing — both
//! paths run one query).

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;
use rampart_core::{Heartbeat, MonitorStatus};
use serde::Deserialize;
use time::OffsetDateTime;

pub fn router() -> Router<AppState> {
    // Accept both POST and GET — cron/curl snippets commonly use GET;
    // POST is more correct for "I'm asserting state". Both work.
    Router::new().route("/{token}", post(push).get(push))
}

#[derive(Debug, Deserialize)]
pub struct PushParams {
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub ping: Option<i32>,
}
fn default_status() -> String {
    "up".into()
}

async fn push(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(p): Query<PushParams>,
) -> Result<(StatusCode, &'static str), ApiError> {
    let monitor_id = rampart_db::monitors::find_by_push_token(state.pool(), &token)
        .await?
        .ok_or(ApiError::NotFound)?;

    let status = match p.status.as_str() {
        "up" => MonitorStatus::Up,
        "down" => MonitorStatus::Down,
        "warn" => MonitorStatus::Warn,
        _ => {
            return Err(ApiError::BadRequest(
                "status must be one of up / down / warn".into(),
            ))
        }
    };

    // Write the heartbeat + bump last_push_at. Both go through the writer
    // path the scheduler uses, so dashboard reads see them consistently.
    let hb = Heartbeat {
        monitor_id,
        ts: OffsetDateTime::now_utc(),
        status,
        latency_ms: p.ping,
        status_code: None,
        msg: p.msg.or_else(|| Some("push received".into())),
        retries: 0,
        important: false,
    };
    rampart_db::heartbeats::insert_many(state.pool(), std::slice::from_ref(&hb)).await?;
    rampart_db::monitors::bump_push_at(state.pool(), monitor_id).await?;
    // Push heartbeats are direct assertions from an external job, so the
    // monitor's current_status should reflect the latest payload — not
    // the "important flip only" semantics used by scheduler heartbeats.
    // Without this, push monitors never leave their default state and
    // the dashboard reads them as down forever.
    rampart_db::monitors::set_status(state.pool(), monitor_id, status).await?;

    Ok((StatusCode::OK, "ok"))
}
