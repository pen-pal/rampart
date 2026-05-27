//! Admin-only read API over the audit log.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rampart_db::audit::AuditEntry;
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_limit")]
    limit:  i64,
    before: Option<i64>,
    kind:   Option<String>,
}
fn default_limit() -> i64 { 100 }

async fn list(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<AuditEntry>>, ApiError> {
    Ok(Json(
        rampart_db::audit::list(s.pool(), q.limit, q.before, q.kind.as_deref()).await?,
    ))
}
