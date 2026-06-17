//! Channel adapters.
//!
//! One file per channel. Each implements `Channel::send`. The
//! `dispatch` helper inspects the kind tag and instantiates the right
//! adapter from a JSON config blob.

pub mod alerta;
pub mod alertnow;
pub mod alertops;
pub mod aliyun_sms;
pub mod apprise;
pub mod asana;
pub mod aws_sns;
pub mod azure_servicebus;
pub mod bale;
pub mod bandwidth;
pub mod bark;
pub mod betterstack;
pub mod bitrix24;
pub mod brevo;
pub mod callmebot;
pub mod cellsynt;
pub mod clicksend;
pub mod clickup;
pub mod datadog;
pub mod dingtalk;
pub mod discord;
pub mod email;
pub mod feishu;
pub mod flashduty;
pub mod flock;
pub mod fluxer;
pub mod freemobile;
pub mod gcp_pubsub;
pub mod github_issue;
pub mod gitlab_issue;
pub mod goalert;
pub mod google_chat;
pub mod google_sheets;
pub mod gorush;
pub mod gotify;
pub mod grafana_oncall;
pub mod gtxmessaging;
pub mod halo_psa;
pub mod healthchecks_io;
pub mod heii_oncall;
pub mod home_assistant;
pub mod honeybadger;
pub mod ilert;
pub mod jira_sm;
pub mod kook;
pub mod lark;
pub mod line;
pub mod linear;
pub mod mailgun;
pub mod mailjet;
pub mod mandrill;
pub mod mastodon;
pub mod matrix;
pub mod mattermost;
pub mod max_messenger;
pub mod messagebird;
pub mod newrelic;
pub mod nostr;
pub mod notifery;
pub mod notion;
pub mod ntfy;
pub mod octopush;
pub mod onebot;
pub mod onechat;
pub mod onesender;
pub mod opsgenie;
pub mod pagerduty;
pub mod pagertree;
pub mod plivo;
pub mod postmark;
pub mod promosms;
pub mod pumble;
pub mod pushbullet;
pub mod pushcut;
pub mod pushdeer;
pub mod pushover;
pub mod pushplus;
pub mod pushy;
pub mod resend;
pub mod ringcentral;
pub mod rocketchat;
pub mod rollbar;
pub mod sendgrid;
pub mod sentry;
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
pub mod smsglobal;
pub mod smspartner;
pub mod smsplanet;
pub mod sparkpost;
pub mod spike_sh;
pub mod splash;
pub mod splunk;
pub mod spug_push;
pub mod squadcast;
pub mod stackfield;
pub mod statuspage_io;
pub mod teams;
pub mod telegram;
pub mod telnyx;
pub mod teltonika;
pub mod threema;
pub mod trello;
pub mod twilio;
pub mod vk;
pub mod vonage;
pub mod webex;
pub mod webhook;
pub mod webpush;
pub mod webpush_crypto;
pub mod wecom;
pub mod whatsapp_360;
pub mod whatsapp_evolution;
pub mod whatsapp_waha;
pub mod whatsapp_whapi;
pub mod wpush;
pub mod yzj;
pub mod zenduty;
pub mod zoho_cliq;
pub mod zulip;

use crate::{Channel, ChannelError, Event};
use rampart_core::ids::NotificationId;
use rampart_core::ChannelKind;
use rampart_db::DbPool;

/// Recursively collect every `http(s)://` string value in a config blob.
fn collect_urls(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s)
            if s.starts_with("http://") || s.starts_with("https://") =>
        {
            out.push(s.clone());
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_urls(x, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| collect_urls(x, out)),
        _ => {}
    }
}

/// SSRF-vet every http(s) URL in a channel config. Central pre-flight so a
/// literal-IP delivery target (which the reqwest DNS-resolver hook never sees)
/// can't reach the cloud metadata endpoint or internal services.
async fn guard_config_urls(config: &serde_json::Value) -> Result<(), ChannelError> {
    let mut urls = Vec::new();
    collect_urls(config, &mut urls);
    for u in urls {
        rampart_ssrf::guard_url(&u)
            .await
            .map_err(|e| ChannelError::Blocked(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod url_collect_tests {
    use super::collect_urls;

    #[test]
    fn finds_nested_http_urls_only() {
        let cfg = serde_json::json!({
            "url": "http://127.0.0.1:9201/hook",
            "nested": { "endpoint": "https://example.com/x" },
            "list": ["http://a.test/1", "not-a-url", 5],
            "token": "abc",
            "ftp": "ftp://nope/x",
        });
        let mut out = Vec::new();
        collect_urls(&cfg, &mut out);
        out.sort();
        assert_eq!(
            out,
            vec![
                "http://127.0.0.1:9201/hook",
                "http://a.test/1",
                "https://example.com/x",
            ],
        );
    }
}

/// Build an adapter from the persisted `(kind, config)` pair and ask it
/// to send. Returns `Err(BadConfig)` for kinds we haven't built yet so
/// the caller can log it instead of crashing the dispatcher.
///
/// `pool` + `notification_id` are only used by fan-out channels (Web
/// Push) that load per-subscriber state from the DB; the uniform
/// `Channel` adapters ignore them.
pub async fn dispatch(
    kind: ChannelKind,
    config: &serde_json::Value,
    subject: &str,
    body: &str,
    event: &Event,
    pool: &DbPool,
    notification_id: NotificationId,
) -> Result<(), ChannelError> {
    // SSRF pre-flight: vet every http(s) URL in the channel config before we
    // send. The shared client's DNS resolver guards hostname targets at connect,
    // but reqwest skips the resolver for literal-IP URLs (e.g.
    // http://169.254.169.254), so a config URL pointing straight at an internal
    // or metadata IP would otherwise bypass the guard entirely.
    guard_config_urls(config).await?;

    // Web Push is a fan-out channel — handled directly, not via the
    // single-target `Channel` trait.
    if kind == ChannelKind::Webpush {
        return webpush::send_all(pool, notification_id, config, subject, body, event).await;
    }

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
        ChannelKind::Matrix => Box::new(matrix::Matrix::from_config(config)?),
        ChannelKind::GoogleChat => Box::new(google_chat::GoogleChat::from_config(config)?),
        ChannelKind::Wecom => Box::new(wecom::Wecom::from_config(config)?),
        ChannelKind::Dingtalk => Box::new(dingtalk::DingTalk::from_config(config)?),
        ChannelKind::Feishu => Box::new(feishu::Feishu::from_config(config)?),
        ChannelKind::Line => Box::new(line::Line::from_config(config)?),
        ChannelKind::Bark => Box::new(bark::Bark::from_config(config)?),
        ChannelKind::Pushbullet => Box::new(pushbullet::Pushbullet::from_config(config)?),
        ChannelKind::Sendgrid => Box::new(sendgrid::Sendgrid::from_config(config)?),
        ChannelKind::Resend => Box::new(resend::Resend::from_config(config)?),
        ChannelKind::Brevo => Box::new(brevo::Brevo::from_config(config)?),
        ChannelKind::Opsgenie => Box::new(opsgenie::Opsgenie::from_config(config)?),
        ChannelKind::Pagertree => Box::new(pagertree::Pagertree::from_config(config)?),
        ChannelKind::Squadcast => Box::new(squadcast::Squadcast::from_config(config)?),
        ChannelKind::Signal => Box::new(signal::Signal::from_config(config)?),
        ChannelKind::Zulip => Box::new(zulip::Zulip::from_config(config)?),
        ChannelKind::Lark => Box::new(lark::Lark::from_config(config)?),
        ChannelKind::Goalert => Box::new(goalert::GoAlert::from_config(config)?),
        ChannelKind::Alerta => Box::new(alerta::Alerta::from_config(config)?),
        ChannelKind::Alertnow => Box::new(alertnow::AlertNow::from_config(config)?),
        ChannelKind::Signl4 => Box::new(signl4::Signl4::from_config(config)?),
        ChannelKind::HeiiOncall => Box::new(heii_oncall::HeiiOncall::from_config(config)?),
        ChannelKind::Serverchan => Box::new(serverchan::Serverchan::from_config(config)?),
        ChannelKind::Pushplus => Box::new(pushplus::Pushplus::from_config(config)?),
        ChannelKind::Pushdeer => Box::new(pushdeer::Pushdeer::from_config(config)?),
        ChannelKind::AliyunSms => Box::new(aliyun_sms::AliyunSms::from_config(config)?),
        ChannelKind::Mastodon => Box::new(mastodon::Mastodon::from_config(config)?),
        ChannelKind::Pumble => Box::new(pumble::Pumble::from_config(config)?),
        ChannelKind::Bitrix24 => Box::new(bitrix24::Bitrix24::from_config(config)?),
        ChannelKind::Stackfield => Box::new(stackfield::Stackfield::from_config(config)?),
        ChannelKind::Splunk => Box::new(splunk::Splunk::from_config(config)?),
        ChannelKind::GrafanaOncall => Box::new(grafana_oncall::GrafanaOncall::from_config(config)?),
        ChannelKind::HomeAssistant => Box::new(home_assistant::HomeAssistant::from_config(config)?),
        ChannelKind::Clicksend => Box::new(clicksend::Clicksend::from_config(config)?),
        ChannelKind::Sms46elks => Box::new(sms46elks::Sms46elks::from_config(config)?),
        ChannelKind::Callmebot => Box::new(callmebot::Callmebot::from_config(config)?),
        ChannelKind::Telnyx => Box::new(telnyx::Telnyx::from_config(config)?),
        ChannelKind::Notifery => Box::new(notifery::Notifery::from_config(config)?),
        ChannelKind::WhatsappWaha => Box::new(whatsapp_waha::WhatsappWaha::from_config(config)?),
        ChannelKind::Threema => Box::new(threema::Threema::from_config(config)?),
        ChannelKind::Bale => Box::new(bale::Bale::from_config(config)?),
        ChannelKind::Pushy => Box::new(pushy::Pushy::from_config(config)?),
        ChannelKind::ZohoCliq => Box::new(zoho_cliq::ZohoCliq::from_config(config)?),
        ChannelKind::SmsManager => Box::new(sms_manager::SmsManager::from_config(config)?),
        ChannelKind::SmsEagle => Box::new(sms_eagle::SmsEagle::from_config(config)?),
        ChannelKind::Octopush => Box::new(octopush::Octopush::from_config(config)?),
        ChannelKind::WhatsappWhapi => Box::new(whatsapp_whapi::WhatsappWhapi::from_config(config)?),
        ChannelKind::Whatsapp360 => Box::new(whatsapp_360::Whatsapp360::from_config(config)?),
        ChannelKind::WhatsappEvolution => {
            Box::new(whatsapp_evolution::WhatsappEvolution::from_config(config)?)
        }
        ChannelKind::Flock => Box::new(flock::Flock::from_config(config)?),
        ChannelKind::Serwersms => Box::new(serwersms::Serwersms::from_config(config)?),
        ChannelKind::Smsplanet => Box::new(smsplanet::Smsplanet::from_config(config)?),
        ChannelKind::Smsc => Box::new(smsc::Smsc::from_config(config)?),
        ChannelKind::Cellsynt => Box::new(cellsynt::Cellsynt::from_config(config)?),
        ChannelKind::Sevenio => Box::new(sevenio::Sevenio::from_config(config)?),
        ChannelKind::Gtxmessaging => Box::new(gtxmessaging::Gtxmessaging::from_config(config)?),
        ChannelKind::Onesender => Box::new(onesender::Onesender::from_config(config)?),
        ChannelKind::Promosms => Box::new(promosms::Promosms::from_config(config)?),
        ChannelKind::Smspartner => Box::new(smspartner::Smspartner::from_config(config)?),
        ChannelKind::SmsIr => Box::new(sms_ir::SmsIr::from_config(config)?),
        ChannelKind::Freemobile => Box::new(freemobile::Freemobile::from_config(config)?),
        ChannelKind::Flashduty => Box::new(flashduty::Flashduty::from_config(config)?),
        ChannelKind::Teltonika => Box::new(teltonika::Teltonika::from_config(config)?),
        ChannelKind::Kook => Box::new(kook::Kook::from_config(config)?),
        ChannelKind::Nostr => Box::new(nostr::Nostr::from_config(config)?),
        ChannelKind::Onebot => Box::new(onebot::Onebot::from_config(config)?),
        ChannelKind::Onechat => Box::new(onechat::Onechat::from_config(config)?),
        ChannelKind::MaxMessenger => Box::new(max_messenger::MaxMessenger::from_config(config)?),
        ChannelKind::HaloPsa => Box::new(halo_psa::HaloPsa::from_config(config)?),
        ChannelKind::JiraSm => Box::new(jira_sm::JiraSm::from_config(config)?),
        ChannelKind::SpugPush => Box::new(spug_push::SpugPush::from_config(config)?),
        ChannelKind::Wpush => Box::new(wpush::Wpush::from_config(config)?),
        ChannelKind::Vk => Box::new(vk::Vk::from_config(config)?),
        ChannelKind::Yzj => Box::new(yzj::Yzj::from_config(config)?),
        ChannelKind::GoogleSheets => Box::new(google_sheets::GoogleSheets::from_config(config)?),
        ChannelKind::Gorush => Box::new(gorush::Gorush::from_config(config)?),
        ChannelKind::Fluxer => Box::new(fluxer::Fluxer::from_config(config)?),
        ChannelKind::Splash => Box::new(splash::Splash::from_config(config)?),
        ChannelKind::Messagebird => Box::new(messagebird::Messagebird::from_config(config)?),
        ChannelKind::Plivo => Box::new(plivo::Plivo::from_config(config)?),
        ChannelKind::Vonage => Box::new(vonage::Vonage::from_config(config)?),
        ChannelKind::Bandwidth => Box::new(bandwidth::Bandwidth::from_config(config)?),
        ChannelKind::Webex => Box::new(webex::Webex::from_config(config)?),
        ChannelKind::Pushcut => Box::new(pushcut::Pushcut::from_config(config)?),
        ChannelKind::Smsglobal => Box::new(smsglobal::Smsglobal::from_config(config)?),
        ChannelKind::Alertops => Box::new(alertops::Alertops::from_config(config)?),
        ChannelKind::Mailgun => Box::new(mailgun::Mailgun::from_config(config)?),
        ChannelKind::Mailjet => Box::new(mailjet::Mailjet::from_config(config)?),
        ChannelKind::Postmark => Box::new(postmark::Postmark::from_config(config)?),
        ChannelKind::Mandrill => Box::new(mandrill::Mandrill::from_config(config)?),
        ChannelKind::Sparkpost => Box::new(sparkpost::Sparkpost::from_config(config)?),
        ChannelKind::SpikeSh => Box::new(spike_sh::SpikeSh::from_config(config)?),
        ChannelKind::Zenduty => Box::new(zenduty::Zenduty::from_config(config)?),
        ChannelKind::Ringcentral => Box::new(ringcentral::Ringcentral::from_config(config)?),
        ChannelKind::Ilert => Box::new(ilert::Ilert::from_config(config)?),
        ChannelKind::Linear => Box::new(linear::Linear::from_config(config)?),
        ChannelKind::Clickup => Box::new(clickup::Clickup::from_config(config)?),
        ChannelKind::Trello => Box::new(trello::Trello::from_config(config)?),
        ChannelKind::GithubIssue => Box::new(github_issue::GithubIssue::from_config(config)?),
        ChannelKind::GitlabIssue => Box::new(gitlab_issue::GitlabIssue::from_config(config)?),
        ChannelKind::Asana => Box::new(asana::Asana::from_config(config)?),
        ChannelKind::Notion => Box::new(notion::Notion::from_config(config)?),
        ChannelKind::Sentry => Box::new(sentry::Sentry::from_config(config)?),
        ChannelKind::Rollbar => Box::new(rollbar::Rollbar::from_config(config)?),
        ChannelKind::Honeybadger => Box::new(honeybadger::Honeybadger::from_config(config)?),
        ChannelKind::HealthchecksIo => {
            Box::new(healthchecks_io::HealthchecksIo::from_config(config)?)
        }
        ChannelKind::Betterstack => Box::new(betterstack::Betterstack::from_config(config)?),
        ChannelKind::StatuspageIo => Box::new(statuspage_io::StatuspageIo::from_config(config)?),
        ChannelKind::Datadog => Box::new(datadog::Datadog::from_config(config)?),
        ChannelKind::Newrelic => Box::new(newrelic::Newrelic::from_config(config)?),
        ChannelKind::AwsSns => Box::new(aws_sns::AwsSns::from_config(config)?),
        ChannelKind::AzureServicebus => {
            Box::new(azure_servicebus::AzureServicebus::from_config(config)?)
        }
        ChannelKind::GcpPubsub => Box::new(gcp_pubsub::GcpPubsub::from_config(config)?),
        _ => {
            return Err(ChannelError::BadConfig(format!(
                "channel kind {kind:?} not implemented yet"
            )))
        }
    };
    channel.send(subject, body, event).await
}
