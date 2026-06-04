//! Health endpoints.
//!
//! `/healthz` is liveness — always 200 if the process can answer.
//! `/readyz`  is readiness — 200 only when the DB is reachable.
//! `/metrics` exposes Prometheus text — gauges and counters covering
//! monitor status, latency, and domain/cert expiry.
//!
//! Both `/healthz` and `/metrics` surface the binary's version (read at
//! compile time from `CARGO_PKG_VERSION`, which inherits the workspace
//! `[workspace.package].version`). The dashboard reads the version off
//! `/healthz` for the header pill so it never drifts from the build.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

/// Workspace version baked in at compile time. Inherited from
/// `[workspace.package].version` via the `version.workspace = true`
/// member-crate setting, then re-exported by Cargo as the standard
/// `CARGO_PKG_VERSION` env var. A release is "bump that one line".
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(liveness))
        .route("/readyz", get(readiness))
        .route("/metrics", get(metrics))
}

async fn liveness() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "status": "alive", "version": VERSION })),
    )
}

async fn readiness(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    // A cheap roundtrip — confirms the pool can hand out a connection AND
    // the database is responding to queries. Avoids pg_isready false-positives.
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(state.pool())
        .await
        .map_err(rampart_db::DbError::from)?;
    Ok((StatusCode::OK, Json(json!({ "status": "ready" }))))
}

/// Prometheus exposition format. Minimal scaffolding — adds real counters
/// as the scheduler/correlator come online. The format is just text so we
/// hand-format rather than pull in the prometheus crate for one metric.
/// Prometheus exposition. Each line group is a single
/// HELP / TYPE / samples triple per the text format spec; consumers
/// can scrape every 15-60s at negligible DB cost. Failures in any
/// individual aggregate are degraded to a comment line rather than
/// failing the whole scrape — Prometheus prefers partial truth to
/// stale-or-missing.
async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    use std::fmt::Write;
    let mut body = String::with_capacity(1024);

    let _ = writeln!(body, "# HELP rampart_build_info Build metadata.");
    let _ = writeln!(body, "# TYPE rampart_build_info gauge");
    let _ = writeln!(body, "rampart_build_info{{version=\"{VERSION}\"}} 1");

    let pool = state.pool();

    // ── monitors by status ─────────────────────────────────────────
    let _ = writeln!(
        body,
        "# HELP rampart_monitors Number of monitors broken down by current status.",
    );
    let _ = writeln!(body, "# TYPE rampart_monitors gauge");
    match rampart_db::metrics::monitors_by_status(pool).await {
        Ok(rows) => {
            for (status, count) in rows {
                let _ = writeln!(
                    body,
                    "rampart_monitors{{status=\"{status}\"}} {count}",
                    status = sanitize_label(&status),
                );
            }
        }
        Err(e) => {
            let _ = writeln!(body, "# error querying monitors_by_status: {e}");
        }
    }

    // ── monitors by probe kind ─────────────────────────────────────
    let _ = writeln!(
        body,
        "# HELP rampart_monitors_by_kind Number of monitors per probe kind.",
    );
    let _ = writeln!(body, "# TYPE rampart_monitors_by_kind gauge");
    match rampart_db::metrics::monitors_by_kind(pool).await {
        Ok(rows) => {
            for (kind, count) in rows {
                let _ = writeln!(
                    body,
                    "rampart_monitors_by_kind{{kind=\"{kind}\"}} {count}",
                    kind = sanitize_label(&kind),
                );
            }
        }
        Err(e) => {
            let _ = writeln!(body, "# error querying monitors_by_kind: {e}");
        }
    }

    // ── active notification channels ───────────────────────────────
    let _ = writeln!(
        body,
        "# HELP rampart_channels_active Number of currently-active notification channels.",
    );
    let _ = writeln!(body, "# TYPE rampart_channels_active gauge");
    match rampart_db::metrics::channels_active(pool).await {
        Ok(count) => {
            let _ = writeln!(body, "rampart_channels_active {count}");
        }
        Err(e) => {
            let _ = writeln!(body, "# error querying channels_active: {e}");
        }
    }

    // ── web-push subscriber count ──────────────────────────────────
    let _ = writeln!(
        body,
        "# HELP rampart_webpush_subscribers Number of registered web-push subscribers.",
    );
    let _ = writeln!(body, "# TYPE rampart_webpush_subscribers gauge");
    match rampart_db::metrics::webpush_subscribers(pool).await {
        Ok(count) => {
            let _ = writeln!(body, "rampart_webpush_subscribers {count}");
        }
        Err(e) => {
            let _ = writeln!(body, "# error querying webpush_subscribers: {e}");
        }
    }

    // ── recent heartbeats (24h window) ─────────────────────────────
    // Window chosen so a 30s Prometheus scrape against the dashboard's
    // typical row count returns under 50ms even on cold caches.
    let _ = writeln!(
        body,
        "# HELP rampart_heartbeats_24h Heartbeats persisted in the trailing 24 hours, by status.",
    );
    let _ = writeln!(body, "# TYPE rampart_heartbeats_24h gauge");
    match rampart_db::metrics::heartbeats_recent_by_status(pool, 86_400).await {
        Ok(rows) => {
            for (status, count) in rows {
                let _ = writeln!(
                    body,
                    "rampart_heartbeats_24h{{status=\"{status}\"}} {count}",
                    status = sanitize_label(&status),
                );
            }
        }
        Err(e) => {
            let _ = writeln!(body, "# error querying heartbeats_recent_by_status: {e}");
        }
    }

    // ── open incidents ─────────────────────────────────────────────
    let _ = writeln!(
        body,
        "# HELP rampart_incidents_open Number of currently-open / unresolved incidents.",
    );
    let _ = writeln!(body, "# TYPE rampart_incidents_open gauge");
    match rampart_db::metrics::incidents_open(pool).await {
        Ok(count) => {
            let _ = writeln!(body, "rampart_incidents_open {count}");
        }
        Err(e) => {
            let _ = writeln!(body, "# error querying incidents_open: {e}");
        }
    }

    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        body,
    )
}

/// Strip / escape characters that would break the Prometheus exposition
/// label-value syntax. The text format requires `\`, `"`, and newline
/// to be backslash-escaped; everything else is verbatim. All our label
/// sources are enum variants or string columns under our control, but
/// guarding here keeps an unexpected schema change from corrupting a
/// scrape.
fn sanitize_label(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}
