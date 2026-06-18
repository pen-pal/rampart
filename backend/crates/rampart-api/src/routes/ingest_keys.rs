//! `/v1/ingest-keys` — per-org telemetry ingest credentials (multi-tenancy P5).
//!
//! Operates on the caller's ACTIVE org (`OrgContext`) and is admin-gated: a key
//! grants OTLP / Prometheus remote_write / RUM / profiles ingest INTO that org,
//! so minting one is an admin act. Mounted in the `admin_only` subtree, whose
//! `require_admin` resolves to the caller's role in the active org (Phase 4e) —
//! an org-admin therefore manages only their own org's keys.
//!
//! Create returns the raw token exactly once. `list` only ever exposes
//! metadata; the token never leaves the server again.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use rampart_db::ingest_keys::IngestKey;
use rampart_db::users::User;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::delete(delete))
}

/// Body for `POST /v1/ingest-keys`.
#[derive(Debug, Deserialize)]
struct NewIngestKey {
    label: String,
    /// One of `all` / `otlp` / `prometheus` / `rum` / `profiles`. Defaults to
    /// `all` when omitted.
    #[serde(default)]
    kind: Option<String>,
    /// Origins allowed to present this key (RUM origin-binding). Optional.
    #[serde(default)]
    allowed_origins: Option<Vec<String>>,
}

/// `create` response — the key metadata plus the plaintext token, shown ONCE.
#[derive(Debug, Serialize)]
struct IssuedIngestKey {
    #[serde(flatten)]
    key: IngestKey,
    token: String,
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<IngestKey>>, ApiError> {
    Ok(Json(
        rampart_db::ingest_keys::list_for_org(s.pool(), org.org_id).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Json(input): Json<NewIngestKey>,
) -> Result<(StatusCode, Json<IssuedIngestKey>), ApiError> {
    let label = input.label.trim();
    if label.is_empty() {
        return Err(ApiError::BadRequest("label is required".into()));
    }
    let kind = input.kind.as_deref().map(str::trim).unwrap_or("");
    let kind = if kind.is_empty() { "all" } else { kind };
    let origins = input.allowed_origins.unwrap_or_default();

    let (key, token) =
        rampart_db::ingest_keys::create(s.pool(), org.org_id, label, kind, &origins).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "ingest_key.create",
        "ingest_key",
        Some(key.id),
        Some(serde_json::json!({ "label": label, "kind": kind })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(IssuedIngestKey { key, token })))
}

async fn delete(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Extension(org): Extension<OrgContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = Uuid::from_str(&id).map_err(|_| ApiError::BadRequest("invalid ingest key id".into()))?;
    // Org-scoped delete: a cross-org id is invisible, so 404 (not 403).
    let removed = rampart_db::ingest_keys::delete(s.pool(), id, org.org_id).await?;
    if !removed {
        return Err(ApiError::NotFound);
    }
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "ingest_key.delete",
        "ingest_key",
        Some(id),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
