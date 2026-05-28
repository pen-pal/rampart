//! Notifications: channels, per-monitor wiring, and templates.
//!
//! Each notification channel is a row with kind + config;
//! `monitor_notifications` is a junction table that fans an event out to
//! all channels attached to that monitor. No complex routing rules —
//! straight fan-out, which is what 95% of small teams need.
//!
//! Channels can reference a `NotificationTemplate` to customize the
//! rendered message, so the same body text doesn't need to be edited in
//! eight places.

use crate::ids::{MonitorId, NotificationId, NotificationTemplateId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Channel kinds. The first six cover the common cases; the rest are
/// the long tail. We ship 23 directly and an extensible 'custom' webhook
/// covers anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "channel_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Slack,
    Discord,
    Telegram,
    Email,
    Webhook,
    Pagerduty,
    SmsTwilio,
    Teams,
    Gotify,
    Pushover,
    Ntfy,
    Mattermost,
    Matrix,
    Signal,
    RocketChat,
    GoogleChat,
    Zulip,
    Line,
    Lark,
    Wecom,
    Dingtalk,
    Feishu,
    /// Apprise gateway — single channel kind that proxies to ~80 upstream
    /// services through a user-deployed apprise-api sidecar.
    Apprise,
    /// Catch-all for custom webhook payloads.
    Custom,
    Bark,
    Pushbullet,
    Sendgrid,
    Resend,
    Brevo,
    Opsgenie,
    Pagertree,
    Squadcast,
    Goalert,
    Alerta,
    Alertnow,
    Signl4,
    HeiiOncall,
    Serverchan,
    Pushplus,
    Pushdeer,
    AliyunSms,
    Mastodon,
    Pumble,
    Bitrix24,
    Stackfield,
    Splunk,
    GrafanaOncall,
    HomeAssistant,
    Clicksend,
    Sms46elks,
    Callmebot,
    Telnyx,
    Notifery,
    WhatsappWaha,
    Threema,
    Bale,
    Pushy,
    ZohoCliq,
    SmsManager,
    SmsEagle,
    Octopush,
    WhatsappWhapi,
    Whatsapp360,
    WhatsappEvolution,
    Flock,
    Serwersms,
    Smsplanet,
    Smsc,
    Cellsynt,
    Sevenio,
    Gtxmessaging,
    Onesender,
    Promosms,
    Smspartner,
    SmsIr,
    Freemobile,
    Flashduty,
    Teltonika,
    Kook,
    Nostr,
    Onebot,
    Onechat,
    MaxMessenger,
    HaloPsa,
    JiraSm,
    SpugPush,
    Wpush,
    Vk,
    Yzj,
    GoogleSheets,
    Gorush,
    Fluxer,
    Splash,
    Messagebird,
    Plivo,
    Vonage,
    Bandwidth,
    Webex,
    Pushcut,
    Smsglobal,
    Alertops,
    Mailgun,
    Mailjet,
    Postmark,
    Mandrill,
    Sparkpost,
    SpikeSh,
    Zenduty,
    Ringcentral,
    Ilert,
    Linear,
    Clickup,
    Trello,
    GithubIssue,
    GitlabIssue,
    Asana,
    Notion,
    Sentry,
    Rollbar,
    Honeybadger,
    HealthchecksIo,
    Betterstack,
    StatuspageIo,
    Datadog,
    Newrelic,
    AwsSns,
    AzureServicebus,
    GcpPubsub,
    Webpush,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: NotificationId,
    pub name: String,
    pub kind: ChannelKind,
    /// Channel-kind-specific config. Secrets are encrypted before insert.
    pub config: serde_json::Value,
    pub is_default: bool,
    pub template_id: Option<NotificationTemplateId>,
    pub active: bool,
    pub last_fired_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    /// Per-channel cooldown in seconds. Set > 0 to suppress sends
    /// within this many seconds of `last_fired_at`. Useful for
    /// flap-prone monitors paired with high-cost channels (SMS, paging).
    #[serde(default)]
    pub cooldown_seconds: i32,
}

/// Junction row: which channels does a given monitor fire to?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorNotification {
    pub monitor_id: MonitorId,
    pub notification_id: NotificationId,
}

/// Renderable message template — the long-asked feature.
///
/// Body uses simple `{{ handlebars-style }}` interpolation. Available
/// variables depend on event_kind; the renderer documents them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationTemplate {
    pub id: NotificationTemplateId,
    pub name: String,
    pub channel_kinds: Vec<String>,
    pub event_kind: String,
    pub subject_template: Option<String>,
    pub body_template: String,
    pub is_default: bool,
    pub created_at: OffsetDateTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_kind_snake_case_serialization() {
        // Same expectations as MonitorKind — these strings are persisted
        // in Postgres + travel over the API. Any rename here is a
        // breaking change requiring a migration.
        let cases = [
            (ChannelKind::Slack, "slack"),
            (ChannelKind::Discord, "discord"),
            (ChannelKind::Teams, "teams"),
            (ChannelKind::Telegram, "telegram"),
            (ChannelKind::Email, "email"),
            (ChannelKind::Webhook, "webhook"),
            (ChannelKind::Pagerduty, "pagerduty"),
            (ChannelKind::SmsTwilio, "sms_twilio"),
            (ChannelKind::Mattermost, "mattermost"),
            (ChannelKind::RocketChat, "rocket_chat"),
            (ChannelKind::GoogleChat, "google_chat"),
            (ChannelKind::Ntfy, "ntfy"),
            (ChannelKind::Gotify, "gotify"),
            (ChannelKind::Pushover, "pushover"),
            (ChannelKind::Apprise, "apprise"),
            (ChannelKind::Custom, "custom"),
        ];
        for (k, expected) in cases {
            let v = serde_json::to_string(&k).unwrap();
            assert_eq!(v, format!("\"{expected}\""), "{k:?}");
            let back: ChannelKind = serde_json::from_str(&v).unwrap();
            assert_eq!(back, k);
        }
    }
}
