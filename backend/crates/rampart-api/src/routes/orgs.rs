//! `/v1/orgs` — organization CRUD + membership management (multi-tenancy 4c).
//!
//! Authorization is **per-org** and keyed on the org id in the PATH (not the
//! request's active `OrgContext`): a user may be Admin in one org and a mere
//! reader in another, so every handler resolves the caller's role *in the
//! targeted org* via [`require_org_role`]. Non-members get a 404 (the org's
//! existence is hidden from outsiders, IDOR-safe); members below the required
//! role get a 403.
//!
//! Slug is immutable in P4 (it's the stable OIDC-mapping key) and there is no
//! org-delete endpoint — ~30 `org_id` columns have FK `ON DELETE RESTRICT`, so
//! deleting an org with any data would fail; we don't expose the foot-gun.

use crate::auth::SESSION_COOKIE;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use rampart_core::ids::{OrgId, UserId};
use rampart_core::org::Org;
use rampart_core::Role;
use rampart_db::orgs;
use rampart_db::users::User;
use rampart_db::DbResult;
use serde::Deserialize;
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).patch(rename))
        .route("/{id}/members", get(list_members).post(add_member))
        .route(
            "/{id}/members/{user_id}",
            patch(set_member_role).delete(remove_member),
        )
        .route("/{id}/switch", post(switch))
}

// ── path parsing ────────────────────────────────────────────────────────────

fn parse_org(s: &str) -> Result<OrgId, ApiError> {
    Uuid::from_str(s)
        .map(OrgId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid org id".into()))
}

fn parse_user(s: &str) -> Result<UserId, ApiError> {
    Uuid::from_str(s)
        .map(UserId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid user id".into()))
}

// ── authorization helper ────────────────────────────────────────────────────

/// Resolve the caller's role in `org` and assert it's at least `min`.
///
/// Returns the caller's actual role on success. Failure modes are
/// deliberately distinct:
/// - **non-member → `NotFound` (404):** an outsider can't even tell the org
///   exists (prevents enumeration / IDOR).
/// - **member but under-privileged → `Forbidden` (403):** the org is known to
///   exist (the caller is in it), but they lack the rank for this action.
async fn require_org_role(
    store: &dyn rampart_db::store::Store,
    org: OrgId,
    user: &User,
    min: Role,
) -> Result<Role, ApiError> {
    match store.org_member_role(org, user.id).await? {
        None => Err(ApiError::NotFound),
        Some(role) if role.at_least(min) => Ok(role),
        Some(_) => Err(ApiError::Forbidden),
    }
}

// ── GET /v1/orgs — the caller's own orgs ────────────────────────────────────

/// Any authenticated user; lists only the orgs they belong to.
async fn list(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
) -> Result<Json<Vec<Org>>, ApiError> {
    Ok(Json(s.store().orgs_for_user(user.id).await?))
}

// ── POST /v1/orgs — create an org (any authenticated user) ──────────────────

#[derive(Deserialize)]
struct CreateOrgInput {
    slug: String,
    name: String,
}

/// Validate `slug` against `^[a-z0-9-]{2,40}$`: 2-40 chars of lowercase
/// ASCII, digits, and hyphens. Mirrors the DB CHECK so we reject at the edge
/// with a clean 400 instead of leaking a constraint-violation 500.
fn valid_slug(slug: &str) -> bool {
    let len = slug.len();
    (2..=40).contains(&len)
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

async fn create(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Json(input): Json<CreateOrgInput>,
) -> Result<(StatusCode, Json<Org>), ApiError> {
    if !valid_slug(&input.slug) {
        return Err(ApiError::BadRequest(
            "slug must match ^[a-z0-9-]{2,40}$".into(),
        ));
    }
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }
    // Atomic: the creator becomes the org's first Admin. Duplicate slug maps
    // to Conflict (409) inside create_with_owner.
    let org = s
        .store()
        .create_org_with_owner(&input.slug, &input.name, user.id)
        .await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "org.create",
        "org",
        Some(org.id.0),
        Some(serde_json::json!({ "slug": input.slug, "name": input.name })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(org)))
}

// ── GET /v1/orgs/{id} — read one (members only) ─────────────────────────────

/// Any member (Readonly+) may read the org. Non-members get 404.
async fn get_one(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
) -> Result<Json<Org>, ApiError> {
    let org = parse_org(&id)?;
    require_org_role(s.store().as_ref(), org, &user, Role::Readonly).await?;
    Ok(Json(s.store().get_org(org).await?))
}

// ── PATCH /v1/orgs/{id} — rename (org Admin only) ───────────────────────────

#[derive(Deserialize)]
struct RenameOrgInput {
    name: String,
}

async fn rename(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<RenameOrgInput>,
) -> Result<Json<Org>, ApiError> {
    let org = parse_org(&id)?;
    require_org_role(s.store().as_ref(), org, &user, Role::Admin).await?;
    if input.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }
    let updated = s.store().update_org(org, &input.name).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "org.rename",
        "org",
        Some(org.0),
        Some(serde_json::json!({ "name": input.name })),
    )
    .await;
    Ok(Json(updated))
}

// ── GET /v1/orgs/{id}/members — list members (members only) ──────────────────

async fn list_members(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
) -> Result<Json<Vec<orgs::OrgMemberDetail>>, ApiError> {
    let org = parse_org(&id)?;
    require_org_role(s.store().as_ref(), org, &user, Role::Readonly).await?;
    Ok(Json(s.store().list_org_members_detailed(org).await?))
}

// ── POST /v1/orgs/{id}/members — add a member by email (org Admin only) ──────

#[derive(Deserialize)]
struct AddMemberInput {
    email: String,
    role: Role,
}

/// Resolve an *existing* user by email and add (or re-role) them. We never
/// create users here — an unknown email is a 404 (the user must already exist
/// in the install before they can be invited to an org).
async fn add_member(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AddMemberInput>,
) -> Result<StatusCode, ApiError> {
    let org = parse_org(&id)?;
    require_org_role(s.store().as_ref(), org, &user, Role::Admin).await?;
    let target = s
        .store()
        .user_by_email(&input.email)
        .await?
        .ok_or(ApiError::NotFound)?;
    s.store()
        .upsert_org_member(org, target.id, input.role)
        .await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "org.member_add",
        "org_member",
        Some(org.0),
        Some(serde_json::json!({
            "target_user": target.id.0,
            "email": input.email,
            "role": format!("{:?}", input.role),
        })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ── PATCH /v1/orgs/{id}/members/{user_id} — change a role (org Admin only) ───

#[derive(Deserialize)]
struct SetMemberRoleInput {
    role: Role,
}

async fn set_member_role(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(String, String)>,
    Json(input): Json<SetMemberRoleInput>,
) -> Result<StatusCode, ApiError> {
    let org = parse_org(&id)?;
    let target = parse_user(&user_id)?;
    require_org_role(s.store().as_ref(), org, &user, Role::Admin).await?;

    let current = s
        .store()
        .org_member_role(org, target)
        .await?
        .ok_or(ApiError::NotFound)?;
    // Last-admin protection: refuse to demote the org's final Admin, which
    // would leave it with nobody who can manage membership (orphaned).
    if last_admin_demotion(s.store().as_ref(), org, current, input.role).await? {
        return Err(ApiError::Conflict(
            "cannot demote the last admin of the org".into(),
        ));
    }
    s.store().upsert_org_member(org, target, input.role).await?;
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "org.member_role_set",
        "org_member",
        Some(org.0),
        Some(serde_json::json!({
            "target_user": target.0,
            "from": format!("{current:?}"),
            "to": format!("{:?}", input.role),
        })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// True when changing a member from `current` to `new` would remove the org's
/// last remaining Admin: the target is currently Admin, the new role isn't
/// Admin, and there's exactly one Admin in the org.
async fn last_admin_demotion(
    store: &dyn rampart_db::store::Store,
    org: OrgId,
    current: Role,
    new: Role,
) -> DbResult<bool> {
    if current.is_admin() && !new.is_admin() {
        Ok(store.count_org_admins(org).await? <= 1)
    } else {
        Ok(false)
    }
}

// ── POST /v1/orgs/{id}/switch — set the caller's active org ──────────────────

/// Switch the caller's *active* org (persists `sessions.active_org_id` for this
/// cookie session). The caller must be a member of the target org (non-member →
/// 404, IDOR-safe). This is a cookie-session action — bearer/API-key callers
/// have no session cookie and get 401. The choice surfaces immediately via
/// `/v1/auth/me`; it becomes the org that scopes resources once `require_session`
/// resolves the active org (Phase 4e).
async fn switch(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let org = parse_org(&id)?;
    // Membership gate (any role): you can only switch INTO an org you belong to.
    require_org_role(s.store().as_ref(), org, &user, Role::Readonly).await?;
    let token = jar
        .get(SESSION_COOKIE)
        .and_then(|c| Uuid::from_str(c.value()).ok())
        .ok_or(ApiError::Unauthorized)?;
    // Scoped to (session, user); a mismatched/expired session affects 0 rows.
    if !s
        .store()
        .set_session_active_org(token, user.id, org.0)
        .await?
    {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── DELETE /v1/orgs/{id}/members/{user_id} — remove (org Admin only) ─────────

async fn remove_member(
    State(s): State<AppState>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let org = parse_org(&id)?;
    let target = parse_user(&user_id)?;
    require_org_role(s.store().as_ref(), org, &user, Role::Admin).await?;

    let current = s
        .store()
        .org_member_role(org, target)
        .await?
        .ok_or(ApiError::NotFound)?;
    // Last-admin protection: removing the final Admin would orphan the org.
    if current.is_admin() && s.store().count_org_admins(org).await? <= 1 {
        return Err(ApiError::Conflict(
            "cannot remove the last admin of the org".into(),
        ));
    }
    if !s.store().remove_org_member(org, target).await? {
        return Err(ApiError::NotFound);
    }
    crate::audit::record(
        s.pool(),
        &user,
        &headers,
        "org.member_remove",
        "org_member",
        Some(org.0),
        Some(serde_json::json!({ "target_user": target.0, "was": format!("{current:?}") })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
