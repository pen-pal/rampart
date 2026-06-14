//! Admin-only read API over the audit log.

use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rampart_db::audit::{AuditEntry, AuditFilter, ExportFilter, ExportRow};
use serde::Deserialize;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/csv", get(list_csv))
        .route("/export.csv", get(export_csv))
        .route("/verify", get(verify))
}

/// Re-walk the audit hash chain and report whether it's intact. A non-ok
/// result with `first_bad_id` means a row at/after that id was edited, deleted
/// or reordered (the chain no longer recomputes).
async fn verify(
    State(s): State<AppState>,
) -> Result<Json<rampart_db::audit::VerifyReport>, ApiError> {
    Ok(Json(rampart_db::audit::verify_chain(s.pool()).await?))
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
    /// Inclusive lower bound on `ts`, RFC3339.
    #[serde(default, with = "time::serde::rfc3339::option")]
    from: Option<OffsetDateTime>,
    /// Inclusive upper bound on `ts`, RFC3339.
    #[serde(default, with = "time::serde::rfc3339::option")]
    to: Option<OffsetDateTime>,
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
        from: q.from,
        to: q.to,
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
        from: q.from,
        to: q.to,
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
            e.actor_api_key_id
                .map(|k| k.0.to_string())
                .unwrap_or_default(),
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

/// Query params accepted by the streaming export. Only the time window is
/// honoured (the same `from`/`to` the list route takes) — an export is a
/// full window dump, not a filtered slice.
#[derive(Deserialize)]
struct ExportQuery {
    #[serde(default, with = "time::serde::rfc3339::option")]
    from: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    to: Option<OffsetDateTime>,
}

/// Number of rows pulled per DB round-trip. The export streams these pages
/// back-to-back, so this bounds peak memory — never the whole table — while
/// staying large enough that round-trip overhead is negligible.
const EXPORT_BATCH: i64 = 1_000;

/// Streaming CSV export of the audit log (admin-only — gated by the
/// `admin_only` layer the audit router is nested under).
///
/// Unlike [`list_csv`], which materialises up to 50k rows in a `String`,
/// this keyset-paginates the table and writes each page straight onto an
/// axum streaming body. Peak memory stays flat (~one [`EXPORT_BATCH`] page)
/// no matter how large the log grows, so it is safe to dump an arbitrarily
/// long window.
async fn export_csv(
    State(s): State<AppState>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let pool = s.pool().clone();
    let filter = ExportFilter {
        from: q.from,
        to: q.to,
    };

    // State carried across `try_unfold` iterations: the keyset cursor and a
    // one-shot flag for the header line. `None` cursor on the first call
    // means "from the newest row"; thereafter it is the last id we emitted.
    struct Cursor {
        before_id: Option<i64>,
        header_sent: bool,
        done: bool,
    }
    let init = Cursor {
        before_id: None,
        header_sent: false,
        done: false,
    };

    let stream = futures::stream::try_unfold(init, move |mut cur| {
        let pool = pool.clone();
        async move {
            if cur.done {
                return Ok::<_, ApiError>(None);
            }

            let rows =
                rampart_db::audit::export_batch(&pool, cur.before_id, EXPORT_BATCH, filter).await?;

            let mut chunk = String::new();
            if !cur.header_sent {
                chunk.push_str("ts,actor,action,resource_kind,resource_id,ip_addr,user_agent\n");
                cur.header_sent = true;
            }
            for r in &rows {
                write_export_row(&mut chunk, r);
            }

            // Short page (fewer than we asked for) means the table is
            // exhausted; emit this final chunk and stop next round.
            if (rows.len() as i64) < EXPORT_BATCH {
                cur.done = true;
            } else if let Some(last) = rows.last() {
                cur.before_id = Some(last.id);
            } else {
                cur.done = true;
            }

            Ok(Some((axum::body::Bytes::from(chunk), cur)))
        }
    });

    Ok((
        [
            ("content-type", "text/csv; charset=utf-8"),
            (
                "content-disposition",
                "attachment; filename=\"rampart-audit.csv\"",
            ),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

/// Append one CSV record for an [`ExportRow`] to `out`.
fn write_export_row(out: &mut String, r: &ExportRow) {
    let fmt = time::format_description::well_known::Rfc3339;
    out.push_str(&r.ts.format(&fmt).unwrap_or_default());
    out.push(',');
    out.push_str(&csv_escape(&r.actor));
    out.push(',');
    out.push_str(&csv_escape(&r.action));
    out.push(',');
    out.push_str(&csv_escape(&r.resource_kind));
    out.push(',');
    out.push_str(&r.resource_id.map(|i| i.to_string()).unwrap_or_default());
    out.push(',');
    out.push_str(&csv_escape(r.ip_addr.as_deref().unwrap_or("")));
    out.push(',');
    out.push_str(&csv_escape(r.user_agent.as_deref().unwrap_or("")));
    out.push('\n');
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
