//! `/v1/scheduled-reports` routes.
//!
//! Admin CRUD for scheduled weekly uptime reports. A report is a named set
//! of email recipients that receives a per-monitor 7-day uptime digest on
//! a cadence (weekly today). The scheduler's slow-tick check renders +
//! sends due reports via the system SMTP sender; these routes only manage
//! the rows. Admin-gated via the `require_admin` layer in routes/mod.rs.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::scheduled_report::ScheduledReport;
use rampart_core::ScheduledReportId;
use rampart_db::scheduled_reports::{NewScheduledReport, UpdateScheduledReport};
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).patch(update).delete(remove))
        .route("/{id}/send", axum::routing::post(send_now))
}

fn parse(id: &str) -> Result<ScheduledReportId, ApiError> {
    Uuid::from_str(id)
        .map(ScheduledReportId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid scheduled report id".into()))
}

/// Cadences the scheduler knows how to schedule. Stored as free text in the
/// `cadence` column, so the only guard against typos is this route-layer
/// allow-list — an unknown value would otherwise silently fall back to the
/// weekly window forever.
const ALLOWED_CADENCES: [&str; 3] = ["daily", "weekly", "monthly"];

fn validate_cadence(cadence: &str) -> Result<(), ApiError> {
    if ALLOWED_CADENCES.contains(&cadence) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "invalid cadence '{cadence}' (expected one of: daily, weekly, monthly)"
        )))
    }
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<ScheduledReport>>, ApiError> {
    Ok(Json(
        rampart_db::scheduled_reports::list(s.pool(), org.org_id).await?,
    ))
}

async fn get_one(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<ScheduledReport>, ApiError> {
    Ok(Json(
        rampart_db::scheduled_reports::get(s.pool(), parse(&id)?, org.org_id).await?,
    ))
}

async fn create(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Json(input): Json<NewScheduledReport>,
) -> Result<(StatusCode, Json<ScheduledReport>), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    validate_cadence(&input.cadence)?;
    let r = rampart_db::scheduled_reports::create(s.pool(), input, org.org_id).await?;
    Ok((StatusCode::CREATED, Json(r)))
}

async fn update(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
    Json(input): Json<UpdateScheduledReport>,
) -> Result<Json<ScheduledReport>, ApiError> {
    if let Some(cadence) = input.cadence.as_deref() {
        validate_cadence(cadence)?;
    }
    Ok(Json(
        rampart_db::scheduled_reports::update(s.pool(), parse(&id)?, input, org.org_id).await?,
    ))
}

async fn remove(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::scheduled_reports::delete(s.pool(), parse(&id)?, org.org_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/scheduled-reports/:id/send` — render the report's uptime digest
/// and email it to its recipients right now, then stamp `last_sent_at` so the
/// manual send also resets the cadence clock. Reuses the same render + system
/// SMTP path the scheduler's slow-tick check uses. Admin-gated like the rest
/// of this router. Returns the (possibly mutated) report row.
async fn send_now(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<Json<ScheduledReport>, ApiError> {
    let id = parse(&id)?;
    let report = rampart_db::scheduled_reports::get(s.pool(), id, org.org_id).await?;

    if report.recipients.is_empty() {
        return Err(ApiError::BadRequest(
            "report has no recipients to send to".into(),
        ));
    }

    let (subject, body) =
        rampart_db::scheduled_reports::render(s.pool(), &report.name, &report.cadence).await?;
    rampart_notifier::send_system_email(s.pool(), &report.recipients, &subject, &body).await;

    // Stamp regardless of how many recipients SMTP actually reached: a
    // manual send is still a send, and per-recipient failures are logged
    // inside send_system_email. Return the refreshed row so the caller sees
    // the new last_sent_at.
    rampart_db::scheduled_reports::mark_sent(s.pool(), id).await?;
    Ok(Json(
        rampart_db::scheduled_reports::get(s.pool(), id, org.org_id).await?,
    ))
}
