//! Admin-only read API over the audit log.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rampart_db::audit::{AuditEntry, AuditFilter};
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    before: Option<i64>,
    kind: Option<String>,
    /// Prefix match on action, e.g. "monitor." or "monitor.delete".
    action: Option<String>,
    /// Filter to a single actor user id.
    actor: Option<String>,
}
fn default_limit() -> i64 {
    100
}

async fn list(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<AuditEntry>>, ApiError> {
    let actor = match q.actor.as_deref() {
        Some(a) if !a.is_empty() => {
            Some(Uuid::from_str(a).map_err(|_| ApiError::BadRequest("invalid actor id".into()))?)
        }
        _ => None,
    };
    let action_prefix = q.action.as_deref().filter(|s| !s.is_empty());
    let kind = q.kind.as_deref().filter(|s| !s.is_empty());
    let filter = AuditFilter {
        before_id: q.before,
        kind,
        action_prefix,
        actor,
    };
    Ok(Json(
        rampart_db::audit::list(s.pool(), q.limit, filter).await?,
    ))
}
