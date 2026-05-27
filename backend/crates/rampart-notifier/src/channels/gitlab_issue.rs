//! GitLab Issues — POST /projects/:id/issues with PRIVATE-TOKEN header.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GitlabConfig {
    pub base_url:   String,
    pub token:      String,
    pub project_id: String,
}

pub struct GitlabIssue { cfg: GitlabConfig, client: reqwest::Client }

impl GitlabIssue {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: GitlabConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.token.is_empty() || cfg.project_id.is_empty() {
            return Err(ChannelError::BadConfig("token + project_id required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    title:       &'a str,
    description: &'a str,
}

#[async_trait]
impl Channel for GitlabIssue {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let base = if self.cfg.base_url.is_empty() { "https://gitlab.com" } else { self.cfg.base_url.trim_end_matches('/') };
        let url = format!("{}/api/v4/projects/{}/issues", base, self.cfg.project_id);
        let resp = self.client.post(&url)
            .header("PRIVATE-TOKEN", &self.cfg.token)
            .json(&Payload { title: subject, description: body })
            .send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
