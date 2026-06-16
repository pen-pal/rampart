//! Rampart HTTP API — library entry point.
//!
//! `main.rs` calls into this crate; integration tests in `tests/` also
//! import from here so they can drive the Router via `tower::ServiceExt`
//! without binding a real TCP listener.

pub mod audit;
pub mod auth;
pub mod csv;
pub mod error;
pub mod external_ingest;
pub mod http_metrics;
pub mod importers;
pub mod ingest_util;
pub mod otlp_profiles;
pub mod otlp_proto;
pub mod pprof;
pub mod rate_limit;
pub mod routes;
pub mod seed;
pub mod self_metrics;
pub mod self_telemetry;
pub mod smtp;
pub mod state;
pub mod static_assets;
pub mod symbolicate;
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
            // Don't create spans for the ingest + scrape routes. With
            // self-telemetry on (RAMPART_OTLP_ENDPOINT), tracing the OTLP/RUM/
            // error-ingest endpoints would feed our own exports straight back
            // in — an amplifying loop; /healthz + /metrics are pure scrape noise.
            let path = req.uri().path();
            if path.starts_with("/otlp")
                || path.starts_with("/rum")
                || path.starts_with("/api/")
                || path == "/healthz"
                || path == "/metrics"
            {
                return tracing::Span::none();
            }
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

    let protected_v1 = routes::v1_protected()
        // Per-key rate limit + `X-RateLimit-*` headers. Layered INNER to
        // `require_session` (route_layers apply last-outermost, so the
        // require_session layer below — applied last — wraps this one):
        // the request hits `require_session` first, which stamps the
        // `AuthApiKeyId` extension on the api-key path, and only then this
        // layer runs and can read it. Session/cookie requests never carry
        // the extension, so this layer passes them through untouched
        // (unlimited, no headers).
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            enforce_api_key_rate_limit,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));

    // HTTP request metrics middleware. Layered AFTER the router so the
    // counter sees the final response status (route-not-found → 404 is
    // counted as 4xx; an auth-gate rejection → 401 is counted as 4xx).
    // The state passed here is the metrics handle itself, not the full
    // AppState — keeps the middleware allocation-free per request.
    let metrics_handle = state.http_metrics().clone();

    // Public telemetry-ingest surfaces, grouped so one per-IP rate-limit layer
    // covers all of them (anti-DoS — these are unauthenticated by default and
    // accept arbitrary volumes of errors/traces/logs/beacons). The limiter is
    // generous (240 burst, 4/s refill per IP) so real collectors aren't
    // throttled, but a single source can't flood the tiers / fill the disk.
    let ingest = Router::new()
        // OTLP trace+log ingest — public like /push (operator controls exposure).
        .nest("/otlp", routes::otlp::router())
        // /api/:project_id/{envelope,store} — Sentry-compatible error ingest.
        // The DSN key is the auth. Outside /v1 so SDK DSNs point straight at it.
        .nest("/api", routes::error_ingest::router())
        // RUM beacon ingest + collector snippet — public (browsers).
        .nest("/rum", routes::rum::ingest_router())
        // Profiling ingest (folded text now; pprof + OTLP profiles added next).
        .nest("/profiles", routes::profiles::ingest_router())
        // Prometheus remote_write (snappy+protobuf) → metric_samples. Public,
        // optional shared-token gate like the other ingest surfaces.
        .nest("/prom", routes::prom_write::router())
        .route_layer(axum::middleware::from_fn_with_state(
            state.ingest_rate_limiter(),
            crate::rate_limit::enforce_ip_rate_limit,
        ));

    Router::new()
        .merge(routes::health::router())
        // OpenAPI spec — public, root-level (not under /v1). Served as raw
        // YAML + a JSON rendering so a client generator can fetch either.
        .merge(routes::public_root())
        // /push/:token is intentionally public — the token IS the auth.
        // Sits outside /v1 to keep external cron snippets short.
        .nest("/push", routes::push::router())
        .merge(ingest)
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

// ─── Per-API-key rate limiting (item 6) ──────────────────────────────────
//
// A per-key fixed-window counter, keyed by API key id, with a per-key
// budget. Cookie/session requests are unlimited and get no headers — only
// requests carrying an `AuthApiKeyId` extension (set by
// `auth::require_session` on the bearer path) are counted. The counter is
// now DURABLE: it lives in the `api_key_rate_usage` table (via
// `rampart_db::rate_limit::admit`) and survives restarts. This is a courtesy
// throttle and an informational header set, not a hard cross-node quota.

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Fallback per-key budget when a key carries no explicit
/// `rate_limit_per_hour` (e.g. a request that somehow reached this layer
/// without the persisted budget). The authoritative value is the per-key
/// budget read from the `AuthApiKeyId` extension; this const is only the
/// default.
const API_KEY_RATE_LIMIT: u32 = 1000;

/// Middleware: enforce + advertise the per-key rate limit.
///
/// - No `AuthApiKeyId` extension → cookie/session request → pass through
///   unchanged, no headers (session requests are unlimited).
/// - Over the limit → 429 with `Retry-After` + the `X-RateLimit-*` set.
/// - Otherwise → run the request and attach `X-RateLimit-Limit`,
///   `-Remaining`, `-Reset` to the response.
///
/// The admit decision is delegated to a durable, race-safe DB counter so the
/// usage tally persists across restarts; only the per-key BUDGET still rides
/// in on the `AuthApiKeyId` extension.
pub async fn enforce_api_key_rate_limit(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(auth_key) = req.extensions().get::<auth::AuthApiKeyId>().copied() else {
        // Not an api-key request — unlimited, no headers.
        return next.run(req).await;
    };

    // The persisted per-key budget is authoritative; fall back to the
    // process default only for a non-positive value (which the create path
    // already rejects, so this is purely defensive).
    let limit: u32 = u32::try_from(auth_key.rate_limit_per_hour)
        .ok()
        .filter(|&l| l > 0)
        .unwrap_or(API_KEY_RATE_LIMIT);

    let decision = match rampart_db::rate_limit::admit(state.pool(), auth_key.id, limit).await {
        Ok(d) => d,
        // A counter failure must not take the request down — fail open. The
        // throttle is a courtesy, not a security control, so on a DB hiccup
        // we let the request through without rate headers rather than 500.
        Err(_) => return next.run(req).await,
    };
    if !decision.allowed {
        let mut resp = (
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded — slow down",
        )
            .into_response();
        let h = resp.headers_mut();
        h.insert("retry-after", num_header(decision.reset_secs));
        set_rate_headers(h, limit, decision.remaining, decision.reset_secs);
        return resp;
    }

    let mut resp = next.run(req).await;
    set_rate_headers(
        resp.headers_mut(),
        limit,
        decision.remaining,
        decision.reset_secs,
    );
    resp
}

/// Attach the three `X-RateLimit-*` advisory headers. `limit` is the key's
/// own per-hour budget so the advertised ceiling matches what's enforced.
fn set_rate_headers(h: &mut axum::http::HeaderMap, limit: u32, remaining: u32, reset_secs: u64) {
    h.insert("x-ratelimit-limit", num_header(limit as u64));
    h.insert("x-ratelimit-remaining", num_header(remaining as u64));
    h.insert("x-ratelimit-reset", num_header(reset_secs));
}

/// Format a number as a header value. Numeric strings are always valid
/// header values, so the unwrap can't fire in practice.
fn num_header(n: u64) -> HeaderValue {
    HeaderValue::from_str(&n.to_string()).expect("numeric header value is always valid")
}
