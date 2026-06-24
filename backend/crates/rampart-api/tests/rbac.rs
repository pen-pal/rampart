//! RBAC gating integration tests.
//!
//! Verifies the three-role model end-to-end against a real router:
//!   - readonly: GET ok, any mutation 403
//!   - editor:   monitor CRUD ok, admin-only routes 403
//!   - admin:    everything ok, incl. role management
//!
//! Each test gets an isolated DB via `sqlx::test`. We register the first
//! admin through the normal flow, then mint extra roled users directly in
//! the DB (registration is locked after the first user).

mod common;

use axum::http::{Method, StatusCode};
use common::{register_admin, request, router, user_with_role};
use rampart_core::Role;
use serde_json::json;
use sqlx::PgPool;

fn new_monitor_body() -> serde_json::Value {
    json!({
        "name": "Example",
        "kind": "http",
        "url": "https://example.com",
        "interval_seconds": 60
    })
}

#[sqlx::test(migrations = "../../migrations")]
async fn readonly_can_get_but_not_mutate(pool: PgPool) {
    let app = router(pool.clone());
    // Bootstrap the admin so registration is locked and the app is past
    // first-run setup.
    register_admin(&app).await;
    let ro = user_with_role(&pool, "ro@example.com", Role::Readonly).await;

    // GET is allowed.
    let (status, _, _) = request(&app, Method::GET, "/v1/monitors", None, Some(&ro)).await;
    assert_eq!(status, StatusCode::OK, "readonly GET /v1/monitors");

    // POST (create) is forbidden.
    let (status, _, body) = request(
        &app,
        Method::POST,
        "/v1/monitors",
        Some(new_monitor_body()),
        Some(&ro),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "readonly POST should 403, got body: {}",
        String::from_utf8_lossy(&body)
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn editor_can_mutate_monitors_but_not_users(pool: PgPool) {
    let app = router(pool.clone());
    register_admin(&app).await;
    let editor = user_with_role(&pool, "editor@example.com", Role::Editor).await;

    // Editor can create a monitor.
    let (status, _, body) = request(
        &app,
        Method::POST,
        "/v1/monitors",
        Some(new_monitor_body()),
        Some(&editor),
    )
    .await;
    assert!(
        status.is_success(),
        "editor POST /v1/monitors should succeed, got {status}: {}",
        String::from_utf8_lossy(&body)
    );

    // Editor can read the user list? No — admin-only. Expect 403.
    let (status, _, _) = request(&app, Method::GET, "/v1/users", None, Some(&editor)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "editor GET /v1/users (admin-only) should 403"
    );

    // Editor cannot create a user either.
    let (status, _, _) = request(
        &app,
        Method::POST,
        "/v1/users",
        Some(json!({"email":"x@y.com","password":"longenough123","role":"editor"})),
        Some(&editor),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "editor POST /v1/users 403");
}

#[sqlx::test(migrations = "../../migrations")]
async fn readonly_blocked_on_admin_routes(pool: PgPool) {
    let app = router(pool.clone());
    register_admin(&app).await;
    let ro = user_with_role(&pool, "ro2@example.com", Role::Readonly).await;

    // Admin-only GET is blocked for readonly too (require_admin, all verbs).
    let (status, _, _) = request(&app, Method::GET, "/v1/users", None, Some(&ro)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "readonly GET /v1/users 403");

    let (status, _, _) = request(&app, Method::GET, "/v1/proxies", None, Some(&ro)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "readonly GET /v1/proxies 403"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn admin_can_set_role_and_me_includes_role(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;

    // /v1/auth/me serializes the role.
    let (status, _, body) = request(&app, Method::GET, "/v1/auth/me", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["user"]["role"], "admin",
        "me() must expose role, got: {v}"
    );

    // Create an editor, then PATCH them to readonly.
    let target = rampart_db::users::create(
        &pool,
        rampart_db::users::NewUser {
            email: "victim@example.com".into(),
            name: None,
            password_hash: "fake".into(),
            role: Role::Editor,
        },
    )
    .await
    .unwrap();

    let (status, _, body) = request(
        &app,
        Method::PATCH,
        &format!("/v1/users/{}/role", target.id.0),
        Some(json!({"role":"readonly"})),
        Some(&admin),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "admin PATCH role, got body: {}",
        String::from_utf8_lossy(&body)
    );

    let updated = rampart_db::users::get(&pool, target.id).await.unwrap();
    assert_eq!(updated.role, Role::Readonly);
    assert!(!updated.is_admin, "is_admin shim must follow role");
}

/// SECURITY REGRESSION (audit #123): a self-provisioned org admin must NOT reach
/// instance-global, cross-tenant surfaces. `/v1/orgs` is no-role-gated, and
/// `require_session` overwrites `User.role` with the ACTIVE-org role — so before
/// the `require_global_admin` fix, any user could create an org, switch into it
/// (→ role=Admin), and reach global user management = full instance takeover.
#[sqlx::test(migrations = "../../migrations")]
async fn self_provisioned_org_admin_cannot_reach_global_surfaces(pool: PgPool) {
    let app = router(pool.clone());
    // Low-priv user: editor in the Default org → global is_admin = false.
    let cookie = user_with_role(&pool, "editor@example.com", Role::Editor).await;

    // Baseline: a Default-org editor is already 403 on global user management.
    let (st, _, _) = request(&app, Method::GET, "/v1/users", None, Some(&cookie)).await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "editor blocked before the org trick"
    );

    // 1) Self-provision an org → become its Admin.
    let (st, _, body) = request(
        &app,
        Method::POST,
        "/v1/orgs",
        Some(json!({ "slug": "evil-corp", "name": "Evil Corp" })),
        Some(&cookie),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::CREATED,
        "any authenticated user can create an org: {}",
        String::from_utf8_lossy(&body)
    );
    let org: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let org_id = org["id"].as_str().expect("org id");

    // 2) Switch active org → require_session now resolves role=Admin (active org).
    let (st, _, _) = request(
        &app,
        Method::POST,
        &format!("/v1/orgs/{org_id}/switch"),
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT, "switch into own org");

    // 3) PRIVESC ATTEMPT — global user management. role=Admin (active org) but
    //    global is_admin=false → require_global_admin 403s. (Pre-fix: 200.)
    let (st, _, body) = request(&app, Method::GET, "/v1/users", None, Some(&cookie)).await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "self-provisioned org admin must NOT reach global user management: {}",
        String::from_utf8_lossy(&body)
    );

    // 4) Minting a GLOBAL admin via the trick is likewise blocked.
    let (st, _, _) = request(
        &app,
        Method::POST,
        "/v1/users",
        Some(json!({
            "email": "pwn@example.com",
            "password": "correct-horse-battery-staple",
            "is_admin": true
        })),
        Some(&cookie),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "cannot mint a global admin");

    // 5) The other instance-global surfaces are equally gated.
    for path in [
        "/v1/audit-log",
        "/v1/compliance/access-review",
        "/v1/settings/smtp",
    ] {
        let (st, _, _) = request(&app, Method::GET, path, None, Some(&cookie)).await;
        assert_eq!(
            st,
            StatusCode::FORBIDDEN,
            "global surface {path} must reject an active-org admin"
        );
    }

    // 6) NO REGRESSION: a genuine GLOBAL admin still manages users.
    let admin = user_with_role(&pool, "boss@example.com", Role::Admin).await;
    let (st, _, _) = request(&app, Method::GET, "/v1/users", None, Some(&admin)).await;
    assert_eq!(
        st,
        StatusCode::OK,
        "real global admin still reaches /v1/users"
    );
}
