//! `POST /push/:token` — public endpoint for push (cron-job) monitors.
//!
//! Push monitors invert the usual probe direction: the external job
//! calls *us* on a heartbeat. The token in the URL identifies which
//! monitor to credit; it's a 24-character random string assigned when
//! the monitor is created (unique-indexed in Postgres).
//!
//! Cronitor-style run states (path suffix or `?state=`):
//!
//!   POST /push/:token/run       — the job just started. Opens a run;
//!                                 records no heartbeat (a start is not a
//!                                 health sample and must not skew uptime).
//!   POST /push/:token/complete  — the job finished OK. Closes the run;
//!                                 the heartbeat's latency_ms carries the
//!                                 run's wall-clock duration when a /run
//!                                 ping opened it.
//!   POST /push/:token/fail      — the job finished broken. Closes the
//!                                 run, records Down, and pages through
//!                                 the regular notification pipeline.
//!
//! The bare form stays exactly as before for dead-man's-switch users:
//!
//!   ?status=up|down|warn   default "up" ("up" ≙ complete, "down" ≙ fail)
//!   ?msg=Hello             free-text written into the heartbeat
//!   ?ping=42               latency in ms (overrides a computed duration)
//!
//! All pings flow through the shared external-ingest path, so a fail (or
//! legacy `?status=down`) flips the monitor and notifies immediately —
//! it no longer waits for (or gets overwritten by) the scheduler tick.
//! Schedule expectations (`config.cron` + grace, `config.max_run_seconds`)
//! are enforced by the scheduler's push tick, not here.
//!
//! Returns 200 with a one-line confirmation, or 404 if the token doesn't
//! match a monitor (we don't leak which one is which via timing — both
//! paths run one query).

use crate::error::ApiError;
use crate::external_ingest::{ingest, ExternalResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;
use rampart_core::MonitorStatus;
use serde::Deserialize;
use time::OffsetDateTime;

pub fn router() -> Router<AppState> {
    // Accept both POST and GET — cron/curl snippets commonly use GET;
    // POST is more correct for "I'm asserting state". Both work.
    Router::new()
        .route("/{token}", post(push_bare).get(push_bare))
        .route("/{token}/{state}", post(push_state).get(push_state))
}

#[derive(Debug, Deserialize)]
pub struct PushParams {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub ping: Option<i32>,
}

/// What a ping asserts, after folding the legacy `status` vocabulary onto
/// the run-state one.
enum Ping {
    Run,
    Complete,
    Fail,
    Warn,
}

impl Ping {
    fn parse(state: Option<&str>, status: Option<&str>) -> Result<Ping, ApiError> {
        // Path/`state` vocabulary wins when both are sent.
        if let Some(s) = state {
            return match s {
                "run" => Ok(Ping::Run),
                "complete" => Ok(Ping::Complete),
                "fail" => Ok(Ping::Fail),
                _ => Err(ApiError::BadRequest(
                    "state must be one of run / complete / fail".into(),
                )),
            };
        }
        match status.unwrap_or("up") {
            "up" => Ok(Ping::Complete),
            "down" => Ok(Ping::Fail),
            "warn" => Ok(Ping::Warn),
            _ => Err(ApiError::BadRequest(
                "status must be one of up / down / warn".into(),
            )),
        }
    }
}

async fn push_bare(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(p): Query<PushParams>,
) -> Result<(StatusCode, &'static str), ApiError> {
    let ping = Ping::parse(p.state.as_deref(), p.status.as_deref())?;
    apply(&state, &token, ping, p.msg, p.ping).await
}

async fn push_state(
    State(state): State<AppState>,
    Path((token, run_state)): Path<(String, String)>,
    Query(p): Query<PushParams>,
) -> Result<(StatusCode, &'static str), ApiError> {
    let ping = Ping::parse(Some(run_state.as_str()), None)?;
    apply(&state, &token, ping, p.msg, p.ping).await
}

async fn apply(
    state: &AppState,
    token: &str,
    ping: Ping,
    msg: Option<String>,
    explicit_ping_ms: Option<i32>,
) -> Result<(StatusCode, &'static str), ApiError> {
    let monitor_id = state
        .store()
        .find_monitor_by_push_token(token)
        .await?
        .ok_or(ApiError::NotFound)?;

    // A run-start opens the duration clock and records nothing else — a
    // start is not a health sample, and counting it would skew uptime.
    if matches!(ping, Ping::Run) {
        state.store().mark_monitor_run_started(monitor_id).await?;
        return Ok((StatusCode::OK, "ok run"));
    }

    // Terminal ping: stamp liveness, close any open run, and get its
    // start back for the duration.
    let run_started = state.store().close_monitor_run(monitor_id).await?;
    let duration_ms: Option<i32> = explicit_ping_ms.or_else(|| {
        run_started.map(|t| {
            ((OffsetDateTime::now_utc() - t).whole_milliseconds()).clamp(0, i32::MAX as i128) as i32
        })
    });

    let (status, default_msg): (MonitorStatus, &str) = match ping {
        Ping::Complete => (MonitorStatus::Up, "run complete"),
        Ping::Fail => (MonitorStatus::Down, "run failed"),
        Ping::Warn => (MonitorStatus::Warn, "push received"),
        Ping::Run => unreachable!("handled above"),
    };

    // Flip detection needs the full row (current_status). The push token (not
    // a session) authenticated this request and already resolved monitor_id,
    // so fetch unscoped.
    let monitor = state.store().get_monitor_unscoped(monitor_id).await?;
    ingest(
        state,
        &monitor,
        ExternalResult {
            status,
            latency_ms: duration_ms,
            status_code: None,
            msg: msg.or_else(|| Some(default_msg.into())),
            retries: 0,
        },
    )
    .await?;

    Ok((StatusCode::OK, "ok"))
}
