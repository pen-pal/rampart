//! Web Push channel — fans an alert out to every browser subscription
//! attached to this notification row.
//!
//! Unlike the uniform `Channel` adapters this needs DB access (to load
//! subscriptions + the shared VAPID keypair) and per-subscription
//! delivery, so the notifier dispatches it through `send_all` directly
//! rather than the `Box<dyn Channel>` path.

use crate::channels::webpush_crypto::{encrypt, generate_vapid_keys, vapid_authorization};
use crate::{ChannelError, Event};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rampart_core::ids::NotificationId;
use rampart_db::store::Store;
use serde::Deserialize;
use tracing::{info, warn};

#[derive(Debug, Deserialize, Default)]
pub struct WebpushConfig {
    /// VAPID `sub` claim — a contact URI push services can reach. Defaults
    /// to a mailto placeholder if unset.
    #[serde(default)]
    pub subject: Option<String>,
}

/// Deliver to all subscriptions for `notification_id`. Best-effort: a
/// single failed endpoint doesn't fail the others, and 404/410 responses
/// prune the dead subscription.
pub async fn send_all(
    store: &dyn Store,
    notification_id: NotificationId,
    config: &serde_json::Value,
    subject: &str,
    body: &str,
    event: &Event,
) -> Result<(), ChannelError> {
    let cfg: WebpushConfig = serde_json::from_value(config.clone()).unwrap_or_default();
    let vapid_subject = cfg
        .subject
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "mailto:admin@localhost".to_string());

    // `Store`-callers compose the two primitives + their own generator (the
    // `get_or_create_vapid` free fn takes a closure, which isn't object-safe on
    // `&dyn Store`). Same get-then-set semantics as before.
    let vapid = match store
        .get_vapid_keys()
        .await
        .map_err(|e| ChannelError::BadConfig(format!("vapid keys: {e}")))?
    {
        Some(k) => k,
        None => {
            let (public, private) = generate_vapid_keys();
            let keys = rampart_db::webpush::VapidKeys { public, private };
            store
                .set_vapid_keys(&keys)
                .await
                .map_err(|e| ChannelError::BadConfig(format!("vapid keys: {e}")))?;
            keys
        }
    };

    let subs = store
        .list_webpush_subs(notification_id)
        .await
        .map_err(|e| ChannelError::BadConfig(format!("load subscriptions: {e}")))?;
    if subs.is_empty() {
        // Nothing subscribed yet — not an error.
        info!("webpush: no subscriptions to deliver to");
        return Ok(());
    }

    // Compact JSON payload the service worker renders as a notification.
    let payload = serde_json::json!({
        "title":      subject,
        "body":       body,
        "monitor":    event.monitor.name,
        "monitor_id": event.monitor.id.0.to_string(),
        "status":     event.status_str(),
    });
    let payload_bytes = serde_json::to_vec(&payload).unwrap_or_default();

    let client = crate::http::client();
    let mut delivered = 0usize;
    let mut last_err: Option<String> = None;

    for sub in subs {
        let p256dh = match URL_SAFE_NO_PAD.decode(sub.p256dh.trim_end_matches('=')) {
            Ok(b) => b,
            Err(e) => {
                warn!(endpoint = %sub.endpoint, error = %e, "bad p256dh");
                continue;
            }
        };
        let auth = match URL_SAFE_NO_PAD.decode(sub.auth.trim_end_matches('=')) {
            Ok(b) => b,
            Err(e) => {
                warn!(endpoint = %sub.endpoint, error = %e, "bad auth");
                continue;
            }
        };

        let enc = match encrypt(&payload_bytes, &p256dh, &auth) {
            Ok(e) => e,
            Err(e) => {
                warn!(endpoint = %sub.endpoint, error = %e, "encrypt failed");
                last_err = Some(e.to_string());
                continue;
            }
        };
        let auth_header =
            match vapid_authorization(&sub.endpoint, &vapid_subject, &vapid.private, &vapid.public)
            {
                Ok(h) => h,
                Err(e) => {
                    warn!(endpoint = %sub.endpoint, error = %e, "vapid header failed");
                    last_err = Some(e.to_string());
                    continue;
                }
            };

        let resp = client
            .post(&sub.endpoint)
            .header("Authorization", auth_header)
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("TTL", "86400")
            .header("Urgency", "high")
            .body(enc.body)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                delivered += 1;
            }
            Ok(r) if r.status().as_u16() == 404 || r.status().as_u16() == 410 => {
                // Subscription expired/gone — prune it so we stop trying.
                info!(endpoint = %sub.endpoint, "pruning expired webpush subscription");
                let _ = store.delete_webpush_sub_by_endpoint(&sub.endpoint).await;
            }
            Ok(r) => {
                let code = r.status().as_u16();
                let txt = r.text().await.unwrap_or_default();
                warn!(endpoint = %sub.endpoint, code, "webpush push rejected");
                last_err = Some(format!("{code}: {txt}"));
            }
            Err(e) => {
                warn!(endpoint = %sub.endpoint, error = %e, "webpush send failed");
                last_err = Some(e.to_string());
            }
        }
    }

    if delivered == 0 {
        if let Some(e) = last_err {
            return Err(ChannelError::Upstream(
                0,
                format!("all webpush deliveries failed: {e}"),
            ));
        }
    }
    info!(delivered, "webpush fan-out complete");
    Ok(())
}
