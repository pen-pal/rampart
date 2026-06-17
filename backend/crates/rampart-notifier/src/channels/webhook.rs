//! Generic webhook.
//!
//! POSTs a JSON payload with monitor + heartbeat + rendered body. Lets
//! users route into Zapier / n8n / Make / their own service to fan
//! further out. The body is the rendered template; for fully custom
//! payloads users should run their own service.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    /// Optional custom headers, e.g. `Authorization: Bearer …`.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Optional shared secret. When set, the request carries
    /// `X-Rampart-Signature: sha256=<hex>` — HMAC-SHA256 over the
    /// raw JSON body. Receivers can verify with the same secret to
    /// confirm the payload originated from Rampart.
    #[serde(default)]
    pub secret: Option<String>,
}

fn default_method() -> String {
    "POST".into()
}

#[derive(Debug)]
pub struct Webhook {
    cfg: WebhookConfig,
    client: reqwest::Client,
}

impl Webhook {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: WebhookConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.url.starts_with("http://") && !cfg.url.starts_with("https://") {
            return Err(ChannelError::BadConfig(
                "url must start with http:// or https://".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct WebhookPayload<'a> {
    subject: &'a str,
    body: &'a str,
    monitor: WebhookMonitor<'a>,
    status: &'a str,
    prev_status: &'a str,
    latency_ms: Option<i32>,
    status_code: Option<i32>,
    msg: Option<&'a str>,
    ts: String,
}

#[derive(Serialize)]
struct WebhookMonitor<'a> {
    id: String,
    name: &'a str,
    kind: &'a rampart_core::MonitorKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_non_http_url() {
        let err = Webhook::from_config(&json!({"url": "ftp://example.com"})).unwrap_err();
        assert!(matches!(err, ChannelError::BadConfig(_)));
    }

    #[test]
    fn accepts_http_and_https() {
        crate::init_test_crypto();
        assert!(Webhook::from_config(&json!({"url": "http://example.com/x"})).is_ok());
        assert!(Webhook::from_config(&json!({"url": "https://example.com/x"})).is_ok());
    }

    #[test]
    fn defaults_method_to_post() {
        let raw = json!({"url": "https://example.com/x"});
        let cfg: WebhookConfig = serde_json::from_value(raw).unwrap();
        assert_eq!(cfg.method, "POST");
    }

    #[test]
    fn keeps_custom_headers() {
        let raw = json!({"url": "https://example.com/x", "headers": {"X-Token": "abc"}});
        let cfg: WebhookConfig = serde_json::from_value(raw).unwrap();
        assert_eq!(cfg.headers.get("X-Token").map(String::as_str), Some("abc"));
    }
}

#[async_trait]
impl Channel for Webhook {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let payload = WebhookPayload {
            subject,
            body,
            monitor: WebhookMonitor {
                id: event.monitor.id.0.to_string(),
                name: &event.monitor.name,
                kind: &event.monitor.kind,
            },
            status: event.status_str(),
            prev_status: event.prev_status_str(),
            latency_ms: event.heartbeat.latency_ms,
            status_code: event.heartbeat.status_code,
            msg: event.heartbeat.msg.as_deref(),
            ts: event
                .heartbeat
                .ts
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
        };

        let method = reqwest::Method::from_bytes(self.cfg.method.as_bytes())
            .map_err(|e| ChannelError::BadConfig(format!("invalid method: {e}")))?;

        // Serialize the body up-front so we can sign the exact bytes
        // that go on the wire. Avoids the (subtle) hash-mismatch class
        // of bugs where the receiver hashes one variant of JSON and we
        // hashed another.
        let body_bytes = serde_json::to_vec(&payload)
            .map_err(|e| ChannelError::BadConfig(format!("payload serialize: {e}")))?;

        let mut req = self
            .client
            .request(method, &self.cfg.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body_bytes.clone());

        for (k, v) in &self.cfg.headers {
            req = req.header(k, v);
        }
        if let Some(secret) = self.cfg.secret.as_deref() {
            type HmacSha256 = Hmac<Sha256>;
            let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
                .map_err(|e| ChannelError::BadConfig(format!("hmac key: {e}")))?;
            mac.update(&body_bytes);
            let sig = hex::encode(mac.finalize().into_bytes());
            req = req.header("X-Rampart-Signature", format!("sha256={sig}"));
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, body));
        }
        Ok(())
    }
}
