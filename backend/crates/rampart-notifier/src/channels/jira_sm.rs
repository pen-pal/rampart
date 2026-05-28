//! Jira Service Management — create an incident via REST.
//! POST <site>/rest/api/3/issue with basic-auth (email + API token).

use crate::{Channel, ChannelError, Event};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct JiraSmConfig {
    pub site_url: String,
    pub email: String,
    pub api_token: String,
    pub project_key: String,
    #[serde(default = "default_issue_type")]
    pub issue_type: String,
}
fn default_issue_type() -> String {
    "Incident".into()
}

pub struct JiraSm {
    cfg: JiraSmConfig,
    client: reqwest::Client,
}

impl JiraSm {
    pub fn from_config(raw: &serde_json::Value) -> Result<Self, ChannelError> {
        let cfg: JiraSmConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ChannelError::BadConfig(e.to_string()))?;
        if cfg.api_token.is_empty() || cfg.project_key.is_empty() || cfg.email.is_empty() {
            return Err(ChannelError::BadConfig("missing required fields".into()));
        }
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    fields: Fields<'a>,
}
#[derive(Serialize)]
struct Fields<'a> {
    project: Named<'a>,
    summary: &'a str,
    description: Doc<'a>,
    issuetype: Named<'a>,
}
#[derive(Serialize)]
struct Named<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
}
#[derive(Serialize)]
struct Doc<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    content: Vec<Para<'a>>,
}
#[derive(Serialize)]
struct Para<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    content: Vec<Text<'a>>,
}
#[derive(Serialize)]
struct Text<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    text: &'a str,
}

#[async_trait]
impl Channel for JiraSm {
    async fn send(&self, subject: &str, body: &str, _event: &Event) -> Result<(), ChannelError> {
        let url = format!(
            "{}/rest/api/3/issue",
            self.cfg.site_url.trim_end_matches('/')
        );
        let payload = Payload {
            fields: Fields {
                project: Named {
                    key: Some(&self.cfg.project_key),
                    name: None,
                },
                summary: subject,
                description: Doc {
                    kind: "doc",
                    version: 1,
                    content: vec![Para {
                        kind: "paragraph",
                        content: vec![Text {
                            kind: "text",
                            text: body,
                        }],
                    }],
                },
                issuetype: Named {
                    key: None,
                    name: Some(&self.cfg.issue_type),
                },
            },
        };
        let resp = self
            .client
            .post(&url)
            .basic_auth(&self.cfg.email, Some(&self.cfg.api_token))
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
