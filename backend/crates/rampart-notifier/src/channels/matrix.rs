//! Matrix room — m.room.message via the client-server API.
//!
//! Config:
//!   homeserver  — base URL, e.g. https://matrix.org
//!   access_token — bot account access token
//!   room_id      — `!roomid:server`

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct MatrixConfig {
    pub homeserver: String,
    pub access_token: String,
    pub room_id: String,
}

pub struct Matrix {
    cfg: MatrixConfig,
    client: reqwest::Client,
}

impl Matrix {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: MatrixConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if !cfg.homeserver.starts_with("http") {
            return Err(ChannelError::BadConfig(
                "homeserver must be http(s)://".into(),
            ));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct MsgBody<'a> {
    msgtype: &'static str,
    body: &'a str,
}

#[async_trait]
impl Channel for Matrix {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        // PUT with a random txn id — idempotency required by the spec.
        let txn = Uuid::new_v4();
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            self.cfg.homeserver.trim_end_matches('/'),
            urlencode(&self.cfg.room_id),
            txn,
        );
        let full = format!("{subject}\n\n{body}");
        let payload = MsgBody {
            msgtype: "m.text",
            body: &full,
        };
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.cfg.access_token)
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let txt = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Upstream(code, txt));
        }
        Ok(())
    }
}

// Tiny encoder for the room_id path segment — leading '!' and the colon
// in '!room:server' both need escaping. Pulling in url::form_urlencoded
// for one call is silly; this covers the matrix grammar.
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
