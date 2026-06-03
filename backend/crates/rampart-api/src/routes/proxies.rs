//! `/v1/proxies` — list / create / delete / toggle active.
//!
//! Read responses redact the password (Proxy.password has `#[serde(skip_serializing)]`).
//! Probe runtime gets the full row via the dedicated `rampart_db::proxies::get` path.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::proxy::{NewProxy, Proxy};
use rampart_core::ProxyId;
use rampart_db::users::User;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/:id", axum::routing::delete(remove))
        .route("/:id/active", post(set_active))
}

fn parse(s: &str) -> Result<ProxyId, ApiError> {
    Uuid::from_str(s)
        .map(ProxyId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid proxy id".into()))
}

async fn list(State(s): State<AppState>) -> Result<Json<Vec<Proxy>>, ApiError> {
    Ok(Json(rampart_db::proxies::list(s.pool()).await?))
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<NewProxy>,
) -> Result<(StatusCode, Json<Proxy>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    // Enum-style protocol check upfront so the user gets a friendly 400
    // instead of a Postgres CHECK violation surfacing as a 500.
    if rampart_core::proxy::ProxyProtocol::parse(&input.protocol).is_none() {
        return Err(ApiError::BadRequest(
            "protocol must be one of http / https / socks / socks5 / socks4".into(),
        ));
    }
    let p = rampart_db::proxies::create(s.pool(), input).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "proxy.create",
        "proxy",
        Some(p.id.0),
        Some(serde_json::json!({ "protocol": p.protocol, "host": p.host, "port": p.port })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(p)))
}

async fn remove(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let pid = parse(&id)?;
    rampart_db::proxies::delete(s.pool(), pid).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "proxy.delete",
        "proxy",
        Some(pid.0),
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
struct SetActiveBody {
    active: bool,
}

async fn set_active(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetActiveBody>,
) -> Result<StatusCode, ApiError> {
    let pid = parse(&id)?;
    rampart_db::proxies::set_active(s.pool(), pid, body.active).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "proxy.set_active",
        "proxy",
        Some(pid.0),
        Some(serde_json::json!({ "active": body.active })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
