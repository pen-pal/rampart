//! HTTP routes.
//!
//! Organized by resource. `health` is mounted at root, everything else
//! under `/v1` so we can version the API independently of the binary.
//!
//! Two slices of `/v1`:
//! - `v1_public()` — `/v1/auth/*` (login, register, logout, me).
//!   No session required.
//! - `v1_protected()` — everything else (monitors, summary, history).
//!   The auth middleware is applied in main.rs.

pub mod api_keys;
pub mod audit;
pub mod auth;
pub mod health;
pub mod incidents;
pub mod maintenance;
pub mod monitor_groups;
pub mod monitors;
pub mod notifications;
pub mod proxies;
pub mod push;
pub mod routing;
pub mod status_pages;
pub mod stream;
pub mod subscribers;
pub mod tags;
pub mod templates;
pub mod totp;
pub mod users;
pub mod webpush;

use crate::state::AppState;
use axum::Router;

pub fn v1_public() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::router())
        // The TOTP verify step happens AFTER password (which was public)
        // but BEFORE a session exists — has to live under v1_public.
        .nest("/auth/2fa", totp::public_router())
        // Public status-page reads — embedded under /v1/public so the
        // boundary is explicit and obvious in the routing table.
        .nest("/public/status-pages", status_pages::public_router())
        // Public subscribe + unsubscribe.
        .nest("/public", subscribers::public_router())
}

pub fn v1_protected() -> Router<AppState> {
    Router::new()
        // /v1/monitors itself + /v1/monitors/:id/notifications and /tags subroutes
        .nest(
            "/monitors",
            monitors::router()
                .merge(notifications::monitor_attach_router())
                .merge(tags::monitor_tag_router())
                .merge(monitor_groups::dep_router())
                .merge(routing::monitor_router()),
        )
        // /v1/monitor-groups CRUD + folder tags/channels
        .nest("/monitor-groups", monitor_groups::router().merge(routing::group_router()))
        // /v1/tags CRUD
        .nest("/tags", tags::router())
        // /v1/notifications CRUD + channel tags
        .nest("/notifications", notifications::router().merge(routing::channel_router()))
        // /v1/notification-templates CRUD
        .nest("/notification-templates", templates::router())
        // /v1/maintenance-windows CRUD + attach/detach
        .nest("/maintenance-windows", maintenance::router())
        // /v1/status-pages admin CRUD (public read sits in v1_public)
        .nest("/status-pages", status_pages::admin_router())
        // /v1/status-pages/:id/incidents list+create
        .nest("/status-pages", incidents::page_router())
        // /v1/incidents/:id update/delete/resolve/updates
        .nest("/incidents", incidents::incident_router())
        // Subscribers admin + SMTP settings.
        .merge(subscribers::admin_router())
        // /v1/audit-log — admin only via the layer the users router uses.
        .nest(
            "/audit-log",
            audit::router().route_layer(axum::middleware::from_fn(crate::auth::require_admin)),
        )
        // /v1/api-keys — list/create/revoke
        .nest("/api-keys", api_keys::router())
        // /v1/proxies — list/create/delete/active
        .nest("/proxies", proxies::router())
        // /v1/stream/heartbeats — Server-Sent Events fan-out.
        .nest("/stream", stream::router())
        // /v1/webpush — VAPID key + browser subscription management.
        .nest("/webpush", webpush::router())
        // /v1/auth/2fa/* admin endpoints — verify is public; the rest
        // need an existing session so they sit here.
        .nest("/auth/2fa", totp::router())
        // /v1/users/me/password — self-service; admin gate would
        // bizarrely block users from changing their own password.
        .nest("/users", users::self_router())
        // /v1/users admin CRUD — gated by require_admin on top of the
        // outer require_session middleware.
        .nest(
            "/users",
            users::admin_router()
                .route_layer(axum::middleware::from_fn(crate::auth::require_admin)),
        )
}
