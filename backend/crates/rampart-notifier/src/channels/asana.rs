//! Asana — POST /api/1.0/tasks with bearer PAT.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct AsanaConfig {
    pub access_token: String,
    pub workspace: String,
    pub project: String,
}

pub struct Asana {
    cfg: AsanaConfig,
    client: reqwest::Client,
}

impl Asana {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: AsanaConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.access_token.is_empty() || cfg.workspace.is_empty() || cfg.project.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Wrap<'a> {
    data: Inner<'a>,
}
#[derive(Serialize)]
struct Inner<'a> {
    name: &'a str,
    notes: &'a str,
    workspace: &'a str,
    projects: Vec<&'a str>,
}

#[async_trait]
impl Channel for Asana {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let payload = Wrap {
            data: Inner {
                name: subject,
                notes: body,
                workspace: &self.cfg.workspace,
                projects: vec![&self.cfg.project],
            },
        };
        let resp = self
            .client
            .post("https://app.asana.com/api/1.0/tasks")
            .bearer_auth(&self.cfg.access_token)
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
