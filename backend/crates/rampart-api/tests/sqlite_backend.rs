//! The API on a non-Postgres backend must never panic on a request.
//!
//! `AppState::pool()` used to `.expect()` on MySQL/SQLite; with the release
//! profile's `panic = "abort"`, loading the dashboard (`/v1/monitors/summary`)
//! or scraping `/readyz` / `/metrics` killed the whole process. Seam-backed
//! endpoints must work; genuinely Postgres-only ones must answer 501.
#![cfg(feature = "sqlite")]

mod common;

use axum::http::{Method, StatusCode};
use common::request;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn sqlite_backend_serves_dashboard_and_501s_postgres_only_routes() {
    let db = std::env::temp_dir().join(format!("rampart-test-{}.db", uuid::Uuid::new_v4()));
    let store: Arc<dyn rampart_db::store::Store> = Arc::new(
        rampart_db::sqlite::store::SqliteStore::connect(&format!("sqlite:{}", db.display()))
            .await
            .expect("sqlite connect + migrate"),
    );
    // SMTP config loads through the Store seam, so status-page subscriber
    // emails work off Postgres (the incident fan-out used to skip them).
    assert!(rampart_api::smtp::load(store.as_ref())
        .await
        .unwrap()
        .is_none());
    let smtp = json!({"host": "smtp.example.com", "port": 587, "encryption": "starttls",
                      "from": "status@example.com"});
    store.put_setting("smtp", &smtp).await.unwrap();
    let cfg = rampart_api::smtp::load(store.as_ref())
        .await
        .unwrap()
        .expect("smtp config");
    assert_eq!(cfg.host, "smtp.example.com");

    let scheduler = Arc::new(rampart_scheduler::Scheduler::with_notifier(
        store.clone(),
        None,
    ));
    let state = rampart_api::AppState::with_scheduler_store(
        store,
        Arc::new(tokio::sync::Notify::new()),
        scheduler,
    );
    let app = rampart_api::build_router(state);

    let (status, headers, _) = request(
        &app,
        Method::POST,
        "/v1/auth/register",
        Some(json!({"email": "admin@example.test", "password": "correct-horse-battery-9"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let cookie = headers
        .iter()
        .find(|(k, _)| k == "set-cookie")
        .map(|(_, v)| v.split(';').next().unwrap().to_string())
        .expect("session cookie");

    // Seam-backed: the dashboard's first call, plus the probe/scrape endpoints.
    for path in ["/v1/monitors/summary?window=86400", "/readyz", "/metrics"] {
        let (status, _, _) = request(&app, Method::GET, path, None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
    }

    // Still Postgres-only: a clean 501, not a panic.
    let (status, _, _) = request(&app, Method::POST, "/otlp/v1/logs", Some(json!({})), None).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);

    let _ = std::fs::remove_file(db);
}
