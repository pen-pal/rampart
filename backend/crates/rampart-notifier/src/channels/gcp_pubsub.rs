//! GCP Pub/Sub — Publish via the REST API.
//!
//! Auth: service-account JWT (RS256) → exchange at the OAuth2 token
//! endpoint for a Bearer access token → POST to
//! `pubsub.googleapis.com/v1/projects/<project>/topics/<topic>:publish`.
//!
//! The access token is cached in-memory per-channel until ~60s before
//! its expiry, so a burst of alerts does not mint a fresh JWT each time.
//! When the cache misses or the channel is freshly constructed we mint
//! a JWT and run the token-exchange round trip.
//!
//! Config:
//!   client_email   service-account email
//!   private_key    PEM-encoded PKCS#8 RSA private key (the `private_key`
//!                  field from the downloaded service-account JSON)
//!   project_id     GCP project (no need to embed in topic name)
//!   topic          short name (e.g. "rampart-alerts")

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use base64::Engine;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
pub struct GcpConfig {
    pub client_email: String,
    pub private_key: String,
    pub project_id: String,
    pub topic: String,
}

#[derive(Clone)]
struct CachedToken {
    access: String,
    expires_at: u64, // unix seconds
}

pub struct GcpPubsub {
    cfg: GcpConfig,
    client: reqwest::Client,
    cached: Mutex<Option<CachedToken>>,
}

impl GcpPubsub {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: GcpConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.client_email.is_empty()
            || cfg.private_key.is_empty()
            || cfg.project_id.is_empty()
            || cfg.topic.is_empty()
        {
            return Err(ChannelError::BadConfig(
                "client_email, private_key, project_id, topic required".into(),
            ));
        }
        // Sanity-check the key parses now so a misconfigured channel
        // fails on save rather than at first fire.
        EncodingKey::from_rsa_pem(cfg.private_key.as_bytes())
            .map_err(|e| ChannelError::BadConfig(format!("private_key: {e}")))?;
        Ok(Self {
            cfg,
            client: crate::http::client(),
            cached: Mutex::new(None),
        })
    }

    fn now() -> Result<u64, ChannelError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ChannelError::BadConfig(format!("clock: {e}")))
            .map(|d| d.as_secs())
    }

    /// Mint a service-account JWT good for ~1 hour, then exchange it
    /// at the OAuth2 token endpoint for a Bearer access token.
    async fn fetch_access_token(&self) -> Result<CachedToken, ChannelError> {
        let now = Self::now()?;
        let exp = now + 3600;
        #[derive(Serialize)]
        struct Claims<'a> {
            iss: &'a str,
            scope: &'a str,
            aud: &'a str,
            iat: u64,
            exp: u64,
        }
        let claims = Claims {
            iss: &self.cfg.client_email,
            scope: "https://www.googleapis.com/auth/pubsub",
            aud: "https://oauth2.googleapis.com/token",
            iat: now,
            exp,
        };
        let key = EncodingKey::from_rsa_pem(self.cfg.private_key.as_bytes())
            .map_err(|e| ChannelError::BadConfig(format!("private_key: {e}")))?;
        let jwt = encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|e| ChannelError::BadConfig(format!("jwt sign: {e}")))?;

        #[derive(Deserialize)]
        struct TokenResp {
            access_token: String,
            expires_in: u64,
        }
        let resp = self
            .client
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        let t: TokenResp = resp
            .json()
            .await
            .map_err(|e| ChannelError::Upstream(0, format!("malformed token response: {e}")))?;
        Ok(CachedToken {
            access: t.access_token,
            // Refresh ~60s before expiry so we never publish with a
            // token about to die mid-request.
            expires_at: now + t.expires_in.saturating_sub(60),
        })
    }

    async fn access_token(&self) -> Result<String, ChannelError> {
        let now = Self::now()?;
        {
            let g = self.cached.lock().expect("token mutex poisoned");
            if let Some(t) = g.as_ref() {
                if t.expires_at > now {
                    return Ok(t.access.clone());
                }
            }
        }
        let fresh = self.fetch_access_token().await?;
        let access = fresh.access.clone();
        *self.cached.lock().expect("token mutex poisoned") = Some(fresh);
        Ok(access)
    }
}

#[derive(Serialize)]
struct PubsubBody {
    messages: Vec<Message>,
}
#[derive(Serialize)]
struct Message {
    data: String, // base64
    attributes: serde_json::Value,
}

#[async_trait]
impl Channel for GcpPubsub {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError> {
        let token = self.access_token().await?;
        let url = format!(
            "https://pubsub.googleapis.com/v1/projects/{}/topics/{}:publish",
            self.cfg.project_id, self.cfg.topic
        );
        let payload = serde_json::json!({
            "subject":    subject,
            "body":       body,
            "monitor":    event.monitor.name,
            "monitor_id": event.monitor.id.0.to_string(),
            "status":     event.status_str(),
        });
        let data_b64 = base64::engine::general_purpose::STANDARD
            .encode(serde_json::to_vec(&payload).unwrap_or_default());
        let req_body = PubsubBody {
            messages: vec![Message {
                data: data_b64,
                attributes: serde_json::json!({
                    "monitor_id": event.monitor.id.0.to_string(),
                    "status":     event.status_str(),
                }),
            }],
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&req_body)
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
