//! Admin-only read API over the notification delivery log.
//!
//! Lists recent channel send attempts (success + failure) recorded by the
//! notifier. Keyset-paginated by `sent_at`, newest-first — the same shape
//! as the audit-log list route.

use crate::auth::OrgContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Extension, Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_db::delivery_log::DeliveryEntry;
use serde::Deserialize;
use time::OffsetDateTime;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/export.csv", get(export_csv))
        .route("/{id}/retry", post(retry))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    /// Keyset cursor: return rows strictly older than this `sent_at`
    /// (RFC3339). Omit for the first (newest) page.
    #[serde(default, with = "time::serde::rfc3339::option")]
    before: Option<OffsetDateTime>,
}

fn default_limit() -> i64 {
    100
}

async fn list(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<DeliveryEntry>>, ApiError> {
    Ok(Json(
        rampart_db::delivery_log::list(s.pool(), q.limit, q.before, org.org_id).await?,
    ))
}

/// Re-send a past delivery attempt through its original channel.
///
/// Loads the logged attempt (404 if the id is unknown). If the row still
/// references a live channel, the notifier re-sends via that same channel and
/// records a NEW delivery_log row for the retry, which is returned. If the
/// channel was deleted (`notification_id IS NULL` — the FK is ON DELETE SET
/// NULL), there's nothing to re-send through, so we reject with 409.
async fn retry(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
    Path(id): Path<i64>,
) -> Result<Json<DeliveryEntry>, ApiError> {
    let entry = rampart_db::delivery_log::get(s.pool(), id, org.org_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let channel_id = entry.notification_id.ok_or_else(|| {
        ApiError::Conflict("cannot retry: the channel for this delivery has been deleted".into())
    })?;

    let attempt = rampart_notifier::service::resend_delivery(s.pool(), &entry, channel_id)
        .await
        .map_err(|e| ApiError::BadRequest(format!("retry failed: {e}")))?;
    Ok(Json(attempt))
}

/// Upper bound on rows pulled into a single CSV export. Large enough for a
/// routine operational dump of the delivery log, small enough that the whole
/// payload stays comfortably in memory. An export hitting this ceiling is
/// truncated to the newest [`EXPORT_CAP`] attempts; a note is logged so the
/// truncation is visible in the server logs.
const EXPORT_CAP: i64 = 50_000;

/// CSV export of delivery attempts (admin-only — gated by the `admin_only`
/// layer the delivery-log router is nested under).
///
/// Mirrors the audit-log `list_csv` export: materialises up to [`EXPORT_CAP`]
/// rows newest-first and emits them as `text/csv` with an attachment
/// `content-disposition`. Fields that contain a comma, quote, or newline are
/// quoted per RFC 4180.
async fn export_csv(
    State(s): State<AppState>,
    Extension(org): Extension<OrgContext>,
) -> Result<impl IntoResponse, ApiError> {
    let rows = rampart_db::delivery_log::list_all(s.pool(), EXPORT_CAP, org.org_id).await?;
    if rows.len() as i64 == EXPORT_CAP {
        tracing::warn!(
            cap = EXPORT_CAP,
            "delivery-log CSV export hit the row cap; output truncated to the newest {EXPORT_CAP} attempts",
        );
    }

    let fmt = time::format_description::well_known::Rfc3339;
    let mut body = String::with_capacity(64 + rows.len() * 96);
    body.push_str("sent_at,channel_kind,event_kind,monitor_id,ok,error\n");
    for r in &rows {
        body.push_str(&format!(
            "{},{},{},{},{},{}\n",
            r.sent_at.format(&fmt).unwrap_or_default(),
            csv_escape(&r.channel_kind),
            csv_escape(&r.event_kind),
            r.monitor_id.map(|m| m.to_string()).unwrap_or_default(),
            r.ok,
            csv_escape(r.error.as_deref().unwrap_or("")),
        ));
    }

    Ok((
        [
            ("content-type", "text/csv; charset=utf-8".to_string()),
            (
                "content-disposition",
                "attachment; filename=\"delivery-log.csv\"".to_string(),
            ),
        ],
        body,
    ))
}

use crate::csv::csv_escape;

#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use rampart_core::monitor::NewMonitor;
    use rampart_core::{ChannelKind, MonitorKind};
    use rampart_db::delivery_log::{self, NewDelivery};
    use rampart_db::notifications::{self, NewNotification};
    use sqlx::PgPool;
    use std::sync::Arc;
    use tokio::sync::Notify;
    use tower::ServiceExt;

    fn webhook_channel() -> NewNotification {
        NewNotification {
            kind: ChannelKind::Webhook,
            name: "retry-ch".into(),
            // Unroutable host: the re-send will fail, but a NEW row must
            // still be recorded for the attempt.
            config: serde_json::json!({"url": "http://127.0.0.1:1/hook"}),
            active: true,
            template_id: None,
            cooldown_seconds: 0,
            digest_window_secs: 0,
            quiet_hours_start: None,
            quiet_hours_end: None,
            rate_limit_per_hour: 0,
        }
    }

    fn http_monitor() -> NewMonitor {
        NewMonitor {
            name: "retry-mon".into(),
            kind: MonitorKind::Http,
            url: Some("https://retry.example.com".into()),
            hostname: None,
            port: None,
            config: serde_json::Value::Null,
            interval_seconds: 60,
            timeout_seconds: 10,
            max_retries: 0,
            retry_interval_sec: 60,
            resend_interval_sec: 0,
            upside_down: false,
            http_method: "GET".into(),
            http_body: None,
            http_headers: None,
            accepted_statuses: vec![200],
            follow_redirect: true,
            ignore_tls: false,
            proxy_id: None,
            group_id: None,
            slo_target_pct: None,
            slo_window_days: None,
            agent_id: None,
            escalation_policy_id: None,
        }
    }

    /// Seed a failed delivery row for a real channel, re-send it, and assert
    /// the retry recorded a brand-new delivery_log row (distinct id) tied to
    /// the same channel.
    #[sqlx::test(migrations = "../../migrations")]
    async fn retry_records_a_new_delivery_row(pool: PgPool) {
        // The re-send path builds a reqwest client; reqwest 0.13 uses the
        // `rustls-no-provider` feature, so install ring as the process-wide
        // default the same way `main` does (idempotent across tests).
        let _ = rustls::crypto::ring::default_provider().install_default();

        let monitor = rampart_db::monitors::create(&pool, http_monitor())
            .await
            .unwrap();
        let channel = notifications::create(&pool, webhook_channel())
            .await
            .unwrap();

        let original = delivery_log::record(
            &pool,
            NewDelivery {
                notification_id: Some(channel.id),
                channel_kind: "webhook",
                event_kind: "monitor_down",
                monitor_id: Some(monitor.id.0),
                ok: false,
                error: Some("connection refused"),
            },
        )
        .await
        .unwrap();
        assert!(!original.ok);

        let def_org = rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);
        let entry = delivery_log::get(&pool, original.id, def_org)
            .await
            .unwrap()
            .unwrap();
        let attempt = rampart_notifier::service::resend_delivery(&pool, &entry, channel.id)
            .await
            .unwrap();

        // A distinct, newer row was appended for the retry attempt.
        assert_ne!(attempt.id, original.id, "retry must create a new row");
        assert!(attempt.id > original.id);
        assert_eq!(attempt.notification_id, Some(channel.id));

        // The log now holds two rows for this channel.
        let all = delivery_log::list(&pool, 500, None, def_org).await.unwrap();
        let for_channel = all
            .iter()
            .filter(|r| r.notification_id == Some(channel.id))
            .count();
        assert_eq!(for_channel, 2);
    }

    /// Hit GET /export.csv on the delivery-log router and assert a `text/csv`
    /// response whose first line is the documented header row.
    #[sqlx::test(migrations = "../../migrations")]
    async fn export_csv_returns_text_csv_with_header(pool: PgPool) {
        // Seed one delivery so the body has a data row beyond the header.
        delivery_log::record(
            &pool,
            NewDelivery {
                notification_id: None,
                channel_kind: "webhook",
                event_kind: "monitor_down",
                monitor_id: None,
                ok: true,
                error: None,
            },
        )
        .await
        .unwrap();

        let state = AppState::new(pool, Arc::new(Notify::new()));
        let app = super::router().with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/export.csv")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(ct.starts_with("text/csv"), "content-type was {ct}");

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        let header = body.lines().next().unwrap();
        assert_eq!(
            header,
            "sent_at,channel_kind,event_kind,monitor_id,ok,error"
        );
    }
}
