import React, { useState, useRef, useEffect } from 'react';
import {
  Bell, Plus, Trash2, Send, ChevronLeft, MessageSquare, Hash, Mail,
  Webhook as WebhookIcon, AlertCircle, Loader2, Smartphone, Server, Megaphone,
  Siren, Phone, Rocket, Layers, FileText, Edit3, Save, X, BellRing, Search,
  ChevronDown, Check, Copy,
} from 'lucide-react';
import { api, useApi } from '../lib/api.js';
import { canWrite } from '../lib/roles.js';
import { t } from '../lib/i18n.js';
import { confirmDialog, toast } from '../lib/notify.js';

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
    display: grid; grid-template-columns: 38px 1fr auto;
    align-items: center; gap: 14px;
    padding: 14px 20px; border-top: 1px solid var(--border);
    transition: background .12s;
  }
  .channel-row:first-child { border-top: none; }
  .channel-row:hover { background: var(--surface-2); }

  /* Channel icon tile — soft background, accent text, matches the
     visual language of the kind-card chips in the form. Lets users
     scan the channel list by colour quickly. */
  .ch-icon-tile {
    width: 38px; height: 38px; border-radius: 9px;
    display: flex; align-items: center; justify-content: center;
    background: var(--accent-soft); color: var(--accent-2);
    flex-shrink: 0;
  }

  /* Channel-type / status caption — pills instead of an uppercase
     ALL-CAPS string. Reads at a glance against the channel name. */
  .ch-meta {
    display: flex; align-items: center; gap: 6px; margin-top: 4px;
    font-size: 11.5px; color: var(--text-3);
  }
  .ch-type-pill {
    background: var(--surface-2); color: var(--text-2);
    padding: 2px 8px; border-radius: 999px;
    font-weight: 500; font-size: 11px;
  }
  .ch-status-on {
    color: var(--up); font-weight: 500;
  }
  .ch-status-off {
    color: var(--text-3); font-weight: 500;
  }
  .ch-status-on::before, .ch-status-off::before {
    content: ''; display: inline-block; width: 6px; height: 6px;
    border-radius: 50%; margin-right: 5px; vertical-align: 1px;
  }
  .ch-status-on::before  { background: var(--up); box-shadow: 0 0 0 2px var(--up-soft); }
  .ch-status-off::before { background: var(--text-3); }

  /* Add/edit channel form panel — constrained width so the form
     doesn't stretch across the full page. Sectioned underline
     header gives it weight. */
  .form-panel {
    max-width: 720px;
    margin: 0 auto 22px;
    padding: 22px 26px 26px;
  }
  .form-panel h3 {
    margin: 0 0 4px;
    font-size: 16px; font-weight: 600; letter-spacing: -.01em;
  }
  .form-panel .form-eyebrow {
    font-size: 11px; font-weight: 600; color: var(--accent-2);
    text-transform: uppercase; letter-spacing: .06em;
    margin-bottom: 4px;
  }
  .form-panel .form-divider {
    height: 1px; background: var(--border);
    margin: 18px 0 16px;
  }
  .field-hint {
    font-size: 11.5px; color: var(--text-3); margin-top: 4px; line-height: 1.45;
  }

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
    display: grid; grid-template-columns: 30px 1fr auto auto auto auto;
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
  { id: 'slack',       name: 'Slack',           icon: MessageSquare, hint: t('notif.kind.slack.hint') },
  { id: 'discord',     name: 'Discord',         icon: Hash,          hint: t('notif.kind.discord.hint') },
  { id: 'teams',       name: 'MS Teams',        icon: MessageSquare, hint: t('notif.kind.teams.hint') },
  { id: 'mattermost',  name: 'Mattermost',      icon: MessageSquare, hint: t('notif.kind.mattermost.hint') },
  { id: 'rocket_chat', name: 'Rocket.Chat',     icon: Rocket,        hint: t('notif.kind.rocket_chat.hint') },
  { id: 'telegram',    name: 'Telegram',        icon: Megaphone,     hint: t('notif.kind.telegram.hint') },
  { id: 'matrix',      name: 'Matrix',          icon: Hash,          hint: t('notif.kind.matrix.hint') },
  { id: 'google_chat', name: 'Google Chat',     icon: MessageSquare, hint: t('notif.kind.google_chat.hint') },
  { id: 'wecom',       name: 'WeCom 企业微信', icon: MessageSquare, hint: t('notif.kind.wecom.hint') },
  { id: 'dingtalk',    name: 'DingTalk 钉钉', icon: MessageSquare, hint: t('notif.kind.dingtalk.hint') },
  { id: 'feishu',      name: 'Feishu 飞书',   icon: MessageSquare, hint: t('notif.kind.feishu.hint') },
  { id: 'line',        name: 'LINE Messenger',  icon: MessageSquare, hint: t('notif.kind.line.hint') },
  // push
  { id: 'bark',        name: 'Bark (iOS)',      icon: Smartphone,    hint: t('notif.kind.bark.hint') },
  { id: 'pushbullet',  name: 'Pushbullet',      icon: Smartphone,    hint: t('notif.kind.pushbullet.hint') },
  // email APIs
  { id: 'sendgrid',    name: 'SendGrid',        icon: Mail,          hint: t('notif.kind.sendgrid.hint') },
  { id: 'resend',      name: 'Resend',          icon: Mail,          hint: t('notif.kind.resend.hint') },
  { id: 'brevo',       name: 'Brevo',           icon: Mail,          hint: t('notif.kind.brevo.hint') },
  // incident management
  { id: 'opsgenie',    name: 'Opsgenie',        icon: Siren,         hint: t('notif.kind.opsgenie.hint') },
  { id: 'pagertree',   name: 'PagerTree',       icon: Siren,         hint: t('notif.kind.pagertree.hint') },
  { id: 'squadcast',   name: 'Squadcast',       icon: Siren,         hint: t('notif.kind.squadcast.hint') },
  { id: 'signal',      name: 'Signal',          icon: MessageSquare, hint: t('notif.kind.signal.hint') },
  { id: 'zulip',       name: 'Zulip',           icon: MessageSquare, hint: t('notif.kind.zulip.hint') },
  { id: 'lark',        name: 'Lark',            icon: MessageSquare, hint: t('notif.kind.lark.hint') },
  { id: 'goalert',     name: 'GoAlert',         icon: Siren,         hint: t('notif.kind.goalert.hint') },
  { id: 'alerta',      name: 'Alerta',          icon: Siren,         hint: t('notif.kind.alerta.hint') },
  { id: 'alertnow',    name: 'AlertNow',        icon: Siren,         hint: t('notif.kind.alertnow.hint') },
  { id: 'signl4',      name: 'SIGNL4',          icon: Siren,         hint: t('notif.kind.signl4.hint') },
  { id: 'heii_oncall', name: 'Heii On-Call',    icon: Siren,         hint: t('notif.kind.heii_oncall.hint') },
  { id: 'serverchan',  name: 'ServerChan',      icon: Smartphone,    hint: t('notif.kind.serverchan.hint') },
  { id: 'pushplus',    name: 'PushPlus',        icon: Smartphone,    hint: t('notif.kind.pushplus.hint') },
  { id: 'pushdeer',    name: 'PushDeer',        icon: Smartphone,    hint: t('notif.kind.pushdeer.hint') },
  { id: 'aliyun_sms',  name: 'Aliyun SMS',      icon: Phone,         hint: t('notif.kind.aliyun_sms.hint') },
  { id: 'mastodon',    name: 'Mastodon',        icon: MessageSquare, hint: t('notif.kind.mastodon.hint') },
  { id: 'pumble',      name: 'Pumble',          icon: MessageSquare, hint: t('notif.kind.pumble.hint') },
  { id: 'bitrix24',    name: 'Bitrix24',        icon: MessageSquare, hint: t('notif.kind.bitrix24.hint') },
  { id: 'stackfield',  name: 'Stackfield',      icon: MessageSquare, hint: t('notif.kind.stackfield.hint') },
  { id: 'splunk',        name: 'Splunk On-Call', icon: Siren,         hint: t('notif.kind.splunk.hint') },
  { id: 'grafana_oncall',name: 'Grafana OnCall', icon: Siren,         hint: t('notif.kind.grafana_oncall.hint') },
  { id: 'home_assistant',name: 'Home Assistant', icon: Server,        hint: t('notif.kind.home_assistant.hint') },
  { id: 'clicksend',     name: 'ClickSend SMS',  icon: Phone,         hint: t('notif.kind.clicksend.hint') },
  { id: 'sms_46elks',    name: '46elks SMS',     icon: Phone,         hint: t('notif.kind.sms_46elks.hint') },
  { id: 'callmebot',     name: 'CallMeBot',      icon: Phone,         hint: t('notif.kind.callmebot.hint') },
  { id: 'telnyx',        name: 'Telnyx SMS',     icon: Phone,         hint: t('notif.kind.telnyx.hint') },
  { id: 'notifery',      name: 'Notifery',       icon: Smartphone,    hint: t('notif.kind.notifery.hint') },
  { id: 'whatsapp_waha', name: 'WhatsApp (WAHA)',icon: MessageSquare, hint: t('notif.kind.whatsapp_waha.hint') },
  { id: 'threema',       name: 'Threema',        icon: MessageSquare, hint: t('notif.kind.threema.hint') },
  { id: 'bale',          name: 'Bale',           icon: MessageSquare, hint: t('notif.kind.bale.hint') },
  { id: 'pushy',         name: 'Pushy',          icon: Smartphone,    hint: t('notif.kind.pushy.hint') },
  { id: 'zoho_cliq',     name: 'Zoho Cliq',      icon: MessageSquare, hint: t('notif.kind.zoho_cliq.hint') },
  { id: 'sms_manager',   name: 'SmsManager.cz',  icon: Phone,         hint: t('notif.kind.sms_manager.hint') },
  { id: 'sms_eagle',     name: 'SMSEagle',       icon: Phone,         hint: t('notif.kind.sms_eagle.hint') },
  { id: 'octopush',      name: 'Octopush',       icon: Phone,         hint: t('notif.kind.octopush.hint') },
  { id: 'whatsapp_whapi',     name: 'WhatsApp (whapi)',     icon: MessageSquare, hint: t('notif.kind.whatsapp_whapi.hint') },
  { id: 'whatsapp_360',       name: 'WhatsApp (360messenger)', icon: MessageSquare, hint: t('notif.kind.whatsapp_360.hint') },
  { id: 'whatsapp_evolution', name: 'WhatsApp (Evolution)', icon: MessageSquare, hint: t('notif.kind.whatsapp_evolution.hint') },
  { id: 'flock',         name: 'Flock',          icon: MessageSquare, hint: t('notif.kind.flock.hint') },
  { id: 'serwersms',     name: 'SerwerSMS.pl',   icon: Phone,         hint: t('notif.kind.serwersms.hint') },
  { id: 'smsplanet',     name: 'SMSPlanet.pl',   icon: Phone,         hint: t('notif.kind.smsplanet.hint') },
  { id: 'smsc',          name: 'SMSC.ru',        icon: Phone,         hint: t('notif.kind.smsc.hint') },
  { id: 'cellsynt',      name: 'Cellsynt',       icon: Phone,         hint: t('notif.kind.cellsynt.hint') },
  { id: 'sevenio',       name: 'seven.io',       icon: Phone,         hint: t('notif.kind.sevenio.hint') },
  { id: 'gtxmessaging',  name: 'GtxMessaging',   icon: Phone,         hint: t('notif.kind.gtxmessaging.hint') },
  { id: 'onesender',     name: 'Onesender',      icon: MessageSquare, hint: t('notif.kind.onesender.hint') },
  { id: 'promosms',      name: 'PromoSMS.pl',    icon: Phone,         hint: t('notif.kind.promosms.hint') },
  { id: 'smspartner',    name: 'SMSPartner.fr',  icon: Phone,         hint: t('notif.kind.smspartner.hint') },
  { id: 'sms_ir',        name: 'SMS.ir',         icon: Phone,         hint: t('notif.kind.sms_ir.hint') },
  { id: 'freemobile',    name: 'Free Mobile (FR)', icon: Phone,       hint: t('notif.kind.freemobile.hint') },
  { id: 'flashduty',     name: 'FlashDuty',      icon: Siren,         hint: t('notif.kind.flashduty.hint') },
  { id: 'teltonika',     name: 'Teltonika SMS',  icon: Phone,         hint: t('notif.kind.teltonika.hint') },
  { id: 'kook',          name: 'Kook',           icon: MessageSquare, hint: t('notif.kind.kook.hint') },
  { id: 'nostr',         name: 'Nostr',          icon: MessageSquare, hint: t('notif.kind.nostr.hint') },
  { id: 'onebot',        name: 'OneBot',         icon: MessageSquare, hint: t('notif.kind.onebot.hint') },
  { id: 'onechat',       name: 'OneChat (TH)',   icon: MessageSquare, hint: t('notif.kind.onechat.hint') },
  { id: 'max_messenger', name: 'MAX Messenger',  icon: MessageSquare, hint: t('notif.kind.max_messenger.hint') },
  { id: 'halo_psa',      name: 'Halo PSA',       icon: Siren,         hint: t('notif.kind.halo_psa.hint') },
  { id: 'jira_sm',       name: 'Jira Service Mgmt', icon: Siren,      hint: t('notif.kind.jira_sm.hint') },
  { id: 'spug_push',     name: 'SpugPush',       icon: Smartphone,    hint: t('notif.kind.spug_push.hint') },
  { id: 'wpush',         name: 'WPush.cn',       icon: Smartphone,    hint: t('notif.kind.wpush.hint') },
  { id: 'vk',            name: 'VK',             icon: MessageSquare, hint: t('notif.kind.vk.hint') },
  { id: 'yzj',           name: 'YZJ (云之家)',   icon: MessageSquare, hint: t('notif.kind.yzj.hint') },
  { id: 'google_sheets', name: 'Google Sheets',  icon: FileText,      hint: t('notif.kind.google_sheets.hint') },
  { id: 'gorush',        name: 'Gorush',         icon: Smartphone,    hint: t('notif.kind.gorush.hint') },
  { id: 'fluxer',        name: 'Fluxer',         icon: MessageSquare, hint: t('notif.kind.fluxer.hint') },
  { id: 'splash',        name: 'Splash',         icon: Siren,         hint: t('notif.kind.splash.hint') },
  { id: 'messagebird',   name: 'MessageBird',    icon: Phone,         hint: t('notif.kind.messagebird.hint') },
  { id: 'plivo',         name: 'Plivo SMS',      icon: Phone,         hint: t('notif.kind.plivo.hint') },
  { id: 'vonage',        name: 'Vonage (Nexmo)', icon: Phone,         hint: t('notif.kind.vonage.hint') },
  { id: 'bandwidth',     name: 'Bandwidth',      icon: Phone,         hint: t('notif.kind.bandwidth.hint') },
  { id: 'webex',         name: 'Cisco Webex',    icon: MessageSquare, hint: t('notif.kind.webex.hint') },
  { id: 'pushcut',       name: 'Pushcut',        icon: Smartphone,    hint: t('notif.kind.pushcut.hint') },
  { id: 'smsglobal',     name: 'SMSGlobal',      icon: Phone,         hint: t('notif.kind.smsglobal.hint') },
  { id: 'alertops',      name: 'AlertOps',       icon: Siren,         hint: t('notif.kind.alertops.hint') },
  { id: 'mailgun',       name: 'Mailgun',        icon: Mail,          hint: t('notif.kind.mailgun.hint') },
  { id: 'mailjet',       name: 'Mailjet',        icon: Mail,          hint: t('notif.kind.mailjet.hint') },
  { id: 'postmark',      name: 'Postmark',       icon: Mail,          hint: t('notif.kind.postmark.hint') },
  { id: 'mandrill',      name: 'Mandrill',       icon: Mail,          hint: t('notif.kind.mandrill.hint') },
  { id: 'sparkpost',     name: 'SparkPost',      icon: Mail,          hint: t('notif.kind.sparkpost.hint') },
  { id: 'spike_sh',      name: 'Spike.sh',       icon: Siren,         hint: t('notif.kind.spike_sh.hint') },
  { id: 'zenduty',       name: 'Zenduty',        icon: Siren,         hint: t('notif.kind.zenduty.hint') },
  { id: 'ringcentral',   name: 'RingCentral',    icon: MessageSquare, hint: t('notif.kind.ringcentral.hint') },
  { id: 'ilert',         name: 'iLert',          icon: Siren,         hint: t('notif.kind.ilert.hint') },
  { id: 'linear',        name: 'Linear',         icon: FileText,      hint: t('notif.kind.linear.hint') },
  { id: 'clickup',       name: 'ClickUp',        icon: FileText,      hint: t('notif.kind.clickup.hint') },
  { id: 'trello',        name: 'Trello',         icon: FileText,      hint: t('notif.kind.trello.hint') },
  { id: 'github_issue',  name: 'GitHub Issue',   icon: FileText,      hint: t('notif.kind.github_issue.hint') },
  { id: 'gitlab_issue',  name: 'GitLab Issue',   icon: FileText,      hint: t('notif.kind.gitlab_issue.hint') },
  { id: 'asana',         name: 'Asana',          icon: FileText,      hint: t('notif.kind.asana.hint') },
  { id: 'notion',        name: 'Notion',         icon: FileText,      hint: t('notif.kind.notion.hint') },
  { id: 'sentry',          name: 'Sentry',          icon: Siren,    hint: t('notif.kind.sentry.hint') },
  { id: 'rollbar',         name: 'Rollbar',         icon: Siren,    hint: t('notif.kind.rollbar.hint') },
  { id: 'honeybadger',     name: 'Honeybadger',     icon: Siren,    hint: t('notif.kind.honeybadger.hint') },
  { id: 'healthchecks_io', name: 'Healthchecks.io', icon: Smartphone, hint: t('notif.kind.healthchecks_io.hint') },
  { id: 'betterstack',     name: 'BetterStack',     icon: Siren,    hint: t('notif.kind.betterstack.hint') },
  { id: 'statuspage_io',   name: 'Statuspage.io',   icon: Siren,    hint: t('notif.kind.statuspage_io.hint') },
  { id: 'datadog',         name: 'Datadog Events',  icon: Siren,    hint: t('notif.kind.datadog.hint') },
  { id: 'newrelic',        name: 'New Relic Events',icon: Siren,    hint: t('notif.kind.newrelic.hint') },
  { id: 'aws_sns',         name: 'AWS SNS',         icon: Siren,    hint: t('notif.kind.aws_sns.hint') },
  { id: 'azure_servicebus',name: 'Azure Service Bus',icon: Siren,    hint: t('notif.kind.azure_servicebus.hint') },
  { id: 'gcp_pubsub',      name: 'GCP Pub/Sub',     icon: Siren,    hint: t('notif.kind.gcp_pubsub.hint') },
  { id: 'webpush',         name: 'Web Push',        icon: Bell,     hint: t('notif.kind.webpush.hint') },
  // push
  { id: 'ntfy',       name: 'ntfy.sh',         icon: Smartphone,    hint: t('notif.kind.ntfy.hint') },
  { id: 'gotify',     name: 'Gotify',          icon: Server,        hint: t('notif.kind.gotify.hint') },
  { id: 'pushover',   name: 'Pushover',        icon: Smartphone,    hint: t('notif.kind.pushover.hint') },
  // ops
  { id: 'pagerduty',  name: 'PagerDuty',       icon: Siren,         hint: t('notif.kind.pagerduty.hint') },
  // sms / email
  { id: 'sms_twilio', name: 'Twilio SMS',      icon: Phone,         hint: t('notif.kind.sms_twilio.hint') },
  { id: 'email',      name: 'Email (SMTP)',    icon: Mail,          hint: t('notif.kind.email.hint') },
  // gateway
  { id: 'apprise',    name: 'Apprise gateway', icon: Layers,        hint: t('notif.kind.apprise.hint') },
  // catch-all
  { id: 'webhook',    name: 'Generic Webhook', icon: WebhookIcon,   hint: t('notif.kind.webhook.hint') },
];

// Per-kind form fields rendered on the right when a kind is selected.
function ConfigForm({ kind, config, setConfig }) {
  const set = (k, v) => setConfig({ ...config, [k]: v });
  if (kind === 'slack') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.webhook_url')}</label>
          <input className="input mono" value={config.webhook_url || ''}
            onChange={e => set('webhook_url', e.target.value)}
            placeholder="https://hooks.slack.com/services/T.../B.../xxxxx"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.channel_override')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
          <label className="field-label">{t('notif.f.webhook_url')}</label>
          <input className="input mono" value={config.webhook_url || ''}
            onChange={e => set('webhook_url', e.target.value)}
            placeholder="https://discord.com/api/webhooks/.../..."/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.display_name_override')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
          <label className="field-label">{t('notif.f.url')}</label>
          <input className="input mono" value={config.url || ''}
            onChange={e => set('url', e.target.value)}
            placeholder="https://your-service.example.com/hooks/rampart"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.method')}</label>
          <select className="select" value={config.method || 'POST'} onChange={e => set('method', e.target.value)}>
            <option>POST</option><option>PUT</option><option>PATCH</option>
          </select>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.hmac_secret')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
        <label className="field-label">{t('notif.f.incoming_webhook_url')}</label>
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
          <label className="field-label">{t('notif.f.bot_token')}</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}
            placeholder="123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.chat_id')}</label>
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
            <label className="field-label">{t('notif.f.smtp_host')}</label>
            <input className="input mono" value={config.smtp_host || ''}
              onChange={e => set('smtp_host', e.target.value)} placeholder="smtp.gmail.com"/>
          </div>
          <div className="field">
            <label className="field-label">{t('notif.f.port')}</label>
            <input className="input mono" value={config.smtp_port || ''}
              onChange={e => set('smtp_port', parseInt(e.target.value, 10) || '')} placeholder="587"/>
          </div>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.encryption')}</label>
          <select className="select" value={config.encryption || 'starttls'} onChange={e => set('encryption', e.target.value)}>
            <option value="starttls">{t('notif.opt.enc_starttls')}</option>
            <option value="tls">{t('notif.opt.enc_tls')}</option>
            <option value="plain">{t('notif.opt.enc_none')}</option>
          </select>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
          <div className="field">
            <label className="field-label">{t('notif.f.username')}</label>
            <input className="input mono" value={config.smtp_user || ''}
              onChange={e => set('smtp_user', e.target.value)} placeholder="alerts@example.com"/>
          </div>
          <div className="field">
            <label className="field-label">{t('notif.f.password')}</label>
            <input className="input mono" type="password" value={config.smtp_password || ''}
              onChange={e => set('smtp_password', e.target.value)} placeholder="app password"/>
          </div>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.from')}</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder='"Rampart Alerts" &lt;alerts@example.com&gt;'/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
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
          <label className="field-label">{t('notif.f.server')}</label>
          <input className="input mono" value={config.server || 'https://ntfy.sh'}
            onChange={e => set('server', e.target.value)} placeholder="https://ntfy.sh"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.topic')}</label>
          <input className="input mono" value={config.topic || ''}
            onChange={e => set('topic', e.target.value)} placeholder="rampart-myhomelab-x9z"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.priority')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· 1 (min) – 5 (max)</span></label>
          <input className="input mono" type="number" min="1" max="5"
            value={config.priority ?? 3} onChange={e => set('priority', parseInt(e.target.value, 10) || 3)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.auth_header')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional (self-hosted)</span></label>
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
          <label className="field-label">{t('notif.f.server_url')}</label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="https://gotify.example.com"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.application_token')}</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)} placeholder="A.gotify.app.token"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.priority')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· 0–10</span></label>
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
          <label className="field-label">{t('notif.f.integration_routing_key')}</label>
          <input className="input mono" type="password" value={config.routing_key || ''}
            onChange={e => set('routing_key', e.target.value)} placeholder="32-char Events API v2 key"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.component')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
          <label className="field-label">{t('notif.f.api_token')}</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)} placeholder="30-char application token"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.user_key')}</label>
          <input className="input mono" type="password" value={config.user_key || ''}
            onChange={e => set('user_key', e.target.value)} placeholder="30-char user / group key"/>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
          <div className="field">
            <label className="field-label">{t('notif.f.priority')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· -2..2</span></label>
            <input className="input mono" type="number" min="-2" max="2"
              value={config.priority ?? 0} onChange={e => set('priority', parseInt(e.target.value, 10) || 0)}/>
          </div>
          <div className="field">
            <label className="field-label">{t('notif.f.device')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
          <label className="field-label">{t('notif.f.incoming_webhook_url')}</label>
          <input className="input mono" value={config.webhook_url || ''}
            onChange={e => set('webhook_url', e.target.value)}
            placeholder={isRocket ? 'https://chat.example.com/hooks/...' : 'https://mattermost.example.com/hooks/...'}/>
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
          <div className="field">
            <label className="field-label">{t('notif.f.channel_override')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
            <input className="input mono" value={config.channel || ''}
              onChange={e => set('channel', e.target.value)} placeholder="#alerts"/>
          </div>
          <div className="field">
            <label className="field-label">{isRocket ? 'Alias' : 'Username'} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
          <label className="field-label">{t('notif.f.apprise_api_server_url')}</label>
          <input className="input mono" value={config.apprise_url || ''}
            onChange={e => set('apprise_url', e.target.value)}
            placeholder="http://apprise:8000"/>
          <div className="field-hint">{t('notif.hint.run_sidecar')} <code style={{ background: 'var(--surface-2)', padding: '0 4px', borderRadius: 3 }}>docker run -d -p 8000:8000 caronc/apprise:latest</code></div>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.apprise_urls')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
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
          <label className="field-label">{t('notif.f.homeserver')}</label>
          <input className="input mono" value={config.homeserver || ''}
            onChange={e => set('homeserver', e.target.value)} placeholder="https://matrix.org"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.access_token')}</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)} placeholder="syt_..."/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.room_id')}</label>
          <input className="input mono" value={config.room_id || ''}
            onChange={e => set('room_id', e.target.value)} placeholder="!roomid:matrix.org"/>
        </div>
      </>
    );
  }
  if (kind === 'google_chat') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.incoming_webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://chat.googleapis.com/v1/spaces/.../messages?key=..."/>
      </div>
    );
  }
  if (kind === 'wecom') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.bot_key')}</label>
          <input className="input mono" type="password" value={config.bot_key || ''}
            onChange={e => set('bot_key', e.target.value)} placeholder="key from the bot URL"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.mention_mobiles')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated, optional</span></label>
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
          <label className="field-label">{t('notif.f.access_token')}</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)} placeholder="token from bot URL"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.secret')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional, only if signing is on</span></label>
          <input className="input mono" type="password" value={config.secret || ''}
            onChange={e => set('secret', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'feishu') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://open.feishu.cn/open-apis/bot/v2/hook/..."/>
      </div>
    );
  }
  if (kind === 'line') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.channel_access_token')}</label>
          <input className="input mono" type="password" value={config.channel_access_token || ''}
            onChange={e => set('channel_access_token', e.target.value)} placeholder="LINE Developers Console → Messaging API → Channel access token"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· user / group / room id</span></label>
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
          <label className="field-label">{t('notif.f.device_key')}</label>
          <input className="input mono" type="password" value={config.device_key || ''}
            onChange={e => set('device_key', e.target.value)} placeholder="from the Bark app"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.server')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="https://api.day.app"/>
        </div>
      </>
    );
  }
  if (kind === 'pushbullet') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.access_token')}</label>
        <input className="input mono" type="password" value={config.access_token || ''}
          onChange={e => set('access_token', e.target.value)} placeholder="o.xxxxx..."/>
      </div>
    );
  }
  if (kind === 'sendgrid') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)} placeholder="SG.xxxxx"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.from_email')}</label>
          <input className="input" value={config.from_email || ''}
            onChange={e => set('from_email', e.target.value)} placeholder="alerts@example.com"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
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
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)} placeholder="re_xxxxx"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.from')}</label>
          <input className="input" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder='"Rampart" <alerts@example.com>'/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
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
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)} placeholder="xkeysib-..."/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.from_email')}</label>
          <input className="input" value={config.from_email || ''}
            onChange={e => set('from_email', e.target.value)} placeholder="alerts@example.com"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to_email')}</label>
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
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)} placeholder="GenieKey"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.region')}</label>
          <select className="select" value={config.region || 'us'} onChange={e => set('region', e.target.value)}>
            <option value="us">US (api.opsgenie.com)</option>
            <option value="eu">EU (api.eu.opsgenie.com)</option>
          </select>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.priority')}</label>
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
          <label className="field-label">{t('notif.f.integration_url')}</label>
          <input className="input mono" value={config.integration_url || ''}
            onChange={e => set('integration_url', e.target.value)} placeholder="https://api.pagertree.com/integration/..."/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.severity')}</label>
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
        <label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://api.squadcast.com/v2/incidents/..."/>
      </div>
    );
  }
  if (kind === 'signal') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.signal_cli_rest_api_url')}</label>
          <input className="input mono" value={config.api_url || ''}
            onChange={e => set('api_url', e.target.value)} placeholder="http://signal-cli:8080"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.from_number')}</label>
          <input className="input mono" value={config.number || ''}
            onChange={e => set('number', e.target.value)} placeholder="+15551234567"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.recipients')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated phone numbers or group ids</span></label>
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
          <label className="field-label">{t('notif.f.server')}</label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="https://yourzulip.example.com"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.bot_email')}</label>
          <input className="input mono" value={config.bot_email || ''}
            onChange={e => set('bot_email', e.target.value)} placeholder="bot@yourzulip.example.com"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.bot_api_key')}</label>
          <input className="input mono" type="password" value={config.bot_key || ''}
            onChange={e => set('bot_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.type')}</label>
          <select className="select" value={config.kind || 'stream'} onChange={e => set('kind', e.target.value)}>
            <option value="stream">{t('notif.opt.zulip_stream')}</option>
            <option value="private">{t('notif.opt.zulip_private')}</option>
          </select>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· stream name or email(s)</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="alerts / alice@example.com"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.topic')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· streams only</span></label>
          <input className="input mono" value={config.topic || ''}
            onChange={e => set('topic', e.target.value)} placeholder="rampart"/>
        </div>
      </>
    );
  }
  if (kind === 'lark') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://open.larksuite.com/open-apis/bot/v2/hook/..."/>
      </div>
    );
  }
  if (kind === 'goalert') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.integration_url')}</label>
        <input className="input mono" value={config.integration_url || ''}
          onChange={e => set('integration_url', e.target.value)} placeholder="https://goalert.example.com/api/v2/generic/incoming?token=..."/>
      </div>
    );
  }
  if (kind === 'alerta') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.api_url')}</label>
          <input className="input mono" value={config.api_url || ''}
            onChange={e => set('api_url', e.target.value)} placeholder="https://alerta.example.com/api"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.environment')}</label>
          <input className="input" value={config.environment || ''}
            onChange={e => set('environment', e.target.value)} placeholder="Production"/>
        </div>
      </>
    );
  }
  if (kind === 'alertnow') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://api.alertnow.io/..."/>
      </div>
    );
  }
  if (kind === 'signl4') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.team_secret')}</label>
        <input className="input mono" type="password" value={config.team_secret || ''}
          onChange={e => set('team_secret', e.target.value)} placeholder="UUID from the SIGNL4 connect URL"/>
      </div>
    );
  }
  if (kind === 'heii_oncall') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.trigger_url')}</label>
          <input className="input mono" value={config.trigger_url || ''}
            onChange={e => set('trigger_url', e.target.value)} placeholder="https://api.heiioncall.com/..."/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.close_url')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" value={config.close_url || ''}
            onChange={e => set('close_url', e.target.value)} placeholder="https://api.heiioncall.com/.../close"/>
        </div>
      </>
    );
  }
  if (kind === 'serverchan') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.sendkey')}</label>
        <input className="input mono" type="password" value={config.send_key || ''}
          onChange={e => set('send_key', e.target.value)} placeholder="SCT..."/>
      </div>
    );
  }
  if (kind === 'pushplus') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.token')}</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.topic')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional, for group send</span></label>
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
          <label className="field-label">{t('notif.f.push_key')}</label>
          <input className="input mono" type="password" value={config.push_key || ''}
            onChange={e => set('push_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.server')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
          <label className="field-label">{t('notif.f.access_key_id')}</label>
          <input className="input mono" value={config.access_key_id || ''}
            onChange={e => set('access_key_id', e.target.value)} placeholder="LTAI..."/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.access_key_secret')}</label>
          <input className="input mono" type="password" value={config.access_key_secret || ''}
            onChange={e => set('access_key_secret', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.sign_name')}</label>
          <input className="input" value={config.sign_name || ''}
            onChange={e => set('sign_name', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.template_code')}</label>
          <input className="input mono" value={config.template_code || ''}
            onChange={e => set('template_code', e.target.value)} placeholder="SMS_..."/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.phone_numbers')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
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
          <label className="field-label">{t('notif.f.server')}</label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="https://mastodon.social"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.access_token')}</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.visibility')}</label>
          <select className="select" value={config.visibility || 'private'} onChange={e => set('visibility', e.target.value)}>
            <option value="public">{t('notif.opt.vis_public')}</option>
            <option value="unlisted">{t('notif.opt.vis_unlisted')}</option>
            <option value="private">{t('notif.opt.vis_private_followers')}</option>
            <option value="direct">{t('notif.opt.vis_direct')}</option>
          </select>
        </div>
      </>
    );
  }
  if (kind === 'pumble') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://pumble.com/api/incoming-webhooks/..."/>
      </div>
    );
  }
  if (kind === 'bitrix24') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.webhook_url')}</label>
          <input className="input mono" value={config.webhook_url || ''}
            onChange={e => set('webhook_url', e.target.value)} placeholder="https://yourorg.bitrix24.com/rest/<user>/<token>"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.user_id')}</label>
          <input className="input mono" value={config.user_id || ''}
            onChange={e => set('user_id', e.target.value)} placeholder="1"/>
        </div>
      </>
    );
  }
  if (kind === 'stackfield') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.webhook_url')}</label>
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
          <label className="field-label">{t('notif.f.base_url')}</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://homeassistant.local:8123"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.long_lived_access_token')}</label>
          <input className="input mono" type="password" value={config.long_lived_token || ''}
            onChange={e => set('long_lived_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.notify_service')}</label>
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
          <label className="field-label">{t('notif.f.username')}</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.from')}</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="Rampart"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated E.164</span></label>
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
          <label className="field-label">{t('notif.f.api_username')}</label>
          <input className="input mono" value={config.api_username || ''}
            onChange={e => set('api_username', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.api_password')}</label>
          <input className="input mono" type="password" value={config.api_password || ''}
            onChange={e => set('api_password', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.from')}</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="Rampart"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated E.164</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'callmebot') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.endpoint_url')}</label>
        <input className="input mono" value={config.endpoint_url || ''}
          onChange={e => set('endpoint_url', e.target.value)} placeholder="https://api.callmebot.com/whatsapp.php?phone=...&apikey=..."/>
      </div>
    );
  }
  if (kind === 'telnyx') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.from')}</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="+15551234567"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
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
          <label className="field-label">{t('notif.f.api_token')}</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.group')}</label>
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
          <label className="field-label">{t('notif.f.base_url')}</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://waha.local:3000"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.session')}</label>
          <input className="input mono" value={config.session || ''}
            onChange={e => set('session', e.target.value)} placeholder="default"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.chat_id')}</label>
          <input className="input mono" value={config.chat_id || ''}
            onChange={e => set('chat_id', e.target.value)} placeholder="15551234567@c.us"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.api_key')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
          <label className="field-label">{t('notif.f.gateway_id')}</label>
          <input className="input mono" value={config.gateway_id || ''}
            onChange={e => set('gateway_id', e.target.value)} placeholder="*MYGW01"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.secret')}</label>
          <input className="input mono" type="password" value={config.secret || ''}
            onChange={e => set('secret', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· Threema ID, email, or phone</span></label>
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
          <label className="field-label">{t('notif.f.bot_token')}</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.chat_id')}</label>
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
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.device_tokens')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={(config.to || []).join(',')}
            onChange={e => set('to', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}/>
        </div>
      </>
    );
  }
  if (kind === 'zoho_cliq') {
    return (
      <div className="field">
        <label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://cliq.zoho.com/api/v2/channelsbyname/...incoming?zapikey=..."/>
      </div>
    );
  }
  if (kind === 'sms_manager') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.numbers')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.numbers || ''}
            onChange={e => set('numbers', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.quality')}</label>
          <select className="select" value={config.quality || 'economy'} onChange={e => set('quality', e.target.value)}>
            <option>lowcost</option><option>economy</option><option>high</option>
          </select>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.sender_id')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
          <label className="field-label">{t('notif.f.base_url')}</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://smseagle.local"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.access_token')}</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated E.164</span></label>
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
          <label className="field-label">{t('notif.f.api_login')}</label>
          <input className="input" value={config.api_login || ''}
            onChange={e => set('api_login', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.sender')}</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)} placeholder="Rampart"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/>
        </div>
      </>
    );
  }
  if (kind === 'whatsapp_whapi') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_token')}</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· jid (15551234567@s.whatsapp.net)</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.base_url')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://gate.whapi.cloud"/></div>
      </>
    );
  }
  if (kind === 'whatsapp_360') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.phone')}</label>
          <input className="input mono" value={config.phone || ''}
            onChange={e => set('phone', e.target.value)} placeholder="15551234567"/></div>
      </>
    );
  }
  if (kind === 'whatsapp_evolution') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.base_url')}</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://evolution.local:8080"/></div>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.instance')}</label>
          <input className="input mono" value={config.instance || ''}
            onChange={e => set('instance', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.number')}</label>
          <input className="input mono" value={config.number || ''}
            onChange={e => set('number', e.target.value)} placeholder="15551234567"/></div>
      </>
    );
  }
  if (kind === 'flock') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://api.flock.com/hooks/sendMessage/..."/></div>
    );
  }
  if (kind === 'serwersms') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.username')}</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.password')}</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.sender')}</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.phone')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.phone || ''}
            onChange={e => set('phone', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'smsplanet') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.sender')}</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'smsc') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.login')}</label>
          <input className="input" value={config.login || ''}
            onChange={e => set('login', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.password')}</label>
          <input className="input mono" type="password" value={config.psw || ''}
            onChange={e => set('psw', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.phones')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.phones || ''}
            onChange={e => set('phones', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'cellsynt') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.username')}</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.password')}</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.originator')}</label>
          <input className="input mono" value={config.originator || ''}
            onChange={e => set('originator', e.target.value)} placeholder="Rampart"/></div>
        <div className="field"><label className="field-label">{t('notif.f.destination')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.destination || ''}
            onChange={e => set('destination', e.target.value)} placeholder="0046701234567"/></div>
      </>
    );
  }
  if (kind === 'sevenio') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated E.164</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.from')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'gtxmessaging') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.sender_id')}</label>
          <input className="input mono" value={config.sender_id || ''}
            onChange={e => set('sender_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'onesender') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.base_url')}</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://onesender.local"/></div>
        <div className="field"><label className="field-label">{t('notif.f.api_token')}</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.recipient')}</label>
          <input className="input mono" value={config.recipient || ''}
            onChange={e => set('recipient', e.target.value)} placeholder="15551234567"/></div>
      </>
    );
  }
  if (kind === 'promosms') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.username')}</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.password')}</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.sender')}</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'smspartner') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.sender')}</label>
          <input className="input mono" value={config.sender || ''}
            onChange={e => set('sender', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'sms_ir') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.line_number')}</label>
          <input className="input mono" value={config.line_number || ''}
            onChange={e => set('line_number', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.mobiles')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.mobiles || ''}
            onChange={e => set('mobiles', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'freemobile') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.user')}</label>
          <input className="input" value={config.user || ''}
            onChange={e => set('user', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.pass')}</label>
          <input className="input mono" type="password" value={config.pass || ''}
            onChange={e => set('pass', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'flashduty') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.integration_url')}</label>
          <input className="input mono" value={config.integration_url || ''}
            onChange={e => set('integration_url', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.severity')}</label>
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
        <div className="field"><label className="field-label">{t('notif.f.base_url')}</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="http://192.168.1.1"/></div>
        <div className="field"><label className="field-label">{t('notif.f.username')}</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.password')}</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.number')}</label>
          <input className="input mono" value={config.number || ''}
            onChange={e => set('number', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'kook') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.bot_token')}</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.target_type')}</label>
          <select className="select" value={config.target_type || 'GROUP'} onChange={e => set('target_type', e.target.value)}>
            <option>GROUP</option><option>PERSON</option>
          </select>
        </div>
        <div className="field"><label className="field-label">{t('notif.f.target_id')}</label>
          <input className="input mono" value={config.target_id || ''}
            onChange={e => set('target_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'nostr') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.bridge_url')}</label>
          <input className="input mono" value={config.bridge_url || ''}
            onChange={e => set('bridge_url', e.target.value)} placeholder="http://nostr-bridge.local/dm"/></div>
        <div className="field"><label className="field-label">{t('notif.f.recipient')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· npub or hex pubkey</span></label>
          <input className="input mono" value={config.recipient || ''}
            onChange={e => set('recipient', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.api_key')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'onebot') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.http_url')}</label>
          <input className="input mono" value={config.http_url || ''}
            onChange={e => set('http_url', e.target.value)} placeholder="http://onebot.local:5700"/></div>
        <div className="field"><label className="field-label">{t('notif.f.kind')}</label>
          <select className="select" value={config.kind || 'group'} onChange={e => set('kind', e.target.value)}>
            <option>group</option><option>private</option>
          </select>
        </div>
        <div className="field"><label className="field-label">{t('notif.f.target_id')}</label>
          <input className="input mono" type="number" value={config.target_id || ''}
            onChange={e => set('target_id', parseInt(e.target.value, 10) || 0)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.access_token')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'onechat') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.bot_token')}</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.chat_id')}</label>
          <input className="input mono" value={config.chat_id || ''}
            onChange={e => set('chat_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'max_messenger') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.access_token')}</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.chat_id')}</label>
          <input className="input mono" value={config.chat_id || ''}
            onChange={e => set('chat_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'halo_psa') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.base_url')}</label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://yourorg.halopsa.com"/></div>
        <div className="field"><label className="field-label">{t('notif.f.client_id')}</label>
          <input className="input mono" value={config.client_id || ''}
            onChange={e => set('client_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.client_secret')}</label>
          <input className="input mono" type="password" value={config.client_secret || ''}
            onChange={e => set('client_secret', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.team')}</label>
          <input className="input" value={config.team || ''}
            onChange={e => set('team', e.target.value)} placeholder="Service Desk"/></div>
        <div className="field"><label className="field-label">{t('notif.f.ticket_type_id')}</label>
          <input className="input mono" type="number" value={config.ticket_type_id || ''}
            onChange={e => set('ticket_type_id', parseInt(e.target.value, 10) || 0)}/></div>
      </>
    );
  }
  if (kind === 'jira_sm') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.site_url')}</label>
          <input className="input mono" value={config.site_url || ''}
            onChange={e => set('site_url', e.target.value)} placeholder="https://yourorg.atlassian.net"/></div>
        <div className="field"><label className="field-label">{t('notif.f.email')}</label>
          <input className="input" value={config.email || ''}
            onChange={e => set('email', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.api_token')}</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.project_key')}</label>
          <input className="input mono" value={config.project_key || ''}
            onChange={e => set('project_key', e.target.value)} placeholder="INC"/></div>
        <div className="field"><label className="field-label">{t('notif.f.issue_type')}</label>
          <input className="input mono" value={config.issue_type || ''}
            onChange={e => set('issue_type', e.target.value)} placeholder="Incident"/></div>
      </>
    );
  }
  if (kind === 'spug_push') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.template_code')}</label>
        <input className="input mono" value={config.template_code || ''}
          onChange={e => set('template_code', e.target.value)}/></div>
    );
  }
  if (kind === 'wpush') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.channel')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.channel || ''}
            onChange={e => set('channel', e.target.value)} placeholder="wechat,email"/></div>
      </>
    );
  }
  if (kind === 'vk') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.access_token')}</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.peer_id')}</label>
          <input className="input mono" type="number" value={config.peer_id || ''}
            onChange={e => set('peer_id', parseInt(e.target.value, 10) || 0)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.api_version')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" value={config.api_version || ''}
            onChange={e => set('api_version', e.target.value)} placeholder="5.199"/></div>
      </>
    );
  }
  if (kind === 'yzj') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)}/></div>
    );
  }
  if (kind === 'google_sheets') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.apps_script_web_app_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://script.google.com/macros/s/.../exec"/></div>
    );
  }
  if (kind === 'gorush') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.server')}</label>
          <input className="input mono" value={config.server || ''}
            onChange={e => set('server', e.target.value)} placeholder="http://gorush.local:8088"/></div>
        <div className="field"><label className="field-label">{t('notif.f.platform')}</label>
          <select className="select" value={config.platform || 'ios'} onChange={e => set('platform', e.target.value)}>
            <option>ios</option><option>android</option>
          </select>
        </div>
        <div className="field"><label className="field-label">{t('notif.f.tokens')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={(config.tokens || []).join(',')}
            onChange={e => set('tokens', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}/></div>
        <div className="field"><label className="field-label">{t('notif.f.topic')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" value={config.topic || ''}
            onChange={e => set('topic', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'fluxer' || kind === 'splash') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)}/></div>
    );
  }
  if (kind === 'messagebird') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.access_key')}</label>
          <input className="input mono" type="password" value={config.access_key || ''}
            onChange={e => set('access_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.originator')}</label>
          <input className="input mono" value={config.originator || ''}
            onChange={e => set('originator', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.recipients')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.recipients || ''}
            onChange={e => set('recipients', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'plivo') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.auth_id')}</label>
          <input className="input mono" value={config.auth_id || ''}
            onChange={e => set('auth_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.auth_token')}</label>
          <input className="input mono" type="password" value={config.auth_token || ''}
            onChange={e => set('auth_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.from')}</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="+15551234567"/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· angle-bracket separated, e.g. {'+15551111<+15552222>'}</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'vonage') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.api_secret')}</label>
          <input className="input mono" type="password" value={config.api_secret || ''}
            onChange={e => set('api_secret', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.from')}</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')}</label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="15551234567"/></div>
      </>
    );
  }
  if (kind === 'bandwidth') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.account_id')}</label>
          <input className="input mono" value={config.account_id || ''}
            onChange={e => set('account_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.username')}</label>
          <input className="input" value={config.username || ''}
            onChange={e => set('username', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.password')}</label>
          <input className="input mono" type="password" value={config.password || ''}
            onChange={e => set('password', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.application_id')}</label>
          <input className="input mono" value={config.application_id || ''}
            onChange={e => set('application_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.from')}</label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="+15551234567"/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'webex') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.bot_token')}</label>
          <input className="input mono" type="password" value={config.bot_token || ''}
            onChange={e => set('bot_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.room_id')}</label>
          <input className="input mono" value={config.room_id || ''}
            onChange={e => set('room_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'pushcut') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.notification_name')}</label>
          <input className="input mono" value={config.notification_name || ''}
            onChange={e => set('notification_name', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'smsglobal') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.api_secret')}</label>
          <input className="input mono" type="password" value={config.api_secret || ''}
            onChange={e => set('api_secret', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.origin')}</label>
          <input className="input mono" value={config.origin || ''}
            onChange={e => set('origin', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.destination')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input mono" value={config.destination || ''}
            onChange={e => set('destination', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'alertops') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.integration_url')}</label>
        <input className="input mono" value={config.integration_url || ''}
          onChange={e => set('integration_url', e.target.value)}/></div>
    );
  }
  if (kind === 'mailgun') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.domain')}</label>
          <input className="input mono" value={config.domain || ''}
            onChange={e => set('domain', e.target.value)} placeholder="mg.example.com"/></div>
        <div className="field"><label className="field-label">{t('notif.f.base_url')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· EU: https://api.eu.mailgun.net</span></label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://api.mailgun.net"/></div>
        <div className="field"><label className="field-label">{t('notif.f.from')}</label>
          <input className="input" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'mailjet') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.api_secret')}</label>
          <input className="input mono" type="password" value={config.api_secret || ''}
            onChange={e => set('api_secret', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.from_email')}</label>
          <input className="input" value={config.from_email || ''}
            onChange={e => set('from_email', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to_email')}</label>
          <input className="input" value={config.to_email || ''}
            onChange={e => set('to_email', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'postmark') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.server_token')}</label>
          <input className="input mono" type="password" value={config.server_token || ''}
            onChange={e => set('server_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.from')}</label>
          <input className="input" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')}</label>
          <input className="input" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.message_stream')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" value={config.message_stream || ''}
            onChange={e => set('message_stream', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'mandrill') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.from_email')}</label>
          <input className="input" value={config.from_email || ''}
            onChange={e => set('from_email', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to_email')}</label>
          <input className="input" value={config.to_email || ''}
            onChange={e => set('to_email', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'sparkpost') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.from')}</label>
          <input className="input" value={config.from || ''}
            onChange={e => set('from', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated</span></label>
          <input className="input" value={config.to || ''}
            onChange={e => set('to', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.base_url')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· EU: https://api.eu.sparkpost.com</span></label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://api.sparkpost.com"/></div>
      </>
    );
  }
  if (kind === 'spike_sh' || kind === 'zenduty') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.integration_url')}</label>
        <input className="input mono" value={config.integration_url || ''}
          onChange={e => set('integration_url', e.target.value)}/></div>
    );
  }
  if (kind === 'ringcentral') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.webhook_url')}</label>
        <input className="input mono" value={config.webhook_url || ''}
          onChange={e => set('webhook_url', e.target.value)} placeholder="https://hooks.ringcentral.com/webhook/..."/></div>
    );
  }
  if (kind === 'ilert') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.integration_key')}</label>
        <input className="input mono" type="password" value={config.integration_key || ''}
          onChange={e => set('integration_key', e.target.value)}/></div>
    );
  }
  if (kind === 'linear') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.team_id')}</label>
          <input className="input mono" value={config.team_id || ''}
            onChange={e => set('team_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'clickup') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_token')}</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.list_id')}</label>
          <input className="input mono" value={config.list_id || ''}
            onChange={e => set('list_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'trello') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.key')}</label>
          <input className="input mono" value={config.key || ''}
            onChange={e => set('key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.token')}</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.list_id')}</label>
          <input className="input mono" value={config.list_id || ''}
            onChange={e => set('list_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'github_issue') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.personal_access_token')}</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.owner')}</label>
          <input className="input mono" value={config.owner || ''}
            onChange={e => set('owner', e.target.value)} placeholder="myorg"/></div>
        <div className="field"><label className="field-label">{t('notif.f.repo')}</label>
          <input className="input mono" value={config.repo || ''}
            onChange={e => set('repo', e.target.value)} placeholder="incidents"/></div>
        <div className="field"><label className="field-label">{t('notif.f.labels')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· comma-separated, optional</span></label>
          <input className="input mono" value={(config.labels || []).join(',')}
            onChange={e => set('labels', e.target.value.split(',').map(s => s.trim()).filter(Boolean))}/></div>
      </>
    );
  }
  if (kind === 'gitlab_issue') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.base_url')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional, defaults to gitlab.com</span></label>
          <input className="input mono" value={config.base_url || ''}
            onChange={e => set('base_url', e.target.value)} placeholder="https://gitlab.example.com"/></div>
        <div className="field"><label className="field-label">{t('notif.f.personal_access_token')}</label>
          <input className="input mono" type="password" value={config.token || ''}
            onChange={e => set('token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.project_id')}</label>
          <input className="input mono" value={config.project_id || ''}
            onChange={e => set('project_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'asana') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.access_token')}</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.workspace')}</label>
          <input className="input mono" value={config.workspace || ''}
            onChange={e => set('workspace', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.project')}</label>
          <input className="input mono" value={config.project || ''}
            onChange={e => set('project', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'notion') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.integration_token')}</label>
          <input className="input mono" type="password" value={config.api_token || ''}
            onChange={e => set('api_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.database_id')}</label>
          <input className="input mono" value={config.database_id || ''}
            onChange={e => set('database_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'sentry') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.dsn')}</label>
        <input className="input mono" type="password" value={config.dsn || ''}
          onChange={e => set('dsn', e.target.value)} placeholder="https://<public>@<host>/<project>"/></div>
    );
  }
  if (kind === 'rollbar') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.access_token')}</label>
          <input className="input mono" type="password" value={config.access_token || ''}
            onChange={e => set('access_token', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.environment')}</label>
          <input className="input mono" value={config.environment || ''}
            onChange={e => set('environment', e.target.value)} placeholder="production"/></div>
      </>
    );
  }
  if (kind === 'honeybadger') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.environment')}</label>
          <input className="input mono" value={config.environment || ''}
            onChange={e => set('environment', e.target.value)} placeholder="production"/></div>
      </>
    );
  }
  if (kind === 'healthchecks_io') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.ping_url')}</label>
        <input className="input mono" value={config.ping_url || ''}
          onChange={e => set('ping_url', e.target.value)} placeholder="https://hc-ping.com/&lt;uuid&gt;"/></div>
    );
  }
  if (kind === 'betterstack') {
    return (
      <div className="field"><label className="field-label">{t('notif.f.integration_url')}</label>
        <input className="input mono" value={config.integration_url || ''}
          onChange={e => set('integration_url', e.target.value)}/></div>
    );
  }
  if (kind === 'statuspage_io') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.page_id')}</label>
          <input className="input mono" value={config.page_id || ''}
            onChange={e => set('page_id', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'datadog') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.api_key')}</label>
          <input className="input mono" type="password" value={config.api_key || ''}
            onChange={e => set('api_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.site')}</label>
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
        <div className="field"><label className="field-label">{t('notif.f.insert_key')}</label>
          <input className="input mono" type="password" value={config.insert_key || ''}
            onChange={e => set('insert_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.account_id')}</label>
          <input className="input mono" value={config.account_id || ''}
            onChange={e => set('account_id', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.region')}</label>
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
        <div className="field"><label className="field-label">{t('notif.f.region')}</label>
          <input className="input mono" value={config.region || ''}
            onChange={e => set('region', e.target.value)} placeholder="us-east-1"/></div>
        <div className="field"><label className="field-label">{t('notif.f.topic_arn')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· or phone number</span></label>
          <input className="input mono" value={config.topic_arn || ''}
            onChange={e => set('topic_arn', e.target.value)} placeholder="arn:aws:sns:us-east-1:123456789012:alerts"/></div>
        <div className="field"><label className="field-label">{t('notif.f.phone_number_sms_e_164')}</label>
          <input className="input mono" value={config.phone_number || ''}
            onChange={e => set('phone_number', e.target.value)} placeholder="+15555550100"/></div>
        <div className="field"><label className="field-label">{t('notif.f.access_key_id')}</label>
          <input className="input mono" value={config.access_key_id || ''}
            onChange={e => set('access_key_id', e.target.value)} placeholder="AKIA…"/></div>
        <div className="field"><label className="field-label">{t('notif.f.secret_access_key')}</label>
          <input className="input mono" type="password" value={config.secret_access_key || ''}
            onChange={e => set('secret_access_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.session_token')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional, STS</span></label>
          <input className="input mono" type="password" value={config.session_token || ''}
            onChange={e => set('session_token', e.target.value)}/></div>
      </>
    );
  }
  if (kind === 'webpush') {
    return (
      <>
        <div className="field"><label className="field-label">{t('notif.f.contact_uri')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
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
        <div className="field"><label className="field-label">{t('notif.f.project_id')}</label>
          <input className="input mono" value={config.project_id || ''}
            onChange={e => set('project_id', e.target.value)} placeholder="my-gcp-project"/></div>
        <div className="field"><label className="field-label">{t('notif.f.topic')}</label>
          <input className="input mono" value={config.topic || ''}
            onChange={e => set('topic', e.target.value)} placeholder="rampart-alerts"/></div>
        <div className="field"><label className="field-label">{t('notif.f.service_account_email')}</label>
          <input className="input mono" value={config.client_email || ''}
            onChange={e => set('client_email', e.target.value)} placeholder="rampart@my-project.iam.gserviceaccount.com"/></div>
        <div className="field"><label className="field-label">{t('notif.f.private_key_pem_from_service_account_json')}</label>
          <textarea className="input mono" rows={6} value={config.private_key || ''}
            onChange={e => set('private_key', e.target.value)}
            placeholder={'-----BEGIN ' + 'PRIVATE KEY-----\n...'}/></div>
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
        <div className="field"><label className="field-label">{t('notif.f.namespace')}</label>
          <input className="input mono" value={config.namespace || ''}
            onChange={e => set('namespace', e.target.value)} placeholder="my-namespace (omit .servicebus.windows.net)"/></div>
        <div className="field"><label className="field-label">{t('notif.f.queue_or_topic')}</label>
          <input className="input mono" value={config.entity || ''}
            onChange={e => set('entity', e.target.value)} placeholder="alerts"/></div>
        <div className="field"><label className="field-label">{t('notif.f.sas_key_name')}</label>
          <input className="input mono" value={config.sas_key_name || ''}
            onChange={e => set('sas_key_name', e.target.value)} placeholder="RootManageSharedAccessKey"/></div>
        <div className="field"><label className="field-label">{t('notif.f.sas_key')}</label>
          <input className="input mono" type="password" value={config.sas_key || ''}
            onChange={e => set('sas_key', e.target.value)}/></div>
        <div className="field"><label className="field-label">{t('notif.f.token_ttl_seconds')}</label>
          <input className="input mono" type="number" min="60" value={config.ttl_seconds || 300}
            onChange={e => set('ttl_seconds', Number(e.target.value) || 300)}/></div>
      </>
    );
  }
  if (kind === 'sms_twilio') {
    return (
      <>
        <div className="field">
          <label className="field-label">{t('notif.f.account_sid')}</label>
          <input className="input mono" value={config.account_sid || ''}
            onChange={e => set('account_sid', e.target.value)} placeholder="ACxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.auth_token')}</label>
          <input className="input mono" type="password" value={config.auth_token || ''}
            onChange={e => set('auth_token', e.target.value)} placeholder="32-char token"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.from')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· E.164</span></label>
          <input className="input mono" value={config.from || ''}
            onChange={e => set('from', e.target.value)} placeholder="+15551234567"/>
        </div>
        <div className="field">
          <label className="field-label">{t('notif.f.to')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· E.164, comma-separated</span></label>
          <input className="input mono" value={config.to || ''}
            onChange={e => set('to', e.target.value)} placeholder="+15559876543, +44..."/>
        </div>
        <div className="field-hint">{t('notif.hint.sms_metered')}</div>
      </>
    );
  }
  return null;
}

export default function Notifications({ user } = {}) {
  const writable  = canWrite(user);
  const list      = useApi(() => api.notifications.list(), [], { pollMs: 0 });
  const templates = useApi(() => api.templates.list(),      [], { pollMs: 0 });
  const allTags   = useApi(() => api.tags.list(),           [], { pollMs: 0 });

  const [tab,     setTab]     = useState('channels');  // 'channels' | 'templates'
  const [showAdd, setShowAdd] = useState(false);
  const [editId,  setEditId]  = useState(null);        // null = add mode; id = editing
  const [kind,    setKind]    = useState('slack');
  const [name,    setName]    = useState('');
  const [config,  setConfig]  = useState({});
  const [templateId, setTemplateId] = useState('');
  const [cooldown,   setCooldown]   = useState(0);
  const [digest,     setDigest]     = useState(0);
  const [quietStart, setQuietStart] = useState('');  // '' = unset
  const [quietEnd,   setQuietEnd]   = useState('');
  const [rateLimit,  setRateLimit]  = useState(0);
  const [busy,    setBusy]    = useState(false);
  const [msg,     setMsg]     = useState(null);
  const [kindQuery, setKindQuery] = useState('');  // filters the channel picker

  const reload = async () => {
    // useApi doesn't expose a refetch; bounce the hash to nothing visible
    // and back. Simpler: just reload the page once after add/delete.
    window.location.reload();
  };

  const resetForm = () => {
    setEditId(null); setKind('slack'); setName(''); setConfig({});
    setTemplateId(''); setCooldown(0); setDigest(0);
    setQuietStart(''); setQuietEnd(''); setRateLimit(0);
    setMsg(null); setKindQuery('');
  };

  // Prefill the form from an existing channel and switch to edit mode.
  // Kind is locked while editing — config shape is kind-specific, so
  // changing it would orphan the existing config blob.
  const startEdit = (c) => {
    // If already editing this row, toggle the inline editor closed.
    if (editId === c.id) { resetForm(); return; }
    setEditId(c.id);
    setKind(c.kind);
    setName(c.name);
    setConfig(c.config || {});
    setTemplateId(c.template_id || '');
    setCooldown(c.cooldown_seconds || 0);
    setDigest(c.digest_window_secs || 0);
    setQuietStart(c.quiet_hours_start == null ? '' : String(c.quiet_hours_start));
    setQuietEnd(c.quiet_hours_end == null ? '' : String(c.quiet_hours_end));
    setRateLimit(c.rate_limit_per_hour || 0);
    setMsg(null);
    setShowAdd(false);  // edit renders inline under the row, not at top
  };

  const submit = async (e) => {
    e?.preventDefault?.();
    setMsg(null);
    if (!name.trim()) { setMsg({ kind: 'err', text: 'Name is required.' }); return; }
    // '' → null (no quiet-hours bound); otherwise a 0-23 integer.
    const qStart = quietStart === '' ? null : Number(quietStart);
    const qEnd   = quietEnd   === '' ? null : Number(quietEnd);
    const rate   = Number(rateLimit) || 0;
    setBusy(true);
    try {
      if (editId) {
        await api.notifications.update(editId, {
          name: name.trim(),
          config,
          template_id: templateId || null,
          cooldown_seconds: Number(cooldown) || 0,
          digest_window_secs: Number(digest) || 0,
          quiet_hours_start: qStart,
          quiet_hours_end: qEnd,
          rate_limit_per_hour: rate,
        });
        setMsg({ kind: 'ok', text: 'Channel updated. Reloading…' });
      } else {
        await api.notifications.create(kind, name.trim(), config, templateId || null, cooldown, digest, {
          quiet_hours_start: qStart,
          quiet_hours_end: qEnd,
          rate_limit_per_hour: rate,
        });
        setMsg({ kind: 'ok', text: 'Channel added. Reloading…' });
      }
      setTimeout(reload, 400);
    } catch (e2) {
      setMsg({ kind: 'err', text: e2.message || 'Failed to save channel.' });
      setBusy(false);
    }
  };

  const removeOne = async (id) => {
    if (!(await confirmDialog({ message: t('notifications.channel.delete_confirm') }))) return;
    try {
      await api.notifications.remove(id);
      reload();
    } catch (e) { toast(e.message, 'error'); }
  };

  const sendTest = async (id) => {
    try {
      await api.notifications.test(id);
      toast(t('notifications.test_sent'), 'error');
    } catch (e) { toast(`Failed: ${e.message}`, 'error'); }
  };

  const channels = list.data || [];

  // The add/edit channel form. Rendered at the top in add mode, or inline
  // under the row being edited. `inline` tweaks margins for the nested case.
  const channelFormCard = (inline = false) => (
    <div className={`card form-panel${inline ? ' form-panel-inline' : ''}`} style={inline ? { padding: 20, margin: 0 } : undefined}>
      <div className="form-eyebrow">
        {editId ? t('notifications.form.eyebrow_edit') : t('notifications.form.eyebrow_new')}
      </div>
      <h3>
        {editId ? name : t('notifications.form.title_new')}
      </h3>
      <div className="form-divider"/>
      {editId && (
        <div className="field-hint" style={{ marginBottom: 12, color: 'var(--text-3)' }}>
          {t('notifications.form.kind_locked')}
        </div>
      )}
      {!editId && (
        <div className="field">
          <label className="field-label">{t('notifications.form.channel_type')}</label>
          <ChannelTypeDropdown
            value={kind}
            query={kindQuery}
            setQuery={setKindQuery}
            onSelect={(id) => { setKind(id); setConfig({}); }}
          />
        </div>
      )}

      <form onSubmit={submit}>
        <div className="field">
          <label className="field-label">{t('notifications.form.display_name')}</label>
          <input className="input" value={name} onChange={e => setName(e.target.value)}
            placeholder={t('notifications.form.display_name_placeholder')}/>
        </div>
        <ConfigForm kind={kind} config={config} setConfig={setConfig}/>

        <div className="field">
          <label className="field-label">{t('notifications.form.template')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <select className="select" value={templateId} onChange={e => setTemplateId(e.target.value)}>
            <option value="">{t('notifications.form.template_default')}</option>
            {(templates.data || []).map(tpl => (
              <option key={tpl.id} value={tpl.id}>{tpl.name} ({tpl.event_kind})</option>
            ))}
          </select>
          <div className="field-hint">{t('notifications.form.template_hint')}</div>
        </div>

        <div className="field">
          <label className="field-label">{t('notifications.form.cooldown')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('notifications.form.cooldown_unit')}</span></label>
          <input className="input" type="number" min="0" step="1" value={cooldown}
            onChange={e => setCooldown(e.target.value)} placeholder={t('notifications.form.cooldown_placeholder')}/>
          <div className="field-hint">{t('notifications.form.cooldown_hint')}</div>
        </div>

        <div className="field">
          <label className="field-label">{t('notifications.form.digest')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('notifications.form.digest_unit')}</span></label>
          <input className="input" type="number" min="0" max="3600" step="1" value={digest}
            onChange={e => setDigest(e.target.value)} placeholder={t('notifications.form.digest_placeholder')}/>
          <div className="field-hint">{t('notifications.form.digest_hint')}</div>
        </div>

        <div className="field">
          <label className="field-label">{t('notifications.form.quiet_hours')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('notifications.form.quiet_hours_unit')}</span></label>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <input className="input" type="number" min="0" max="23" step="1" value={quietStart}
              onChange={e => setQuietStart(e.target.value)} placeholder={t('notifications.form.quiet_hours_start_placeholder')} style={{ width: 120 }}/>
            <span style={{ color: 'var(--text-3)' }}>→</span>
            <input className="input" type="number" min="0" max="23" step="1" value={quietEnd}
              onChange={e => setQuietEnd(e.target.value)} placeholder={t('notifications.form.quiet_hours_end_placeholder')} style={{ width: 120 }}/>
          </div>
          <div className="field-hint">{t('notifications.form.quiet_hours_hint')}</div>
        </div>

        <div className="field">
          <label className="field-label">{t('notifications.form.rate_limit')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('notifications.form.rate_limit_unit')}</span></label>
          <input className="input" type="number" min="0" step="1" value={rateLimit}
            onChange={e => setRateLimit(e.target.value)} placeholder={t('notifications.form.rate_limit_placeholder')}/>
          <div className="field-hint">{t('notifications.form.rate_limit_hint')}</div>
        </div>

        {msg && <div className={msg.kind === 'ok' ? 'banner-ok' : 'banner-err'} style={{ marginBottom: 12 }}>{msg.text}</div>}

        <div style={{ display: 'flex', gap: 8 }}>
          <button className="btn btn-accent" type="submit" disabled={busy}>
            {busy ? <><Loader2 size={13} className="spin"/> {t('common.saving')}</>
                  : editId ? <><Save size={13}/> {t('notifications.form.update_channel')}</>
                           : <><Plus size={13}/> {t('notifications.form.save_channel')}</>}
          </button>
          <button className="btn" type="button" onClick={() => { setShowAdd(false); resetForm(); }}>{t('common.cancel')}</button>
        </div>
      </form>

      {/* Channel routing tags — edit mode only (needs a saved channel id).
          A channel sharing a tag with a monitor or its folder auto-routes. */}
      {editId && (
        <div style={{ borderTop: '1px solid var(--border)', marginTop: 16, paddingTop: 14 }}>
          <div className="field-label" style={{ marginBottom: 6 }}>{t('notifications.form.routing_tags')}</div>
          <div className="field-hint" style={{ marginBottom: 8 }}>
            {t('notifications.form.routing_tags_hint')}
          </div>
          <ChannelTagEditor channelId={editId} allTags={allTags.data || []}/>
        </div>
      )}
    </div>
  );

  return (
    <div className="rampart">
      <style>{css}</style>

      <header style={{
        background: 'var(--surface)', borderBottom: '1px solid var(--border)',
        padding: '14px 24px', display: 'flex', alignItems: 'center', gap: 14,
        position: 'sticky', top: 0, zIndex: 10,
      }}>
        <a href="#/" style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text-3)', textDecoration: 'none', fontSize: 13 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>
        <span style={{ color: 'var(--text-3)' }}>/</span>
        <span style={{ fontSize: 14, fontWeight: 500, display: 'flex', alignItems: 'center', gap: 8 }}>
          <Bell size={15}/> {t('notifications.title')}
        </span>
        {tab === 'channels' && writable && (
          showAdd ? (
            <button className="btn" style={{ marginLeft: 'auto' }}
              onClick={() => { setShowAdd(false); resetForm(); }}>
              <X size={13}/> {t('notifications.close_form')}
            </button>
          ) : (
            <button className="btn btn-accent" style={{ marginLeft: 'auto' }}
              onClick={() => { resetForm(); setShowAdd(true); }}>
              <Plus size={13}/> {t('notifications.add_channel')}
            </button>
          )
        )}
      </header>

      <main style={{ padding: '28px 32px', maxWidth: 1000, margin: '0 auto' }}>
        <h1 style={{ fontSize: 22, fontWeight: 600, margin: '0 0 8px', letterSpacing: '-.02em' }}>
          {t('notifications.title')}
        </h1>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 18px' }}>
          {t('notifications.subtitle')}
        </p>

        <div className="tabs">
          <button className={tab === 'channels'  ? 'active' : ''} onClick={() => setTab('channels')}>
            <Bell size={12}/> {t('notifications.tab.channels')} {(list.data || []).length > 0 && <span className="mono" style={{ opacity: .7 }}>{(list.data || []).length}</span>}
          </button>
          <button className={tab === 'templates' ? 'active' : ''} onClick={() => setTab('templates')}>
            <FileText size={12}/> {t('notifications.tab.templates')} {(templates.data || []).length > 0 && <span className="mono" style={{ opacity: .7 }}>{(templates.data || []).length}</span>}
          </button>
        </div>

        {tab === 'templates' && <TemplatesPanel state={templates} reload={reload}/>}

        {tab === 'channels' && (<>

        {/* Add form — top of page, only in add mode. Edit mode renders the
            same form inline under the row being edited (see channel list). */}
        {showAdd && !editId && channelFormCard()}

        {/* Channel list */}
        <div className="card" style={{ padding: 0 }}>
          {list.loading && <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>{t('notifications.loading_channels')}</div>}
          {!list.loading && channels.length === 0 && (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
              <Bell size={20} style={{ opacity: .4, marginBottom: 8 }}/>
              <div>{t('notifications.empty.title')}</div>
              <div style={{ marginTop: 6 }}>{t('notifications.empty.cta')}</div>
            </div>
          )}
          {channels.map(c => {
            const meta = SUPPORTED.find(s => s.id === c.kind);
            const Icon = meta ? meta.icon : AlertCircle;
            const tpl = c.template_id && (templates.data || []).find(t => t.id === c.template_id);
            const editingThis = editId === c.id;
            return (
              <React.Fragment key={c.id}>
              <div className="channel-row" style={editingThis ? { background: 'var(--surface-2)' } : undefined}>
                <div className="ch-icon-tile"><Icon size={17}/></div>
                <div style={{ minWidth: 0 }}>
                  <div style={{ fontSize: 14, fontWeight: 500, display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    {c.name}
                    {tpl && <span className="template-pill"><FileText size={9}/> {tpl.name}</span>}
                    {(c.tags || []).map(t => (
                      <span key={t.id} style={{
                        background: t.color || '#888', color: '#fff',
                        fontSize: 10, fontWeight: 500, padding: '2px 7px', borderRadius: 999,
                      }}>{t.name}</span>
                    ))}
                  </div>
                  <div className="ch-meta">
                    <span className="ch-type-pill">{meta ? meta.name : c.kind}</span>
                    <span className={c.active ? 'ch-status-on' : 'ch-status-off'}>
                      {c.active ? t('notifications.channel.enabled') : t('notifications.channel.disabled')}
                    </span>
                  </div>
                </div>
                <div style={{ gridColumn: '3', display: 'flex', alignItems: 'center', gap: 8, justifyContent: 'flex-end' }}>
                  {c.kind === 'webpush' && <EnablePushButton notificationId={c.id}/>}
                  {writable && (
                    <>
                      <button className={`btn ${editingThis ? 'btn-accent' : ''}`} onClick={() => startEdit(c)} title={t('notifications.channel.edit_title')}>
                        <Edit3 size={12}/> {editingThis ? t('common.close') : t('common.edit')}
                      </button>
                      <button className="btn" onClick={() => sendTest(c.id)} title={t('notifications.channel.test_title')}>
                        <Send size={12}/> {t('common.test')}
                      </button>
                      <button className="btn btn-danger" onClick={() => removeOne(c.id)} title={t('notifications.channel.delete_title')}>
                        <Trash2 size={12}/>
                      </button>
                    </>
                  )}
                </div>
              </div>
              {editingThis && (
                <div style={{ padding: '0 16px 16px' }}>{channelFormCard(true)}</div>
              )}
              </React.Fragment>
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
  { id: 'slo_breached',      label: 'SLO breached' },
  { id: 'slo_recovered',     label: 'SLO recovered' },
];

// Starter templates — click to prefill the editor, tweak, then save.
// Covers the common alert tones (terse paging, rich chat, recovery,
// maintenance, incident) so users aren't staring at a blank form.
const TEMPLATE_PRESETS = [
  {
    name: 'Concise outage (paging)',
    event_kind: 'monitor_down',
    subject_template: '🔴 DOWN: {{monitor.name}}',
    body_template: '{{monitor.name}} is DOWN ({{status_code}}).\n{{msg}}',
  },
  {
    name: 'Rich chat alert',
    event_kind: 'monitor_down',
    subject_template: '🔴 {{monitor.name}} is down',
    body_template:
      '*{{monitor.name}}* went *{{status}}* (was {{prev_status}})\n' +
      '• URL: {{monitor.url}}\n• Code: {{status_code}}\n• Latency: {{latency_ms}}ms\n' +
      '• Message: {{msg}}\n• At: {{ts}}',
  },
  {
    name: 'Recovery / all clear',
    event_kind: 'monitor_up',
    subject_template: '✅ RECOVERED: {{monitor.name}}',
    body_template: '{{monitor.name}} is back UP (was {{prev_status}}).\nLatency: {{latency_ms}}ms at {{ts}}',
  },
  {
    name: 'Degraded warning',
    event_kind: 'monitor_warn',
    subject_template: '🟡 DEGRADED: {{monitor.name}}',
    body_template: '{{monitor.name}} is degraded.\nLatency: {{latency_ms}}ms · Code: {{status_code}}\n{{msg}}',
  },
  {
    name: 'Maintenance window',
    event_kind: 'maintenance_start',
    subject_template: '🔧 Maintenance: {{monitor.name}}',
    body_template: 'Planned maintenance affecting {{monitor.name}} has started. Alerts are suppressed until it ends.',
  },
  {
    name: 'Incident update',
    event_kind: 'incident_updated',
    subject_template: '📣 Incident update: {{monitor.name}}',
    body_template: '{{msg}}\n\nStatus: {{status}} · {{ts}}',
  },
];

function TemplatesPanel({ state, reload }) {
  const [showAdd, setShowAdd] = useState(false);
  const [prefill, setPrefill] = useState(null);  // preset to seed a new template
  const [editing, setEditing] = useState(null);  // template object being edited, or null

  return (
    <>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
          {t('notifications.templates.subtitle')}
        </p>
        {!showAdd && !editing && (
          <button className="btn btn-accent" onClick={() => { setPrefill(null); setShowAdd(true); }}>
            <Plus size={13}/> {t('notifications.templates.new')}
          </button>
        )}
      </div>

      {/* Starter presets — one click to seed the editor. */}
      {!showAdd && !editing && (
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em', marginBottom: 8 }}>
            {t('notifications.templates.start_preset')}
          </div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
            {TEMPLATE_PRESETS.map(p => (
              <button key={p.name} className="btn" style={{ fontSize: 12 }}
                onClick={() => { setPrefill(p); setShowAdd(true); }}
                title={p.subject_template}>
                <FileText size={12}/> {p.name}
              </button>
            ))}
          </div>
        </div>
      )}

      {(showAdd || editing) && (
        <TemplateForm
          initial={editing}
          prefill={prefill}
          onCancel={() => { setShowAdd(false); setEditing(null); setPrefill(null); }}
          onSaved={() => { setShowAdd(false); setEditing(null); setPrefill(null); reload(); }}
        />
      )}

      <div className="card" style={{ padding: 0, marginTop: 16 }}>
        {state.loading && <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>{t('notifications.templates.loading')}</div>}
        {!state.loading && (state.data || []).length === 0 && !showAdd && (
          <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
            <FileText size={20} style={{ opacity: .4, marginBottom: 8 }}/>
            <div>{t('notifications.templates.empty.title')}</div>
            <div style={{ marginTop: 6 }}>{t('notifications.templates.empty.cta')}</div>
          </div>
        )}
        {(state.data || []).map(tpl => (
          <div key={tpl.id} className="template-row">
            <FileText size={16} color="var(--text-2)"/>
            <div>
              <div style={{ fontSize: 13.5, fontWeight: 500 }}>{tpl.name}</div>
              <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 2 }}>
                <span style={{ textTransform: 'uppercase', letterSpacing: '.04em' }}>{tpl.event_kind.replace(/_/g, ' ')}</span>
                {tpl.channel_kinds.length > 0 && <> · for: {tpl.channel_kinds.join(', ')}</>}
              </div>
            </div>
            <span style={{ fontSize: 11, color: 'var(--text-3)' }}>
              {t('notifications.templates.chars', { n: tpl.body_template.length })}
            </span>
            <button className="btn" onClick={() => setEditing(tpl)} title={t('notifications.templates.edit_title')}>
              <Edit3 size={12}/>
            </button>
            <button className="btn" onClick={() => {
              // Clone — open the new-template form pre-filled from this template.
              // Drop the id so save() POSTs as a fresh row, and tweak name so the
              // unique constraint doesn't trip on first save.
              setEditing(null);
              setPrefill({
                name: `${tpl.name} (copy)`,
                event_kind: tpl.event_kind,
                subject_template: tpl.subject_template,
                body_template: tpl.body_template,
              });
              setShowAdd(true);
            }} title={t('notifications.templates.clone_title')}>
              <Copy size={12}/>
            </button>
            <button className="btn btn-danger" onClick={async () => {
              if (!(await confirmDialog({ message: `Delete template "${tpl.name}"?` }))) return;
              try { await api.templates.remove(tpl.id); reload(); }
              catch (e) { toast(e.message, 'error'); }
            }} title={t('notifications.templates.delete_title')}>
              <Trash2 size={12}/>
            </button>
          </div>
        ))}
      </div>
    </>
  );
}

function TemplateForm({ initial, prefill, onCancel, onSaved }) {
  // initial = editing an existing template (has id); prefill = seed a new
  // one from a preset. initial wins if both somehow present.
  const seed = initial || prefill;
  const [name,       setName]       = useState(seed?.name || '');
  const [eventKind,  setEventKind]  = useState(seed?.event_kind || 'monitor_down');
  const [subject,    setSubject]    = useState(seed?.subject_template || '[{{status}}] {{monitor.name}}');
  const [body,       setBody]       = useState(seed?.body_template ||
    '{{monitor.name}} is now {{status}} (was {{prev_status}}).\n\nLatency: {{latency_ms}}ms\nCode: {{status_code}}\nMessage: {{msg}}\nTime: {{ts}}');
  const [busy, setBusy] = useState(false);
  const [err,  setErr]  = useState(null);
  const [preview,    setPreview]    = useState(null);  // { subject, body } | null
  const [previewing, setPreviewing] = useState(false);

  const insertPlaceholder = (ph, target) => {
    if (target === 'subject') setSubject(s => s + ph);
    else                      setBody(b => b + ph);
  };

  const doPreview = async () => {
    setPreviewing(true);
    setErr(null);
    try {
      const r = await api.templates.preview(subject, body);
      setPreview(r);
    } catch (e) { setErr(e.message || 'Preview failed.'); }
    finally { setPreviewing(false); }
  };

  const save = async (e) => {
    e?.preventDefault?.();
    setErr(null);
    if (!name.trim()) { setErr(t('validation.name_required')); return; }
    if (!body.trim()) { setErr(t('validation.body_required')); return; }
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
        {initial ? t('notifications.editor.title_edit', { name: initial.name }) : t('notifications.editor.title_new')}
      </h3>
      <form onSubmit={save}>
        <div style={{ display: 'grid', gridTemplateColumns: '2fr 1fr', gap: 12 }}>
          <div className="field">
            <label className="field-label">{t('notifications.editor.name')}</label>
            <input className="input" value={name} onChange={e => setName(e.target.value)} placeholder={t('notifications.editor.name_placeholder')} autoFocus/>
          </div>
          <div className="field">
            <label className="field-label">{t('notifications.editor.event_kind')}</label>
            <select className="select" value={eventKind} onChange={e => setEventKind(e.target.value)}>
              {EVENT_KINDS.map(k => <option key={k.id} value={k.id}>{k.label}</option>)}
            </select>
          </div>
        </div>

        <div className="field">
          <label className="field-label">{t('notifications.editor.subject')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('common.optional')}</span></label>
          <input className="input mono" value={subject} onChange={e => setSubject(e.target.value)} placeholder="[{{status}}] {{monitor.name}}"/>
        </div>

        <div className="field">
          <label className="field-label">{t('notifications.editor.body')}</label>
          <textarea className="input mono" rows={7} value={body} onChange={e => setBody(e.target.value)}
            style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 12.5, padding: '10px 12px', lineHeight: 1.5 }}/>
        </div>

        <div className="field">
          <label className="field-label">{t('notifications.editor.placeholders')} <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· {t('notifications.editor.placeholders_hint')}</span></label>
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

        {preview && (
          <div className="field" style={{ background: 'var(--surface-2)', border: '1px solid var(--border)', borderRadius: 8, padding: 12, marginBottom: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
              <span className="field-label" style={{ margin: 0 }}>{t('notifications.editor.preview_header')}</span>
              <button type="button" className="btn btn-ghost" style={{ padding: '2px 8px', fontSize: 11 }} onClick={() => setPreview(null)}>
                <X size={11}/> {t('common.close')}
              </button>
            </div>
            {preview.subject && (
              <div style={{ marginBottom: 8 }}>
                <div style={{ fontSize: 10.5, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em', marginBottom: 2 }}>{t('notifications.editor.subject_label')}</div>
                <div className="mono" style={{ fontSize: 12.5, color: 'var(--text)' }}>{preview.subject}</div>
              </div>
            )}
            <div>
              <div style={{ fontSize: 10.5, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em', marginBottom: 2 }}>{t('notifications.editor.body_label')}</div>
              <pre className="mono" style={{ fontSize: 12, color: 'var(--text)', margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>{preview.body}</pre>
            </div>
          </div>
        )}

        <div style={{ display: 'flex', gap: 8 }}>
          <button className="btn btn-accent" type="submit" disabled={busy}>
            {busy ? <><Loader2 size={13} className="spin"/> {t('common.saving')}</> : <><Save size={13}/> {initial ? t('notifications.editor.update') : t('notifications.editor.save')}</>}
          </button>
          <button className="btn" type="button" onClick={doPreview} disabled={previewing}>
            {previewing ? <><Loader2 size={13} className="spin"/> {t('notifications.editor.rendering')}</> : <><BellRing size={13}/> {t('notifications.editor.preview')}</>}
          </button>
          <button className="btn" type="button" onClick={onCancel}><X size={13}/> {t('common.cancel')}</button>
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
    return <span className="btn" style={{ cursor: 'default', color: 'var(--up)' }}><BellRing size={12}/> {t('notifications.subscribed')}</span>;
  }
  return (
    <button className="btn" onClick={enable} disabled={state === 'working'} title={t('notifications.push.subscribe_tip')}>
      {state === 'working'
        ? <><Loader2 size={12} className="spin"/> …</>
        : <><BellRing size={12}/> {t('notifications.enable_push')}</>}
      {state === 'unsupported' && <span style={{ marginLeft: 6, fontSize: 11, color: 'var(--down)' }}>{t('notifications.not_supported')}</span>}
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

// Channel-type combobox. Click opens a panel listing ALL channel types
// (grouped by category, click-out + Esc to close); type to filter. No
// "popular" shortlist — opening shows everything.
function ChannelTypeDropdown({ value, query, setQuery, onSelect }) {
  const [open, setOpen] = useState(false);
  const ref = useRef(null);
  const inputRef = useRef(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    const onKey = (e) => { if (e.key === 'Escape') setOpen(false); };
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    // Focus the search box when the panel opens.
    setTimeout(() => inputRef.current && inputRef.current.focus(), 0);
    return () => { document.removeEventListener('mousedown', onDoc); document.removeEventListener('keydown', onKey); };
  }, [open]);

  const selected = SUPPORTED.find(s => s.id === value) || SUPPORTED[0];
  const SelIcon = selected.icon;
  const q = query.trim().toLowerCase();
  const matches = q
    ? SUPPORTED.filter(s =>
        s.name.toLowerCase().includes(q) || s.id.includes(q) || (s.hint || '').toLowerCase().includes(q))
    : SUPPORTED;

  return (
    <div ref={ref} style={{ position: 'relative' }}>
      {/* Trigger */}
      <button type="button" className="input" onClick={() => setOpen(o => !o)}
        style={{ display: 'flex', alignItems: 'center', gap: 8, textAlign: 'left', cursor: 'pointer', width: '100%' }}>
        <SelIcon size={15} color="var(--accent-2)"/>
        <span style={{ fontWeight: 500, fontSize: 13 }}>{selected.name}</span>
        <span style={{ marginLeft: 'auto', color: 'var(--text-3)', fontSize: 11 }}>{t('notifications.types_count', { n: SUPPORTED.length })}</span>
        <ChevronDown size={14} color="var(--text-3)" style={{ transform: open ? 'rotate(180deg)' : 'none', transition: 'transform .15s' }}/>
      </button>

      {open && (
        <div style={{
          position: 'absolute', top: 'calc(100% + 4px)', left: 0, right: 0, zIndex: 50,
          background: 'var(--surface)', border: '1px solid var(--border-2)', borderRadius: 10,
          boxShadow: '0 12px 32px rgba(0,0,0,.28)', overflow: 'hidden',
        }}>
          <div style={{ position: 'relative', padding: 8, borderBottom: '1px solid var(--border)' }}>
            <Search size={13} color="var(--text-3)" style={{ position: 'absolute', left: 18, top: '50%', transform: 'translateY(-50%)' }}/>
            <input ref={inputRef} className="input" value={query} onChange={e => setQuery(e.target.value)}
              placeholder={t('notifications.filter_types')} style={{ paddingLeft: 28 }}/>
          </div>
          <div style={{ maxHeight: 320, overflowY: 'auto', padding: 4 }}>
            {matches.map(s => {
              const Icon = s.icon;
              const active = s.id === value;
              return (
                <button type="button" key={s.id}
                  onClick={() => { onSelect(s.id); setQuery(''); setOpen(false); }}
                  style={{
                    display: 'flex', alignItems: 'flex-start', gap: 10, width: '100%',
                    padding: '8px 10px', borderRadius: 7, border: 'none', cursor: 'pointer',
                    background: active ? 'var(--accent-soft)' : 'transparent', textAlign: 'left',
                  }}
                  onMouseEnter={e => { if (!active) e.currentTarget.style.background = 'var(--surface-2)'; }}
                  onMouseLeave={e => { if (!active) e.currentTarget.style.background = 'transparent'; }}>
                  <Icon size={15} color={active ? 'var(--accent-2)' : 'var(--text-2)'} style={{ marginTop: 1, flexShrink: 0 }}/>
                  <span style={{ minWidth: 0 }}>
                    <span style={{ fontSize: 13, fontWeight: 500, color: 'var(--text)' }}>{s.name}</span>
                    <span style={{ display: 'block', fontSize: 11, color: 'var(--text-3)', lineHeight: 1.3, marginTop: 1 }}>{s.hint}</span>
                  </span>
                  {active && <Check size={14} color="var(--accent-2)" style={{ marginLeft: 'auto', flexShrink: 0 }}/>}
                </button>
              );
            })}
            {matches.length === 0 && (
              <div style={{ padding: 16, textAlign: 'center', fontSize: 12, color: 'var(--text-3)' }}>
                No channel type matches “{query}”.
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

// Tag chips for a notification channel — drives tag-based auto-routing.
function ChannelTagEditor({ channelId, allTags }) {
  const [tagIds, setTagIds] = useState(null);  // null = loading
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    api.routing.channelTags(channelId)
      .then(ids => { if (live) setTagIds(ids); })
      .catch(() => { if (live) setTagIds([]); });
    return () => { live = false; };
  }, [channelId]);

  const toggle = async (tagId) => {
    setBusy(true);
    try {
      if (tagIds.includes(tagId)) { await api.routing.delChannelTag(channelId, tagId); setTagIds(ids => ids.filter(x => x !== tagId)); }
      else { await api.routing.addChannelTag(channelId, tagId); setTagIds(ids => [...ids, tagId]); }
    } catch (e) { toast(e.message, 'error'); } finally { setBusy(false); }
  };

  if (tagIds === null) return <span style={{ fontSize: 12, color: 'var(--text-3)' }}>…</span>;
  const byId = new Map(allTags.map(t => [t.id, t]));
  const avail = allTags.filter(t => !tagIds.includes(t.id));

  return (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
      {tagIds.length === 0 && <span style={{ fontSize: 12, color: 'var(--text-3)' }}>No tags. </span>}
      {tagIds.map(id => {
        const t = byId.get(id);
        return (
          <span key={id} style={{
            display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 11, fontWeight: 500,
            padding: '3px 4px 3px 9px', borderRadius: 999, background: t?.color || '#888', color: '#fff',
          }}>
            {t?.name || id.slice(0, 8)}
            <button disabled={busy} onClick={() => toggle(id)} style={{
              background: 'rgba(255,255,255,.25)', border: 'none', color: '#fff', borderRadius: '50%',
              width: 15, height: 15, cursor: 'pointer', display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            }}><X size={9}/></button>
          </span>
        );
      })}
      {avail.length > 0 && (
        <select className="select" style={{ width: 'auto', padding: '4px 8px', fontSize: 12 }} disabled={busy}
          value="" onChange={e => e.target.value && toggle(e.target.value)}>
          <option value="">{t('notif.opt.add_tag')}</option>
          {avail.map(t => <option key={t.id} value={t.id}>{t.name}</option>)}
        </select>
      )}
      {allTags.length === 0 && <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>Create tags on a monitor first.</span>}
    </div>
  );
}
