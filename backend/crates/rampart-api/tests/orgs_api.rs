//! `/v1/orgs` — org CRUD + membership-management API (multi-tenancy 4c).
//!
//! Exercises the per-org authorization model end-to-end against a real
//! router: an authenticated user creates an org (becoming its Admin), a
//! second user is invisible until invited, role gating (Readonly may read but
//! not rename), member-add by email (existing users only), and the last-admin
//! protection on demote + remove.

mod common;

use axum::http::{Method, StatusCode};
use common::{register_admin, request, router, user_with_role};
use serde_json::{json, Value};
use sqlx::PgPool;

fn org_body(slug: &str, name: &str) -> Value {
    json!({ "slug": slug, "name": name })
}

/// Create an org as `cookie` and return its id (caller becomes Admin).
async fn make_org(app: &axum::Router, cookie: &str, slug: &str, name: &str) -> String {
    let (status, _, body) = request(app, Method::POST, "/v1/orgs", Some(org_body(slug, name)), Some(cookie)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "create org should 201, got body: {}",
        String::from_utf8_lossy(&body)
    );
    let v: Value = serde_json::from_slice(&body).unwrap();
    v["id"].as_str().unwrap().to_string()
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_lists_and_creator_is_admin(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;

    let id = make_org(&app, &admin, "acme", "Acme Inc").await;

    // GET /v1/orgs lists the new org (plus the Default org the user is in).
    let (status, _, body) = request(&app, Method::GET, "/v1/orgs", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let orgs: Value = serde_json::from_slice(&body).unwrap();
    let slugs: Vec<&str> = orgs.as_array().unwrap().iter().map(|o| o["slug"].as_str().unwrap()).collect();
    assert!(slugs.contains(&"acme"), "new org should appear in list: {slugs:?}");

    // GET /v1/orgs/{id} works for the member.
    let (status, _, _) = request(&app, Method::GET, &format!("/v1/orgs/{id}"), None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK, "member GET org");

    // The creator shows up in members as Admin.
    let (status, _, body) = request(&app, Method::GET, &format!("/v1/orgs/{id}/members"), None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let members: Value = serde_json::from_slice(&body).unwrap();
    let arr = members.as_array().unwrap();
    assert_eq!(arr.len(), 1, "exactly the creator");
    assert_eq!(arr[0]["role"], "admin");
    assert_eq!(arr[0]["email"], "admin@example.com");
}

#[sqlx::test(migrations = "../../migrations")]
async fn non_member_cannot_see_or_rename_then_editor_can_read_not_rename(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;
    let bob = user_with_role(&pool, "bob@example.com", rampart_core::Role::Editor).await;

    let id = make_org(&app, &admin, "acme", "Acme Inc").await;

    // Bob is NOT a member: GET and PATCH both 404 (hide existence).
    let (status, _, _) = request(&app, Method::GET, &format!("/v1/orgs/{id}"), None, Some(&bob)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-member GET → 404");
    let (status, _, _) = request(&app, Method::PATCH, &format!("/v1/orgs/{id}"), Some(json!({"name":"X"})), Some(&bob)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-member PATCH → 404");

    // Add bob as an Editor of the org.
    let (status, _, body) = request(
        &app,
        Method::POST,
        &format!("/v1/orgs/{id}/members"),
        Some(json!({"email":"bob@example.com","role":"editor"})),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "add member: {}", String::from_utf8_lossy(&body));

    // Now bob (Editor) can READ the org but cannot RENAME it (Admin-only).
    let (status, _, _) = request(&app, Method::GET, &format!("/v1/orgs/{id}"), None, Some(&bob)).await;
    assert_eq!(status, StatusCode::OK, "member Editor GET → 200");
    let (status, _, _) = request(&app, Method::PATCH, &format!("/v1/orgs/{id}"), Some(json!({"name":"Renamed"})), Some(&bob)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "Editor rename → 403");

    // The org Admin can rename it.
    let (status, _, _) = request(&app, Method::PATCH, &format!("/v1/orgs/{id}"), Some(json!({"name":"Renamed"})), Some(&admin)).await;
    assert_eq!(status, StatusCode::OK, "Admin rename → 200");
}

#[sqlx::test(migrations = "../../migrations")]
async fn add_member_unknown_email_404(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;
    let id = make_org(&app, &admin, "acme", "Acme Inc").await;

    let (status, _, _) = request(
        &app,
        Method::POST,
        &format!("/v1/orgs/{id}/members"),
        Some(json!({"email":"nobody@example.com","role":"editor"})),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown email → 404 (no user creation)");
}

#[sqlx::test(migrations = "../../migrations")]
async fn role_change_and_last_admin_protection(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;
    let _bob = user_with_role(&pool, "bob@example.com", rampart_core::Role::Editor).await;
    let id = make_org(&app, &admin, "acme", "Acme Inc").await;

    // Resolve the admin user's id from the members list.
    let (_, _, body) = request(&app, Method::GET, &format!("/v1/orgs/{id}/members"), None, Some(&admin)).await;
    let members: Value = serde_json::from_slice(&body).unwrap();
    let admin_uid = members.as_array().unwrap()[0]["user_id"].as_str().unwrap().to_string();

    // Add bob as Editor.
    let (status, _, _) = request(&app, Method::POST, &format!("/v1/orgs/{id}/members"), Some(json!({"email":"bob@example.com","role":"editor"})), Some(&admin)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, _, body) = request(&app, Method::GET, &format!("/v1/orgs/{id}/members"), None, Some(&admin)).await;
    let members: Value = serde_json::from_slice(&body).unwrap();
    let bob_uid = members.as_array().unwrap().iter().find(|m| m["email"] == "bob@example.com").unwrap()["user_id"].as_str().unwrap().to_string();

    // Promote bob to Admin, then demote him back (two admins exist → ok).
    let (status, _, _) = request(&app, Method::PATCH, &format!("/v1/orgs/{id}/members/{bob_uid}"), Some(json!({"role":"admin"})), Some(&admin)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "promote bob → admin");
    let (status, _, _) = request(&app, Method::PATCH, &format!("/v1/orgs/{id}/members/{bob_uid}"), Some(json!({"role":"readonly"})), Some(&admin)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "demote bob (two admins) → ok");

    // Now the only Admin is the creator. Demoting them → 409 (last admin).
    let (status, _, _) = request(&app, Method::PATCH, &format!("/v1/orgs/{id}/members/{admin_uid}"), Some(json!({"role":"editor"})), Some(&admin)).await;
    assert_eq!(status, StatusCode::CONFLICT, "demote last admin → 409");

    // Remove a non-last member (bob, who is Readonly now) → 204.
    let (status, _, _) = request(&app, Method::DELETE, &format!("/v1/orgs/{id}/members/{bob_uid}"), None, Some(&admin)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "remove non-last member → 204");

    // Removing the last admin (the creator) → 409.
    let (status, _, _) = request(&app, Method::DELETE, &format!("/v1/orgs/{id}/members/{admin_uid}"), None, Some(&admin)).await;
    assert_eq!(status, StatusCode::CONFLICT, "remove last admin → 409");
}

#[sqlx::test(migrations = "../../migrations")]
async fn slug_validation_and_duplicate(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;

    // Bad slug (uppercase / space / too short) → 400.
    for bad in ["A", "Acme Inc", "x", "has space", "UPPER", "with_underscore"] {
        let (status, _, _) = request(&app, Method::POST, "/v1/orgs", Some(org_body(bad, "Name")), Some(&admin)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "bad slug {bad:?} → 400");
    }

    // Empty name → 400.
    let (status, _, _) = request(&app, Method::POST, "/v1/orgs", Some(json!({"slug":"good","name":"  "})), Some(&admin)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty name → 400");

    // Create, then duplicate slug → 409.
    make_org(&app, &admin, "dup", "First").await;
    let (status, _, _) = request(&app, Method::POST, "/v1/orgs", Some(org_body("dup", "Second")), Some(&admin)).await;
    assert_eq!(status, StatusCode::CONFLICT, "duplicate slug → 409");
}

// ── 4d: org switcher (set active_org_id) + /v1/auth/me enrichment ─────────────
#[sqlx::test(migrations = "../../migrations")]
async fn switch_active_org_and_me_reflects_it(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;
    let acme = make_org(&app, &admin, "acme", "Acme Inc").await;

    // /v1/auth/me now carries the org list (Default + acme) + an active org.
    let (st, _, body) = request(&app, Method::GET, "/v1/auth/me", None, Some(&admin)).await;
    assert_eq!(st, StatusCode::OK);
    let me: Value = serde_json::from_slice(&body).unwrap();
    assert!(me["orgs"].as_array().unwrap().len() >= 2, "me lists the caller's orgs");
    assert!(me["active_org_id"].is_string(), "me carries an active org (Default fallback)");

    // Switch into acme (the caller is its Admin → a member) → 204.
    let (st, _, _) = request(&app, Method::POST, &format!("/v1/orgs/{acme}/switch"), None, Some(&admin)).await;
    assert_eq!(st, StatusCode::NO_CONTENT, "switch into a joined org → 204");
    let (_, _, body) = request(&app, Method::GET, "/v1/auth/me", None, Some(&admin)).await;
    let me: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(me["active_org_id"].as_str().unwrap(), acme, "me reflects the switched org");

    // Cannot switch into an org the caller is NOT a member of → 404.
    let other = "00000000-0000-0000-0000-0000000c0ffe";
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(other).execute(&pool).await.unwrap();
    let (st, _, _) = request(&app, Method::POST, &format!("/v1/orgs/{other}/switch"), None, Some(&admin)).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "switch into a non-member org → 404");
}
