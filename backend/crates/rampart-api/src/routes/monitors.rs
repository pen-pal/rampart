//! `/v1/monitors` routes.
//!
//! Single-tenant: no workspace scoping. Authentication is a TODO —
//! the scaffold passes through. Add a session/JWT extractor before
//! exposing this to the internet.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::monitor::{NewMonitor, UpdateMonitor};
use rampart_core::{Heartbeat, Monitor, MonitorId, MonitorStatus};
use rampart_db::users::User;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    // Static segments must be declared before the `:id` route so axum
    // matches them before treating the segment as an id.
    Router::new()
        .route("/", get(list).post(create))
        .route("/bulk", post(bulk))
        .route("/summary", get(summary))
        .route("/history", get(history_all))
        .route("/{id}", get(get_one).patch(update).delete(delete_one))
        .route("/{id}/heartbeats", get(heartbeats))
        .route("/{id}/heartbeats.csv", get(heartbeats_csv))
        .route("/{id}/reliability", get(reliability))
        .route("/{id}/slo/error-budget", get(slo_error_budget))
        .route("/{id}/pause", post(pause))
        .route("/{id}/resume", post(resume))
        .route("/{id}/clone", post(clone_one))
        .route("/{id}/regenerate-push-token", post(regenerate_push_token))
        .route("/{id}/test-now", post(test_now))
}

fn parse_monitor_id(s: &str) -> Result<MonitorId, ApiError> {
    Uuid::from_str(s)
        .map(MonitorId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid monitor id".into()))
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<Monitor>>, ApiError> {
    let monitors = rampart_db::monitors::list(state.pool()).await?;
    Ok(Json(monitors))
}

async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<NewMonitor>,
) -> Result<(StatusCode, Json<Monitor>), ApiError> {
    input.validate()?;
    let monitor = rampart_db::monitors::create(state.pool(), input).await?;
    state.poke_scheduler();
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.create",
        "monitor",
        Some(monitor.id.0),
        Some(serde_json::json!({ "name": monitor.name, "kind": monitor.kind })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(monitor)))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Monitor>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let monitor = rampart_db::monitors::get(state.pool(), monitor_id).await?;
    Ok(Json(monitor))
}

async fn delete_one(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    rampart_db::monitors::delete(state.pool(), monitor_id).await?;
    state.poke_scheduler();
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.delete",
        "monitor",
        Some(monitor_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn update(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateMonitor>,
) -> Result<Json<Monitor>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    input.validate()?;
    let monitor = rampart_db::monitors::update(state.pool(), monitor_id, input).await?;
    // Interval / url / proxy_id changes need the running probe task to
    // pick up the new config — poke triggers a reload diff.
    state.poke_scheduler();
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.update",
        "monitor",
        Some(monitor_id.0),
        None,
    )
    .await;
    Ok(Json(monitor))
}

async fn pause(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    rampart_db::monitors::set_active(state.pool(), monitor_id, false).await?;
    state.poke_scheduler();
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.pause",
        "monitor",
        Some(monitor_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn resume(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    rampart_db::monitors::set_active(state.pool(), monitor_id, true).await?;
    state.poke_scheduler();
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.resume",
        "monitor",
        Some(monitor_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
enum BulkAction {
    Pause,
    Resume,
    Delete,
    SetGroup { group_id: Option<String> },
    AddTag { tag_id: String },
    RemoveTag { tag_id: String },
    AttachChannel { notification_id: String },
    DetachChannel { notification_id: String },
}

#[derive(Debug, Deserialize)]
pub struct BulkRequest {
    monitor_ids: Vec<String>,
    #[serde(flatten)]
    action: BulkAction,
}

#[derive(Serialize)]
struct BulkResult {
    ok: usize,
    failed: usize,
}

/// Apply one action to many monitors in a single call. Best-effort: a
/// failure on one id doesn't abort the rest; the response reports counts.
/// The scheduler is poked once at the end rather than per-monitor.
async fn bulk(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(req): Json<BulkRequest>,
) -> Result<Json<BulkResult>, ApiError> {
    if req.monitor_ids.is_empty() {
        return Err(ApiError::BadRequest("monitor_ids is empty".into()));
    }
    if req.monitor_ids.len() > 500 {
        return Err(ApiError::BadRequest("too many monitors (max 500)".into()));
    }

    use rampart_core::ids::{MonitorGroupId, NotificationId, TagId};
    // Resolve action-level params once before the loop.
    let group: Option<Option<MonitorGroupId>> = match &req.action {
        BulkAction::SetGroup { group_id } => Some(match group_id.as_deref() {
            Some(g) if !g.is_empty() => Some(
                Uuid::from_str(g)
                    .map(MonitorGroupId::from_uuid)
                    .map_err(|_| ApiError::BadRequest("invalid group_id".into()))?,
            ),
            _ => None,
        }),
        _ => None,
    };
    let tag: Option<TagId> = match &req.action {
        BulkAction::AddTag { tag_id } | BulkAction::RemoveTag { tag_id } => Some(
            Uuid::from_str(tag_id)
                .map(TagId::from_uuid)
                .map_err(|_| ApiError::BadRequest("invalid tag_id".into()))?,
        ),
        _ => None,
    };
    let notif: Option<NotificationId> = match &req.action {
        BulkAction::AttachChannel { notification_id }
        | BulkAction::DetachChannel { notification_id } => Some(
            Uuid::from_str(notification_id)
                .map(NotificationId::from_uuid)
                .map_err(|_| ApiError::BadRequest("invalid notification_id".into()))?,
        ),
        _ => None,
    };

    let pool = state.pool();
    let mut ok = 0usize;
    let mut failed = 0usize;
    for raw in &req.monitor_ids {
        let mid = match parse_monitor_id(raw) {
            Ok(m) => m,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        let res = match &req.action {
            BulkAction::Pause => rampart_db::monitors::set_active(pool, mid, false).await,
            BulkAction::Resume => rampart_db::monitors::set_active(pool, mid, true).await,
            BulkAction::Delete => rampart_db::monitors::delete(pool, mid).await,
            BulkAction::SetGroup { .. } => {
                rampart_db::monitors::set_group(pool, mid, group.flatten()).await
            }
            BulkAction::AddTag { .. } => rampart_db::tags::attach(pool, mid, tag.unwrap()).await,
            BulkAction::RemoveTag { .. } => rampart_db::tags::detach(pool, mid, tag.unwrap()).await,
            BulkAction::AttachChannel { .. } => {
                rampart_db::notifications::attach(pool, mid, notif.unwrap()).await
            }
            BulkAction::DetachChannel { .. } => {
                rampart_db::notifications::detach(pool, mid, notif.unwrap()).await
            }
        };
        match res {
            Ok(()) => ok += 1,
            Err(_) => failed += 1,
        }
    }

    state.poke_scheduler();
    let action_name = match &req.action {
        BulkAction::Pause => "pause",
        BulkAction::Resume => "resume",
        BulkAction::Delete => "delete",
        BulkAction::SetGroup { .. } => "set_group",
        BulkAction::AddTag { .. } => "add_tag",
        BulkAction::RemoveTag { .. } => "remove_tag",
        BulkAction::AttachChannel { .. } => "attach_channel",
        BulkAction::DetachChannel { .. } => "detach_channel",
    };
    crate::audit::record(
        pool,
        &user,
        &headers,
        "monitor.bulk",
        "monitor",
        None,
        Some(serde_json::json!({ "action": action_name, "ok": ok, "failed": failed })),
    )
    .await;

    Ok(Json(BulkResult { ok, failed }))
}

/// Duplicate a monitor with the same config under a "<name> (copy)"
/// name. Heartbeat history is intentionally NOT copied — a clone is a
/// fresh probe surface, not a fork of past state. Tags, dependencies,
/// and notification channel attachments are also left empty so the
/// operator can re-wire deliberately. Push tokens are regenerated for
/// push monitors (the token has to be unique per row).
async fn clone_one(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Monitor>), ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let src = rampart_db::monitors::get(state.pool(), monitor_id).await?;
    let copy = rampart_core::monitor::NewMonitor {
        name: format!("{} (copy)", src.name),
        kind: src.kind,
        url: src.url.clone(),
        hostname: src.hostname.clone(),
        port: src.port,
        config: src.config.clone(),
        interval_seconds: src.interval_seconds,
        retry_interval_sec: src.retry_interval_sec,
        max_retries: src.max_retries,
        timeout_seconds: src.timeout_seconds,
        resend_interval_sec: src.resend_interval_sec,
        upside_down: src.upside_down,
        http_method: src.http_method.clone(),
        http_body: src.http_body.clone(),
        http_headers: src.http_headers.clone(),
        accepted_statuses: src.accepted_statuses.clone(),
        follow_redirect: src.follow_redirect,
        ignore_tls: src.ignore_tls,
        proxy_id: src.proxy_id,
        group_id: src.group_id,
        slo_target_pct: src.slo_target_pct,
        slo_window_days: src.slo_window_days,
    };
    let cloned = rampart_db::monitors::create(state.pool(), copy).await?;
    state.poke_scheduler();
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.clone",
        "monitor",
        Some(cloned.id.0),
        Some(serde_json::json!({ "source": monitor_id.0, "name": cloned.name })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(cloned)))
}

/// Run a one-shot probe right now, write the resulting heartbeat as if
/// it were a scheduled one, and return it to the caller. Used by the UI
/// "Test now" button so operators can verify a freshly-edited monitor
/// without waiting up to `interval_seconds` for the scheduler's next
/// tick. Push monitors don't have a real probe — they receive instead
/// of send — so they're rejected with a 400.
async fn test_now(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<rampart_core::Heartbeat>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let monitor = rampart_db::monitors::get(state.pool(), monitor_id).await?;
    if monitor.kind == rampart_core::MonitorKind::Push {
        return Err(ApiError::BadRequest(
            "push monitors can't be probed from the server — they receive heartbeats, not send them".into(),
        ));
    }

    // Run the probe synchronously. If the monitor has a proxy, route
    // through it the same way the scheduler does.
    let probes = rampart_checker::Probes::new();
    let hb = if let Some(pid) = monitor.proxy_id {
        match rampart_db::proxies::get(state.pool(), pid).await {
            Ok(proxy) => probes.http_with_proxy(&monitor, &proxy).await,
            // Proxy reference dangling — fall back to direct probe so the
            // test still completes; the surfaced status will show the
            // misconfiguration via msg.
            Err(_) => probes.run(&monitor).await,
        }
    } else {
        probes.run(&monitor).await
    };

    // Persist the heartbeat + bump current_status to match — same shape
    // as a scheduled tick that flipped status.
    rampart_db::heartbeats::insert_many(state.pool(), std::slice::from_ref(&hb)).await?;
    if hb.status != monitor.current_status {
        rampart_db::monitors::set_status(state.pool(), monitor_id, hb.status).await?;
    }
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.test_now",
        "monitor",
        Some(monitor_id.0),
        Some(serde_json::json!({ "status": hb.status })),
    )
    .await;
    Ok(Json(hb))
}

/// Rotate a push monitor's token. Used when the existing token leaks or
/// is suspected of being compromised — any caller still holding the old
/// token starts getting 404s immediately.
async fn regenerate_push_token(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let token = rampart_db::monitors::regenerate_push_token(state.pool(), monitor_id).await?;
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.regenerate_push_token",
        "monitor",
        Some(monitor_id.0),
        // Don't log the new token itself; the previous one is already
        // invalidated and an attacker who has the audit log already has
        // bigger problems, but no need to make it worse.
        None,
    )
    .await;
    Ok(Json(serde_json::json!({ "push_token": token })))
}

#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    /// Rollup window in seconds. Default 24h.
    #[serde(default = "default_window")]
    pub window: i64,
}
fn default_window() -> i64 {
    86_400
}

#[derive(Debug, Serialize)]
pub struct MonitorSummaryDto {
    pub monitor_id: MonitorId,
    pub total: i64,
    pub up: i64,
    pub uptime_pct: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    pub last_status: Option<MonitorStatus>,
    pub last_ts: Option<OffsetDateTime>,
}

async fn summary(
    State(state): State<AppState>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<Vec<MonitorSummaryDto>>, ApiError> {
    let rows = rampart_db::heartbeats::summary_window(state.pool(), q.window).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| MonitorSummaryDto {
                monitor_id: r.monitor_id,
                total: r.total,
                up: r.up,
                uptime_pct: if r.total > 0 {
                    Some(r.up as f64 / r.total as f64 * 100.0)
                } else {
                    None
                },
                avg_latency_ms: r.avg_latency_ms,
                last_status: r.last_status,
                last_ts: r.last_ts,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// How many heartbeats per monitor. Default 60 (the dashboard strip).
    #[serde(default = "default_history_per")]
    pub per: i64,
}
fn default_history_per() -> i64 {
    60
}

async fn history_all(
    State(state): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<Heartbeat>>, ApiError> {
    let per = q.per.clamp(1, 500);
    let hbs = rampart_db::heartbeats::recent_per_monitor(state.pool(), per).await?;
    Ok(Json(hbs))
}

#[derive(Debug, Deserialize)]
pub struct HeartbeatsQuery {
    /// Max rows. Default 100. Clamped to 2000.
    #[serde(default = "default_hb_limit")]
    pub limit: i64,
    /// Cursor — return heartbeats strictly older than this RFC3339 ts.
    /// Frontend's "Load more" sets it to the oldest already-loaded ts.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub before: Option<OffsetDateTime>,
}
fn default_hb_limit() -> i64 {
    100
}

/// Reliability rollup — MTBF / MTTR + downtime event count over the
/// trailing `window_days` (7 / 30 / 90). Defaults to 30 when the
/// `?window_days=` query param is omitted so older clients (and the
/// shipped widget before the toggle landed) keep their current
/// behaviour.
#[derive(Debug, Serialize)]
pub struct ReliabilityDto {
    pub window_days: i64,
    pub mtbf_secs: Option<i64>,
    pub mttr_secs: Option<i64>,
    pub downtime_events: i64,
}

const RELIABILITY_WINDOW_DAYS_DEFAULT: i64 = 30;
/// Whitelisted window sizes. Anything else returns 400. We keep this a
/// tight set because each option is a deliberate dashboard preset and
/// because broader / unbounded values would let a caller force a
/// full-history walk on monitors with years of heartbeats.
const RELIABILITY_WINDOW_DAYS_ALLOWED: &[i64] = &[7, 30, 90];

#[derive(Debug, Deserialize)]
pub struct ReliabilityQuery {
    /// Trailing window in days. Allowed: 7, 30, 90. Defaults to 30.
    pub window_days: Option<i64>,
}

async fn reliability(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ReliabilityQuery>,
) -> Result<Json<ReliabilityDto>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let window_days = q.window_days.unwrap_or(RELIABILITY_WINDOW_DAYS_DEFAULT);
    if !RELIABILITY_WINDOW_DAYS_ALLOWED.contains(&window_days) {
        return Err(ApiError::BadRequest(
            "window_days must be one of 7, 30, 90".into(),
        ));
    }
    let window_secs = window_days * 86_400;
    let r = rampart_db::heartbeats::mtbf_mttr(state.pool(), monitor_id, window_secs).await?;
    Ok(Json(ReliabilityDto {
        window_days,
        mtbf_secs: r.mtbf_secs,
        mttr_secs: r.mttr_secs,
        downtime_events: r.downtime_events,
    }))
}

/// SLO error-budget for the monitor's configured window. Returns 404
/// when either `slo_target_pct` or `slo_window_days` is unset — the
/// frontend reads the monitor row first to decide whether to render
/// the fuel gauge, so an unconfigured monitor never reaches this
/// endpoint in normal use.
async fn slo_error_budget(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<rampart_db::heartbeats::ErrorBudget>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let monitor = rampart_db::monitors::get(state.pool(), monitor_id).await?;
    let target = monitor.slo_target_pct.ok_or(ApiError::NotFound)?;
    let window_days = monitor.slo_window_days.ok_or(ApiError::NotFound)?;
    let budget =
        rampart_db::heartbeats::error_budget(state.pool(), monitor_id, window_days, target).await?;
    Ok(Json(budget))
}

async fn heartbeats(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HeartbeatsQuery>,
) -> Result<Json<Vec<Heartbeat>>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let limit = q.limit.clamp(1, 2000);
    let hbs = rampart_db::heartbeats::recent_for_monitor_before(
        state.pool(),
        monitor_id,
        limit,
        q.before,
    )
    .await?;
    Ok(Json(hbs))
}

#[derive(Debug, Deserialize)]
pub struct CsvQuery {
    /// Window start (RFC3339). Defaults to 30 days ago.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub since: Option<OffsetDateTime>,
    /// Window end (RFC3339, exclusive). Defaults to "now".
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub until: Option<OffsetDateTime>,
}

/// CSV export of a monitor's heartbeats in `[since, until)`. Hard-capped
/// at 50_000 rows so a runaway export can't blow up memory; operators
/// wanting more should narrow the window.
async fn heartbeats_csv(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<CsvQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let until = q.until.unwrap_or_else(OffsetDateTime::now_utc);
    let since = q.since.unwrap_or_else(|| until - time::Duration::days(30));
    if since >= until {
        return Err(ApiError::BadRequest("since must be before until".into()));
    }
    let hbs =
        rampart_db::heartbeats::range_for_monitor(state.pool(), monitor_id, since, until, 50_000)
            .await?;

    // RFC3339 timestamps; CSV-escape only `msg` (everything else is
    // numeric / enum / boolean and can't contain commas or quotes).
    let mut body = String::with_capacity(64 + hbs.len() * 80);
    body.push_str("ts,status,latency_ms,status_code,retries,important,msg\n");
    let fmt = time::format_description::well_known::Rfc3339;
    for h in &hbs {
        let ts = h.ts.format(&fmt).unwrap_or_default();
        let status = match h.status {
            MonitorStatus::Up => "up",
            MonitorStatus::Down => "down",
            MonitorStatus::Warn => "warn",
            MonitorStatus::Paused => "paused",
            MonitorStatus::Pending => "pending",
            MonitorStatus::Maintenance => "maintenance",
        };
        body.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            ts,
            status,
            h.latency_ms.map(|v| v.to_string()).unwrap_or_default(),
            h.status_code.map(|v| v.to_string()).unwrap_or_default(),
            h.retries,
            h.important,
            csv_escape(h.msg.as_deref().unwrap_or("")),
        ));
    }
    let filename = format!("heartbeats-{}.csv", monitor_id.0);
    Ok((
        [
            ("content-type", "text/csv; charset=utf-8".to_string()),
            (
                "content-disposition",
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    ))
}

/// Minimal CSV escape: double the embedded quotes and wrap in quotes if
/// the field contains a comma, quote, or newline.
fn csv_escape(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let needs = s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r');
    if !needs {
        return s.to_string();
    }
    let escaped = s.replace('"', "\"\"");
    format!("\"{escaped}\"")
}
