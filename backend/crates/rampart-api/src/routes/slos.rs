//! SLO CRUD + live budget snapshots (`/v1/slos`).
//!
//! Management surface for the SLO tier; the scheduler evaluates budgets and
//! pages on its tick (`rampart_db::slos::evaluate_tick`). `list` enriches each
//! row with a freshly computed snapshot (achieved %, budget remaining, 1h burn
//! rate) so the UI renders without a second round-trip. Mounted in the editor
//! slice — editors manage SLOs, readonly users GET.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use rampart_core::ids::SloId;
use rampart_core::slo::{NewSlo, Slo, SloSnapshot, UpdateSlo};
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", axum::routing::patch(update).delete(delete_slo))
}

/// An SLO row plus its computed health, flattened so the client sees one object.
#[derive(Serialize)]
struct SloView {
    #[serde(flatten)]
    slo: Slo,
    snapshot: SloSnapshot,
    /// Achieved-ratio trend (oldest → newest) for a sparkline.
    trend: Vec<f64>,
}

fn parse_id(s: &str) -> Result<SloId, ApiError> {
    Uuid::from_str(s)
        .map(SloId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid slo id".into()))
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<Json<Vec<SloView>>, ApiError> {
    let rows = rampart_db::slos::list_with_snapshots(s.pool(), org.org_id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| SloView {
                slo: r.slo,
                snapshot: r.snapshot,
                trend: r.trend,
            })
            .collect(),
    ))
}

async fn create(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Json(input): Json<NewSlo>,
) -> Result<(StatusCode, Json<Slo>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    validate_indicator(&input)?;
    let slo = rampart_db::slos::create(s.pool(), input, org.org_id).await?;
    Ok((StatusCode::CREATED, Json(slo)))
}

async fn update(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
    Json(input): Json<UpdateSlo>,
) -> Result<Json<Slo>, ApiError> {
    let slo_id = parse_id(&id)?;
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(
        rampart_db::slos::update(s.pool(), slo_id, input, org.org_id).await?,
    ))
}

async fn delete_slo(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    rampart_db::slos::delete(s.pool(), parse_id(&id)?, org.org_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The DB CHECK constraints enforce indicator completeness too, but a friendly
/// 400 beats a 500 from a constraint violation.
fn validate_indicator(input: &NewSlo) -> Result<(), ApiError> {
    use rampart_core::slo::SliKind;
    match input.sli_kind {
        SliKind::Monitor if input.monitor_id.is_none() => {
            Err(ApiError::BadRequest("monitor SLI needs monitor_id".into()))
        }
        SliKind::Metric if input.good_metric.is_none() || input.total_metric.is_none() => Err(
            ApiError::BadRequest("metric SLI needs good_metric and total_metric".into()),
        ),
        _ => Ok(()),
    }
}
