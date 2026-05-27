import React, { useState } from 'react';
import {
  Bell, Plus, Trash2, Send, ChevronLeft, MessageSquare, Hash, Mail,
  Webhook as WebhookIcon, AlertCircle, Loader2, Smartphone, Server, Megaphone,
  Siren, Phone, Rocket, Layers, FileText, Edit3, Save, X,
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
  const [kind,    setKind]    = useState('slack');
  const [name,    setName]    = useState('');
  const [config,  setConfig]  = useState({});
  const [templateId, setTemplateId] = useState('');
  const [busy,    setBusy]    = useState(false);
  const [msg,     setMsg]     = useState(null);

  const reload = async () => {
    // useApi doesn't expose a refetch; bounce the hash to nothing visible
    // and back. Simpler: just reload the page once after add/delete.
    window.location.reload();
  };

  const submit = async (e) => {
    e?.preventDefault?.();
    setMsg(null);
    if (!name.trim()) { setMsg({ kind: 'err', text: 'Name is required.' }); return; }
    setBusy(true);
    try {
      await api.notifications.create(kind, name.trim(), config, templateId || null);
      setMsg({ kind: 'ok', text: 'Channel added. Reloading…' });
      setTimeout(reload, 400);
    } catch (e2) {
      setMsg({ kind: 'err', text: e2.message || 'Failed to create channel.' });
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
          <button className="btn btn-accent" style={{ marginLeft: 'auto' }} onClick={() => setShowAdd(s => !s)}>
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
            <h3 style={{ fontSize: 14, fontWeight: 600, margin: '0 0 14px' }}>Add a new channel</h3>
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

              {msg && <div className={msg.kind === 'ok' ? 'banner-ok' : 'banner-err'} style={{ marginBottom: 12 }}>{msg.text}</div>}

              <div style={{ display: 'flex', gap: 8 }}>
                <button className="btn btn-accent" type="submit" disabled={busy}>
                  {busy ? <><Loader2 size={13} className="spin"/> Saving…</> : <><Plus size={13}/> Save channel</>}
                </button>
                <button className="btn" type="button" onClick={() => setShowAdd(false)}>Cancel</button>
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
