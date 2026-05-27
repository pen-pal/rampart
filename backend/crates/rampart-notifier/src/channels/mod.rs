//! Channel adapters.
//!
//! One file per channel. Each implements `Channel::send`. The
//! `dispatch` helper inspects the kind tag and instantiates the right
//! adapter from a JSON config blob.

pub mod apprise;
pub mod dingtalk;
pub mod discord;
pub mod email;
pub mod feishu;
pub mod google_chat;
pub mod gotify;
pub mod line;
pub mod matrix;
pub mod mattermost;
pub mod ntfy;
pub mod pagerduty;
pub mod pushover;
pub mod rocketchat;
pub mod slack;
pub mod teams;
pub mod telegram;
pub mod twilio;
pub mod webhook;
pub mod wecom;

use crate::{Channel, ChannelError, Event};
use rampart_core::ChannelKind;

/// Build an adapter from the persisted `(kind, config)` pair and ask it
/// to send. Returns `Err(BadConfig)` for kinds we haven't built yet so
/// the caller can log it instead of crashing the dispatcher.
pub async fn dispatch(
    kind: ChannelKind,
    config: &serde_json::Value,
    subject: &str,
    body: &str,
    event: &Event,
) -> Result<(), ChannelError> {
    let channel: Box<dyn Channel> = match kind {
        ChannelKind::Slack => Box::new(slack::Slack::from_config(config)?),
        ChannelKind::Discord => Box::new(discord::Discord::from_config(config)?),
        ChannelKind::Teams => Box::new(teams::Teams::from_config(config)?),
        ChannelKind::Telegram => Box::new(telegram::Telegram::from_config(config)?),
        ChannelKind::Email => Box::new(email::Email::from_config(config)?),
        ChannelKind::Ntfy => Box::new(ntfy::Ntfy::from_config(config)?),
        ChannelKind::Gotify => Box::new(gotify::Gotify::from_config(config)?),
        ChannelKind::Pagerduty => Box::new(pagerduty::PagerDuty::from_config(config)?),
        ChannelKind::Pushover => Box::new(pushover::Pushover::from_config(config)?),
        ChannelKind::Mattermost => Box::new(mattermost::Mattermost::from_config(config)?),
        ChannelKind::RocketChat => Box::new(rocketchat::RocketChat::from_config(config)?),
        ChannelKind::SmsTwilio => Box::new(twilio::Twilio::from_config(config)?),
        ChannelKind::Apprise => Box::new(apprise::Apprise::from_config(config)?),
        ChannelKind::Webhook => Box::new(webhook::Webhook::from_config(config)?),
        ChannelKind::Matrix      => Box::new(matrix::Matrix::from_config(config)?),
        ChannelKind::GoogleChat  => Box::new(google_chat::GoogleChat::from_config(config)?),
        ChannelKind::Wecom       => Box::new(wecom::Wecom::from_config(config)?),
        ChannelKind::Dingtalk    => Box::new(dingtalk::DingTalk::from_config(config)?),
        ChannelKind::Feishu      => Box::new(feishu::Feishu::from_config(config)?),
        ChannelKind::Line        => Box::new(line::Line::from_config(config)?),
        _ => {
            return Err(ChannelError::BadConfig(format!(
                "channel kind {kind:?} not implemented yet"
            )))
        }
    };
    channel.send(subject, body, event).await
}
