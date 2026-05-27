//! Rampart HTTP API — library entry point.
//!
//! `main.rs` calls into this crate; integration tests in `tests/` also
//! import from here so they can drive the Router via `tower::ServiceExt`
//! without binding a real TCP listener.

pub mod auth;
pub mod error;
pub mod routes;
pub mod state;
pub mod static_assets;

use axum::Router;
use rampart_db::DbPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub use state::AppState;

/// Build a Router for the given AppState. Used by `main.rs` for the
/// production binary and by `tests/` to drive the API in-process.
pub fn build_router(state: AppState) -> Router {
    let middleware = tower::ServiceBuilder::new()
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(15),
        ))
        .layer(
            CorsLayer::new()
                .allow_methods(Any)
                .allow_headers(Any)
                .allow_origin(Any),
        );

    let protected_v1 = routes::v1_protected().route_layer(axum::middleware::from_fn_with_state(
        state.clone(),
        auth::require_session,
    ));

    Router::new()
        .merge(routes::health::router())
        // /push/:token is intentionally public — the token IS the auth.
        // Sits outside /v1 to keep external cron snippets short.
        .nest("/push", routes::push::router())
        .nest("/v1", routes::v1_public().merge(protected_v1))
        .with_state(state)
        .fallback(static_assets::handler)
        .layer(middleware)
}

/// Convenience for tests: build a router around a pre-existing DB pool.
/// Creates a fresh `Notify` for the scheduler-reload handle; tests
/// generally don't care about that signal.
pub fn test_router(pool: DbPool) -> Router {
    let state = AppState::new(pool, Arc::new(Notify::new()));
    build_router(state)
}
