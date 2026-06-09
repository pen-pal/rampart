//! Rampart HTTP API — library entry point.
//!
//! `main.rs` calls into this crate; integration tests in `tests/` also
//! import from here so they can drive the Router via `tower::ServiceExt`
//! without binding a real TCP listener.

pub mod audit;
pub mod auth;
pub mod error;
pub mod http_metrics;
pub mod importers;
pub mod rate_limit;
pub mod routes;
pub mod smtp;
pub mod state;
pub mod static_assets;
pub mod totp;

use axum::http::{header, HeaderName, HeaderValue};
use axum::Router;
use rampart_db::DbPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
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

    // Security response headers. Applied to every response — the dashboard,
    // status pages, API endpoints, embedded assets. Values follow the
    // OWASP secure-headers project recommendations adjusted for what
    // Rampart actually does:
    //
    //   - HSTS: 1-year max-age, includeSubDomains. No `preload` — operators
    //     own the decision to submit their domain to the browser preload
    //     list since it's effectively irreversible.
    //   - X-Content-Type-Options: nosniff so the browser honours the
    //     Content-Type the embedded asset serve sets rather than guessing.
    //   - X-Frame-Options: SAMEORIGIN. Defence-in-depth against clickjacking
    //     for browsers that don't honour the frame-ancestors directive
    //     below.
    //   - Referrer-Policy: strict-origin-when-cross-origin. Standard
    //     "don't leak the path to third parties" baseline.
    //   - Permissions-Policy: lock out browser APIs Rampart doesn't use.
    //     Reduces the blast radius if an XSS lands a script through
    //     a template / notification body / etc.
    //   - Content-Security-Policy: 'self' for everything except styles
    //     (the inline-CSS-in-JSX pattern needs 'unsafe-inline' in
    //     style-src) and the QR-code fallback URL the 2FA enroll panel
    //     loads from api.qrserver.com. 'frame-ancestors: none' is the
    //     modern X-Frame-Options replacement.
    let security_headers = tower::ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::if_not_present(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("SAMEORIGIN"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static(
                "accelerometer=(), camera=(), geolocation=(), gyroscope=(), \
                 magnetometer=(), microphone=(), payment=(), usb=()",
            ),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; \
                 script-src 'self'; \
                 style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
                 font-src 'self' https://fonts.gstatic.com; \
                 img-src 'self' data: https://api.qrserver.com https://api.star-history.com https://img.shields.io; \
                 connect-src 'self'; \
                 frame-ancestors 'none'; \
                 base-uri 'self'; \
                 form-action 'self'",
            ),
        ));

    let middleware = tower::ServiceBuilder::new()
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(trace_layer)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(security_headers)
        // 2 MiB request body cap. Generous enough for the largest
        // legitimate inbound payload (an audit-CSV export request has
        // no body; a bulk-monitor action could carry hundreds of IDs;
        // a notification-template body is bounded by the editor at
        // ~64 KB). Anything over the cap is a misuse or an attack —
        // 413 + drop.
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(15),
        ))
        .layer(
            // CORS: Rampart sits behind a reverse proxy in production.
            // The dashboard ships from the same origin as the API; the
            // public status pages are server-rendered HTML; the
            // browser-side scripts come from the same origin too. So
            // `Any` here is acceptable for the actual deployments we
            // target (single-origin behind proxy). Operators wanting
            // a stricter policy can layer one upstream — that's the
            // proxy's job, not ours.
            CorsLayer::new()
                .allow_methods(Any)
                .allow_headers(Any)
                .allow_origin(Any),
        );

    let protected_v1 = routes::v1_protected().route_layer(axum::middleware::from_fn_with_state(
        state.clone(),
        auth::require_session,
    ));

    // HTTP request metrics middleware. Layered AFTER the router so the
    // counter sees the final response status (route-not-found → 404 is
    // counted as 4xx; an auth-gate rejection → 401 is counted as 4xx).
    // The state passed here is the metrics handle itself, not the full
    // AppState — keeps the middleware allocation-free per request.
    let metrics_handle = state.http_metrics().clone();

    Router::new()
        .merge(routes::health::router())
        // OpenAPI spec — public, root-level (not under /v1). Served as raw
        // YAML + a JSON rendering so a client generator can fetch either.
        .merge(routes::public_root())
        // /push/:token is intentionally public — the token IS the auth.
        // Sits outside /v1 to keep external cron snippets short.
        .nest("/push", routes::push::router())
        .nest("/v1", routes::v1_public(&state).merge(protected_v1))
        .with_state(state)
        .fallback(static_assets::handler)
        .layer(axum::middleware::from_fn_with_state(
            metrics_handle,
            http_metrics::record_http_metrics,
        ))
        .layer(middleware)
}

/// Convenience for tests: build a router around a pre-existing DB pool.
/// Creates a fresh `Notify` for the scheduler-reload handle; tests
/// generally don't care about that signal.
pub fn test_router(pool: DbPool) -> Router {
    let state = AppState::new(pool, Arc::new(Notify::new()));
    build_router(state)
}
