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

pub mod auth;
pub mod health;
pub mod maintenance;
pub mod monitors;
pub mod notifications;
pub mod push;
pub mod status_pages;
pub mod templates;

use crate::state::AppState;
use axum::Router;

pub fn v1_public() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::router())
        // Public status-page reads — embedded under /v1/public so the
        // boundary is explicit and obvious in the routing table.
        .nest("/public/status-pages", status_pages::public_router())
}

pub fn v1_protected() -> Router<AppState> {
    Router::new()
        // /v1/monitors itself and /v1/monitors/:id/notifications attach/detach
        .nest(
            "/monitors",
            monitors::router().merge(notifications::monitor_attach_router()),
        )
        // /v1/notifications CRUD
        .nest("/notifications", notifications::router())
        // /v1/notification-templates CRUD
        .nest("/notification-templates", templates::router())
        // /v1/maintenance-windows CRUD + attach/detach
        .nest("/maintenance-windows", maintenance::router())
        // /v1/status-pages admin CRUD (public read sits in v1_public)
        .nest("/status-pages", status_pages::admin_router())
}
