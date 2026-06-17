//! Azure Service Bus — POST a message into a queue/topic using a SAS
//! token. SAS auth is HMAC-SHA256 over `url-encoded-resource + "\n" +
//! expiry-epoch`, base64-encoded, wrapped in a SharedAccessSignature
//! Authorization header. The send endpoint accepts a plain JSON body and
//! returns 201 on success.
//!
//! Docs:
//!   https://learn.microsoft.com/en-us/rest/api/servicebus/send-message-to-queue
//!   https://learn.microsoft.com/en-us/rest/api/eventhub/generate-sas-token

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
pub struct AzureSbConfig {
    /// e.g. "my-namespace" (omit ".servicebus.windows.net")
    pub namespace: String,
    /// Queue or topic name.
    pub entity: String,
    /// Name of the SAS policy (e.g. "RootManageSharedAccessKey" or
    /// a send-only policy).
    pub sas_key_name: String,
    /// Primary or secondary key value associated with the policy.
    pub sas_key: String,
    /// Token TTL in seconds. Defaults to 5 minutes — long enough for
    /// retries, short enough to bound replay damage if the token leaks.
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u64,
}
fn default_ttl() -> u64 {
    300
}

pub struct AzureServicebus {
    cfg: AzureSbConfig,
    client: reqwest::Client,
}

impl AzureServicebus {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: AzureSbConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.namespace.is_empty()
            || cfg.entity.is_empty()
            || cfg.sas_key_name.is_empty()
            || cfg.sas_key.is_empty()
        {
            return Err(ChannelError::BadConfig(
                "namespace, entity, sas_key_name + sas_key required".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }

    fn sign(&self, target_uri: &str) -> Result<String, ChannelError> {
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ChannelError::BadConfig(format!("clock: {e}")))?
            .as_secs()
            + self.cfg.ttl_seconds;
        let encoded_uri = urlencoding::encode(target_uri).into_owned();
        let to_sign = format!("{encoded_uri}\n{expiry}");
        let mut mac = Hmac::<Sha256>::new_from_slice(self.cfg.sas_key.as_bytes())
            .map_err(|e| ChannelError::BadConfig(format!("hmac key: {e}")))?;
        mac.update(to_sign.as_bytes());
        let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        let sig_enc = urlencoding::encode(&sig).into_owned();
        Ok(format!(
            "SharedAccessSignature sr={encoded_uri}&sig={sig_enc}&se={expiry}&skn={}",
            self.cfg.sas_key_name
        ))
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    subject: &'a str,
    body: &'a str,
    monitor: &'a str,
    monitor_id: String,
    status: &'a str,
}

#[async_trait]
impl Channel for AzureServicebus {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let target = format!(
            "https://{}.servicebus.windows.net/{}",
            self.cfg.namespace, self.cfg.entity
        );
        let url = format!("{target}/messages");
        let auth = self.sign(&target)?;
        let payload = Payload {
            subject,
            body,
            monitor: &event.monitor.name,
            monitor_id: event.monitor.id.0.to_string(),
            status: event.status_str(),
        };
        let resp = self
            .client
            .post(&url)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }
}
