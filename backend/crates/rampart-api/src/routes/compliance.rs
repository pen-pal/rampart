//! `/v1/compliance` — auditor-facing evidence reports (admin-gated).
//!
//! Slice 2 of the compliance-evidence epic: an access-review snapshot. Where
//! the GDPR routes (slice 1) handle data-subject rights and the audit log
//! captures access-change *events*, this exposes the point-in-time *state* an
//! auditor requests for a SOC 2 CC6 user-access review — who holds what role in
//! every org, with last-login + MFA status. JSON for the UI/programmatic use,
//! CSV for the evidence binder. Pulling a report is itself an audited event.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use rampart_db::access_review::AccessReviewRow;
use rampart_db::users::User;

pub fn router() -> Router<AppState> {
    // Mounted under the admin_only subtree (require_admin) in routes/mod.rs.
    Router::new()
        .route("/access-review", get(access_review))
        .route("/access-review.csv", get(access_review_csv))
}

/// JSON access-review: one row per (org, member) grant.
async fn access_review(
    State(s): State<AppState>,
    Extension(caller): Extension<User>,
    headers: HeaderMap,
) -> Result<Json<Vec<AccessReviewRow>>, ApiError> {
    let rows = s.store().access_review().await?;
    audit_report(&s, &caller, &headers).await;
    Ok(Json(rows))
}

/// CSV access-review for the auditor's evidence binder.
async fn access_review_csv(
    State(s): State<AppState>,
    Extension(caller): Extension<User>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let rows = s.store().access_review().await?;
    audit_report(&s, &caller, &headers).await;

    let fmt = time::format_description::well_known::Rfc3339;
    let mut body = String::with_capacity(96 + rows.len() * 96);
    body.push_str("org_slug,org_name,email,name,role,member_since,last_login_at,mfa_enabled\n");
    for r in &rows {
        body.push_str(&format!(
            "{},{},{},{},{:?},{},{},{}\n",
            csv_escape(&r.org_slug),
            csv_escape(&r.org_name),
            csv_escape(&r.email),
            csv_escape(r.name.as_deref().unwrap_or("")),
            r.role,
            r.member_since.format(&fmt).unwrap_or_default(),
            r.last_login_at
                .and_then(|t| t.format(&fmt).ok())
                .unwrap_or_default(),
            r.totp_enabled,
        ));
    }
    Ok((
        StatusCode::OK,
        [
            ("content-type", "text/csv; charset=utf-8".to_string()),
            (
                "content-disposition",
                "attachment; filename=\"rampart-access-review.csv\"".to_string(),
            ),
        ],
        body,
    ))
}

/// Record that an access-review was pulled — the act of exporting a compliance
/// report is itself an auditable event (mirrors the GDPR-export precedent).
async fn audit_report(s: &AppState, caller: &User, headers: &HeaderMap) {
    crate::audit::record(
        s.store(),
        caller,
        headers,
        "compliance.access_review",
        "compliance",
        None,
        None,
    )
    .await;
}

use crate::csv::csv_escape;
