//! Rampart HTTP API — library entry point.
//!
//! `main.rs` calls into this crate; integration tests in `tests/` also
//! import from here so they can drive the Router via `tower::ServiceExt`
//! without binding a real TCP listener.

pub mod audit;
pub mod auth;
pub mod error;
pub mod importers;
pub mod routes;
pub mod smtp;
pub mod state;
pub mod static_assets;
pub mod totp;

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
    // Custom span builder so every per-request log line is decorated
    // with the `x-request-id` value `SetRequestIdLayer` minted upstream.
    // The header exists by the time TraceLayer sees the request because
    // ServiceBuilder applies layers in declaration order on the
    // inbound path — SetRequestIdLayer wraps outermost, TraceLayer
    // sees a request whose `x-request-id` is already populated.
    let trace_layer =
        TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<axum::body::Body>| {
            let request_id = req
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            tracing::info_span!(
                "http",
                method     = %req.method(),
                uri        = %req.uri(),
                request_id = %request_id,
            )
        });

    let middleware = tower::ServiceBuilder::new()
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(trace_layer)
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
