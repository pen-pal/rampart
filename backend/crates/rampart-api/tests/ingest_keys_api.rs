//! `/v1/ingest-keys` API integration tests (multi-tenancy Phase 5).
//!
//! These are per-org telemetry ingest credentials, admin-gated in the
//! `admin_only` subtree. The create response carries the plaintext token
//! EXACTLY once; the list endpoint only ever returns metadata. Verifies:
//!   - admin can create (token returned), list (no token), and delete a key
//!   - a non-admin (editor) is 403 on every verb
//!   - a key minted under one org is invisible to another org's list

mod common;

use axum::http::{Method, StatusCode};
use common::{register_admin, request, router, user_with_role};
use rampart_core::ids::OrgId;
use rampart_core::org::DEFAULT_ORG_ID;
use rampart_core::Role;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = "../../migrations")]
async fn admin_create_list_delete(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;

    // Empty to start.
    let (status, _, body) = request(&app, Method::GET, "/v1/ingest-keys", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let listed: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(listed.is_empty(), "no keys at first");

    // Create — response carries the plaintext token, shown ONCE.
    let (status, _, body) = request(
        &app,
        Method::POST,
        "/v1/ingest-keys",
        Some(json!({ "label": "prod collector", "kind": "otlp" })),
        Some(&admin),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "admin create, got body: {}",
        String::from_utf8_lossy(&body)
    );
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = created["token"].as_str().expect("token in create response");
    assert!(token.starts_with("ingk_"), "token has the ingest prefix");
    assert_eq!(created["label"], "prod collector");
    assert_eq!(created["kind"], "otlp");
    let id = created["id"].as_str().expect("id in create response").to_string();

    // List shows the key — but NEVER the token.
    let (status, _, body) = request(&app, Method::GET, "/v1/ingest-keys", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let listed: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(listed.len(), 1, "one key listed");
    assert_eq!(listed[0]["label"], "prod collector");
    assert!(
        listed[0].get("token").is_none(),
        "list must NOT expose the token, got: {}",
        listed[0]
    );

    // Delete — 204, then it's gone.
    let (status, _, _) = request(
        &app,
        Method::DELETE,
        &format!("/v1/ingest-keys/{id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete returns 204");

    let (status, _, body) = request(&app, Method::GET, "/v1/ingest-keys", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let listed: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(listed.is_empty(), "key removed after delete");

    // Deleting again (or an unknown id) is 404 — invisible.
    let (status, _, _) = request(
        &app,
        Method::DELETE,
        &format!("/v1/ingest-keys/{id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "second delete 404");
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_label_rejected(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;

    let (status, _, _) = request(
        &app,
        Method::POST,
        "/v1/ingest-keys",
        Some(json!({ "label": "   " })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "blank label rejected");
}

#[sqlx::test(migrations = "../../migrations")]
async fn kind_defaults_to_all(pool: PgPool) {
    let app = router(pool.clone());
    let admin = register_admin(&app).await;

    // Omitting `kind` defaults to "all".
    let (status, _, body) = request(
        &app,
        Method::POST,
        "/v1/ingest-keys",
        Some(json!({ "label": "catch-all" })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(created["kind"], "all", "default kind is all");
}

#[sqlx::test(migrations = "../../migrations")]
async fn editor_is_forbidden(pool: PgPool) {
    let app = router(pool.clone());
    register_admin(&app).await;
    let editor = user_with_role(&pool, "editor@example.com", Role::Editor).await;

    // GET — admin-only, editor 403.
    let (status, _, _) =
        request(&app, Method::GET, "/v1/ingest-keys", None, Some(&editor)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "editor GET 403");

    // POST — 403.
    let (status, _, _) = request(
        &app,
        Method::POST,
        "/v1/ingest-keys",
        Some(json!({ "label": "x", "kind": "all" })),
        Some(&editor),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "editor POST 403");

    // DELETE — 403.
    let (status, _, _) = request(
        &app,
        Method::DELETE,
        &format!("/v1/ingest-keys/{}", uuid::Uuid::now_v7()),
        None,
        Some(&editor),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "editor DELETE 403");
}

#[sqlx::test(migrations = "../../migrations")]
async fn cross_org_key_not_listed(pool: PgPool) {
    // The admin's active org is the Default org (session fallback). A key
    // minted directly under a DIFFERENT org must not appear in the admin's
    // (Default-org-scoped) list, and must not be deletable through it.
    let app = router(pool.clone());
    let admin = register_admin(&app).await;

    // Mint a key under a brand-new, unrelated org via the DB layer.
    let org = rampart_db::orgs::create(&pool, "other-org", "Other Org")
        .await
        .expect("create other org");
    let other_org = org.id;
    let (other_key, _token) =
        rampart_db::ingest_keys::create(&pool, other_org, "their collector", "all", &[])
            .await
            .expect("mint other-org key");

    // The Default-org admin sees none of it.
    assert_ne!(other_org, OrgId::from_uuid(DEFAULT_ORG_ID));
    let (status, _, body) = request(&app, Method::GET, "/v1/ingest-keys", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let listed: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(
        listed.is_empty(),
        "cross-org key must be invisible, got: {listed:?}"
    );

    // And can't reach it by id either — 404, not 204.
    let (status, _, _) = request(
        &app,
        Method::DELETE,
        &format!("/v1/ingest-keys/{}", other_key.id),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "cross-org delete is 404");
}
