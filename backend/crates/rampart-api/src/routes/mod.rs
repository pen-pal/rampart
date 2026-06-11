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

pub mod agents;
pub mod api_keys;
pub mod audit;
pub mod auth;
pub mod delivery_log;
pub mod escalations;
pub mod health;
pub mod incident_templates;
pub mod incidents;
pub mod ingest;
pub mod maintenance;
pub mod metric_ingest;
pub mod monitor_groups;
pub mod monitor_templates;
pub mod monitors;
pub mod notifications;
pub mod on_call;
pub mod openapi;
pub mod prefs;
pub mod proxies;
pub mod push;
pub mod routing;
pub mod scheduled_reports;
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

/// Public, root-level routes that aren't versioned under `/v1`. Today this
/// is only the OpenAPI spec (`/openapi.yaml`, `/openapi.json`) — the spec
/// describes the whole surface, isn't sensitive, and is convenient to fetch
/// from a fixed top-level path that a client generator can hard-code.
pub fn public_root() -> Router<AppState> {
    openapi::router()
}

pub fn v1_public(state: &AppState) -> Router<AppState> {
    // Per-IP rate limit on the auth surface only. Cap = 10 attempts /
    // 6-second refill → ~10/min average, no lockout on a single typo.
    // Layered via `route_layer` so it scopes to just the inner auth +
    // 2fa-verify routes; the public status-page reads + subscriber
    // endpoints below are unaffected.
    let rate_limited_auth = Router::new()
        .nest("/auth", auth::router())
        .nest("/auth/2fa", totp::public_router())
        .route_layer(axum::middleware::from_fn_with_state(
            state.auth_rate_limiter(),
            crate::rate_limit::enforce_ip_rate_limit,
        ));
    Router::new()
        .merge(rate_limited_auth)
        // Public status-page reads — embedded under /v1/public so the
        // boundary is explicit and obvious in the routing table.
        .nest("/public/status-pages", status_pages::public_router())
        // Public subscribe + unsubscribe.
        .nest("/public", subscribers::public_router())
        // Public inbound webhook receivers (Alertmanager). The {token} in
        // the URL is the auth — no session.
        .nest("/public", ingest::public_router())
        // Remote-agent wire protocol. The `Authorization: Bearer rmpa_…`
        // token is the auth — agents are headless, no session.
        .nest("/agent", agents::agent_router())
}

pub fn v1_protected() -> Router<AppState> {
    // ── RBAC layering ────────────────────────────────────────────────────
    // Three slices, all sitting under the outer `require_session` layer
    // (applied in lib.rs, which populates the `User` in extensions):
    //
    //  1. `editor_or_read` — monitors / groups / tags / notifications /
    //     templates / maintenance / status-pages / incidents / subscribers /
    //     stream. Wrapped by `require_write_or_readonly_get`: readonly users
    //     GET fine but 403 on any mutation; editors + admins write freely.
    //
    //  2. `admin_only` — users / audit-log / settings / api-keys / proxies /
    //     ingest-token admin. Wrapped by `require_admin` (403 for everyone
    //     who isn't admin, on every verb).
    //
    //  3. `self_service` — the caller acting on their OWN account: password
    //     change, 2FA setup/enable/disable, webpush subscription. No role
    //     gate — a readonly user must still be able to secure their own
    //     account, so these are exempt from the readonly write-guard.

    let editor_or_read = Router::new()
        // /v1/monitors itself + /v1/monitors/:id/notifications and /tags subroutes
        .nest(
            "/monitors",
            monitors::router()
                .merge(notifications::monitor_attach_router())
                .merge(tags::monitor_tag_router())
                .merge(monitor_groups::dep_router())
                .merge(routing::monitor_router())
                .merge(escalations::monitor_router()),
        )
        // /v1/monitor-groups CRUD + folder tags/channels
        .nest(
            "/monitor-groups",
            monitor_groups::router().merge(routing::group_router()),
        )
        // /v1/monitor-templates CRUD + instantiate (whole reusable monitor specs)
        .nest("/monitor-templates", monitor_templates::router())
        // /v1/tags CRUD
        .nest("/tags", tags::router())
        // /v1/notifications CRUD + channel tags
        .nest(
            "/notifications",
            notifications::router().merge(routing::channel_router()),
        )
        // /v1/notification-templates CRUD
        .nest("/notification-templates", templates::router())
        // /v1/escalation-policies CRUD — notification ladders (editor)
        .nest("/escalation-policies", escalations::router())
        // /v1/on-call-schedules CRUD — channel rotations for ladder steps
        .nest("/on-call-schedules", on_call::router())
        // /v1/maintenance-windows CRUD + attach/detach
        .nest("/maintenance-windows", maintenance::router())
        // /v1/status-pages admin CRUD (public read sits in v1_public)
        .nest("/status-pages", status_pages::admin_router())
        // /v1/status-pages/:id/incidents list+create
        .nest("/status-pages", incidents::page_router())
        // /v1/incidents/:id update/delete/resolve/updates
        .nest("/incidents", incidents::incident_router())
        // /v1/incident-templates CRUD — global canned incident-update library
        .nest("/incident-templates", incident_templates::router())
        // Subscribers admin (list/delete) — editors manage status pages.
        .merge(subscribers::admin_router())
        // /v1/stream/heartbeats — Server-Sent Events fan-out (GET only).
        .nest("/stream", stream::router())
        // /v1/metrics — external metric ingest (POST, write-gated) + reads.
        .nest("/metrics", metric_ingest::router())
        // /v1/agents read — feeds the probe-location picker (names +
        // locations only; token material never leaves the create call).
        .nest("/agents", agents::read_router())
        .route_layer(axum::middleware::from_fn(
            crate::auth::require_write_or_readonly_get,
        ));

    let admin_only = Router::new()
        // /v1/users admin CRUD + role management.
        .nest("/users", users::admin_router())
        // /v1/audit-log
        .nest("/audit-log", audit::router())
        // /v1/delivery-log — recent notification delivery attempts (admin)
        .nest("/delivery-log", delivery_log::router())
        // SMTP + retention settings.
        .merge(subscribers::settings_router())
        // /v1/api-keys — list/create/revoke
        .nest("/api-keys", api_keys::router())
        // /v1/proxies — list/create/delete/active
        .nest("/proxies", proxies::router())
        // /v1/agents — register (token mint) / rename / revoke
        .nest("/agents", agents::admin_router())
        // /v1/scheduled-reports — weekly uptime report CRUD (admin)
        .nest("/scheduled-reports", scheduled_reports::router())
        // /v1/status-pages/:id/ingest-tokens list+create (admin-managed)
        .nest("/status-pages", ingest::page_router())
        // /v1/ingest-tokens/:id revoke
        .nest("/ingest-tokens", ingest::token_router())
        .route_layer(axum::middleware::from_fn(crate::auth::require_admin));

    let self_service = Router::new()
        // /v1/users/me/password — self-service password change.
        .nest("/users", users::self_router())
        // /v1/auth/2fa/* — the caller securing their own account.
        .nest("/auth/2fa", totp::router())
        // /v1/webpush — the caller's own browser push subscription.
        .nest("/webpush", webpush::router())
        // /v1/me/prefs — the caller's own dashboard views + default folder.
        .nest("/me", prefs::router());

    Router::new()
        .merge(editor_or_read)
        .merge(admin_only)
        .merge(self_service)
}
