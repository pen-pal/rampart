//! Channel adapters.
//!
//! One file per channel. Each implements `Channel::send`. The
//! `dispatch` helper inspects the kind tag and instantiates the right
//! adapter from a JSON config blob.

pub mod alerta;
pub mod alertnow;
pub mod aliyun_sms;
pub mod apprise;
pub mod bark;
pub mod bitrix24;
pub mod brevo;
pub mod dingtalk;
pub mod discord;
pub mod email;
pub mod feishu;
pub mod goalert;
pub mod google_chat;
pub mod gotify;
pub mod heii_oncall;
pub mod lark;
pub mod line;
pub mod mastodon;
pub mod matrix;
pub mod mattermost;
pub mod ntfy;
pub mod opsgenie;
pub mod pagerduty;
pub mod pagertree;
pub mod pumble;
pub mod pushbullet;
pub mod pushdeer;
pub mod pushover;
pub mod pushplus;
pub mod resend;
pub mod rocketchat;
pub mod sendgrid;
pub mod serverchan;
pub mod signal;
pub mod signl4;
pub mod slack;
pub mod squadcast;
pub mod stackfield;
pub mod teams;
pub mod telegram;
pub mod twilio;
pub mod webhook;
pub mod wecom;
pub mod zulip;

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
        ChannelKind::Bark        => Box::new(bark::Bark::from_config(config)?),
        ChannelKind::Pushbullet  => Box::new(pushbullet::Pushbullet::from_config(config)?),
        ChannelKind::Sendgrid    => Box::new(sendgrid::Sendgrid::from_config(config)?),
        ChannelKind::Resend      => Box::new(resend::Resend::from_config(config)?),
        ChannelKind::Brevo       => Box::new(brevo::Brevo::from_config(config)?),
        ChannelKind::Opsgenie    => Box::new(opsgenie::Opsgenie::from_config(config)?),
        ChannelKind::Pagertree   => Box::new(pagertree::Pagertree::from_config(config)?),
        ChannelKind::Squadcast   => Box::new(squadcast::Squadcast::from_config(config)?),
        ChannelKind::Signal      => Box::new(signal::Signal::from_config(config)?),
        ChannelKind::Zulip       => Box::new(zulip::Zulip::from_config(config)?),
        ChannelKind::Lark        => Box::new(lark::Lark::from_config(config)?),
        ChannelKind::Goalert     => Box::new(goalert::GoAlert::from_config(config)?),
        ChannelKind::Alerta      => Box::new(alerta::Alerta::from_config(config)?),
        ChannelKind::Alertnow    => Box::new(alertnow::AlertNow::from_config(config)?),
        ChannelKind::Signl4      => Box::new(signl4::Signl4::from_config(config)?),
        ChannelKind::HeiiOncall  => Box::new(heii_oncall::HeiiOncall::from_config(config)?),
        ChannelKind::Serverchan  => Box::new(serverchan::Serverchan::from_config(config)?),
        ChannelKind::Pushplus    => Box::new(pushplus::Pushplus::from_config(config)?),
        ChannelKind::Pushdeer    => Box::new(pushdeer::Pushdeer::from_config(config)?),
        ChannelKind::AliyunSms   => Box::new(aliyun_sms::AliyunSms::from_config(config)?),
        ChannelKind::Mastodon    => Box::new(mastodon::Mastodon::from_config(config)?),
        ChannelKind::Pumble      => Box::new(pumble::Pumble::from_config(config)?),
        ChannelKind::Bitrix24    => Box::new(bitrix24::Bitrix24::from_config(config)?),
        ChannelKind::Stackfield  => Box::new(stackfield::Stackfield::from_config(config)?),
        _ => {
            return Err(ChannelError::BadConfig(format!(
                "channel kind {kind:?} not implemented yet"
            )))
        }
    };
    channel.send(subject, body, event).await
}
