//! Admin-only read API over the notification delivery log.
//!
//! Lists recent channel send attempts (success + failure) recorded by the
//! notifier. Keyset-paginated by `sent_at`, newest-first — the same shape
//! as the audit-log list route.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rampart_db::delivery_log::DeliveryEntry;
use serde::Deserialize;
use time::OffsetDateTime;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    /// Keyset cursor: return rows strictly older than this `sent_at`
    /// (RFC3339). Omit for the first (newest) page.
    #[serde(default, with = "time::serde::rfc3339::option")]
    before: Option<OffsetDateTime>,
}

fn default_limit() -> i64 {
    100
}

async fn list(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<DeliveryEntry>>, ApiError> {
    Ok(Json(
        rampart_db::delivery_log::list(s.pool(), q.limit, q.before).await?,
    ))
}
