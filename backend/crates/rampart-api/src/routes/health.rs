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
        Json(json!({
            "status": "alive",
            "version": VERSION,
            // Surfaced so the dashboard can warn admins when channel secrets
            // are stored unencrypted (no RAMPART_SECRET_KEY configured).
            "secrets_at_rest": if rampart_db::secrets::is_enabled() { "encrypted" } else { "plaintext" },
        })),
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

    // Secrets-at-rest posture (1 = channel credentials encrypted via
    // RAMPART_SECRET_KEY, 0 = stored plaintext). Lets ops alert on an
    // unencrypted install.
    let _ = writeln!(
        body,
        "# HELP rampart_secrets_at_rest_encrypted Whether channel secrets are encrypted at rest.",
    );
    let _ = writeln!(body, "# TYPE rampart_secrets_at_rest_encrypted gauge");
    let _ = writeln!(
        body,
        "rampart_secrets_at_rest_encrypted {}",
        if rampart_db::secrets::is_enabled() { 1 } else { 0 },
    );

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

    // ── alerting-pipeline gauges (rules / firing / findings / escalations) ─
    match rampart_db::metrics::pipeline_gauges(pool).await {
        Ok(g) => {
            let _ = writeln!(body, "# HELP rampart_metric_rules Configured metric threshold rules.");
            let _ = writeln!(body, "# TYPE rampart_metric_rules gauge");
            let _ = writeln!(body, "rampart_metric_rules {}", g.metric_rules);
            let _ = writeln!(body, "# HELP rampart_metric_rules_firing Metric rules currently firing.");
            let _ = writeln!(body, "# TYPE rampart_metric_rules_firing gauge");
            let _ = writeln!(body, "rampart_metric_rules_firing {}", g.metric_rules_firing);
            let _ = writeln!(body, "# HELP rampart_telemetry_rules Configured telemetry alert rules.");
            let _ = writeln!(body, "# TYPE rampart_telemetry_rules gauge");
            let _ = writeln!(body, "rampart_telemetry_rules {}", g.telemetry_rules);
            let _ = writeln!(body, "# HELP rampart_telemetry_rules_firing Telemetry rules currently firing.");
            let _ = writeln!(body, "# TYPE rampart_telemetry_rules_firing gauge");
            let _ = writeln!(body, "rampart_telemetry_rules_firing {}", g.telemetry_rules_firing);
            let _ = writeln!(body, "# HELP rampart_detection_rules_enabled Enabled SIEM detection rules.");
            let _ = writeln!(body, "# TYPE rampart_detection_rules_enabled gauge");
            let _ = writeln!(body, "rampart_detection_rules_enabled {}", g.detection_rules_enabled);
            let _ = writeln!(body, "# HELP rampart_detection_findings_open Unacknowledged detection findings.");
            let _ = writeln!(body, "# TYPE rampart_detection_findings_open gauge");
            let _ = writeln!(body, "rampart_detection_findings_open {}", g.detection_findings_open);
            let _ = writeln!(body, "# HELP rampart_escalations_open Unresolved escalation episodes.");
            let _ = writeln!(body, "# TYPE rampart_escalations_open gauge");
            let _ = writeln!(body, "rampart_escalations_open {}", g.escalations_open);
            let _ = writeln!(body, "# HELP rampart_error_issues_unresolved Unresolved error-tracking issues.");
            let _ = writeln!(body, "# TYPE rampart_error_issues_unresolved gauge");
            let _ = writeln!(body, "rampart_error_issues_unresolved {}", g.error_issues_unresolved);
        }
        Err(e) => {
            let _ = writeln!(body, "# error querying pipeline_gauges: {e}");
        }
    }

    // ── ingest volume (trailing 24h, by tier) ──────────────────────
    match rampart_db::metrics::ingest_gauges(pool).await {
        Ok(g) => {
            let _ = writeln!(body, "# HELP rampart_ingest_24h Telemetry records ingested in the trailing 24h, by tier.");
            let _ = writeln!(body, "# TYPE rampart_ingest_24h gauge");
            let _ = writeln!(body, "rampart_ingest_24h{{tier=\"logs\"}} {}", g.logs_24h);
            let _ = writeln!(body, "rampart_ingest_24h{{tier=\"spans\"}} {}", g.spans_24h);
            let _ = writeln!(body, "rampart_ingest_24h{{tier=\"error_events\"}} {}", g.error_events_24h);
        }
        Err(e) => {
            let _ = writeln!(body, "# error querying ingest_gauges: {e}");
        }
    }

    // ── per-tier storage footprint (heap + indexes + TOAST) ────────
    match rampart_db::metrics::storage_usage(pool).await {
        Ok(tables) => {
            let _ = writeln!(body, "# HELP rampart_table_bytes On-disk size of a telemetry table (bytes, incl. indexes + TOAST).");
            let _ = writeln!(body, "# TYPE rampart_table_bytes gauge");
            for tbl in tables {
                let _ = writeln!(
                    body,
                    "rampart_table_bytes{{table=\"{}\"}} {}",
                    sanitize_label(&tbl.table),
                    tbl.bytes,
                );
            }
        }
        Err(e) => {
            let _ = writeln!(body, "# error querying storage_usage: {e}");
        }
    }

    // ── HTTP request counter + latency histogram ───────────────────
    // Lives in AppState; populated by the `record_http_metrics`
    // middleware layered above the router in `lib.rs::build_router`.
    body.push_str(&state.http_metrics().render());

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
