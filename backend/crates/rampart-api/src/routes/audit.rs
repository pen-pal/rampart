//! Admin-only read API over the audit log.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use rampart_db::audit::{AuditEntry, AuditFilter};
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/csv", get(list_csv))
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

/// CSV export of audit entries, honouring the same filters as the JSON
/// list route. Capped at 50_000 rows per request — enough for routine
/// compliance dumps, not so large the server holds the whole table in
/// memory at once. Compliance review covering more than 50k mutations
/// at once should paginate via the `before` cursor.
async fn list_csv(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, ApiError> {
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
    let entries = rampart_db::audit::list(s.pool(), 50_000, filter).await?;
    let fmt = time::format_description::well_known::Rfc3339;
    let mut body = String::with_capacity(96 + entries.len() * 160);
    body.push_str("id,ts,actor_user_id,actor_api_key_id,action,resource_kind,resource_id,ip_addr,user_agent,payload\n");
    for e in &entries {
        body.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            e.id,
            e.ts.format(&fmt).unwrap_or_default(),
            e.actor_user_id.map(|u| u.0.to_string()).unwrap_or_default(),
            e.actor_api_key_id.map(|k| k.0.to_string()).unwrap_or_default(),
            csv_escape(&e.action),
            csv_escape(&e.resource_kind),
            e.resource_id.map(|i| i.to_string()).unwrap_or_default(),
            csv_escape(e.ip_addr.as_deref().unwrap_or("")),
            csv_escape(e.user_agent.as_deref().unwrap_or("")),
            csv_escape(
                &e.payload
                    .as_ref()
                    .map(|p| p.to_string())
                    .unwrap_or_default(),
            ),
        ));
    }
    Ok((
        [
            ("content-type", "text/csv; charset=utf-8".to_string()),
            (
                "content-disposition",
                "attachment; filename=\"audit-log.csv\"".to_string(),
            ),
        ],
        body,
    ))
}

/// Minimal CSV escape: double embedded quotes and wrap if the field
/// contains a comma, quote, or newline. Local copy of the same helper
/// from routes/monitors.rs — small enough that duplicating is cheaper
/// than carving out a shared module.
fn csv_escape(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if !(s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r')) {
        return s.to_string();
    }
    let escaped = s.replace('"', "\"\"");
    format!("\"{escaped}\"")
}
