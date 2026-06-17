//! Remote probe agents.
//!
//! Two surfaces in one module:
//!
//! - Operator management under `/v1/agents`:
//!   GET    /v1/agents          — list (editor+; feeds the assignment picker)
//!   POST   /v1/agents          — register, returns the raw token ONCE (admin)
//!   PATCH  /v1/agents/{id}     — rename / relocate (admin)
//!   DELETE /v1/agents/{id}     — revoke; monitors fall back to local probing (admin)
//!
//! - The agent-facing wire protocol under `/v1/agent`, authenticated by
//!   `Authorization: Bearer rmpa_…` (NOT a session — agents are headless):
//!   GET  /v1/agent/monitors    — pull the full specs of assigned monitors
//!   POST /v1/agent/heartbeats  — report a batch of probe results
//!
//! The agent always dials out, so it works from behind NAT / firewalls
//! with zero inbound connectivity. Reported heartbeats are fed through
//! the scheduler's writer pipeline (`submit_external`), which gives them
//! the identical downstream path a local probe has: batched insert, SSE
//! broadcast, current_status bounce, SLO breach detection, and result
//! webhooks. Flip detection happens here against `monitors.current_status`
//! since there is no local probe task holding in-memory state for these.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, patch};
use axum::{Json, Router};
use rampart_core::agent::{Agent, AgentResult, IssuedAgent, NewAgent, UpdateAgent};
use rampart_core::ids::AgentId;
use rampart_core::{Monitor, MonitorKind};
use rampart_db::users::User;
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

/// Hard cap on one report batch. An agent batches ~1s of results, so even
/// a thousand-monitor agent stays far below this; anything larger is a
/// runaway client.
const MAX_BATCH: usize = 1000;

/// Editor-readable slice: the list feeds the probe-location picker in the
/// monitor wizard / edit modal, which editors can use. Names + locations
/// only — no token material exists to leak (only hashes are stored).
pub fn read_router() -> Router<AppState> {
    Router::new().route("/", get(list))
}

/// Admin-only slice: registration (token mint), rename, revoke.
pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::post(create))
        .route("/{id}", patch(update).delete(revoke))
}

/// Token-authed agent wire protocol. Mounted under `/v1/agent` in the
/// public slice — the bearer token IS the auth, like `/push/{token}`.
pub fn agent_router() -> Router<AppState> {
    Router::new()
        .route("/monitors", get(pull_monitors))
        .route("/heartbeats", axum::routing::post(report_heartbeats))
        .route("/metrics", axum::routing::post(report_metrics))
}

fn parse(s: &str) -> Result<AgentId, ApiError> {
    Uuid::from_str(s)
        .map(AgentId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid agent id".into()))
}

// ---- operator management --------------------------------------------------

async fn list(State(s): State<AppState>) -> Result<Json<Vec<Agent>>, ApiError> {
    Ok(Json(rampart_db::agents::list(s.pool()).await?))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<NewAgent>,
) -> Result<(StatusCode, Json<IssuedAgent>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let name = input.name.clone();
    let issued = rampart_db::agents::create(s.pool(), input).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "agent.create",
        "agent",
        Some(issued.agent.id.0),
        Some(serde_json::json!({ "name": name })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(issued)))
}

async fn update(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateAgent>,
) -> Result<Json<Agent>, ApiError> {
    let agent_id = parse(&id)?;
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let agent = rampart_db::agents::update(s.pool(), agent_id, input).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "agent.update",
        "agent",
        Some(agent_id.0),
        None,
    )
    .await;
    Ok(Json(agent))
}

async fn revoke(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let agent_id = parse(&id)?;
    rampart_db::agents::delete(s.pool(), agent_id).await?;
    // ON DELETE SET NULL just returned this agent's monitors to local
    // probing — poke so the scheduler starts their tasks immediately
    // instead of on the 30s fallback tick.
    s.poke_scheduler();
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "agent.revoke",
        "agent",
        Some(agent_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---- agent wire protocol ---------------------------------------------------

/// Resolve the bearer token to an agent or 401. No timing side-channel
/// beyond the unique-index lookup, same as API keys.
async fn authenticate(s: &AppState, headers: &HeaderMap) -> Result<Agent, ApiError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;
    rampart_db::agents::lookup(s.pool(), token)
        .await
        .map_err(|_| ApiError::Unauthorized)
}

/// `GET /v1/agent/monitors` — the assignment pull. Bumps `last_seen_at`
/// (+ self-reported `X-Rampart-Agent-Version`) so the dashboard's
/// online badge and the stale-agent watchdog track liveness off the
/// poll itself.
async fn pull_monitors(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Monitor>>, ApiError> {
    let agent = authenticate(&s, &headers).await?;
    let version = headers
        .get("x-rampart-agent-version")
        .and_then(|v| v.to_str().ok());
    // Liveness bump is best-effort; the pull itself matters more.
    let _ = rampart_db::agents::touch_seen(s.pool(), agent.id, version).await;
    Ok(Json(
        rampart_db::monitors::list_for_agent(s.pool(), agent.id).await?,
    ))
}

#[derive(Serialize)]
struct RejectedResult {
    monitor_id: Uuid,
    reason: String,
}

#[derive(Serialize)]
struct ReportOutcome {
    accepted: usize,
    rejected: Vec<RejectedResult>,
}

/// `POST /v1/agent/heartbeats` — batch result ingestion. Each entry is
/// validated independently (the monitor must exist and be assigned to
/// the reporting agent); one bad row never sinks the batch. The server
/// stamps receive time rather than trusting agent clocks.
async fn report_heartbeats(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(results): Json<Vec<AgentResult>>,
) -> Result<Json<ReportOutcome>, ApiError> {
    let agent = authenticate(&s, &headers).await?;
    if results.len() > MAX_BATCH {
        return Err(ApiError::BadRequest(format!(
            "batch too large: {} results (max {MAX_BATCH})",
            results.len()
        )));
    }
    let _ = rampart_db::agents::touch_seen(s.pool(), agent.id, None).await;

    let mut accepted = 0usize;
    let mut rejected: Vec<RejectedResult> = Vec::new();

    for r in results {
        // Agent reporting heartbeats for monitors it was assigned; the agent
        // credential is the authority, so resolve the monitor unscoped.
        let monitor = match rampart_db::monitors::get_unscoped(s.pool(), r.monitor_id).await {
            Ok(m) => m,
            Err(_) => {
                rejected.push(RejectedResult {
                    monitor_id: r.monitor_id.0,
                    reason: "unknown monitor".into(),
                });
                continue;
            }
        };
        if monitor.agent_id != Some(agent.id) {
            rejected.push(RejectedResult {
                monitor_id: r.monitor_id.0,
                reason: "monitor is not assigned to this agent".into(),
            });
            continue;
        }
        // Push monitors are inbound-only; a result for one is a confused
        // client. (Assignment is also blocked at create/update.)
        if monitor.kind == MonitorKind::Push {
            rejected.push(RejectedResult {
                monitor_id: r.monitor_id.0,
                reason: "push monitors cannot be agent-probed".into(),
            });
            continue;
        }

        ingest(&s, &monitor, r).await?;
        accepted += 1;
    }

    Ok(Json(ReportOutcome { accepted, rejected }))
}

/// Apply one agent-reported result through the shared external-ingest
/// path (maintenance suppression, flip detection, notifier fan-out,
/// writer pipeline).
async fn ingest(s: &AppState, monitor: &Monitor, r: AgentResult) -> Result<(), ApiError> {
    crate::external_ingest::ingest(
        s,
        monitor,
        crate::external_ingest::ExternalResult {
            status: r.status,
            latency_ms: r.latency_ms,
            status_code: r.status_code,
            msg: r.msg,
            retries: r.retries,
        },
    )
    .await
}

#[derive(Serialize)]
struct MetricsOutcome {
    accepted: usize,
    skipped: usize,
}

/// `POST /v1/agent/metrics` — host/custom metrics from an agent, in
/// Prometheus text format. Every sample gets an `agent="<name>"` label
/// injected server-side before storage, so dashboards and threshold
/// rules can tell hosts apart without the agent ever knowing its own
/// name (renames apply retroactively to nothing — old samples keep the
/// old label, which is the honest reading of history). Bumps liveness
/// like any other agent call.
async fn report_metrics(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<MetricsOutcome>, ApiError> {
    let agent = authenticate(&s, &headers).await?;
    let _ = rampart_db::agents::touch_seen(s.pool(), agent.id, None).await;

    let mut parsed = rampart_core::promtext::parse(&body);
    if parsed.samples.is_empty() && parsed.skipped > 0 {
        return Err(ApiError::BadRequest(
            "no parseable samples in payload (expected Prometheus text format)".into(),
        ));
    }
    for sample in &mut parsed.samples {
        sample
            .labels
            .insert("agent".to_string(), agent.name.clone());
    }
    rampart_db::metric_samples::insert_many(s.pool(), &parsed.samples).await?;
    Ok(Json(MetricsOutcome {
        accepted: parsed.samples.len(),
        skipped: parsed.skipped,
    }))
}
