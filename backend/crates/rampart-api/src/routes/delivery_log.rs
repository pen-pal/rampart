//! Admin-only read API over the notification delivery log.
//!
//! Lists recent channel send attempts (success + failure) recorded by the
//! notifier. Keyset-paginated by `sent_at`, newest-first — the same shape
//! as the audit-log list route.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_db::delivery_log::DeliveryEntry;
use serde::Deserialize;
use time::OffsetDateTime;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
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
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<DeliveryEntry>>, ApiError> {
    Ok(Json(
        rampart_db::delivery_log::list(s.pool(), q.limit, q.before).await?,
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
    Path(id): Path<i64>,
) -> Result<Json<DeliveryEntry>, ApiError> {
    let entry = rampart_db::delivery_log::get(s.pool(), id)
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

#[cfg(test)]
mod tests {
    use rampart_core::monitor::NewMonitor;
    use rampart_core::{ChannelKind, MonitorKind};
    use rampart_db::delivery_log::{self, NewDelivery};
    use rampart_db::notifications::{self, NewNotification};
    use sqlx::PgPool;

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

        let entry = delivery_log::get(&pool, original.id)
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
        let all = delivery_log::list(&pool, 500, None).await.unwrap();
        let for_channel = all
            .iter()
            .filter(|r| r.notification_id == Some(channel.id))
            .count();
        assert_eq!(for_channel, 2);
    }
}
