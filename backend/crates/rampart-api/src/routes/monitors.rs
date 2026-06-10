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
        .route("/import-csv", post(import_csv))
        .route("/bulk", post(bulk))
        .route("/bulk-edit", post(bulk_edit))
        .route("/bulk-by-tag", post(bulk_by_tag))
        // Reusable config presets (saved header sets / TLS posture). Nested
        // under the monitors router so the one mod.rs router edit this wave
        // belongs to another feature — these land at /v1/monitors/presets.
        .route("/presets", get(list_presets).post(create_preset))
        .route("/presets/{id}", get(get_preset).delete(delete_preset))
        .route("/summary", get(summary))
        .route("/history", get(history_all))
        .route("/{id}", get(get_one).patch(update).delete(delete_one))
        .route("/{id}/heartbeats", get(heartbeats))
        .route("/{id}/heartbeats.csv", get(heartbeats_csv))
        .route("/{id}/reliability", get(reliability))
        .route("/{id}/rollups", get(rollups))
        .route("/{id}/uptime-history", get(uptime_history))
        .route("/{id}/slo/error-budget", get(slo_error_budget))
        .route("/{id}/slo/burndown", get(slo_burndown))
        .route("/{id}/pause", post(pause))
        .route("/{id}/resume", post(resume))
        .route("/{id}/clone", post(clone_one))
        .route("/{id}/regenerate-push-token", post(regenerate_push_token))
        .route("/{id}/test-now", post(test_now))
        .route("/{id}/test-notifications", post(test_notifications))
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

/// One row that couldn't be turned into a monitor — either skipped by the
/// CSV mapper (unknown kind / missing required field) or rejected by the DB
/// insert. `row` is the source name when known so the operator can find it.
#[derive(Serialize)]
struct ImportSkip {
    row: String,
    reason: String,
}

#[derive(Serialize)]
struct ImportResult {
    created: usize,
    skipped: Vec<ImportSkip>,
}

/// Bulk-import monitors from a Rampart-native CSV posted as the raw request
/// body (`text/csv` or `text/plain`). Mirrors the `rampart-import csv` CLI
/// path: parse + map via [`csv_import::parse_csv_and_map`], then create each
/// mapped monitor through the same `rampart_db::monitors::create` the API
/// `POST /monitors` uses. Rows the mapper couldn't translate, plus any that
/// fail the insert, come back in `skipped` rather than aborting the batch.
/// The 2 MiB global request-body limit caps the upload size.
async fn import_csv(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<ImportResult>, ApiError> {
    let plan = crate::importers::csv_import::parse_csv_and_map(&body)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let pool = state.pool();
    let mut created = 0usize;
    let mut skipped: Vec<ImportSkip> = plan
        .skipped
        .into_iter()
        .map(|s| ImportSkip {
            row: s.source_name,
            reason: s.reason,
        })
        .collect();

    for m in plan.mapped {
        let name = m.new_monitor.name.clone();
        match rampart_db::monitors::create(pool, m.new_monitor).await {
            Ok(_) => created += 1,
            Err(e) => skipped.push(ImportSkip {
                row: name,
                reason: e.to_string(),
            }),
        }
    }

    if created > 0 {
        state.poke_scheduler();
    }
    crate::audit::record(
        pool,
        &user,
        &headers,
        "monitor.import_csv",
        "monitor",
        None,
        Some(serde_json::json!({ "created": created, "skipped": skipped.len() })),
    )
    .await;

    Ok(Json(ImportResult { created, skipped }))
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

/// The `patch` object inside a [`BulkEditRequest`]. Every field is optional;
/// omitted fields leave that attribute untouched on every listed monitor.
/// `group_id` uses a double-option so an explicit `"group_id": null` clears
/// the group while omitting it leaves the group as-is. `tags`, when present,
/// REPLACES the monitor's full tag set (empty list clears all tags).
#[derive(Debug, Default, Deserialize)]
pub struct BulkEditPatch {
    #[serde(default)]
    interval_secs: Option<i32>,
    #[serde(default)]
    timeout_secs: Option<i32>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    group_id: Option<Option<String>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct BulkEditRequest {
    ids: Vec<String>,
    patch: BulkEditPatch,
}

#[derive(Serialize)]
struct BulkEditResult {
    /// Monitors found and mutated.
    updated: usize,
    /// Requested ids that didn't resolve to a monitor and were skipped.
    skipped: usize,
}

/// `POST /v1/monitors/bulk-edit` — apply the same partial `patch` to a list
/// of monitors in ONE transaction (all-or-nothing). Unlike the best-effort
/// [`bulk`] route, every supplied field is validated up front and the whole
/// batch is committed atomically. `interval_secs` / `timeout_secs` are range-
/// checked via the shared `UpdateMonitor` validator; `group_id` is tri-state
/// (omit → leave, `null` → clear, set → assign); `tags` replaces the full tag
/// set when present. Ids that don't exist are skipped and reported in
/// `skipped`. The scheduler is poked once so interval/enabled changes are
/// picked up by the running probes.
async fn bulk_edit(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(req): Json<BulkEditRequest>,
) -> Result<Json<BulkEditResult>, ApiError> {
    if req.ids.is_empty() {
        return Err(ApiError::BadRequest("ids is empty".into()));
    }
    if req.ids.len() > 500 {
        return Err(ApiError::BadRequest("too many monitors (max 500)".into()));
    }

    use rampart_core::ids::{MonitorGroupId, TagId};

    // Range-check interval / timeout up front via the canonical UpdateMonitor
    // validator so a bad payload fails fast for the whole batch.
    if req.patch.interval_secs.is_some() || req.patch.timeout_secs.is_some() {
        let probe: UpdateMonitor = serde_json::from_value(serde_json::json!({
            "interval_seconds": req.patch.interval_secs,
            "timeout_seconds": req.patch.timeout_secs,
        }))
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
        probe.validate()?;
    }

    // Parse the monitor ids; a malformed id is a 400 for the whole request.
    let ids: Vec<MonitorId> = req
        .ids
        .iter()
        .map(|raw| parse_monitor_id(raw))
        .collect::<Result<_, _>>()?;

    // Resolve the tri-state group: omit → leave alone, null/empty → clear,
    // set → assign that group.
    let group_id: Option<Option<MonitorGroupId>> = match &req.patch.group_id {
        None => None,
        Some(None) => Some(None),
        Some(Some(g)) if g.is_empty() => Some(None),
        Some(Some(g)) => Some(Some(
            Uuid::from_str(g)
                .map(MonitorGroupId::from_uuid)
                .map_err(|_| ApiError::BadRequest("invalid group_id".into()))?,
        )),
    };

    // Parse the tag set, when supplied.
    let tags: Option<Vec<TagId>> = match &req.patch.tags {
        None => None,
        Some(list) => Some(
            list.iter()
                .map(|t| {
                    Uuid::from_str(t)
                        .map(TagId::from_uuid)
                        .map_err(|_| ApiError::BadRequest("invalid tag_id".into()))
                })
                .collect::<Result<_, _>>()?,
        ),
    };

    let patch = rampart_db::monitors::BulkEditPatch {
        interval_seconds: req.patch.interval_secs,
        timeout_seconds: req.patch.timeout_secs,
        active: req.patch.enabled,
        group_id,
        tags,
    };

    let outcome = rampart_db::monitors::bulk_edit(state.pool(), &ids, &patch).await?;

    state.poke_scheduler();
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.bulk_edit",
        "monitor",
        None,
        Some(serde_json::json!({
            "updated": outcome.updated,
            "skipped": outcome.skipped_unknown,
            "ids": req.ids.len(),
        })),
    )
    .await;

    Ok(Json(BulkEditResult {
        updated: outcome.updated,
        skipped: outcome.skipped_unknown,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BulkByTagAction {
    Pause,
    Resume,
}

#[derive(Debug, Deserialize)]
pub struct BulkByTagRequest {
    tag_id: String,
    action: BulkByTagAction,
}

#[derive(Serialize)]
struct BulkByTagResult {
    affected: usize,
}

/// `POST /v1/monitors/bulk-by-tag` — resolve every monitor carrying the
/// given tag and pause/resume all of them in one statement. Unlike the
/// id-list `bulk` route the caller never enumerates monitors; they name a
/// tag and an action. Reuses the same `active` flip the per-monitor
/// pause/resume helpers use (`set_active_by_tag`), so the scheduler picks
/// up the change on the next poke. `affected` counts genuine transitions
/// — monitors already in the target state aren't recounted.
async fn bulk_by_tag(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(req): Json<BulkByTagRequest>,
) -> Result<Json<BulkByTagResult>, ApiError> {
    use rampart_core::ids::TagId;

    let tag = Uuid::from_str(&req.tag_id)
        .map(TagId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid tag_id".into()))?;
    let active = match req.action {
        BulkByTagAction::Pause => false,
        BulkByTagAction::Resume => true,
    };

    let affected =
        rampart_db::monitors::set_active_by_tag(state.pool(), tag, active).await? as usize;
    state.poke_scheduler();

    let action_name = if active { "resume" } else { "pause" };
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.bulk_by_tag",
        "monitor",
        None,
        Some(serde_json::json!({ "tag_id": req.tag_id, "action": action_name, "affected": affected })),
    )
    .await;

    Ok(Json(BulkByTagResult { affected }))
}

// ─── monitor presets (saved header sets / TLS posture) ──────────────────
//
// Nested under the monitors router so we don't touch routes/mod.rs this
// wave; the public paths are /v1/monitors/presets[/:id]. CRUD without
// PATCH — a preset is a small immutable bag the operator deletes + recreates
// rather than edits in place.

fn parse_preset_id(s: &str) -> Result<rampart_core::MonitorPresetId, ApiError> {
    Uuid::from_str(s)
        .map(rampart_core::MonitorPresetId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid preset id".into()))
}

async fn list_presets(
    State(state): State<AppState>,
) -> Result<Json<Vec<rampart_core::MonitorPreset>>, ApiError> {
    Ok(Json(rampart_db::monitor_presets::list(state.pool()).await?))
}

async fn get_preset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<rampart_core::MonitorPreset>, ApiError> {
    Ok(Json(
        rampart_db::monitor_presets::get(state.pool(), parse_preset_id(&id)?).await?,
    ))
}

async fn create_preset(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<rampart_core::NewMonitorPreset>,
) -> Result<(StatusCode, Json<rampart_core::MonitorPreset>), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let preset = rampart_db::monitor_presets::create(state.pool(), input).await?;
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor_preset.create",
        "monitor_preset",
        Some(preset.id.0),
        Some(serde_json::json!({ "name": preset.name, "kind": preset.kind.as_str() })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(preset)))
}

async fn delete_preset(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let preset_id = parse_preset_id(&id)?;
    rampart_db::monitor_presets::delete(state.pool(), preset_id).await?;
    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor_preset.delete",
        "monitor_preset",
        Some(preset_id.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Optional body for `POST /v1/monitors/{id}/clone`.
///
/// Both fields are optional and the whole body may be omitted entirely
/// (the legacy no-body call still works): `group_id` retargets the clone
/// into a different folder/group (validated to exist; `null` clones into
/// "ungrouped"), and `name` overrides the default "<name> (copy)" label.
#[derive(Debug, Default, Deserialize)]
pub struct CloneRequest {
    /// Tri-state: omitted → inherit the source's group; `null` → ungrouped;
    /// set → clone into this group (must exist).
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    group_id: Option<Option<String>>,
    name: Option<String>,
}

/// Distinguish "key absent" (`None`) from "key present and null"
/// (`Some(None)`) for the tri-state `group_id`. Serde's plain
/// `Option<Option<T>>` collapses both to `None`; this restores the
/// distinction so an explicit `"group_id": null` can mean "ungroup".
fn deserialize_optional_field<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(de)?))
}

/// Duplicate a monitor with the same config under a "<name> (copy)"
/// name. Heartbeat history is intentionally NOT copied — a clone is a
/// fresh probe surface, not a fork of past state. Tags, dependencies,
/// and notification channel attachments are also left empty so the
/// operator can re-wire deliberately. Push tokens are regenerated for
/// push monitors (the token has to be unique per row).
///
/// Accepts an optional JSON body (`CloneRequest`) to retarget the clone
/// into a chosen group and/or override its name.
async fn clone_one(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<CloneRequest>>,
) -> Result<(StatusCode, Json<Monitor>), ApiError> {
    use rampart_core::ids::MonitorGroupId;

    let monitor_id = parse_monitor_id(&id)?;
    let src = rampart_db::monitors::get(state.pool(), monitor_id).await?;

    let req = body.map(|Json(b)| b).unwrap_or_default();

    // Resolve the target group. Tri-state: absent → inherit source's group,
    // explicit null → ungrouped, set → that group (validated to exist).
    let group_id = match req.group_id {
        None => src.group_id,
        Some(None) => None,
        Some(Some(ref g)) if g.is_empty() => None,
        Some(Some(ref g)) => {
            let gid = Uuid::from_str(g)
                .map(MonitorGroupId::from_uuid)
                .map_err(|_| ApiError::BadRequest("invalid group_id".into()))?;
            let exists = rampart_db::monitor_groups::list(state.pool())
                .await?
                .iter()
                .any(|grp| grp.id == gid);
            if !exists {
                return Err(ApiError::BadRequest("target group does not exist".into()));
            }
            Some(gid)
        }
    };

    let name = match req.name.map(|n| n.trim().to_string()) {
        Some(n) if !n.is_empty() => n,
        _ => format!("{} (copy)", src.name),
    };

    let copy = rampart_core::monitor::NewMonitor {
        name,
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
        group_id,
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
        Some(serde_json::json!({
            "source": monitor_id.0,
            "name": cloned.name,
            "group_id": group_id.map(|g| g.0),
        })),
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

#[derive(Debug, Serialize)]
struct ChannelTestResult {
    channel_id: Uuid,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct TestNotificationsResponse {
    sent: Vec<ChannelTestResult>,
}

/// `POST /v1/monitors/:id/test-notifications` — resolve every channel
/// attached to this monitor (directly + via tag / folder routing, the
/// same resolution the alerting path uses) and fire a synthetic test
/// notification through each. Returns a per-channel ok/error list so the
/// UI can show which channels failed without aborting the whole batch.
async fn test_notifications(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TestNotificationsResponse>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    // 404 if the monitor doesn't exist.
    let _ = rampart_db::monitors::get(state.pool(), monitor_id).await?;

    let channels =
        rampart_db::routing::resolve_channels_for_monitor(state.pool(), monitor_id).await?;

    // Synthesize a fixed test payload — same shape as the per-channel
    // test endpoint so users see a recognisable "test" message.
    let now = OffsetDateTime::now_utc();
    let test_monitor = rampart_core::Monitor {
        id: rampart_core::ids::MonitorId::new(),
        name: "Rampart test monitor".into(),
        kind: rampart_core::MonitorKind::Http,
        url: Some("https://example.com".into()),
        hostname: None,
        port: None,
        config: serde_json::Value::Null,
        interval_seconds: 60,
        retry_interval_sec: 60,
        max_retries: 0,
        timeout_seconds: 10,
        resend_interval_sec: 0,
        upside_down: false,
        http_method: "GET".into(),
        http_body: None,
        http_headers: None,
        accepted_statuses: vec![200],
        follow_redirect: true,
        ignore_tls: false,
        proxy_id: None,
        push_token: None,
        last_push_at: None,
        active: true,
        current_status: rampart_core::MonitorStatus::Up,
        created_at: now,
        updated_at: now,
        tags: Vec::new(),
        cert_days_left: None,
        cert_subject: None,
        cert_checked_at: None,
        group_id: None,
        slo_target_pct: None,
        slo_window_days: None,
    };
    let test_hb = rampart_core::Heartbeat {
        monitor_id: test_monitor.id,
        ts: now,
        status: rampart_core::MonitorStatus::Up,
        latency_ms: Some(42),
        status_code: Some(200),
        msg: Some("This is a test notification from Rampart.".into()),
        retries: 0,
        important: true,
    };
    let event = rampart_notifier::Event {
        kind: rampart_notifier::EventKind::Test,
        monitor: test_monitor,
        heartbeat: test_hb,
        prev_status: Some(rampart_core::MonitorStatus::Down),
        slo_current_pct: None,
    };
    let subject = rampart_notifier::template::default_subject(&event);
    let body = rampart_notifier::template::default_body(&event);

    let mut sent = Vec::with_capacity(channels.len());
    for ch in channels {
        let result = rampart_notifier::channels::dispatch(
            ch.kind,
            &ch.config,
            &subject,
            &body,
            &event,
            state.pool(),
            ch.id,
        )
        .await;
        match result {
            Ok(()) => sent.push(ChannelTestResult {
                channel_id: ch.id.0,
                ok: true,
                error: None,
            }),
            Err(e) => sent.push(ChannelTestResult {
                channel_id: ch.id.0,
                ok: false,
                error: Some(e.to_string()),
            }),
        }
    }

    crate::audit::record(
        state.pool(),
        &user,
        &headers,
        "monitor.test_notifications",
        "monitor",
        Some(monitor_id.0),
        Some(serde_json::json!({ "channels": sent.len() })),
    )
    .await;
    Ok(Json(TestNotificationsResponse { sent }))
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

/// SLO error-budget burn-down: one point per day across the configured
/// window showing the budget depleting over time. Same 404 contract as
/// [`slo_error_budget`] — returns Not Found when `slo_target_pct` /
/// `slo_window_days` are unset, so the frontend only requests it after
/// reading the monitor row and confirming a target is configured.
#[derive(Debug, Serialize)]
pub struct BurndownDto {
    pub window_days: i32,
    pub target_pct: f64,
    pub allowed_downtime_secs: i64,
    pub points: Vec<rampart_db::heartbeats::BurndownPoint>,
}

/// Whitelisted burn-down window sizes. Same tight set + rationale as the
/// reliability card: each is a deliberate dashboard preset, and an
/// unbounded value would let a caller force a full-history walk. When the
/// param is omitted we fall back to the monitor's configured
/// `slo_window_days` to preserve the pre-toggle behaviour.
const BURNDOWN_WINDOW_DAYS_ALLOWED: &[i32] = &[7, 30, 90];

#[derive(Debug, Deserialize)]
pub struct BurndownQuery {
    /// Trailing window in days. Allowed: 7, 30, 90. Defaults to the
    /// monitor's configured `slo_window_days`.
    pub window_days: Option<i32>,
}

async fn slo_burndown(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<BurndownQuery>,
) -> Result<Json<BurndownDto>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let monitor = rampart_db::monitors::get(state.pool(), monitor_id).await?;
    let target = monitor.slo_target_pct.ok_or(ApiError::NotFound)?;
    let configured_window = monitor.slo_window_days.ok_or(ApiError::NotFound)?;
    // Resolve the requested window: explicit `?window_days=` (whitelisted)
    // or the monitor's configured window when omitted.
    let window_days = match q.window_days {
        Some(w) => {
            if !BURNDOWN_WINDOW_DAYS_ALLOWED.contains(&w) {
                return Err(ApiError::BadRequest(
                    "window_days must be one of 7, 30, 90".into(),
                ));
            }
            w
        }
        None => configured_window,
    };
    let points = rampart_db::heartbeats::error_budget_burndown(
        state.pool(),
        monitor_id,
        window_days,
        target,
    )
    .await?;
    // Same allowance formula the burn-down helper uses internally; recompute
    // here so the response header matches the point values without threading
    // it back out of the helper. Uses the resolved (requested) window, not
    // the stored one, so the allowance lines up with the returned points.
    let allowed_downtime_secs =
        ((((100.0 - target) / 100.0) * (window_days as i64 * 86_400) as f64).round()).max(0.0)
            as i64;
    Ok(Json(BurndownDto {
        window_days,
        target_pct: target,
        allowed_downtime_secs,
        points,
    }))
}

// ─── hourly rollups + long-range uptime history ─────────────────────────

/// One hourly rollup bucket, as returned by `GET /{id}/rollups`.
#[derive(Debug, Serialize)]
pub struct RollupDto {
    #[serde(with = "time::serde::rfc3339")]
    pub bucket_start: OffsetDateTime,
    pub up_count: i32,
    pub down_count: i32,
    pub other_count: i32,
    pub sample_count: i32,
    pub avg_latency_ms: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct RollupsQuery {
    /// Window start (RFC3339). Defaults to 30 days ago.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub from: Option<OffsetDateTime>,
    /// Window end (RFC3339, exclusive). Defaults to "now".
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub to: Option<OffsetDateTime>,
}

/// `GET /v1/monitors/{id}/rollups?from=&to=` — the hourly `heartbeat_rollups`
/// rows for the monitor over the half-open `[from, to)` range. Defaults to
/// the last 30 days. Read access, same RBAC as `/heartbeats` (GET handling
/// at the router layer). Rollups are retained ~1y, so this serves history
/// long past the raw-heartbeat retention horizon.
async fn rollups(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<RollupsQuery>,
) -> Result<Json<Vec<RollupDto>>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    // 404 if the monitor doesn't exist (parity with the other read routes).
    let _ = rampart_db::monitors::get(state.pool(), monitor_id).await?;

    let to = q.to.unwrap_or_else(OffsetDateTime::now_utc);
    let from = q.from.unwrap_or_else(|| to - time::Duration::days(30));
    if from >= to {
        return Err(ApiError::BadRequest("from must be before to".into()));
    }

    let rows = rampart_db::prune::rollups_for_monitor(state.pool(), monitor_id.0, from, to).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| RollupDto {
                bucket_start: r.bucket_start,
                up_count: r.up_count,
                down_count: r.down_count,
                other_count: r.other_count,
                sample_count: r.sample_count,
                avg_latency_ms: r.avg_latency_ms,
            })
            .collect(),
    ))
}

/// One day's uptime% in the long-range history series.
#[derive(Debug, Serialize)]
pub struct UptimeHistoryPoint {
    /// UTC calendar date (YYYY-MM-DD).
    pub date: String,
    pub up_count: i64,
    pub sample_count: i64,
    /// `null` when no samples were recorded that day.
    pub uptime_pct: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UptimeHistoryQuery {
    /// One of `30d`, `90d`, `1y`. Defaults to `30d`.
    pub range: Option<String>,
}

/// Whitelisted ranges → (days back). Tight set: each is a deliberate
/// dashboard preset, and an unbounded value would let a caller force a
/// full-history walk.
fn parse_uptime_range(range: Option<&str>) -> Result<i64, ApiError> {
    match range.unwrap_or("30d") {
        "30d" => Ok(30),
        "90d" => Ok(90),
        "1y" => Ok(365),
        _ => Err(ApiError::BadRequest(
            "range must be one of 30d, 90d, 1y".into(),
        )),
    }
}

/// `GET /v1/monitors/{id}/uptime-history?range=30d|90d|1y` — a daily uptime%
/// series over the trailing range, oldest first. Read access, same RBAC as
/// `/heartbeats`.
///
/// Stitching: the series is assembled from two sources so it stays correct
/// past the raw-heartbeat retention horizon. Within the raw-retention window
/// the high-resolution `heartbeats` are still present, so those days are
/// computed directly from raw (`daily_uptime_from_raw`). Older days no longer
/// have raw rows — they've been folded into the hourly `heartbeat_rollups`
/// (kept ~1y) — so those days are derived from rollups
/// (`up_count / sample_count`).
///
/// We read the raw-retention tier from settings, then split the range at that
/// boundary: rollups for `[range_start, raw_boundary)`, raw for
/// `[raw_boundary, now)`. A day is never double-counted because the prune job
/// deletes a raw heartbeat in the same transaction that folds it into a
/// rollup, so any given heartbeat lives in exactly one of the two tables. If
/// the boundary lands mid-day the two sources contribute disjoint hours of
/// that day, which the per-day map merges by summing.
async fn uptime_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<UptimeHistoryQuery>,
) -> Result<Json<Vec<UptimeHistoryPoint>>, ApiError> {
    let monitor_id = parse_monitor_id(&id)?;
    let _ = rampart_db::monitors::get(state.pool(), monitor_id).await?;

    let days = parse_uptime_range(q.range.as_deref())?;
    let now = OffsetDateTime::now_utc();
    let range_start = now - time::Duration::days(days);

    // Raw-retention boundary: heartbeats older than this have been pruned and
    // only survive as rollups. Clamp into the range so a retention window
    // wider than the request just means "all raw, no rollup tail".
    let cfg = rampart_db::prune::load_config(state.pool()).await?;
    let raw_boundary = (now - time::Duration::days(cfg.heartbeats as i64)).max(range_start);

    // Accumulate (up, samples) per UTC day across both sources.
    use std::collections::BTreeMap;
    let mut by_day: BTreeMap<time::Date, (i64, i64)> = BTreeMap::new();

    // Older portion: rollups for [range_start, raw_boundary).
    if range_start < raw_boundary {
        let pts = rampart_db::prune::daily_uptime_from_rollups(
            state.pool(),
            monitor_id.0,
            range_start,
            raw_boundary,
        )
        .await?;
        for p in pts {
            let e = by_day.entry(p.day).or_insert((0, 0));
            e.0 += p.up_count;
            e.1 += p.sample_count;
        }
    }

    // Recent portion: raw heartbeats for [raw_boundary, now).
    let pts =
        rampart_db::prune::daily_uptime_from_raw(state.pool(), monitor_id.0, raw_boundary, now)
            .await?;
    for p in pts {
        let e = by_day.entry(p.day).or_insert((0, 0));
        e.0 += p.up_count;
        e.1 += p.sample_count;
    }

    let series = by_day
        .into_iter()
        .map(|(day, (up, samples))| UptimeHistoryPoint {
            date: day.to_string(),
            up_count: up,
            sample_count: samples,
            uptime_pct: if samples > 0 {
                Some(up as f64 / samples as f64 * 100.0)
            } else {
                None
            },
        })
        .collect();
    Ok(Json(series))
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
