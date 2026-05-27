//! GitHub Issues — POST /repos/:owner/:repo/issues with bearer PAT.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GithubConfig {
    pub token: String,
    pub owner: String,
    pub repo:  String,
    #[serde(default)]
    pub labels: Vec<String>,
}

pub struct GithubIssue { cfg: GithubConfig, client: reqwest::Client }

impl GithubIssue {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: GithubConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.token.is_empty() || cfg.owner.is_empty() || cfg.repo.is_empty() {
            return Err(ChannelError::BadConfig("token + owner + repo required".into()));
        }
        Ok(Self { cfg, client: reqwest::Client::new() })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    title:  &'a str,
    body:   &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: &'a Vec<String>,
}

#[async_trait]
impl Channel for GithubIssue {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!("https://api.github.com/repos/{}/{}/issues",
            self.cfg.owner, self.cfg.repo);
        let payload = Payload { title: subject, body, labels: &self.cfg.labels };
        let resp = self.client.post(&url)
            .bearer_auth(&self.cfg.token)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "rampart")
            .json(&payload).send().await?;
        if !resp.status().is_success() {
            return Err(ChannelError::Upstream(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }
}
