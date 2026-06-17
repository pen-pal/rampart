//! Linear — create an issue via the GraphQL API.

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LinearConfig {
    pub api_key: String,
    pub team_id: String,
}

pub struct Linear {
    cfg: LinearConfig,
    client: reqwest::Client,
}

impl Linear {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: LinearConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_key.is_empty() || cfg.team_id.is_empty() {
            return Err(ChannelError::BadConfig("api_key + team_id required".into()));
        }
        Ok(Self {
            cfg,
            client: crate::http::client(),
        })
    }
}

#[derive(Serialize)]
struct GqlReq {
    query: String,
    variables: serde_json::Value,
}

#[async_trait]
impl Channel for Linear {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let req = GqlReq {
            query: r#"
                mutation IssueCreate($title: String!, $description: String, $teamId: String!) {
                  issueCreate(input: { title: $title, description: $description, teamId: $teamId }) {
                    success
                  }
                }
            "#.into(),
            variables: serde_json::json!({
                "title": subject,
                "description": body,
                "teamId": self.cfg.team_id,
            }),
        };
        let resp = self
            .client
            .post("https://api.linear.app/graphql")
            .header("Authorization", &self.cfg.api_key)
            .json(&req)
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
