//! Incidents — status-page announcements.
//!
//! Two mounting points:
//!
//! - `/v1/status-pages/:page_id/incidents` (page-scoped)
//!   GET → list (all, including resolved)
//!   POST → create + return
//! - `/v1/incidents/:id` (top-level operations on a single incident)
//!   PATCH → update title / content / style / pinned
//!   DELETE → remove (and cascade its updates)
//!   POST /resolve → mark resolved
//!   GET  /updates → list running updates
//!   POST /updates → append running update

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::{IncidentId, StatusPageId};
use rampart_core::{Incident, IncidentUpdate};
use rampart_db::incidents::{NewIncident, UpdateIncident};
use rampart_db::users::User;
use serde::Deserialize;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn page_router() -> Router<AppState> {
    Router::new().route("/{page_id}/incidents", get(list_for_page).post(create))
}

pub fn incident_router() -> Router<AppState> {
    Router::new()
        .route("/recent", get(recent))
        .route("/{id}", axum::routing::patch(update).delete(delete_one))
        .route("/{id}/resolve", post(resolve))
        .route("/{id}/updates", get(list_updates).post(post_update))
}

async fn recent(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<rampart_core::incident::Incident>>, ApiError> {
    Ok(Json(
        rampart_db::incidents::recent(s.pool(), 10, org.org_id).await?,
    ))
}

fn parse_page(s: &str) -> Result<StatusPageId, ApiError> {
    Uuid::from_str(s)
        .map(StatusPageId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid status page id".into()))
}
fn parse_incident(s: &str) -> Result<IncidentId, ApiError> {
    Uuid::from_str(s)
        .map(IncidentId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid incident id".into()))
}

async fn list_for_page(
    State(s): State<AppState>,
    Path(page): Path<String>,
) -> Result<Json<Vec<Incident>>, ApiError> {
    Ok(Json(
        rampart_db::incidents::list_all(s.pool(), parse_page(&page)?, 500).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Path(page): Path<String>,
    Extension(user): Extension<User>,
    Json(input): Json<NewIncident>,
) -> Result<(StatusCode, Json<Incident>), ApiError> {
    if input.title.trim().is_empty() {
        return Err(ApiError::BadRequest("title is required".into()));
    }
    let page_id = parse_page(&page)?;
    let i = rampart_db::incidents::create(s.pool(), page_id, Some(user.id), input).await?;
    // Best-effort subscriber fan-out — failures are logged, not surfaced.
    fan_out_incident(s.clone(), page_id, i.clone(), None);
    Ok((StatusCode::CREATED, Json(i)))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<UpdateIncident>,
) -> Result<Json<Incident>, ApiError> {
    Ok(Json(
        rampart_db::incidents::update(s.pool(), parse_incident(&id)?, patch).await?,
    ))
}

async fn delete_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::incidents::delete(s.pool(), parse_incident(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resolve(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::incidents::resolve(s.pool(), parse_incident(&id)?, OffsetDateTime::now_utc())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_updates(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<IncidentUpdate>>, ApiError> {
    Ok(Json(
        rampart_db::incidents::list_updates(s.pool(), parse_incident(&id)?).await?,
    ))
}

#[derive(Deserialize)]
struct UpdateBody {
    message: String,
}

async fn post_update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Extension(user): Extension<User>,
    Json(body): Json<UpdateBody>,
) -> Result<StatusCode, ApiError> {
    if body.message.trim().is_empty() {
        return Err(ApiError::BadRequest("message is required".into()));
    }
    let incident_id = parse_incident(&id)?;
    rampart_db::incidents::post_update(s.pool(), incident_id, Some(user.id), body.message.clone())
        .await?;
    let inc = rampart_db::incidents::get(s.pool(), incident_id).await?;
    fan_out_incident(s.clone(), inc.status_page_id, inc, Some(body.message));
    Ok(StatusCode::CREATED)
}

/// Spawn a background task that emails confirmed subscribers about a
/// status-page incident (or running update). Failures are logged inside
/// the task — we never block the request on SMTP.
fn fan_out_incident(
    state: AppState,
    page: StatusPageId,
    incident: Incident,
    update_message: Option<String>,
) {
    tokio::spawn(async move {
        let cfg = match crate::smtp::load(state.pool()).await {
            Ok(Some(c)) => c,
            Ok(None) => return, // no SMTP configured — silent no-op
            Err(e) => {
                tracing::warn!("smtp config load: {e}");
                return;
            }
        };
        // Background fan-out (spawned, no request context); the page was
        // org-checked when the incident was created, so fetch unscoped.
        let page_row = match rampart_db::status_pages::get_unscoped(state.pool(), page).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("status page lookup: {e}");
                return;
            }
        };
        let emails =
            match rampart_db::subscribers::confirmed_emails_for_page(state.pool(), page).await {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("subscriber lookup: {e}");
                    return;
                }
            };
        if emails.is_empty() {
            return;
        }

        let subject = match &update_message {
            None => format!("[{}] {}", page_row.title, incident.title),
            Some(_) => format!("[{}] Update: {}", page_row.title, incident.title),
        };
        let body = match &update_message {
            None => format!(
                "{}\n\n{}\n\n— {}\n",
                incident.title, incident.content, page_row.title,
            ),
            Some(msg) => format!(
                "New update on {}\n\n{}\n\n— {}\n",
                incident.title, msg, page_row.title,
            ),
        };

        for addr in emails {
            if let Err(e) = crate::smtp::send(&cfg, &addr, &subject, &body).await {
                tracing::warn!(recipient = %addr, error = %e, "subscriber email failed");
            }
        }
    });
}
