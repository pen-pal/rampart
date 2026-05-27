//! Halo PSA — ticket creation via REST API (client-credentials OAuth).
//!
//! Two-step: POST /auth/token to exchange client_credentials, then POST
//! /api/tickets with the bearer access token. We do both per call —
//! token caching skipped for v1 (notifications are infrequent enough
//! that the extra round-trip is irrelevant).

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct HaloPsaConfig {
    pub base_url:      String,
    pub client_id:     String,
    pub client_secret: String,
    pub team:          String,
    pub ticket_type_id: i64,
}

pub struct HaloPsa { cfg: HaloPsaConfig, client: reqwest::Client }

impl HaloPsa {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: HaloPsaConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.client_id.is_empty() || cfg.client_secret.is_empty() {
            return Err(ChannelError::BadConfig("client_id + client_secret required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Deserialize)]
struct TokenResp { access_token: String }

#[derive(Serialize)]
struct Ticket<'a> {
    summary:        &'a str,
    details:        &'a str,
    tickettype_id:  i64,
    team:           &'a str,
}

#[async_trait]
impl Channel for HaloPsa {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let token_url = format!("{}/auth/token", self.cfg.base_url.trim_end_matches('/'));
        let token_resp = self.client.post(&token_url)
            .form(&[
                ("grant_type",    "client_credentials"),
                ("client_id",     self.cfg.client_id.as_str()),
                ("client_secret", self.cfg.client_secret.as_str()),
                ("scope",         "all"),
            ])
            .send().await?;
        if !token_resp.status().is_success() {
            return Err(ChannelError::Upstream(token_resp.status().as_u16(), token_resp.text().await.unwrap_or_default()));
        }
        let token: TokenResp = token_resp.json().await.map_err(ChannelError::from)?;

        let tickets_url = format!("{}/api/tickets", self.cfg.base_url.trim_end_matches('/'));
        let payload = vec![Ticket {
            summary: subject,
            details: body,
            tickettype_id: self.cfg.ticket_type_id,
            team: &self.cfg.team,
        }];
        let resp = self.client.post(&tickets_url)
            .bearer_auth(&token.access_token)
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
