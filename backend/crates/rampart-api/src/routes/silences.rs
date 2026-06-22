//! Alert silences — mute notifications during a window (deploys, known noise).
//! Editor-gated (running a deploy is an operator action). A silence is global
//! (no monitor) or scoped to one monitor, with an optional expiry. The notifier
//! enforces them at its dispatch chokepoint.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use rampart_db::silences::{NewSilence, Silence};
use rampart_db::users::User;
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::delete(remove))
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<Silence>>, ApiError> {
    Ok(Json(s.store().list_active_silences(org.org_id).await?))
}

#[derive(Deserialize)]
struct NewSilenceInput {
    /// None = global (mute everything).
    monitor_id: Option<Uuid>,
    #[serde(default)]
    reason: String,
    /// Minutes from now; absent / 0 = until manually removed.
    duration_minutes: Option<i64>,
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Json(input): Json<NewSilenceInput>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let expires_at = input
        .duration_minutes
        .filter(|m| *m > 0)
        .map(|m| OffsetDateTime::now_utc() + time::Duration::minutes(m));
    let id = s
        .store()
        .create_silence(
            NewSilence {
                monitor_id: input.monitor_id,
                reason: input.reason.trim(),
                created_by: Some(user.id.0),
                expires_at,
            },
            org.org_id,
        )
        .await?;
    crate::audit::record(
        s.store(),
        &user,
        &headers,
        "silence.create",
        "silence",
        Some(id),
        Some(serde_json::json!({ "monitor_id": input.monitor_id, "reason": input.reason })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id }))))
}

async fn remove(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if !s.store().delete_silence(id, org.org_id).await? {
        return Err(ApiError::NotFound);
    }
    crate::audit::record(
        s.store(),
        &user,
        &headers,
        "silence.delete",
        "silence",
        Some(id),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
