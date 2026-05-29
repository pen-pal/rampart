import React, { useState } from 'react';
import {
  Bell, Plus, Trash2, Send, ChevronLeft, MessageSquare, Hash, Mail,
  Webhook as WebhookIcon, AlertCircle, Loader2, Smartphone, Server, Megaphone,
  Siren, Phone, Rocket, Layers, FileText, Edit3, Save, X, BellRing,
} from 'lucide-react';
import { api, useApi } from '../lib/api.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');

  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    --warn:#f59e0b; --warn-soft:#fef3c7;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', monospace; }

  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 12px; border-radius: 8px; cursor: pointer;
    font-size: 13px; font-weight: 500; line-height: 1;
    background: var(--surface); border: 1px solid var(--border); color: var(--text-2);
    font-family: inherit;
  }
  .btn:hover { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .btn:disabled { opacity: .55; cursor: not-allowed; }
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-danger { color: var(--down); }
  .btn-danger:hover { background: var(--down-soft); border-color: var(--down); }
  .btn-ghost { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }

  .field { margin-bottom: 14px; }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); margin-bottom: 6px; display: block; }
  .input, .select {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input.mono { font-family: 'JetBrains Mono', monospace; font-size: 12.5px; }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }

  .kind-card {
    display: flex; align-items: center; gap: 12px;
    padding: 12px 14px; border: 1px solid var(--border);
    border-radius: 10px; cursor: pointer; background: var(--surface);
  }
  .kind-card:hover { background: var(--surface-2); }
  .kind-card.active { border-color: var(--accent); background: var(--accent-soft); }

  .channel-row {
    display: grid; grid-template-columns: 30px 1fr auto auto;
    align-items: center; gap: 14px;
    padding: 12px 18px; border-top: 1px solid var(--border);
  }
  .channel-row:first-child { border-top: none; }

  .banner-ok  { background: var(--up-soft);   color: #047857; border: 1px solid #a7f3d0; padding: 10px 14px; border-radius: 8px; font-size: 13px; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; }

  .tabs {
    display: inline-flex; gap: 2px; padding: 3px;
    background: var(--surface-2); border-radius: 8px;
    border: 1px solid var(--border); margin-bottom: 20px;
  }
  .tabs button {
    background: transparent; border: none; padding: 6px 14px; border-radius: 6px;
    font-size: 12px; font-weight: 500; color: var(--text-2); cursor: pointer;
    font-family: inherit;
    display: inline-flex; align-items: center; gap: 6px;
  }
  .tabs button:hover { color: var(--text); }
  .tabs button.active { background: var(--surface); color: var(--text); box-shadow: 0 1px 2px rgba(0,0,0,.04); }

  .template-row {
    display: grid; grid-template-columns: 30px 1fr auto auto auto;
    align-items: center; gap: 14px;
    padding: 12px 18px; border-top: 1px solid var(--border);
  }
  .template-row:first-child { border-top: none; }

  .template-pill {
    display: inline-flex; align-items: center; gap: 4px;
    font-size: 10.5px; color: var(--accent-2);
    background: var(--accent-soft); padding: 2px 7px; border-radius: 999px;
    font-weight: 500;
  }

  .code {
    font-family: 'JetBrains Mono', monospace;
    background: var(--surface-2); padding: 1px 5px; border-radius: 4px;
    font-size: 11.5px; color: var(--text-2);
  }
`;

// Channels wired into the notifier. 20 first-party + 1 generic.
const SUPPORTED = [
  // chat
  { id: 'slack',       name: 'Slack',           icon: MessageSquare, hint: 'Incoming-webhook URL from https://api.slack.com/messaging/webhooks' },
  { id: 'discord',     name: 'Discord',         icon: Hash,          hint: 'Webhook URL from Discord channel → Edit → Integrations → Webhooks' },
  { id: 'teams',       name: 'MS Teams',        icon: MessageSquare, hint: 'Incoming-webhook from Teams channel → Connectors → Incoming Webhook' },
  { id: 'mattermost',  name: 'Mattermost',      icon: MessageSquare, hint: 'Self-hosted Mattermost incoming webhook' },
  { id: 'rocket_chat', name: 'Rocket.Chat',     icon: Rocket,        hint: 'Self-hosted Rocket.Chat incoming webhook' },
  { id: 'telegram',    name: 'Telegram',        icon: Megaphone,     hint: 'Bot token from @BotFather, and a chat_id from /getUpdates' },
  { id: 'matrix',      name: 'Matrix',          icon: Hash,          hint: 'Homeserver + access token + room id. Open standard.' },
  { id: 'google_chat', name: 'Google Chat',     icon: MessageSquare, hint: 'Incoming-webhook URL from Google Workspace space integrations' },
  { id: 'wecom',       name: 'WeCom 企业微信', icon: MessageSquare, hint: 'Bot key from a group bot URL — qyapi.weixin.qq.com' },
  { id: 'dingtalk',    name: 'DingTalk 钉钉', icon: MessageSquare, hint: 'Custom robot access token; optional HMAC secret for signing' },
  { id: 'feishu',      name: 'Feishu 飞书',   icon: MessageSquare, hint: 'Custom bot webhook URL from open.feishu.cn' },
  { id: 'line',        name: 'LINE Messenger',  icon: MessageSquare, hint: 'Channel access token + recipient ID (Messaging API)' },
  // push
  { id: 'bark',        name: 'Bark (iOS)',      icon: Smartphone,    hint: 'Push to iOS via day.app or a self-hosted Bark server' },
  { id: 'pushbullet',  name: 'Pushbullet',      icon: Smartphone,    hint: 'Push notifications via pushbullet.com — access token' },
  // email APIs
  { id: 'sendgrid',    name: 'SendGrid',        icon: Mail,          hint: 'Twilio SendGrid transactional email API' },
  { id: 'resend',      name: 'Resend',          icon: Mail,          hint: 'Resend transactional email API' },
  { id: 'brevo',       name: 'Brevo',           icon: Mail,          hint: 'Brevo (formerly Sendinblue) transactional email' },
  // incident management
  { id: 'opsgenie',    name: 'Opsgenie',        icon: Siren,         hint: 'Atlassian Opsgenie — alerts auto-resolve on recovery' },
  { id: 'pagertree',   name: 'PagerTree',       icon: Siren,         hint: 'PagerTree integration URL — auto-resolves on recovery' },
  { id: 'squadcast',   name: 'Squadcast',       icon: Siren,         hint: 'Squadcast webhook integration — auto-resolves on recovery' },
  { id: 'signal',      name: 'Signal',          icon: MessageSquare, hint: 'Self-hosted signal-cli REST API — sender number + recipients' },
  { id: 'zulip',       name: 'Zulip',           icon: MessageSquare, hint: 'Bot email + API key; stream + topic or private email list' },
  { id: 'lark',        name: 'Lark',            icon: MessageSquare, hint: 'Lark / Feishu international custom-bot webhook URL' },
  { id: 'goalert',     name: 'GoAlert',         icon: Siren,         hint: 'GoAlert integration URL — close action on recovery' },
  { id: 'alerta',      name: 'Alerta',          icon: Siren,         hint: 'Alerta REST API — severity mapped from monitor status' },
  { id: 'alertnow',    name: 'AlertNow',        icon: Siren,         hint: 'AlertNow webhook integration URL' },
  { id: 'signl4',      name: 'SIGNL4',          icon: Siren,         hint: 'SIGNL4 mobile alerting — team secret from the connect URL' },
  { id: 'heii_oncall', name: 'Heii On-Call',    icon: Siren,         hint: 'Heii On-Call trigger URL; optional close URL on recovery' },
  { id: 'serverchan',  name: 'ServerChan',      icon: Smartphone,    hint: 'WeChat push via sct.ftqq.com — SendKey' },
  { id: 'pushplus',    name: 'PushPlus',        icon: Smartphone,    hint: 'WeChat push via pushplus.plus — token + optional topic' },
  { id: 'pushdeer',    name: 'PushDeer',        icon: Smartphone,    hint: 'PushDeer.com or self-hosted; pushkey' },
  { id: 'aliyun_sms',  name: 'Aliyun SMS',      icon: Phone,         hint: 'Alibaba Cloud SMS — signed SendSms with template' },
  { id: 'mastodon',    name: 'Mastodon',        icon: MessageSquare, hint: 'Post a toot via /api/v1/statuses; configurable visibility' },
  { id: 'pumble',      name: 'Pumble',          icon: MessageSquare, hint: 'Pumble incoming webhook (Slack-compatible payload)' },
  { id: 'bitrix24',    name: 'Bitrix24',        icon: MessageSquare, hint: 'Bitrix24 inbound webhook + USER_ID for im.notify.system.add' },
  { id: 'stackfield',  name: 'Stackfield',      icon: MessageSquare, hint: 'Stackfield room incoming webhook' },
  { id: 'splunk',        name: 'Splunk On-Call', icon: Siren,         hint: 'Splunk On-Call (VictorOps) REST integration URL' },
  { id: 'grafana_oncall',name: 'Grafana OnCall', icon: Siren,         hint: 'Grafana OnCall integration webhook' },
  { id: 'home_assistant',name: 'Home Assistant', icon: Server,        hint: 'Home Assistant /api/services/notify/<service> with long-lived token' },
  { id: 'clicksend',     name: 'ClickSend SMS',  icon: Phone,         hint: 'ClickSend REST SMS — username + API key' },
  { id: 'sms_46elks',    name: '46elks SMS',     icon: Phone,         hint: '46elks SMS — API username + password' },
  { id: 'callmebot',     name: 'CallMeBot',      icon: Phone,         hint: 'Free WhatsApp / Signal / Telegram push via callmebot.com' },
  { id: 'telnyx',        name: 'Telnyx SMS',     icon: Phone,         hint: 'Telnyx v2/messages — API key + from number' },
  { id: 'notifery',      name: 'Notifery',       icon: Smartphone,    hint: 'Notifery event API — token + group' },
  { id: 'whatsapp_waha', name: 'WhatsApp (WAHA)',icon: MessageSquare, hint: 'WhatsApp via the self-hosted WAHA gateway' },
  { id: 'threema',       name: 'Threema',        icon: MessageSquare, hint: 'Threema Gateway — simple text mode' },
  { id: 'bale',          name: 'Bale',           icon: MessageSquare, hint: 'Bale Messenger bot — token + chat_id' },
  { id: 'pushy',         name: 'Pushy',          icon: Smartphone,    hint: 'Pushy.me push notifications — api_key + device tokens' },
  { id: 'zoho_cliq',     name: 'Zoho Cliq',      icon: MessageSquare, hint: 'Zoho Cliq incoming webhook' },
  { id: 'sms_manager',   name: 'SmsManager.cz',  icon: Phone,         hint: 'SmsManager.cz (CZ-focused SMS) — apikey + numbers' },
  { id: 'sms_eagle',     name: 'SMSEagle',       icon: Phone,         hint: 'Self-hosted GSM gateway — base URL + access token' },
  { id: 'octopush',      name: 'Octopush',       icon: Phone,         hint: 'Octopush SMS — api-login + api-key' },
  { id: 'whatsapp_whapi',     name: 'WhatsApp (whapi)',     icon: MessageSquare, hint: 'whapi.cloud — bearer token + recipient jid' },
  { id: 'whatsapp_360',       name: 'WhatsApp (360messenger)', icon: MessageSquare, hint: '360messenger — bearer key + phone' },
  { id: 'whatsapp_evolution', name: 'WhatsApp (Evolution)', icon: MessageSquare, hint: 'Self-hosted Evolution API gateway' },
  { id: 'flock',         name: 'Flock',          icon: MessageSquare, hint: 'Flock incoming-webhook URL' },
  { id: 'serwersms',     name: 'SerwerSMS.pl',   icon: Phone,         hint: 'Polish SMS gateway — username + password + sender' },
  { id: 'smsplanet',     name: 'SMSPlanet.pl',   icon: Phone,         hint: 'Polish SMS — bearer key + from sender' },
  { id: 'smsc',          name: 'SMSC.ru',        icon: Phone,         hint: 'Russian SMSC.ru — login + psw + phones' },
  { id: 'cellsynt',      name: 'Cellsynt',       icon: Phone,         hint: 'Swedish Cellsynt — username + password + originator' },
  { id: 'sevenio',       name: 'seven.io',       icon: Phone,         hint: 'seven.io (formerly sms77) — X-Api-Key + recipient' },
  { id: 'gtxmessaging',  name: 'GtxMessaging',   icon: Phone,         hint: 'GTX SMS — api_key + sender_id + numbers' },
  { id: 'onesender',     name: 'Onesender',      icon: MessageSquare, hint: 'Self-hosted Onesender WhatsApp gateway' },
  { id: 'promosms',      name: 'PromoSMS.pl',    icon: Phone,         hint: 'Polish PromoSMS — username + password + sender' },
  { id: 'smspartner',    name: 'SMSPartner.fr',  icon: Phone,         hint: 'French SMSPartner — api_key + sender' },
  { id: 'sms_ir',        name: 'SMS.ir',         icon: Phone,         hint: 'Iranian SMS.ir — X-API-KEY + line number' },
  { id: 'freemobile',    name: 'Free Mobile (FR)', icon: Phone,       hint: 'Free Mobile self-SMS — user + pass' },
  { id: 'flashduty',     name: 'FlashDuty',      icon: Siren,         hint: 'FlashDuty integration URL — auto-resolves on recovery' },
  { id: 'teltonika',     name: 'Teltonika SMS',  icon: Phone,         hint: 'Teltonika RUT router SMS gateway' },
  { id: 'kook',          name: 'Kook',           icon: MessageSquare, hint: 'Kook (former Kaiheila) bot — group or DM' },
  { id: 'nostr',         name: 'Nostr',          icon: MessageSquare, hint: 'Nostr DM via a user-supplied HTTP relay bridge' },
  { id: 'onebot',        name: 'OneBot',         icon: MessageSquare, hint: 'OneBot v11 HTTP — QQ group or private' },
  { id: 'onechat',       name: 'OneChat (TH)',   icon: MessageSquare, hint: 'OneChat push message — bot token + chat_id' },
  { id: 'max_messenger', name: 'MAX Messenger',  icon: MessageSquare, hint: 'MAX (RU) bot — access_token + chat_id' },
  { id: 'halo_psa',      name: 'Halo PSA',       icon: Siren,         hint: 'Halo PSA — opens a ticket via OAuth client credentials' },
  { id: 'jira_sm',       name: 'Jira Service Mgmt', icon: Siren,      hint: 'Jira Service Management — creates an incident via REST' },
  { id: 'spug_push',     name: 'SpugPush',       icon: Smartphone,    hint: 'push.spug.cc — template_code from the Spug dashboard' },
  { id: 'wpush',         name: 'WPush.cn',       icon: Smartphone,    hint: 'WPush.cn — api_key + comma-separated channels' },
  { id: 'vk',            name: 'VK',             icon: MessageSquare, hint: 'VK messages.send — access_token + peer_id' },
  { id: 'yzj',           name: 'YZJ (云之家)',   icon: MessageSquare, hint: 'YZJ (Cloudhome) custom robot webhook' },
  { id: 'google_sheets', name: 'Google Sheets',  icon: FileText,      hint: 'Append a row via a deployed Apps Script web-app URL' },
  { id: 'gorush',        name: 'Gorush',         icon: Smartphone,    hint: 'Self-hosted Gorush relay for FCM / APNs' },
  { id: 'fluxer',        name: 'Fluxer',         icon: MessageSquare, hint: 'Fluxer webhook URL — title + body payload' },
  { id: 'splash',        name: 'Splash',         icon: Siren,         hint: 'Splash incident webhook — auto-resolves on recovery' },
  { id: 'messagebird',   name: 'MessageBird',    icon: Phone,         hint: 'MessageBird /messages — AccessKey auth' },
  { id: 'plivo',         name: 'Plivo SMS',      icon: Phone,         hint: 'Plivo /Message/ — auth_id + auth_token' },
  { id: 'vonage',        name: 'Vonage (Nexmo)', icon: Phone,         hint: 'Vonage Nexmo /sms/json — api_key + api_secret' },
  { id: 'bandwidth',     name: 'Bandwidth',      icon: Phone,         hint: 'Bandwidth /messages — basic auth + application_id' },
  { id: 'webex',         name: 'Cisco Webex',    icon: MessageSquare, hint: 'Webex Teams /messages — bot token + room_id' },
  { id: 'pushcut',       name: 'Pushcut',        icon: Smartphone,    hint: 'Pushcut iOS automation — API key + notification name' },
  { id: 'smsglobal',     name: 'SMSGlobal',      icon: Phone,         hint: 'SMSGlobal /v2/sms — api_key + api_secret' },
  { id: 'alertops',      name: 'AlertOps',       icon: Siren,         hint: 'AlertOps integration URL — auto-resolves on recovery' },
  { id: 'mailgun',       name: 'Mailgun',        icon: Mail,          hint: 'Mailgun /messages — domain + private API key' },
  { id: 'mailjet',       name: 'Mailjet',        icon: Mail,          hint: 'Mailjet /v3.1/send — API key + secret' },
  { id: 'postmark',      name: 'Postmark',       icon: Mail,          hint: 'Postmark /email — server token' },
  { id: 'mandrill',      name: 'Mandrill',       icon: Mail,          hint: 'Mailchimp Transactional /messages/send.json' },
  { id: 'sparkpost',     name: 'SparkPost',      icon: Mail,          hint: 'SparkPost /api/v1/transmissions — API key' },
  { id: 'spike_sh',      name: 'Spike.sh',       icon: Siren,         hint: 'Spike.sh integration URL — auto-resolves on recovery' },
  { id: 'zenduty',       name: 'Zenduty',        icon: Siren,         hint: 'Zenduty integration URL — auto-resolves on recovery' },
  { id: 'ringcentral',   name: 'RingCentral',    icon: MessageSquare, hint: 'RingCentral incoming-webhook URL' },
  { id: 'ilert',         name: 'iLert',          icon: Siren,         hint: 'iLert event API — integration key + auto-resolve' },
  { id: 'linear',        name: 'Linear',         icon: FileText,      hint: 'Linear issue via GraphQL — api_key + team_id' },
  { id: 'clickup',       name: 'ClickUp',        icon: FileText,      hint: 'ClickUp task — api_token + list_id' },
  { id: 'trello',        name: 'Trello',         icon: FileText,      hint: 'Trello card — key + token + list_id' },
  { id: 'github_issue',  name: 'GitHub Issue',   icon: FileText,      hint: 'Create a GitHub issue — PAT + owner/repo + optional labels' },
  { id: 'gitlab_issue',  name: 'GitLab Issue',   icon: FileText,      hint: 'Create a GitLab issue — PRIVATE-TOKEN + project_id' },
  { id: 'asana',         name: 'Asana',          icon: FileText,      hint: 'Asana task — PAT + workspace + project' },
  { id: 'notion',        name: 'Notion',         icon: FileText,      hint: 'Notion page — integration token + database_id' },
  { id: 'sentry',          name: 'Sentry',          icon: Siren,    hint: 'Sentry event via project DSN' },
  { id: 'rollbar',         name: 'Rollbar',         icon: Siren,    hint: 'Rollbar /api/1/item/ — server access token' },
  { id: 'honeybadger',     name: 'Honeybadger',     icon: Siren,    hint: 'Honeybadger /v1/notices — X-API-Key' },
  { id: 'healthchecks_io', name: 'Healthchecks.io', icon: Smartphone, hint: 'Chained ping — /<uuid> on Up, /<uuid>/fail on Down' },
  { id: 'betterstack',     name: 'BetterStack',     icon: Siren,    hint: 'BetterStack (was Better Uptime) integration URL' },
  { id: 'statuspage_io',   name: 'Statuspage.io',   icon: Siren,    hint: 'Atlassian Statuspage incidents — API key + page_id' },
  { id: 'datadog',         name: 'Datadog Events',  icon: Siren,    hint: 'Datadog /api/v1/events — DD-API-KEY' },
  { id: 'newrelic',        name: 'New Relic Events',icon: Siren,    hint: 'New Relic events insert — Api-Key + account_id' },
  { id: 'aws_sns',         name: 'AWS SNS',         icon: Siren,    hint: 'AWS SNS Publish — SigV4 signed; topic ARN or SMS phone number' },
  { id: 'azure_servicebus',name: 'Azure Service Bus',icon: Siren,    hint: 'Azure SB queue/topic — SAS token, send-only policy supported' },
  { id: 'gcp_pubsub',      name: 'GCP Pub/Sub',     icon: Siren,    hint: 'Google Cloud Pub/Sub — service-account JSON; auto-rotates OAuth2 token' },
  { id: 'webpush',         name: 'Web Push',        icon: Bell,     hint: 'Browser push notifications (RFC 8291). Subscribe each device after creating the channel.' },
  // push
  { id: 'ntfy',       name: 'ntfy.sh',         icon: Smartphone,    hint: 'Push to phone via ntfy.sh (free) or self-hosted ntfy server' },
  { id: 'gotify',     name: 'Gotify',          icon: Server,        hint: 'Self-hosted push server (https://gotify.net)' },
  { id: 'pushover',   name: 'Pushover',        icon: Smartphone,    hint: 'Pushover.net push service (paid app, free API)' },
  // ops
  { id: 'pagerduty',  name: 'PagerDuty',       icon: Siren,         hint: 'Events API v2 integration key. Triggers + resolves automatically.' },
  // sms / email
  { id: 'sms_twilio', name: 'Twilio SMS',      icon: Phone,         hint: 'Twilio AccountSID + AuthToken; E.164 phone numbers' },
  { id: 'email',      name: 'Email (SMTP)',    icon: Mail,          hint: 'Any SMTP relay: Gmail, SendGrid, Mailgun, self-hosted Postfix…' },
  // gateway
  { id: 'apprise',    name: 'Apprise gateway', icon: Layers,        hint: '80+ services via apprise-api sidecar. One channel fans out to many.' },
  // catch-all
  { id: 'webhook',    name: 'Generic Webhook', icon: WebhookIcon,   hint: 'Any endpoint that accepts POST application/json. Use for Zapier/n8n/Make.' },
];

// Per-kind form fields rendered on the right when a kind is selected.
function ConfigForm({ kind, config, setConfig }) {
  const set = (k, v) => setConfig({ ...config, [k]: v });
  if (kind === 'slack') {
    return (
      <>
        <div className="field">
          <label className="field-label">Webhook URL</label>
          <input className="input mono" value={config.webhook_url || ''}
            onChange={e => set('webhook_url', e.target.value)}
            placeholder="https://hooks.slack.com/services/T.../B.../xxxxx"/>
        </div>
        <div className="field">
          <label className="field-label">Channel override <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.channel || ''}
            onChange={e => set('channel', e.target.value)}
            placeholder="#alerts"/>
        </div>
      </>
    );
  }
  if (kind === 'discord') {
    return (
      <>
        <div className="field">
          <label className="field-label">Webhook URL</label>
          <input className="input mono" value={config.webhook_url || ''}
            onChange={e => set('webhook_url', e.target.value)}
            placeholder="https://discord.com/api/webhooks/.../..."/>
        </div>
        <div className="field">
          <label className="field-label">Display name override <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}
            placeholder="Rampart"/>
        </div>
      </>
    );
  }
  if (kind === 'webhook') {
    return (
      <>
        <div className="field">
          <label className="field-label">URL</label>
          <input className="input mono" value={config.url || ''}
            onChange={e => set('url', e.target.value)}
            placeholder="https://your-service.example.com/hooks/rampart"/>
        </div>
        <div className="field">
          <label className="field-label">Method</label>
          <select className="select" value={config.method || 'POST'} onChange={e => set('method', e.target.value)}>
            <option>POST</option><option>PUT</option><option>PATCH</option>
          </select>
        </div>
        <div className="field">
          <label className="field-label">HMAC secret <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" type="password" value={config.secret || ''}
            onChange={e => set('secret', e.target.value)}
            placeholder="shared secret — receiver verifies X-Rampart-Signature"/>
          <div className="field-hint">
            When set, every request gets <code>X-Rampart-Signature: sha256=&lt;hex&gt;</code>
            computed as <code>HMAC-SHA256(secret, raw_body)</code>. Receiver must
            recompute against the raw bytes — re-serialized JSON will not match.
          </div>
        </div>
      </>
    );
  }
  if (kind === 'teams') {
    return (
      <div className="field">
        <label className="field-label">Incoming Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)}
          placeholder="https://<tenant>.webhook.office.com/webhookb2/..."/>
      </div>
    );
  }
  if (kind === 'telegram') {
    return (
      <>
        <div className="field">
          <label className="field-label">Bot token</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}
            placeholder="123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"/>
        </div>
        <div className="field">
          <label className="field-label">Chat ID</label>
          <input className="input mono" value={config.chat_id || ''}
            onChange={e => set('chat_id', e.target.value)}
            placeholder="-1001234567890 (group) or 123456789 (DM)"/>
        </div>
      </>
    );
  }
  if (kind === 'email') {
    return (
      <>
        <div style={{ display: 'grid', gridTemplateColumns: '2fr 1fr', gap: 10 }}>
          <div className="field">
            <label className="field-label">SMTP host</label>
            <input className="input mono" value={config.smtp_host || ''}
              onChange={e => set('smtp_host', e.target.value)} placeholder="smtp.gmail.com"/>
          </div>
          <div className="field">
            <label className="field-label">Port</label>
            <input className="input mono" value={config.smtp_port || ''}
              onChange={e => set('smtp_port', parseInt(e.target.value, 10) || '')} placeholder="587"/>
          </div>
        </div>
        <div className="field">
          <label className="field-label">Encryption</label>
          <select className="select" value={config.encryption || 'starttls'} onChange={e => set('encryption', e.target.value)}>
            <option value="starttls">STARTTLS (port 587)</option>
            <option value="tls">Implicit TLS (port 465)</option>
            <option value="plain">None (no TLS)</option>
          </select>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
          <div className="field">
            <label className="field-label">Username</label>
            <input className="input mono" value={config.smtp_user || ''}
              onChange={e => set('smtp_user', e.target.value)} placeholder="alerts@example.com"/>
          </div>
          <div className="field">
            <label className="field-label">Password</label>
            <input className="input mono" type="password" value={config.smtp_password || ''}
              onChange={e => set('smtp_password', e.target.value)} placeholder="app password"/>
          </div>
        </div>
        <div className="field">
          <label className="field-label">From</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder='"Rampart Alerts" &lt;alerts@example.com&gt;'/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="ops@example.com, oncall@example.com"/>
        </div>
      </>
    );
  }
  if (kind === 'ntfy') {
    return (
      <>
        <div className="field">
          <label className="field-label">Server</label>
          <input className="input mono" value={config.server || 'https://ntfy.sh'}
            onChange={e => set('server', e.target.value)} placeholder="https://ntfy.sh"/>
        </div>
        <div className="field">
          <label className="field-label">Topic</label>
          <input className="input mono" value={config.topic || ''}
            onChange={e => set('topic', e.target.value)} placeholder="rampart-myhomelab-x9z"/>
        </div>
        <div className="field">
          <label className="field-label">Priority <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· 1 (min) – 5 (max)</span></label>
          <input className="input mono" type="number" min="1" max="5"
            value={config.priority ?? 3} onChange={e => set('priority', parseInt(e.target.value, 10) || 3)}/>
        </div>
        <div className="field">
          <label className="field-label">Auth header <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional (self-hosted)</span></label>
          <input className="input mono" type="password" value={config.auth || ''}
            onChange={e => set('auth', e.target.value)} placeholder="Bearer tk_..."/>
        </div>
      </>
    );
  }
  if (kind === 'gotify') {
    return (
      <>
        <div className="field">
          <label className="field-label">Server URL</label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="https://gotify.example.com"/>
        </div>
        <div className="field">
          <label className="field-label">Application token</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)} placeholder="A.gotify.app.token"/>
        </div>
        <div className="field">
          <label className="field-label">Priority <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· 0–10</span></label>
          <input className="input mono" type="number" min="0" max="10"
            value={config.priority ?? 5} onChange={e => set('priority', parseInt(e.target.value, 10) || 5)}/>
        </div>
      </>
    );
  }
  if (kind === 'pagerduty') {
    return (
      <>
        <div className="field">
          <label className="field-label">Integration (routing) key</label>
          <input className="input mono" type="password" value={config.routing_key || ''}
            onChange={e => set('routing_key', e.target.value)} placeholder="32-char Events API v2 key"/>
        </div>
        <div className="field">
          <label className="field-label">Component <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input" value={config.component || ''}
            onChange={e => set('component', e.target.value)} placeholder="payments-api"/>
        </div>
        <div className="field-hint">
          Status flips to <strong>down/warn</strong> send <code>trigger</code>;
          flips back to <strong>up</strong> send <code>resolve</code> with the same
          dedup_key (monitor id), so a single incident covers the full outage.
        </div>
      </>
    );
  }
  if (kind === 'pushover') {
    return (
      <>
        <div className="field">
          <label className="field-label">API token</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)} placeholder="30-char application token"/>
        </div>
        <div className="field">
          <label className="field-label">User key</label>
          <input className="input mono" type="password" value={config.user_key || ''}
            onChange={e => set('user_key', e.target.value)} placeholder="30-char user / group key"/>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
          <div className="field">
            <label className="field-label">Priority <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· -2..2</span></label>
            <input className="input mono" type="number" min="-2" max="2"
              value={config.priority ?? 0} onChange={e => set('priority', parseInt(e.target.value, 10) || 0)}/>
          </div>
          <div className="field">
            <label className="field-label">Device <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
            <input className="input mono" value={config.device || ''}
              onChange={e => set('device', e.target.value)} placeholder="phone-pixel"/>
          </div>
        </div>
      </>
    );
  }
  if (kind === 'mattermost' || kind === 'rocket_chat') {
    const isRocket = kind === 'rocket_chat';
    return (
      <>
        <div className="field">
          <label className="field-label">Incoming webhook URL</label>
          <input className="input mono" value={config.webhook_url || ''}
            onChange={e => set('webhook_url', e.target.value)}
            placeholder={isRocket ? 'https://chat.example.com/hooks/...' : 'https://mattermost.example.com/hooks/...'}/>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
          <div className="field">
            <label className="field-label">Channel override <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
            <input className="input mono" value={config.channel || ''}
              onChange={e => set('channel', e.target.value)} placeholder="#alerts"/>
          </div>
          <div className="field">
            <label className="field-label">{isRocket ? 'Alias' : 'Username'} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
            <input className="input" value={isRocket ? (config.alias || '') : (config.username || '')}
              onChange={e => set(isRocket ? 'alias' : 'username', e.target.value)} placeholder="Rampart"/>
          </div>
        </div>
      </>
    );
  }
  if (kind === 'apprise') {
    return (
      <>
        <div className="field">
          <label className="field-label">apprise-api server URL</label>
          <input className="input mono" value={config.apprise_url || ''}
            onChange={e => set('apprise_url', e.target.value)}
            placeholder="http://apprise:8000"/>
          <div className="field-hint">Run the sidecar with: <code style={{ background: 'var(--surface-2)', padding: '0 4px', borderRadius: 3 }}>docker run -d -p 8000:8000 caronc/apprise:latest</code></div>
        </div>
        <div className="field">
          <label className="field-label">Apprise URLs <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <textarea className="input mono" rows={4}
            value={config.urls || ''}
            onChange={e => set('urls', e.target.value)}
            placeholder="tgram://botid:token/chatid,&#10;discord://webhook_id/webhook_token,&#10;mailto://user:pass@gmail.com,&#10;pbul://accesstoken"
            style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 12, padding: '10px 12px', lineHeight: 1.5 }}/>
          <div className="field-hint">
            One Rampart channel fans out to all listed services at once.
            Full URL syntax for 80+ services:{' '}
            <a href="https://github.com/caronc/apprise/wiki" target="_blank" rel="noreferrer" style={{ color: 'var(--accent)' }}>apprise wiki ↗</a>
          </div>
        </div>
      </>
    );
  }
  if (kind === 'matrix') {
    return (
      <>
        <div className="field">
          <label className="field-label">Homeserver</label>
          <input className="input mono" value={config.homeserver || ''}
            onChange={e => set('homeserver', e.target.value)} placeholder="https://matrix.org"/>
        </div>
        <div className="field">
          <label className="field-label">Access token</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)} placeholder="syt_..."/>
        </div>
        <div className="field">
          <label className="field-label">Room ID</label>
          <input className="input mono" value={config.room_id || ''}
            onChange={e => set('room_id', e.target.value)} placeholder="!roomid:matrix.org"/>
        </div>
      </>
    );
  }
  if (kind === 'google_chat') {
    return (
      <div className="field">
        <label className="field-label">Incoming webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://chat.googleapis.com/v1/spaces/.../messages?key=..."/>
      </div>
    );
  }
  if (kind === 'wecom') {
    return (
      <>
        <div className="field">
          <label className="field-label">Bot key</label>
          <input className="input mono" type="password" value={config.bot_key || ''}
            onChange={e => set('bot_key', e.target.value)} placeholder="key from the bot URL"/>
        </div>
        <div className="field">
          <label className="field-label">Mention mobiles <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated, optional</span></label>
          <input className="input mono" value={(config.mentioned_mobile_list || []).join(',')}
            onChange={e => set('mentioned_mobile_list', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}
            placeholder="13800001111,13900002222"/>
        </div>
      </>
    );
  }
  if (kind === 'dingtalk') {
    return (
      <>
        <div className="field">
          <label className="field-label">Access token</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)} placeholder="token from bot URL"/>
        </div>
        <div className="field">
          <label className="field-label">Secret <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional, only if signing is on</span></label>
          <input className="input mono" type="password" value={config.secret || ''}
            onChange={e => set('secret', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'feishu') {
    return (
      <div className="field">
        <label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://open.feishu.cn/open-apis/bot/v2/hook/..."/>
      </div>
    );
  }
  if (kind === 'line') {
    return (
      <>
        <div className="field">
          <label className="field-label">Channel access token</label>
          <input className="input mono" type="password" value={config.channel_access_token || ''}
            onChange={e => set('channel_access_token', e.target.value)} placeholder="LINE Developers Console → Messaging API → Channel access token"/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· user / group / room id</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="Uxxxxxxxxxxxxxxxxxx"/>
        </div>
      </>
    );
  }
  if (kind === 'bark') {
    return (
      <>
        <div className="field">
          <label className="field-label">Device key</label>
          <input className="input mono" type="password" value={config.device_key || ''}
            onChange={e => set('device_key', e.target.value)} placeholder="from the Bark app"/>
        </div>
        <div className="field">
          <label className="field-label">Server <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="https://api.day.app"/>
        </div>
      </>
    );
  }
  if (kind === 'pushbullet') {
    return (
      <div className="field">
        <label className="field-label">Access token</label>
        <input className="input mono" type="password" value={config.access_token || ''}
          onChange={e => set('access_token', e.target.value)} placeholder="o.xxxxx..."/>
      </div>
    );
  }
  if (kind === 'sendgrid') {
    return (
      <>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)} placeholder="SG.xxxxx"/>
        </div>
        <div className="field">
          <label className="field-label">From email</label>
          <input className="input" value={config.from_email || ''}
            onChange={e => set('from_email', e.target.value)} placeholder="alerts@example.com"/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="ops@example.com, sre@example.com"/>
        </div>
      </>
    );
  }
  if (kind === 'resend') {
    return (
      <>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)} placeholder="re_xxxxx"/>
        </div>
        <div className="field">
          <label className="field-label">From</label>
          <input className="input" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder='"Rampart" <alerts@example.com>'/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="ops@example.com"/>
        </div>
      </>
    );
  }
  if (kind === 'brevo') {
    return (
      <>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)} placeholder="xkeysib-..."/>
        </div>
        <div className="field">
          <label className="field-label">From email</label>
          <input className="input" value={config.from_email || ''}
            onChange={e => set('from_email', e.target.value)} placeholder="alerts@example.com"/>
        </div>
        <div className="field">
          <label className="field-label">To email</label>
          <input className="input" value={config.to_email || ''}
            onChange={e => set('to_email', e.target.value)} placeholder="ops@example.com"/>
        </div>
      </>
    );
  }
  if (kind === 'opsgenie') {
    return (
      <>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)} placeholder="GenieKey"/>
        </div>
        <div className="field">
          <label className="field-label">Region</label>
          <select className="select" value={config.region || 'us'} onChange={e => set('region', e.target.value)}>
            <option value="us">US (api.opsgenie.com)</option>
            <option value="eu">EU (api.eu.opsgenie.com)</option>
          </select>
        </div>
        <div className="field">
          <label className="field-label">Priority</label>
          <select className="select" value={config.priority || 'P3'} onChange={e => set('priority', e.target.value)}>
            <option>P1</option><option>P2</option><option>P3</option><option>P4</option><option>P5</option>
          </select>
        </div>
      </>
    );
  }
  if (kind === 'pagertree') {
    return (
      <>
        <div className="field">
          <label className="field-label">Integration URL</label>
          <input className="input mono" value={config.integration_url || ''}
            onChange={e => set('integration_url', e.target.value)} placeholder="https://api.pagertree.com/integration/..."/>
        </div>
        <div className="field">
          <label className="field-label">Severity</label>
          <select className="select" value={config.severity || 'SEV-3'} onChange={e => set('severity', e.target.value)}>
            <option>SEV-1</option><option>SEV-2</option><option>SEV-3</option><option>SEV-4</option><option>SEV-5</option>
          </select>
        </div>
      </>
    );
  }
  if (kind === 'squadcast') {
    return (
      <div className="field">
        <label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://api.squadcast.com/v2/incidents/..."/>
      </div>
    );
  }
  if (kind === 'signal') {
    return (
      <>
        <div className="field">
          <label className="field-label">signal-cli REST API URL</label>
          <input className="input mono" value={config.api_url || ''}
            onChange={e => set('api_url', e.target.value)} placeholder="http://signal-cli:8080"/>
        </div>
        <div className="field">
          <label className="field-label">From number</label>
          <input className="input mono" value={config.number || ''}
            onChange={e => set('number', e.target.value)} placeholder="+15551234567"/>
        </div>
        <div className="field">
          <label className="field-label">Recipients <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated phone numbers or group ids</span></label>
          <input className="input mono" value={(config.recipients || []).join(',')}
            onChange={e => set('recipients', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}
            placeholder="+15559876543, +44..."/>
        </div>
      </>
    );
  }
  if (kind === 'zulip') {
    return (
      <>
        <div className="field">
          <label className="field-label">Server</label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="https://yourzulip.example.com"/>
        </div>
        <div className="field">
          <label className="field-label">Bot email</label>
          <input className="input mono" value={config.bot_email || ''}
            onChange={e => set('bot_email', e.target.value)} placeholder="bot@yourzulip.example.com"/>
        </div>
        <div className="field">
          <label className="field-label">Bot API key</label>
          <input className="input mono" type="password" value={config.bot_key || ''}
            onChange={e => set('bot_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Type</label>
          <select className="select" value={config.kind || 'stream'} onChange={e => set('kind', e.target.value)}>
            <option value="stream">stream</option>
            <option value="private">private</option>
          </select>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· stream name or email(s)</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="alerts / alice@example.com"/>
        </div>
        <div className="field">
          <label className="field-label">Topic <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· streams only</span></label>
          <input className="input mono" value={config.topic || ''}
            onChange={e => set('topic', e.target.value)} placeholder="rampart"/>
        </div>
      </>
    );
  }
  if (kind === 'lark') {
    return (
      <div className="field">
        <label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://open.larksuite.com/open-apis/bot/v2/hook/..."/>
      </div>
    );
  }
  if (kind === 'goalert') {
    return (
      <div className="field">
        <label className="field-label">Integration URL</label>
        <input className="input mono" value={config.integration_url || ''}
          onChange={e => set('integration_url', e.target.value)} placeholder="https://goalert.example.com/api/v2/generic/incoming?token=..."/>
      </div>
    );
  }
  if (kind === 'alerta') {
    return (
      <>
        <div className="field">
          <label className="field-label">API URL</label>
          <input className="input mono" value={config.api_url || ''}
            onChange={e => set('api_url', e.target.value)} placeholder="https://alerta.example.com/api"/>
        </div>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Environment</label>
          <input className="input" value={config.environment || ''}
            onChange={e => set('environment', e.target.value)} placeholder="Production"/>
        </div>
      </>
    );
  }
  if (kind === 'alertnow') {
    return (
      <div className="field">
        <label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://api.alertnow.io/..."/>
      </div>
    );
  }
  if (kind === 'signl4') {
    return (
      <div className="field">
        <label className="field-label">Team secret</label>
        <input className="input mono" type="password" value={config.team_secret || ''}
          onChange={e => set('team_secret', e.target.value)} placeholder="UUID from the SIGNL4 connect URL"/>
      </div>
    );
  }
  if (kind === 'heii_oncall') {
    return (
      <>
        <div className="field">
          <label className="field-label">Trigger URL</label>
          <input className="input mono" value={config.trigger_url || ''}
            onChange={e => set('trigger_url', e.target.value)} placeholder="https://api.heiioncall.com/..."/>
        </div>
        <div className="field">
          <label className="field-label">Close URL <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.close_url || ''}
            onChange={e => set('close_url', e.target.value)} placeholder="https://api.heiioncall.com/.../close"/>
        </div>
      </>
    );
  }
  if (kind === 'serverchan') {
    return (
      <div className="field">
        <label className="field-label">SendKey</label>
        <input className="input mono" type="password" value={config.send_key || ''}
          onChange={e => set('send_key', e.target.value)} placeholder="SCT..."/>
      </div>
    );
  }
  if (kind === 'pushplus') {
    return (
      <>
        <div className="field">
          <label className="field-label">Token</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Topic <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional, for group send</span></label>
          <input className="input mono" value={config.topic || ''}
            onChange={e => set('topic', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'pushdeer') {
    return (
      <>
        <div className="field">
          <label className="field-label">Push key</label>
          <input className="input mono" type="password" value={config.push_key || ''}
            onChange={e => set('push_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Server <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="https://api2.pushdeer.com"/>
        </div>
      </>
    );
  }
  if (kind === 'aliyun_sms') {
    return (
      <>
        <div className="field">
          <label className="field-label">Access Key ID</label>
          <input className="input mono" value={config.access_key_id || ''}
            onChange={e => set('access_key_id', e.target.value)} placeholder="LTAI..."/>
        </div>
        <div className="field">
          <label className="field-label">Access Key Secret</label>
          <input className="input mono" type="password" value={config.access_key_secret || ''}
            onChange={e => set('access_key_secret', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Sign name</label>
          <input className="input" value={config.sign_name || ''}
            onChange={e => set('sign_name', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Template code</label>
          <input className="input mono" value={config.template_code || ''}
            onChange={e => set('template_code', e.target.value)} placeholder="SMS_..."/>
        </div>
        <div className="field">
          <label className="field-label">Phone numbers <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.phone_numbers || ''}
            onChange={e => set('phone_numbers', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'mastodon') {
    return (
      <>
        <div className="field">
          <label className="field-label">Server</label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="https://mastodon.social"/>
        </div>
        <div className="field">
          <label className="field-label">Access token</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Visibility</label>
          <select className="select" value={config.visibility || 'private'} onChange={e => set('visibility', e.target.value)}>
            <option value="public">public</option>
            <option value="unlisted">unlisted</option>
            <option value="private">private (followers-only)</option>
            <option value="direct">direct</option>
          </select>
        </div>
      </>
    );
  }
  if (kind === 'pumble') {
    return (
      <div className="field">
        <label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://pumble.com/api/incoming-webhooks/..."/>
      </div>
    );
  }
  if (kind === 'bitrix24') {
    return (
      <>
        <div className="field">
          <label className="field-label">Webhook URL</label>
          <input className="input mono" value={config.webhook_url || ''}
            onChange={e => set('webhook_url', e.target.value)} placeholder="https://yourorg.bitrix24.com/rest/<user>/<token>"/>
        </div>
        <div className="field">
          <label className="field-label">USER_ID</label>
          <input className="input mono" value={config.user_id || ''}
            onChange={e => set('user_id', e.target.value)} placeholder="1"/>
        </div>
      </>
    );
  }
  if (kind === 'stackfield') {
    return (
      <div className="field">
        <label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://www.stackfield.com/api/incoming-webhook/..."/>
      </div>
    );
  }
  if (kind === 'splunk' || kind === 'grafana_oncall') {
    const label = kind === 'splunk' ? 'Integration URL' : 'Webhook URL';
    const k = kind === 'splunk' ? 'integration_url' : 'webhook_url';
    return (
      <div className="field">
        <label className="field-label">{label}</label>
        <input className="input mono" value={config[k] || ''}
          onChange={e => set(k, e.target.value)} placeholder="https://..."/>
      </div>
    );
  }
  if (kind === 'home_assistant') {
    return (
      <>
        <div className="field">
          <label className="field-label">Base URL</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://homeassistant.local:8123"/>
        </div>
        <div className="field">
          <label className="field-label">Long-lived access token</label>
          <input className="input mono" type="password" value={config.long_lived_token || ''}
            onChange={e => set('long_lived_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Notify service</label>
          <input className="input mono" value={config.notify_service || ''}
            onChange={e => set('notify_service', e.target.value)} placeholder="mobile_app_phone / persistent_notification"/>
        </div>
      </>
    );
  }
  if (kind === 'clicksend') {
    return (
      <>
        <div className="field">
          <label className="field-label">Username</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">From</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="Rampart"/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated E.164</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="+15551234567"/>
        </div>
      </>
    );
  }
  if (kind === 'sms_46elks') {
    return (
      <>
        <div className="field">
          <label className="field-label">API username</label>
          <input className="input mono" value={config.api_username || ''}
            onChange={e => set('api_username', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">API password</label>
          <input className="input mono" type="password" value={config.api_password || ''}
            onChange={e => set('api_password', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">From</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="Rampart"/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated E.164</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'callmebot') {
    return (
      <div className="field">
        <label className="field-label">Endpoint URL</label>
        <input className="input mono" value={config.endpoint_url || ''}
          onChange={e => set('endpoint_url', e.target.value)} placeholder="https://api.callmebot.com/whatsapp.php?phone=...&apikey=..."/>
      </div>
    );
  }
  if (kind === 'telnyx') {
    return (
      <>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">From</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="+15551234567"/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'notifery') {
    return (
      <>
        <div className="field">
          <label className="field-label">API token</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Group</label>
          <input className="input" value={config.group || ''}
            onChange={e => set('group', e.target.value)} placeholder="rampart"/>
        </div>
      </>
    );
  }
  if (kind === 'whatsapp_waha') {
    return (
      <>
        <div className="field">
          <label className="field-label">Base URL</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://waha.local:3000"/>
        </div>
        <div className="field">
          <label className="field-label">Session</label>
          <input className="input mono" value={config.session || ''}
            onChange={e => set('session', e.target.value)} placeholder="default"/>
        </div>
        <div className="field">
          <label className="field-label">Chat ID</label>
          <input className="input mono" value={config.chat_id || ''}
            onChange={e => set('chat_id', e.target.value)} placeholder="15551234567@c.us"/>
        </div>
        <div className="field">
          <label className="field-label">API key <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'threema') {
    return (
      <>
        <div className="field">
          <label className="field-label">Gateway ID</label>
          <input className="input mono" value={config.gateway_id || ''}
            onChange={e => set('gateway_id', e.target.value)} placeholder="*MYGW01"/>
        </div>
        <div className="field">
          <label className="field-label">Secret</label>
          <input className="input mono" type="password" value={config.secret || ''}
            onChange={e => set('secret', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· Threema ID, email, or phone</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'bale') {
    return (
      <>
        <div className="field">
          <label className="field-label">Bot token</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Chat ID</label>
          <input className="input mono" value={config.chat_id || ''}
            onChange={e => set('chat_id', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'pushy') {
    return (
      <>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Device tokens <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={(config.to || []).join(',')}
            onChange={e => set('to', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}/>
        </div>
      </>
    );
  }
  if (kind === 'zoho_cliq') {
    return (
      <div className="field">
        <label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://cliq.zoho.com/api/v2/channelsbyname/...incoming?zapikey=..."/>
      </div>
    );
  }
  if (kind === 'sms_manager') {
    return (
      <>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Numbers <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.numbers || ''}
            onChange={e => set('numbers', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Quality</label>
          <select className="select" value={config.quality || 'economy'} onChange={e => set('quality', e.target.value)}>
            <option>lowcost</option><option>economy</option><option>high</option>
          </select>
        </div>
        <div className="field">
          <label className="field-label">Sender ID <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.sender_id || ''}
            onChange={e => set('sender_id', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'sms_eagle') {
    return (
      <>
        <div className="field">
          <label className="field-label">Base URL</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://smseagle.local"/>
        </div>
        <div className="field">
          <label className="field-label">Access token</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated E.164</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'octopush') {
    return (
      <>
        <div className="field">
          <label className="field-label">API login</label>
          <input className="input" value={config.api_login || ''}
            onChange={e => set('api_login', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">Sender</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)} placeholder="Rampart"/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'whatsapp_whapi') {
    return (
      <>
        <div className="field"><label className="field-label">API token</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· jid (15551234567@s.whatsapp.net)</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
        <div className="field"><label className="field-label">Base URL <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://gate.whapi.cloud"/></div>
      </>
    );
  }
  if (kind === 'whatsapp_360') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Phone</label>
          <input className="input mono" value={config.phone || ''}
            onChange={e => set('phone', e.target.value)} placeholder="15551234567"/></div>
      </>
    );
  }
  if (kind === 'whatsapp_evolution') {
    return (
      <>
        <div className="field"><label className="field-label">Base URL</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://evolution.local:8080"/></div>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Instance</label>
          <input className="input mono" value={config.instance || ''}
            onChange={e => set('instance', e.target.value)}/></div>
        <div className="field"><label className="field-label">Number</label>
          <input className="input mono" value={config.number || ''}
            onChange={e => set('number', e.target.value)} placeholder="15551234567"/></div>
      </>
    );
  }
  if (kind === 'flock') {
    return (
      <div className="field"><label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://api.flock.com/hooks/sendMessage/..."/></div>
    );
  }
  if (kind === 'serwersms') {
    return (
      <>
        <div className="field"><label className="field-label">Username</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">Password</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">Sender</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)}/></div>
        <div className="field"><label className="field-label">Phone <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.phone || ''}
            onChange={e => set('phone', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'smsplanet') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Sender</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)}/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'smsc') {
    return (
      <>
        <div className="field"><label className="field-label">Login</label>
          <input className="input" value={config.login || ''}
            onChange={e => set('login', e.target.value)}/></div>
        <div className="field"><label className="field-label">Password</label>
          <input className="input mono" type="password" value={config.psw || ''}
            onChange={e => set('psw', e.target.value)}/></div>
        <div className="field"><label className="field-label">Phones <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.phones || ''}
            onChange={e => set('phones', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'cellsynt') {
    return (
      <>
        <div className="field"><label className="field-label">Username</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">Password</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">Originator</label>
          <input className="input mono" value={config.originator || ''}
            onChange={e => set('originator', e.target.value)} placeholder="Rampart"/></div>
        <div className="field"><label className="field-label">Destination <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.destination || ''}
            onChange={e => set('destination', e.target.value)} placeholder="0046701234567"/></div>
      </>
    );
  }
  if (kind === 'sevenio') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated E.164</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
        <div className="field"><label className="field-label">From <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'gtxmessaging') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Sender ID</label>
          <input className="input mono" value={config.sender_id || ''}
            onChange={e => set('sender_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'onesender') {
    return (
      <>
        <div className="field"><label className="field-label">Base URL</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://onesender.local"/></div>
        <div className="field"><label className="field-label">API token</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Recipient</label>
          <input className="input mono" value={config.recipient || ''}
            onChange={e => set('recipient', e.target.value)} placeholder="15551234567"/></div>
      </>
    );
  }
  if (kind === 'promosms') {
    return (
      <>
        <div className="field"><label className="field-label">Username</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">Password</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">Sender</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)}/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'smspartner') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Sender</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)}/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'sms_ir') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Line number</label>
          <input className="input mono" value={config.line_number || ''}
            onChange={e => set('line_number', e.target.value)}/></div>
        <div className="field"><label className="field-label">Mobiles <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.mobiles || ''}
            onChange={e => set('mobiles', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'freemobile') {
    return (
      <>
        <div className="field"><label className="field-label">User</label>
          <input className="input" value={config.user || ''}
            onChange={e => set('user', e.target.value)}/></div>
        <div className="field"><label className="field-label">Pass</label>
          <input className="input mono" type="password" value={config.pass || ''}
            onChange={e => set('pass', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'flashduty') {
    return (
      <>
        <div className="field"><label className="field-label">Integration URL</label>
          <input className="input mono" value={config.integration_url || ''}
            onChange={e => set('integration_url', e.target.value)}/></div>
        <div className="field"><label className="field-label">Severity</label>
          <select className="select" value={config.severity || 'Warning'} onChange={e => set('severity', e.target.value)}>
            <option>Info</option><option>Warning</option><option>Critical</option>
          </select>
        </div>
      </>
    );
  }
  if (kind === 'teltonika') {
    return (
      <>
        <div className="field"><label className="field-label">Base URL</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://192.168.1.1"/></div>
        <div className="field"><label className="field-label">Username</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">Password</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">Number</label>
          <input className="input mono" value={config.number || ''}
            onChange={e => set('number', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'kook') {
    return (
      <>
        <div className="field"><label className="field-label">Bot token</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Target type</label>
          <select className="select" value={config.target_type || 'GROUP'} onChange={e => set('target_type', e.target.value)}>
            <option>GROUP</option><option>PERSON</option>
          </select>
        </div>
        <div className="field"><label className="field-label">Target ID</label>
          <input className="input mono" value={config.target_id || ''}
            onChange={e => set('target_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'nostr') {
    return (
      <>
        <div className="field"><label className="field-label">Bridge URL</label>
          <input className="input mono" value={config.bridge_url || ''}
            onChange={e => set('bridge_url', e.target.value)} placeholder="http://nostr-bridge.local/dm"/></div>
        <div className="field"><label className="field-label">Recipient <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· npub or hex pubkey</span></label>
          <input className="input mono" value={config.recipient || ''}
            onChange={e => set('recipient', e.target.value)}/></div>
        <div className="field"><label className="field-label">API key <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'onebot') {
    return (
      <>
        <div className="field"><label className="field-label">HTTP URL</label>
          <input className="input mono" value={config.http_url || ''}
            onChange={e => set('http_url', e.target.value)} placeholder="http://onebot.local:5700"/></div>
        <div className="field"><label className="field-label">Kind</label>
          <select className="select" value={config.kind || 'group'} onChange={e => set('kind', e.target.value)}>
            <option>group</option><option>private</option>
          </select>
        </div>
        <div className="field"><label className="field-label">Target ID</label>
          <input className="input mono" type="number" value={config.target_id || ''}
            onChange={e => set('target_id', parseInt(e.target.value, 10) || 0)}/></div>
        <div className="field"><label className="field-label">Access token <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'onechat') {
    return (
      <>
        <div className="field"><label className="field-label">Bot token</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Chat ID</label>
          <input className="input mono" value={config.chat_id || ''}
            onChange={e => set('chat_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'max_messenger') {
    return (
      <>
        <div className="field"><label className="field-label">Access token</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Chat ID</label>
          <input className="input mono" value={config.chat_id || ''}
            onChange={e => set('chat_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'halo_psa') {
    return (
      <>
        <div className="field"><label className="field-label">Base URL</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://yourorg.halopsa.com"/></div>
        <div className="field"><label className="field-label">Client ID</label>
          <input className="input mono" value={config.client_id || ''}
            onChange={e => set('client_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">Client secret</label>
          <input className="input mono" type="password" value={config.client_secret || ''}
            onChange={e => set('client_secret', e.target.value)}/></div>
        <div className="field"><label className="field-label">Team</label>
          <input className="input" value={config.team || ''}
            onChange={e => set('team', e.target.value)} placeholder="Service Desk"/></div>
        <div className="field"><label className="field-label">Ticket type ID</label>
          <input className="input mono" type="number" value={config.ticket_type_id || ''}
            onChange={e => set('ticket_type_id', parseInt(e.target.value, 10) || 0)}/></div>
      </>
    );
  }
  if (kind === 'jira_sm') {
    return (
      <>
        <div className="field"><label className="field-label">Site URL</label>
          <input className="input mono" value={config.site_url || ''}
            onChange={e => set('site_url', e.target.value)} placeholder="https://yourorg.atlassian.net"/></div>
        <div className="field"><label className="field-label">Email</label>
          <input className="input" value={config.email || ''}
            onChange={e => set('email', e.target.value)}/></div>
        <div className="field"><label className="field-label">API token</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Project key</label>
          <input className="input mono" value={config.project_key || ''}
            onChange={e => set('project_key', e.target.value)} placeholder="INC"/></div>
        <div className="field"><label className="field-label">Issue type</label>
          <input className="input mono" value={config.issue_type || ''}
            onChange={e => set('issue_type', e.target.value)} placeholder="Incident"/></div>
      </>
    );
  }
  if (kind === 'spug_push') {
    return (
      <div className="field"><label className="field-label">Template code</label>
        <input className="input mono" value={config.template_code || ''}
          onChange={e => set('template_code', e.target.value)}/></div>
    );
  }
  if (kind === 'wpush') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Channel <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.channel || ''}
            onChange={e => set('channel', e.target.value)} placeholder="wechat,email"/></div>
      </>
    );
  }
  if (kind === 'vk') {
    return (
      <>
        <div className="field"><label className="field-label">Access token</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Peer ID</label>
          <input className="input mono" type="number" value={config.peer_id || ''}
            onChange={e => set('peer_id', parseInt(e.target.value, 10) || 0)}/></div>
        <div className="field"><label className="field-label">API version <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.api_version || ''}
            onChange={e => set('api_version', e.target.value)} placeholder="5.199"/></div>
      </>
    );
  }
  if (kind === 'yzj') {
    return (
      <div className="field"><label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)}/></div>
    );
  }
  if (kind === 'google_sheets') {
    return (
      <div className="field"><label className="field-label">Apps Script web-app URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://script.google.com/macros/s/.../exec"/></div>
    );
  }
  if (kind === 'gorush') {
    return (
      <>
        <div className="field"><label className="field-label">Server</label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="http://gorush.local:8088"/></div>
        <div className="field"><label className="field-label">Platform</label>
          <select className="select" value={config.platform || 'ios'} onChange={e => set('platform', e.target.value)}>
            <option>ios</option><option>android</option>
          </select>
        </div>
        <div className="field"><label className="field-label">Tokens <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={(config.tokens || []).join(',')}
            onChange={e => set('tokens', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}/></div>
        <div className="field"><label className="field-label">Topic <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.topic || ''}
            onChange={e => set('topic', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'fluxer' || kind === 'splash') {
    return (
      <div className="field"><label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)}/></div>
    );
  }
  if (kind === 'messagebird') {
    return (
      <>
        <div className="field"><label className="field-label">Access key</label>
          <input className="input mono" type="password" value={config.access_key || ''}
            onChange={e => set('access_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Originator</label>
          <input className="input mono" value={config.originator || ''}
            onChange={e => set('originator', e.target.value)}/></div>
        <div className="field"><label className="field-label">Recipients <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.recipients || ''}
            onChange={e => set('recipients', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'plivo') {
    return (
      <>
        <div className="field"><label className="field-label">Auth ID</label>
          <input className="input mono" value={config.auth_id || ''}
            onChange={e => set('auth_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">Auth token</label>
          <input className="input mono" type="password" value={config.auth_token || ''}
            onChange={e => set('auth_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">From</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="+15551234567"/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· angle-bracket separated, e.g. {'+15551111<+15552222>'}</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'vonage') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">API secret</label>
          <input className="input mono" type="password" value={config.api_secret || ''}
            onChange={e => set('api_secret', e.target.value)}/></div>
        <div className="field"><label className="field-label">From</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
        <div className="field"><label className="field-label">To</label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="15551234567"/></div>
      </>
    );
  }
  if (kind === 'bandwidth') {
    return (
      <>
        <div className="field"><label className="field-label">Account ID</label>
          <input className="input mono" value={config.account_id || ''}
            onChange={e => set('account_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">Username</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">Password</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">Application ID</label>
          <input className="input mono" value={config.application_id || ''}
            onChange={e => set('application_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">From</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="+15551234567"/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'webex') {
    return (
      <>
        <div className="field"><label className="field-label">Bot token</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Room ID</label>
          <input className="input mono" value={config.room_id || ''}
            onChange={e => set('room_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'pushcut') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Notification name</label>
          <input className="input mono" value={config.notification_name || ''}
            onChange={e => set('notification_name', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'smsglobal') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">API secret</label>
          <input className="input mono" type="password" value={config.api_secret || ''}
            onChange={e => set('api_secret', e.target.value)}/></div>
        <div className="field"><label className="field-label">Origin</label>
          <input className="input mono" value={config.origin || ''}
            onChange={e => set('origin', e.target.value)}/></div>
        <div className="field"><label className="field-label">Destination <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.destination || ''}
            onChange={e => set('destination', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'alertops') {
    return (
      <div className="field"><label className="field-label">Integration URL</label>
        <input className="input mono" value={config.integration_url || ''}
          onChange={e => set('integration_url', e.target.value)}/></div>
    );
  }
  if (kind === 'mailgun') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Domain</label>
          <input className="input mono" value={config.domain || ''}
            onChange={e => set('domain', e.target.value)} placeholder="mg.example.com"/></div>
        <div className="field"><label className="field-label">Base URL <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· EU: https://api.eu.mailgun.net</span></label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://api.mailgun.net"/></div>
        <div className="field"><label className="field-label">From</label>
          <input className="input" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'mailjet') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">API secret</label>
          <input className="input mono" type="password" value={config.api_secret || ''}
            onChange={e => set('api_secret', e.target.value)}/></div>
        <div className="field"><label className="field-label">From email</label>
          <input className="input" value={config.from_email || ''}
            onChange={e => set('from_email', e.target.value)}/></div>
        <div className="field"><label className="field-label">To email</label>
          <input className="input" value={config.to_email || ''}
            onChange={e => set('to_email', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'postmark') {
    return (
      <>
        <div className="field"><label className="field-label">Server token</label>
          <input className="input mono" type="password" value={config.server_token || ''}
            onChange={e => set('server_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">From</label>
          <input className="input" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
        <div className="field"><label className="field-label">To</label>
          <input className="input" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
        <div className="field"><label className="field-label">Message stream <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.message_stream || ''}
            onChange={e => set('message_stream', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'mandrill') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">From email</label>
          <input className="input" value={config.from_email || ''}
            onChange={e => set('from_email', e.target.value)}/></div>
        <div className="field"><label className="field-label">To email</label>
          <input className="input" value={config.to_email || ''}
            onChange={e => set('to_email', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'sparkpost') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">From</label>
          <input className="input" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
        <div className="field"><label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
        <div className="field"><label className="field-label">Base URL <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· EU: https://api.eu.sparkpost.com</span></label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://api.sparkpost.com"/></div>
      </>
    );
  }
  if (kind === 'spike_sh' || kind === 'zenduty') {
    return (
      <div className="field"><label className="field-label">Integration URL</label>
        <input className="input mono" value={config.integration_url || ''}
          onChange={e => set('integration_url', e.target.value)}/></div>
    );
  }
  if (kind === 'ringcentral') {
    return (
      <div className="field"><label className="field-label">Webhook URL</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://hooks.ringcentral.com/webhook/..."/></div>
    );
  }
  if (kind === 'ilert') {
    return (
      <div className="field"><label className="field-label">Integration key</label>
        <input className="input mono" type="password" value={config.integration_key || ''}
          onChange={e => set('integration_key', e.target.value)}/></div>
    );
  }
  if (kind === 'linear') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Team ID</label>
          <input className="input mono" value={config.team_id || ''}
            onChange={e => set('team_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'clickup') {
    return (
      <>
        <div className="field"><label className="field-label">API token</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">List ID</label>
          <input className="input mono" value={config.list_id || ''}
            onChange={e => set('list_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'trello') {
    return (
      <>
        <div className="field"><label className="field-label">Key</label>
          <input className="input mono" value={config.key || ''}
            onChange={e => set('key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Token</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)}/></div>
        <div className="field"><label className="field-label">List ID</label>
          <input className="input mono" value={config.list_id || ''}
            onChange={e => set('list_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'github_issue') {
    return (
      <>
        <div className="field"><label className="field-label">Personal access token</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Owner</label>
          <input className="input mono" value={config.owner || ''}
            onChange={e => set('owner', e.target.value)} placeholder="myorg"/></div>
        <div className="field"><label className="field-label">Repo</label>
          <input className="input mono" value={config.repo || ''}
            onChange={e => set('repo', e.target.value)} placeholder="incidents"/></div>
        <div className="field"><label className="field-label">Labels <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated, optional</span></label>
          <input className="input mono" value={(config.labels || []).join(',')}
            onChange={e => set('labels', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}/></div>
      </>
    );
  }
  if (kind === 'gitlab_issue') {
    return (
      <>
        <div className="field"><label className="field-label">Base URL <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional, defaults to gitlab.com</span></label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://gitlab.example.com"/></div>
        <div className="field"><label className="field-label">Personal access token</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Project ID</label>
          <input className="input mono" value={config.project_id || ''}
            onChange={e => set('project_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'asana') {
    return (
      <>
        <div className="field"><label className="field-label">Access token</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Workspace</label>
          <input className="input mono" value={config.workspace || ''}
            onChange={e => set('workspace', e.target.value)}/></div>
        <div className="field"><label className="field-label">Project</label>
          <input className="input mono" value={config.project || ''}
            onChange={e => set('project', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'notion') {
    return (
      <>
        <div className="field"><label className="field-label">Integration token</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Database ID</label>
          <input className="input mono" value={config.database_id || ''}
            onChange={e => set('database_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'sentry') {
    return (
      <div className="field"><label className="field-label">DSN</label>
        <input className="input mono" type="password" value={config.dsn || ''}
          onChange={e => set('dsn', e.target.value)} placeholder="https://<public>@<host>/<project>"/></div>
    );
  }
  if (kind === 'rollbar') {
    return (
      <>
        <div className="field"><label className="field-label">Access token</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">Environment</label>
          <input className="input mono" value={config.environment || ''}
            onChange={e => set('environment', e.target.value)} placeholder="production"/></div>
      </>
    );
  }
  if (kind === 'honeybadger') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Environment</label>
          <input className="input mono" value={config.environment || ''}
            onChange={e => set('environment', e.target.value)} placeholder="production"/></div>
      </>
    );
  }
  if (kind === 'healthchecks_io') {
    return (
      <div className="field"><label className="field-label">Ping URL</label>
        <input className="input mono" value={config.ping_url || ''}
          onChange={e => set('ping_url', e.target.value)} placeholder="https://hc-ping.com/&lt;uuid&gt;"/></div>
    );
  }
  if (kind === 'betterstack') {
    return (
      <div className="field"><label className="field-label">Integration URL</label>
        <input className="input mono" value={config.integration_url || ''}
          onChange={e => set('integration_url', e.target.value)}/></div>
    );
  }
  if (kind === 'statuspage_io') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Page ID</label>
          <input className="input mono" value={config.page_id || ''}
            onChange={e => set('page_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'datadog') {
    return (
      <>
        <div className="field"><label className="field-label">API key</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Site</label>
          <select className="select" value={config.site || 'us1'} onChange={e => set('site', e.target.value)}>
            <option>us1</option><option>us3</option><option>us5</option><option>eu</option><option>us1-fed</option>
          </select>
        </div>
      </>
    );
  }
  if (kind === 'newrelic') {
    return (
      <>
        <div className="field"><label className="field-label">Insert key</label>
          <input className="input mono" type="password" value={config.insert_key || ''}
            onChange={e => set('insert_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Account ID</label>
          <input className="input mono" value={config.account_id || ''}
            onChange={e => set('account_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">Region</label>
          <select className="select" value={config.region || 'us'} onChange={e => set('region', e.target.value)}>
            <option>us</option><option>eu</option>
          </select>
        </div>
      </>
    );
  }
  if (kind === 'aws_sns') {
    return (
      <>
        <div className="field"><label className="field-label">Region</label>
          <input className="input mono" value={config.region || ''}
            onChange={e => set('region', e.target.value)} placeholder="us-east-1"/></div>
        <div className="field"><label className="field-label">Topic ARN <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· or phone number</span></label>
          <input className="input mono" value={config.topic_arn || ''}
            onChange={e => set('topic_arn', e.target.value)} placeholder="arn:aws:sns:us-east-1:123456789012:alerts"/></div>
        <div className="field"><label className="field-label">Phone number (SMS, E.164)</label>
          <input className="input mono" value={config.phone_number || ''}
            onChange={e => set('phone_number', e.target.value)} placeholder="+15555550100"/></div>
        <div className="field"><label className="field-label">Access key ID</label>
          <input className="input mono" value={config.access_key_id || ''}
            onChange={e => set('access_key_id', e.target.value)} placeholder="AKIA…"/></div>
        <div className="field"><label className="field-label">Secret access key</label>
          <input className="input mono" type="password" value={config.secret_access_key || ''}
            onChange={e => set('secret_access_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Session token <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional, STS</span></label>
          <input className="input mono" type="password" value={config.session_token || ''}
            onChange={e => set('session_token', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'webpush') {
    return (
      <>
        <div className="field"><label className="field-label">Contact URI <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={config.subject || ''}
            onChange={e => set('subject', e.target.value)} placeholder="mailto:ops@example.com"/>
          <div className="field-hint">
            Sent as the VAPID <code>sub</code> claim — push services may use it to contact
            you about delivery problems. After saving this channel, click
            <strong> Enable push</strong> on its row to subscribe this browser.
          </div>
        </div>
      </>
    );
  }
  if (kind === 'gcp_pubsub') {
    return (
      <>
        <div className="field"><label className="field-label">Project ID</label>
          <input className="input mono" value={config.project_id || ''}
            onChange={e => set('project_id', e.target.value)} placeholder="my-gcp-project"/></div>
        <div className="field"><label className="field-label">Topic</label>
          <input className="input mono" value={config.topic || ''}
            onChange={e => set('topic', e.target.value)} placeholder="rampart-alerts"/></div>
        <div className="field"><label className="field-label">Service account email</label>
          <input className="input mono" value={config.client_email || ''}
            onChange={e => set('client_email', e.target.value)} placeholder="rampart@my-project.iam.gserviceaccount.com"/></div>
        <div className="field"><label className="field-label">Private key (PEM, from service-account JSON)</label>
          <textarea className="input mono" rows={6} value={config.private_key || ''}
            onChange={e => set('private_key', e.target.value)}
            placeholder="-----BEGIN PRIVATE KEY-----\n..."/></div>
        <div className="field-hint" style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
          Paste the <code>private_key</code> field verbatim from the downloaded JSON,
          including the BEGIN/END lines. Access tokens are minted on demand and
          cached in memory until ~60s before expiry.
        </div>
      </>
    );
  }
  if (kind === 'azure_servicebus') {
    return (
      <>
        <div className="field"><label className="field-label">Namespace</label>
          <input className="input mono" value={config.namespace || ''}
            onChange={e => set('namespace', e.target.value)} placeholder="my-namespace (omit .servicebus.windows.net)"/></div>
        <div className="field"><label className="field-label">Queue or topic</label>
          <input className="input mono" value={config.entity || ''}
            onChange={e => set('entity', e.target.value)} placeholder="alerts"/></div>
        <div className="field"><label className="field-label">SAS key name</label>
          <input className="input mono" value={config.sas_key_name || ''}
            onChange={e => set('sas_key_name', e.target.value)} placeholder="RootManageSharedAccessKey"/></div>
        <div className="field"><label className="field-label">SAS key</label>
          <input className="input mono" type="password" value={config.sas_key || ''}
            onChange={e => set('sas_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">Token TTL (seconds)</label>
          <input className="input mono" type="number" min="60" value={config.ttl_seconds || 300}
            onChange={e => set('ttl_seconds', Number(e.target.value) || 300)}/></div>
      </>
    );
  }
  if (kind === 'sms_twilio') {
    return (
      <>
        <div className="field">
          <label className="field-label">Account SID</label>
          <input className="input mono" value={config.account_sid || ''}
            onChange={e => set('account_sid', e.target.value)} placeholder="ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"/>
        </div>
        <div className="field">
          <label className="field-label">Auth token</label>
          <input className="input mono" type="password" value={config.auth_token || ''}
            onChange={e => set('auth_token', e.target.value)} placeholder="32-char token"/>
        </div>
        <div className="field">
          <label className="field-label">From <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· E.164</span></label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="+15551234567"/>
        </div>
        <div className="field">
          <label className="field-label">To <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· E.164, comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="+15559876543, +44..."/>
        </div>
        <div className="field-hint">SMS is metered — Twilio charges per message per recipient. Use Pushover or ntfy for free push.</div>
      </>
    );
  }
  return null;
}

export default function Notifications() {
  const list      = useApi(() => api.notifications.list(), [], { pollMs: 0 });
  const templates = useApi(() => api.templates.list(),      [], { pollMs: 0 });

  const [tab,     setTab]     = useState('channels');  // 'channels' | 'templates'
  const [showAdd, setShowAdd] = useState(false);
  const [editId,  setEditId]  = useState(null);        // null = add mode; id = editing
  const [kind,    setKind]    = useState('slack');
  const [name,    setName]    = useState('');
  const [config,  setConfig]  = useState({});
  const [templateId, setTemplateId] = useState('');
  const [cooldown,   setCooldown]   = useState(0);
  const [busy,    setBusy]    = useState(false);
  const [msg,     setMsg]     = useState(null);

  const reload = async () => {
    // useApi doesn't expose a refetch; bounce the hash to nothing visible
    // and back. Simpler: just reload the page once after add/delete.
    window.location.reload();
  };

  const resetForm = () => {
    setEditId(null); setKind('slack'); setName(''); setConfig({});
    setTemplateId(''); setCooldown(0); setMsg(null);
  };

  // Prefill the form from an existing channel and switch to edit mode.
  // Kind is locked while editing — config shape is kind-specific, so
  // changing it would orphan the existing config blob.
  const startEdit = (c) => {
    setEditId(c.id);
    setKind(c.kind);
    setName(c.name);
    setConfig(c.config || {});
    setTemplateId(c.template_id || '');
    setCooldown(c.cooldown_seconds || 0);
    setMsg(null);
    setShowAdd(true);
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const submit = async (e) => {
    e?.preventDefault?.();
    setMsg(null);
    if (!name.trim()) { setMsg({ kind: 'err', text: 'Name is required.' }); return; }
    setBusy(true);
    try {
      if (editId) {
        await api.notifications.update(editId, {
          name: name.trim(),
          config,
          template_id: templateId || null,
          cooldown_seconds: Number(cooldown) || 0,
        });
        setMsg({ kind: 'ok', text: 'Channel updated. Reloading…' });
      } else {
        await api.notifications.create(kind, name.trim(), config, templateId || null, cooldown);
        setMsg({ kind: 'ok', text: 'Channel added. Reloading…' });
      }
      setTimeout(reload, 400);
    } catch (e2) {
      setMsg({ kind: 'err', text: e2.message || 'Failed to save channel.' });
      setBusy(false);
    }
  };

  const removeOne = async (id) => {
    if (!confirm('Delete this channel? Monitors using it will stop notifying via this channel.')) return;
    try {
      await api.notifications.remove(id);
      reload();
    } catch (e) { alert(e.message); }
  };

  const sendTest = async (id) => {
    try {
      await api.notifications.test(id);
      alert('Test message sent. Check the channel.');
    } catch (e) { alert(`Failed: ${e.message}`); }
  };

  const channels = list.data || [];

  return (
    <div className="rampart">
      <style>{css}</style>

      <header style={{
        background: 'var(--surface)', borderBottom: '1px solid var(--border)',
        padding: '14px 24px', display: 'flex', alignItems: 'center', gap: 14,
        position: 'sticky', top: 0, zIndex: 10,
      }}>
        <a href="#/" style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text-3)', textDecoration: 'none', fontSize: 13 }}>
          <ChevronLeft size={14}/> Dashboard
        </a>
        <span style={{ color: 'var(--text-3)' }}>/</span>
        <span style={{ fontSize: 14, fontWeight: 500, display: 'flex', alignItems: 'center', gap: 8 }}>
          <Bell size={15}/> Notifications
        </span>
        {tab === 'channels' && (
          <button className="btn btn-accent" style={{ marginLeft: 'auto' }}
            onClick={() => { if (showAdd) { setShowAdd(false); resetForm(); } else { resetForm(); setShowAdd(true); } }}>
            <Plus size={13}/> {showAdd ? 'Hide form' : 'Add channel'}
          </button>
        )}
      </header>

      <main style={{ padding: '28px 32px', maxWidth: 1000, margin: '0 auto' }}>
        <h1 style={{ fontSize: 22, fontWeight: 600, margin: '0 0 8px', letterSpacing: '-.02em' }}>
          Notifications
        </h1>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 18px' }}>
          Channels are <em>where</em> alerts go; templates are <em>what</em> they say. Attach a template to a channel to customise its subject + body.
        </p>

        <div className="tabs">
          <button className={tab === 'channels'  ? 'active' : ''} onClick={() => setTab('channels')}>
            <Bell size={12}/> Channels {(list.data || []).length > 0 && <span className="mono" style={{ opacity: .7 }}>{(list.data || []).length}</span>}
          </button>
          <button className={tab === 'templates' ? 'active' : ''} onClick={() => setTab('templates')}>
            <FileText size={12}/> Templates {(templates.data || []).length > 0 && <span className="mono" style={{ opacity: .7 }}>{(templates.data || []).length}</span>}
          </button>
        </div>

        {tab === 'templates' && <TemplatesPanel state={templates} reload={reload}/>}

        {tab === 'channels' && (<>

        {/* Add form */}
        {showAdd && (
          <div className="card" style={{ padding: 20, marginBottom: 20 }}>
            <h3 style={{ fontSize: 14, fontWeight: 600, margin: '0 0 14px' }}>
              {editId ? `Edit channel · ${name}` : 'Add a new channel'}
            </h3>
            {editId && (
              <div className="field-hint" style={{ marginBottom: 12, color: 'var(--text-3)' }}>
                Channel type can't be changed — config fields are type-specific. Delete + re-add to switch type.
              </div>
            )}
            {!editId && (
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 8, marginBottom: 14 }}>
              {SUPPORTED.map(s => {
                const Icon = s.icon;
                return (
                  <div key={s.id} className={`kind-card ${kind === s.id ? 'active' : ''}`} onClick={() => { setKind(s.id); setConfig({}); }}>
                    <Icon size={16} color={kind === s.id ? 'var(--accent-2)' : 'var(--text-2)'}/>
                    <div>
                      <div style={{ fontSize: 13, fontWeight: 500 }}>{s.name}</div>
                      <div style={{ fontSize: 11, color: 'var(--text-3)', lineHeight: 1.3, marginTop: 2 }}>{s.hint}</div>
                    </div>
                  </div>
                );
              })}
            </div>
            )}

            <form onSubmit={submit}>
              <div className="field">
                <label className="field-label">Display name</label>
                <input className="input" value={name} onChange={e => setName(e.target.value)}
                  placeholder={kind === 'slack' ? '#alerts (production)' : kind === 'discord' ? 'discord-monitoring' : 'pagerduty webhook'}/>
              </div>
              <ConfigForm kind={kind} config={config} setConfig={setConfig}/>

              <div className="field">
                <label className="field-label">Template <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
                <select className="select" value={templateId} onChange={e => setTemplateId(e.target.value)}>
                  <option value="">— Use default subject/body —</option>
                  {(templates.data || []).map(t => (
                    <option key={t.id} value={t.id}>{t.name} ({t.event_kind})</option>
                  ))}
                </select>
                <div className="field-hint">Manage templates on the <strong>Templates</strong> tab. Leave on default for the built-in subject + body.</div>
              </div>

              <div className="field">
                <label className="field-label">Cooldown <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· seconds · optional</span></label>
                <input className="input" type="number" min="0" step="1" value={cooldown}
                  onChange={e => setCooldown(e.target.value)} placeholder="0 (no cooldown)"/>
                <div className="field-hint">Suppress repeated fires within this window. Useful for flap-prone monitors paired with SMS or paging. <code>0</code> disables.</div>
              </div>

              {msg && <div className={msg.kind === 'ok' ? 'banner-ok' : 'banner-err'} style={{ marginBottom: 12 }}>{msg.text}</div>}

              <div style={{ display: 'flex', gap: 8 }}>
                <button className="btn btn-accent" type="submit" disabled={busy}>
                  {busy ? <><Loader2 size={13} className="spin"/> Saving…</>
                        : editId ? <><Save size={13}/> Update channel</>
                                 : <><Plus size={13}/> Save channel</>}
                </button>
                <button className="btn" type="button" onClick={() => { setShowAdd(false); resetForm(); }}>Cancel</button>
              </div>
            </form>
          </div>
        )}

        {/* Channel list */}
        <div className="card" style={{ padding: 0 }}>
          {list.loading && <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>Loading channels…</div>}
          {!list.loading && channels.length === 0 && (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
              <Bell size={20} style={{ opacity: .4, marginBottom: 8 }}/>
              <div>No notification channels configured yet.</div>
              <div style={{ marginTop: 6 }}>Click <strong>Add channel</strong> above to set up your first one.</div>
            </div>
          )}
          {channels.map(c => {
            const meta = SUPPORTED.find(s => s.id === c.kind);
            const Icon = meta ? meta.icon : AlertCircle;
            const tpl = c.template_id && (templates.data || []).find(t => t.id === c.template_id);
            return (
              <div key={c.id} className="channel-row">
                <Icon size={16} color="var(--text-2)"/>
                <div>
                  <div style={{ fontSize: 13.5, fontWeight: 500, display: 'flex', alignItems: 'center', gap: 8 }}>
                    {c.name}
                    {tpl && <span className="template-pill"><FileText size={9}/> {tpl.name}</span>}
                  </div>
                  <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 2, textTransform: 'uppercase', letterSpacing: '.04em' }}>
                    {meta ? meta.name : c.kind} · {c.active ? 'enabled' : 'disabled'}
                  </div>
                </div>
                {c.kind === 'webpush' && <EnablePushButton notificationId={c.id}/>}
                <button className="btn" onClick={() => startEdit(c)} title="Edit this channel">
                  <Edit3 size={12}/> Edit
                </button>
                <button className="btn" onClick={() => sendTest(c.id)} title="Send a test message">
                  <Send size={12}/> Test
                </button>
                <button className="btn btn-danger" onClick={() => removeOne(c.id)} title="Delete">
                  <Trash2 size={12}/>
                </button>
              </div>
            );
          })}
        </div>

        {list.error && <div className="banner-err" style={{ marginTop: 16 }}>{list.error.message}</div>}

        </>)}

        <div style={{ height: 40 }}/>
      </main>

      <style>{`@keyframes spin { to { transform: rotate(360deg); } } .spin { animation: spin 1s linear infinite; }`}</style>
    </div>
  );
}

// ── Templates panel ───────────────────────────────────────────────────────
// CRUD for notification_templates. v1 is intentionally minimal: list, add,
// edit-in-place, delete. The template body uses {{placeholder}} syntax —
// the supported variables list is shown inline so users don't have to dig.

const PLACEHOLDERS = [
  '{{monitor.name}}', '{{monitor.url}}', '{{monitor.kind}}', '{{monitor.id}}',
  '{{status}}',       '{{prev_status}}',  '{{latency_ms}}',  '{{status_code}}',
  '{{msg}}',          '{{retries}}',      '{{ts}}',
];

const EVENT_KINDS = [
  { id: 'monitor_down',      label: 'Monitor went down' },
  { id: 'monitor_up',        label: 'Monitor recovered' },
  { id: 'monitor_warn',      label: 'Monitor degraded' },
  { id: 'maintenance_start', label: 'Maintenance started' },
  { id: 'maintenance_end',   label: 'Maintenance ended' },
  { id: 'incident_created',  label: 'Incident created' },
  { id: 'incident_updated',  label: 'Incident updated' },
  { id: 'incident_resolved', label: 'Incident resolved' },
];

function TemplatesPanel({ state, reload }) {
  const [showAdd, setShowAdd] = useState(false);
  const [editing, setEditing] = useState(null);  // template object being edited, or null

  return (
    <>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
          Reusable subject + body strings. Attach to channels to customise what they send.
        </p>
        {!showAdd && !editing && (
          <button className="btn btn-accent" onClick={() => setShowAdd(true)}>
            <Plus size={13}/> New template
          </button>
        )}
      </div>

      {(showAdd || editing) && (
        <TemplateForm
          initial={editing}
          onCancel={() => { setShowAdd(false); setEditing(null); }}
          onSaved={() => { setShowAdd(false); setEditing(null); reload(); }}
        />
      )}

      <div className="card" style={{ padding: 0, marginTop: 16 }}>
        {state.loading && <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>Loading templates…</div>}
        {!state.loading && (state.data || []).length === 0 && !showAdd && (
          <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
            <FileText size={20} style={{ opacity: .4, marginBottom: 8 }}/>
            <div>No templates yet.</div>
            <div style={{ marginTop: 6 }}>Channels will use built-in defaults until you add one.</div>
          </div>
        )}
        {(state.data || []).map(t => (
          <div key={t.id} className="template-row">
            <FileText size={16} color="var(--text-2)"/>
            <div>
              <div style={{ fontSize: 13.5, fontWeight: 500 }}>{t.name}</div>
              <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 2 }}>
                <span style={{ textTransform: 'uppercase', letterSpacing: '.04em' }}>{t.event_kind.replace(/_/g, ' ')}</span>
                {t.channel_kinds.length > 0 && <> · for: {t.channel_kinds.join(', ')}</>}
              </div>
            </div>
            <span style={{ fontSize: 11, color: 'var(--text-3)' }}>
              {t.body_template.length} chars
            </span>
            <button className="btn" onClick={() => setEditing(t)} title="Edit">
              <Edit3 size={12}/>
            </button>
            <button className="btn btn-danger" onClick={async () => {
              if (!confirm(`Delete template "${t.name}"?`)) return;
              try { await api.templates.remove(t.id); reload(); }
              catch (e) { alert(e.message); }
            }} title="Delete">
              <Trash2 size={12}/>
            </button>
          </div>
        ))}
      </div>
    </>
  );
}

function TemplateForm({ initial, onCancel, onSaved }) {
  const [name,       setName]       = useState(initial?.name || '');
  const [eventKind,  setEventKind]  = useState(initial?.event_kind || 'monitor_down');
  const [subject,    setSubject]    = useState(initial?.subject_template || '[{{status}}] {{monitor.name}}');
  const [body,       setBody]       = useState(initial?.body_template ||
    '{{monitor.name}} is now {{status}} (was {{prev_status}}).\n\nLatency: {{latency_ms}}ms\nCode: {{status_code}}\nMessage: {{msg}}\nTime: {{ts}}');
  const [busy, setBusy] = useState(false);
  const [err,  setErr]  = useState(null);

  const insertPlaceholder = (ph, target) => {
    if (target === 'subject') setSubject(s => s + ph);
    else                      setBody(b => b + ph);
  };

  const save = async (e) => {
    e?.preventDefault?.();
    setErr(null);
    if (!name.trim()) { setErr('Name is required.'); return; }
    if (!body.trim()) { setErr('Body is required.'); return; }
    setBusy(true);
    try {
      const payload = {
        name:             name.trim(),
        event_kind:       eventKind,
        subject_template: subject || null,
        body_template:    body,
        channel_kinds:    [],
      };
      if (initial) await api.templates.update(initial.id, payload);
      else         await api.templates.create(payload);
      onSaved();
    } catch (e2) {
      setErr(e2.message || 'Failed to save template.');
      setBusy(false);
    }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 12 }}>
      <h3 style={{ fontSize: 14, fontWeight: 600, margin: '0 0 14px' }}>
        {initial ? `Edit template: ${initial.name}` : 'New template'}
      </h3>
      <form onSubmit={save}>
        <div style={{ display: 'grid', gridTemplateColumns: '2fr 1fr', gap: 12 }}>
          <div className="field">
            <label className="field-label">Name</label>
            <input className="input" value={name} onChange={e => setName(e.target.value)} placeholder="Concise outage" autoFocus/>
          </div>
          <div className="field">
            <label className="field-label">Event kind</label>
            <select className="select" value={eventKind} onChange={e => setEventKind(e.target.value)}>
              {EVENT_KINDS.map(k => <option key={k.id} value={k.id}>{k.label}</option>)}
            </select>
          </div>
        </div>

        <div className="field">
          <label className="field-label">Subject template <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
          <input className="input mono" value={subject} onChange={e => setSubject(e.target.value)} placeholder="[{{status}}] {{monitor.name}}"/>
        </div>

        <div className="field">
          <label className="field-label">Body template</label>
          <textarea className="input mono" rows={7} value={body} onChange={e => setBody(e.target.value)}
            style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 12.5, padding: '10px 12px', lineHeight: 1.5 }}/>
        </div>

        <div className="field">
          <label className="field-label">Available placeholders <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· click to insert into body</span></label>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
            {PLACEHOLDERS.map(ph => (
              <button key={ph} type="button" className="code" onClick={() => insertPlaceholder(ph, 'body')}
                style={{ border: '1px solid var(--border)', cursor: 'pointer' }}>
                {ph}
              </button>
            ))}
          </div>
        </div>

        {err && <div className="banner-err" style={{ marginBottom: 12 }}>{err}</div>}

        <div style={{ display: 'flex', gap: 8 }}>
          <button className="btn btn-accent" type="submit" disabled={busy}>
            {busy ? <><Loader2 size={13} className="spin"/> Saving…</> : <><Save size={13}/> {initial ? 'Update template' : 'Save template'}</>}
          </button>
          <button className="btn" type="button" onClick={onCancel}><X size={13}/> Cancel</button>
        </div>
      </form>
    </div>
  );
}

// Per-device Web Push opt-in. Registers the service worker, fetches the
// shared VAPID key, subscribes via the Push API, and posts the resulting
// subscription to the backend keyed to this webpush channel.
function EnablePushButton({ notificationId }) {
  const [state, setState] = useState('idle'); // idle | working | done | unsupported | error
  const [msg, setMsg] = useState(null);

  const supported = typeof window !== 'undefined'
    && 'serviceWorker' in navigator
    && 'PushManager' in window;

  const enable = async () => {
    setMsg(null);
    if (!supported) { setState('unsupported'); return; }
    setState('working');
    try {
      const perm = await Notification.requestPermission();
      if (perm !== 'granted') { setState('error'); setMsg('Permission denied'); return; }

      const reg = await navigator.serviceWorker.register('/sw.js');
      await navigator.serviceWorker.ready;

      const { public_key } = await api.webpush.vapidKey();
      const sub = await reg.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: urlBase64ToUint8Array(public_key),
      });

      // PushSubscription.toJSON() gives endpoint + keys{p256dh,auth}.
      const json = sub.toJSON();
      await api.webpush.subscribe(notificationId, { endpoint: json.endpoint, keys: json.keys });
      setState('done');
    } catch (e) {
      setState('error');
      setMsg(e.message || 'Subscribe failed');
    }
  };

  if (state === 'done') {
    return <span className="btn" style={{ cursor: 'default', color: 'var(--up)' }}><BellRing size={12}/> Subscribed</span>;
  }
  return (
    <button className="btn" onClick={enable} disabled={state === 'working'} title="Subscribe this browser to push alerts">
      {state === 'working'
        ? <><Loader2 size={12} className="spin"/> …</>
        : <><BellRing size={12}/> Enable push</>}
      {state === 'unsupported' && <span style={{ marginLeft: 6, fontSize: 11, color: 'var(--down)' }}>not supported</span>}
      {state === 'error' && msg && <span style={{ marginLeft: 6, fontSize: 11, color: 'var(--down)' }}>{msg}</span>}
    </button>
  );
}

// VAPID public key (base64url) → Uint8Array for applicationServerKey.
function urlBase64ToUint8Array(base64String) {
  const padding = '='.repeat((4 - (base64String.length % 4)) % 4);
  const base64 = (base64String + padding).replace(/-/g, '+').replace(/_/g, '/');
  const raw = atob(base64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}
