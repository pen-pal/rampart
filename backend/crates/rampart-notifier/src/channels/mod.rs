//! Channel adapters.
//!
//! One file per channel. Each implements `Channel::send`. The
//! `dispatch` helper inspects the kind tag and instantiates the right
//! adapter from a JSON config blob.

pub mod alerta;
pub mod alertnow;
pub mod aliyun_sms;
pub mod apprise;
pub mod bale;
pub mod bark;
pub mod bitrix24;
pub mod brevo;
pub mod callmebot;
pub mod cellsynt;
pub mod clicksend;
pub mod dingtalk;
pub mod discord;
pub mod email;
pub mod feishu;
pub mod flashduty;
pub mod flock;
pub mod freemobile;
pub mod goalert;
pub mod google_chat;
pub mod gotify;
pub mod grafana_oncall;
pub mod gtxmessaging;
pub mod heii_oncall;
pub mod home_assistant;
pub mod lark;
pub mod line;
pub mod mastodon;
pub mod matrix;
pub mod mattermost;
pub mod notifery;
pub mod ntfy;
pub mod octopush;
pub mod onesender;
pub mod opsgenie;
pub mod pagerduty;
pub mod pagertree;
pub mod promosms;
pub mod pumble;
pub mod pushbullet;
pub mod pushdeer;
pub mod pushover;
pub mod pushplus;
pub mod pushy;
pub mod resend;
pub mod rocketchat;
pub mod sendgrid;
pub mod serverchan;
pub mod serwersms;
pub mod sevenio;
pub mod signal;
pub mod signl4;
pub mod slack;
pub mod sms46elks;
pub mod sms_eagle;
pub mod sms_ir;
pub mod sms_manager;
pub mod smsc;
pub mod smsplanet;
pub mod smspartner;
pub mod splunk;
pub mod squadcast;
pub mod stackfield;
pub mod teams;
pub mod telegram;
pub mod telnyx;
pub mod threema;
pub mod twilio;
pub mod webhook;
pub mod wecom;
pub mod whatsapp_360;
pub mod whatsapp_evolution;
pub mod whatsapp_waha;
pub mod whatsapp_whapi;
pub mod zoho_cliq;
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
        ChannelKind::Splunk        => Box::new(splunk::Splunk::from_config(config)?),
        ChannelKind::GrafanaOncall => Box::new(grafana_oncall::GrafanaOncall::from_config(config)?),
        ChannelKind::HomeAssistant => Box::new(home_assistant::HomeAssistant::from_config(config)?),
        ChannelKind::Clicksend     => Box::new(clicksend::Clicksend::from_config(config)?),
        ChannelKind::Sms46elks     => Box::new(sms46elks::Sms46elks::from_config(config)?),
        ChannelKind::Callmebot     => Box::new(callmebot::Callmebot::from_config(config)?),
        ChannelKind::Telnyx        => Box::new(telnyx::Telnyx::from_config(config)?),
        ChannelKind::Notifery      => Box::new(notifery::Notifery::from_config(config)?),
        ChannelKind::WhatsappWaha => Box::new(whatsapp_waha::WhatsappWaha::from_config(config)?),
        ChannelKind::Threema      => Box::new(threema::Threema::from_config(config)?),
        ChannelKind::Bale         => Box::new(bale::Bale::from_config(config)?),
        ChannelKind::Pushy        => Box::new(pushy::Pushy::from_config(config)?),
        ChannelKind::ZohoCliq     => Box::new(zoho_cliq::ZohoCliq::from_config(config)?),
        ChannelKind::SmsManager   => Box::new(sms_manager::SmsManager::from_config(config)?),
        ChannelKind::SmsEagle     => Box::new(sms_eagle::SmsEagle::from_config(config)?),
        ChannelKind::Octopush     => Box::new(octopush::Octopush::from_config(config)?),
        ChannelKind::WhatsappWhapi     => Box::new(whatsapp_whapi::WhatsappWhapi::from_config(config)?),
        ChannelKind::Whatsapp360       => Box::new(whatsapp_360::Whatsapp360::from_config(config)?),
        ChannelKind::WhatsappEvolution => Box::new(whatsapp_evolution::WhatsappEvolution::from_config(config)?),
        ChannelKind::Flock             => Box::new(flock::Flock::from_config(config)?),
        ChannelKind::Serwersms         => Box::new(serwersms::Serwersms::from_config(config)?),
        ChannelKind::Smsplanet         => Box::new(smsplanet::Smsplanet::from_config(config)?),
        ChannelKind::Smsc              => Box::new(smsc::Smsc::from_config(config)?),
        ChannelKind::Cellsynt          => Box::new(cellsynt::Cellsynt::from_config(config)?),
        ChannelKind::Sevenio       => Box::new(sevenio::Sevenio::from_config(config)?),
        ChannelKind::Gtxmessaging  => Box::new(gtxmessaging::Gtxmessaging::from_config(config)?),
        ChannelKind::Onesender     => Box::new(onesender::Onesender::from_config(config)?),
        ChannelKind::Promosms      => Box::new(promosms::Promosms::from_config(config)?),
        ChannelKind::Smspartner    => Box::new(smspartner::Smspartner::from_config(config)?),
        ChannelKind::SmsIr         => Box::new(sms_ir::SmsIr::from_config(config)?),
        ChannelKind::Freemobile    => Box::new(freemobile::Freemobile::from_config(config)?),
        ChannelKind::Flashduty     => Box::new(flashduty::Flashduty::from_config(config)?),
        _ => {
            return Err(ChannelError::BadConfig(format!(
                "channel kind {kind:?} not implemented yet"
            )))
        }
    };
    channel.send(subject, body, event).await
}
