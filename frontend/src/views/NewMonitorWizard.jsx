import React, { useState } from 'react';
import {
  Globe, Search, Server, Radio, Hash, Zap, Lock, Database,
  Box, Gamepad2, MessageSquare, Shield, FileSearch,
  ChevronLeft, ChevronRight, Check, X, Plus,
  Bell, Clock, Code, Activity,
} from 'lucide-react';
import { api, useApi } from '../lib/api.js';
import { t } from '../lib/i18n.js';

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
  .tabular { font-variant-numeric: tabular-nums; }

  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 8px 14px; border-radius: 8px; cursor: pointer;
    font-size: 13px; font-weight: 500; line-height: 1;
    background: var(--surface); border: 1px solid var(--border); color: var(--text-2);
    transition: all .12s; font-family: inherit;
  }
  .btn:hover { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .btn:disabled { opacity: .4; cursor: not-allowed; }
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-ghost { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }

  .type-card {
    padding: 14px 14px; border: 1px solid var(--border);
    border-radius: 10px; background: var(--surface); cursor: pointer;
    transition: all .15s; position: relative;
    display: flex; flex-direction: column; gap: 6px;
  }
  .type-card:hover { border-color: var(--border-2); transform: translateY(-1px); box-shadow: 0 4px 12px rgba(0,0,0,.04); }
  .type-card.active { border-color: var(--accent); background: var(--accent-soft); box-shadow: 0 0 0 3px rgba(20,184,166,.1); }
  .type-card .badge {
    position: absolute; top: 8px; right: 8px;
    font-size: 9px; font-weight: 600; padding: 2px 6px;
    border-radius: 4px; letter-spacing: .04em;
    text-transform: uppercase;
  }
  .badge-popular { background: var(--warn-soft); color: #b45309; }
  .badge-new { background: var(--accent-soft); color: #0d9488; }
  .badge-stub { background: var(--surface-2); color: var(--text-3); }

  .steps { display: flex; align-items: center; gap: 8px; }
  .step { display: flex; align-items: center; gap: 8px; }
  .step-num {
    width: 24px; height: 24px; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    font-size: 12px; font-weight: 600;
    background: var(--surface-2); color: var(--text-3); border: 1px solid var(--border);
  }
  .step.active .step-num { background: var(--accent); color: white; border-color: var(--accent); }
  .step.done .step-num { background: var(--surface); color: var(--accent); border-color: var(--accent); }
  .step-line { width: 28px; height: 1px; background: var(--border); }

  .field { margin-bottom: 16px; }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); margin-bottom: 6px; display: flex; align-items: center; gap: 6px; }
  .field-hint { font-size: 11.5px; color: var(--text-3); margin-top: 6px; line-height: 1.5; }
  .input, .select {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none;
    font-family: inherit;
  }
  .input.mono { font-family: 'JetBrains Mono', monospace; font-size: 12.5px; }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }

  .toggle { width: 36px; height: 20px; border-radius: 10px; background: var(--border-2); position: relative; cursor: pointer; transition: background .15s; flex-shrink: 0; }
  .toggle::after { content:''; position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: white; transition: all .15s; box-shadow: 0 1px 2px rgba(0,0,0,.1); }
  .toggle.on { background: var(--accent); }
  .toggle.on::after { left: 18px; }

  .banner {
    padding: 10px 14px; border-radius: 8px; font-size: 13px;
    display: flex; align-items: center; gap: 8px;
  }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; }
  .banner-warn { background: var(--warn-soft); color: #92400e; border: 1px solid #fde68a; }
`;

// ── all 20 monitor types ──────────────────────────────────────────────────
// `stub` flag: backend's probe runner returns Down("not yet implemented") for
// these kinds. We still let the user create one — it just won't probe.
//
// `example` is the real-life "why would I use this" — shown on the type
// card and as a callout in step 2 so users see a concrete use case before
// filling in fields.
const types = [
  { id: 'http',       icon: Globe,         name: 'HTTP / HTTPS',  desc: 'Status code check', badge: 'popular',
    example: "Watch a website or your service's /health endpoint",
    placeholder: { url: 'https://api.example.com/health' } },

  { id: 'keyword',    icon: Search,        name: 'Keyword',       desc: 'Response body contains string',
    example: 'Catch when an upstream status page goes red',
    placeholder: { url: 'https://status.upstream.com', keyword: 'operational' } },

  { id: 'json_query', icon: FileSearch,    name: 'JSON query',    desc: 'JSONPath assertion on body', badge: 'new',
    example: 'Assert your API returns {"status": "ok"}',
    placeholder: { url: 'https://api.example.com/health', jsonPath: 'status', jsonExpected: 'ok' } },

  { id: 'browser',    icon: Globe,         name: 'Browser',       desc: 'Headless render → keyword', badge: 'new',
    example: 'Catch SPA pages that look fine to curl but render an error',
    placeholder: { url: 'https://app.example.com', keyword: 'Dashboard' } },

  { id: 'tcp',        icon: Server,        name: 'TCP port',      desc: 'Raw socket connect', badge: 'popular',
    example: 'Verify a port is open (DB, Redis, MQTT broker)',
    placeholder: { hostname: 'db.internal', port: '5432' } },

  { id: 'ssh',        icon: Server,        name: 'SSH',           desc: 'Connect + check SSH- banner',
    example: 'Confirm a bastion / git host is accepting SSH',
    placeholder: { hostname: 'bastion.internal', port: '22' } },

  { id: 'smtp',       icon: Server,        name: 'SMTP',          desc: 'Connect + check 220 greeting',
    example: 'Make sure your mail relay still answers',
    placeholder: { hostname: 'mail.internal', port: '25' } },

  { id: 'imap',       icon: Server,        name: 'IMAP',          desc: 'Connect + check * OK greeting',
    example: 'Watch your IMAP server availability',
    placeholder: { hostname: 'imap.internal', port: '143' } },

  { id: 'ftp',        icon: Server,        name: 'FTP',           desc: 'Connect + check 220 greeting',
    example: 'Confirm an FTP drop site still answers',
    placeholder: { hostname: 'ftp.internal', port: '21' } },

  { id: 'pop3',       icon: Server,        name: 'POP3',          desc: 'Connect + check +OK greeting',
    example: 'Watch a POP3 mailbox server',
    placeholder: { hostname: 'pop.internal', port: '110' } },

  { id: 'ping',       icon: Radio,         name: 'Ping',          desc: 'ICMP echo',
    example: 'Detect when your home router or VPN endpoint drops',
    placeholder: { hostname: '192.168.1.1' } },

  { id: 'dns',        icon: Hash,          name: 'DNS',           desc: 'Resolve, expect record',
    example: 'Catch DNS hijack on your own domain',
    placeholder: { hostname: 'example.com' } },

  { id: 'push',       icon: Zap,           name: 'Push',          desc: 'Inbound heartbeat',
    example: "Confirm your nightly backup or cron job actually ran",
    placeholder: {} },

  { id: 'grpc',       icon: Zap,           name: 'gRPC',          desc: 'grpc.health.v1',
    example: 'Health-check a gRPC service via the standard protocol',
    placeholder: { hostname: 'grpc.example.com', port: '443' } },

  { id: 'tls',        icon: Lock,          name: 'TLS cert',      desc: 'Expiry + chain',
    example: 'Get alerted 30 days before your cert expires',
    placeholder: { url: 'https://example.com' } },

  { id: 'docker',     icon: Box,           name: 'Docker',        desc: 'Container running',
    example: 'Detect when your Plex/Jellyfin container crashes',
    placeholder: { container: 'plex-server' } },

  { id: 'steam',      icon: Gamepad2,      name: 'Steam',         desc: 'A2S_INFO query',
    example: 'Watch your group\'s Counter-Strike / Valheim server',
    placeholder: { hostname: 'csgo.example.com', port: '27015' } },

  { id: 'mqtt',       icon: MessageSquare, name: 'MQTT',          desc: 'Broker CONNECT',
    example: 'Detect when your MQTT broker stops accepting connections',
    placeholder: { hostname: 'mqtt.iot.local', port: '1883' } },

  { id: 'radius',     icon: Shield,        name: 'RADIUS',        desc: 'Access-Request',
    example: "Make sure your office VPN's RADIUS auth still works",
    placeholder: { hostname: 'radius.internal', port: '1812' } },

  { id: 'kafka',      icon: Zap,           name: 'Kafka',         desc: 'ApiVersions handshake',
    example: 'Verify brokers are reachable before your producer starts dropping',
    placeholder: { hostname: 'kafka.internal', port: '9092' } },

  { id: 'postgres',   icon: Database,      name: 'Postgres',      desc: 'SELECT 1',
    example: 'Catch when your primary DB stops accepting connections',
    placeholder: { hostname: 'db.internal', port: '5432' } },

  { id: 'mysql',      icon: Database,      name: 'MySQL',         desc: 'SELECT 1',
    example: 'Same as Postgres but for MySQL / MariaDB',
    placeholder: { hostname: 'db.internal', port: '3306' } },

  { id: 'mssql',      icon: Database,      name: 'MSSQL',         desc: 'SELECT 1',
    example: 'SQL Server availability check',
    placeholder: { hostname: 'db.internal', port: '1433' } },

  { id: 'redis',      icon: Database,      name: 'Redis',         desc: 'PING',
    example: "More reliable than a raw TCP probe because it tests AUTH too",
    placeholder: { hostname: 'redis.internal', port: '6379' } },

  { id: 'mongodb',    icon: Database,      name: 'MongoDB',       desc: 'ping',
    example: 'Detect MongoDB primary outages and replica-set failover',
    placeholder: { hostname: 'mongo.internal', port: '27017' } },

  { id: 'memcached',  icon: Database,      name: 'Memcached',     desc: 'version handshake',
    example: 'Confirm a memcached node accepts the text protocol',
    placeholder: { hostname: 'memcached.internal', port: '11211' } },

  { id: 'ntp',        icon: Radio,         name: 'NTP',           desc: 'SNTPv4 client packet',
    example: 'Confirm your time source is reachable and synchronised',
    placeholder: { hostname: 'pool.ntp.org', port: '123' } },

  { id: 'websocket',  icon: Activity,      name: 'WebSocket',     desc: 'RFC 6455 handshake',
    example: 'Verify a ws:// or wss:// endpoint completes its upgrade',
    placeholder: { url: 'wss://echo.websocket.org' } },

  { id: 'nats',       icon: Radio,         name: 'NATS',          desc: 'Connect + handshake + flush',
    example: 'Confirm a NATS server is accepting clients and round-trips PING/PONG',
    placeholder: { url: 'nats://nats.internal:4222' } },

  { id: 'ldap',       icon: Lock,          name: 'LDAP',          desc: 'Connect + simple bind',
    example: 'Verify your directory server is reachable and an account still authenticates',
    placeholder: { url: 'ldap://ldap.internal:389' } },

  { id: 'amqp',       icon: Radio,         name: 'AMQP',          desc: 'RabbitMQ-compatible handshake',
    example: 'Confirm a RabbitMQ broker is reachable, authenticated, and serving the vhost',
    placeholder: { url: 'amqp://guest:guest@rabbitmq.internal:5672/%2F' } },

  { id: 'doh',        icon: Globe,         name: 'DNS-over-HTTPS', desc: 'RFC 8484 JSON query',
    example: 'Probe a DoH endpoint (Cloudflare, Google, NextDNS) and confirm it answers',
    placeholder: { url: 'https://cloudflare-dns.com/dns-query' } },

  { id: 'rdap',       icon: Globe,         name: 'RDAP',          desc: 'HTTP successor to WHOIS',
    example: 'Modern domain-registration lookup with structured days-until-expiry',
    placeholder: { url: 'https://rdap.org/domain' } },

  { id: 'snmp',       icon: Server,        name: 'SNMP',          desc: 'v2c GET against an OID',
    example: 'Confirm a network device (switch / router / UPS / printer) is replying to SNMP',
    placeholder: { hostname: 'switch.internal', port: '161' } },

  { id: 'cassandra',  icon: Database,      name: 'Cassandra',     desc: 'CQL handshake + system query',
    example: 'Check a Cassandra / ScyllaDB node is past the CQL handshake and answering queries',
    placeholder: { hostname: 'cassandra.internal', port: '9042' } },

  { id: 'mdns',       icon: Radio,         name: 'mDNS',          desc: 'Multicast service discovery',
    example: 'Confirm at least one peer is advertising mDNS services on the link (LAN homelab probe)',
    placeholder: {} },

  { id: 'ssdp',       icon: Radio,         name: 'SSDP / UPnP',   desc: 'M-SEARCH discovery',
    example: 'Confirm at least one UPnP device (TV, NAS, router) responds on the LAN',
    placeholder: {} },

  { id: 'domain',     icon: Globe,         name: 'Domain expiry', desc: 'WHOIS lookup',
    example: 'Reminder 60 days before your domain registration lapses',
    placeholder: { url: 'example.com' } },
];

// per-kind: which fields the form needs
const fieldsFor = (kind) => {
  const httpKinds = ['http','keyword','json_query'];
  if (httpKinds.includes(kind)) {
    return {
      url: true, method: true, statuses: true, httpExtras: true,
      keyword:  kind === 'keyword',
      jsonPath: kind === 'json_query',
    };
  }
  if (kind === 'browser') {
    // Reuses the existing url + keyword inputs and adds a renderer_url.
    return { url: true, keyword: true, renderer: true };
  }
  if (['ssh','smtp','imap','ftp','pop3'].includes(kind)) return { hostname: true, port: true, banner: true };
  if (['tcp','grpc','mqtt','steam','kafka','radius','ntp'].includes(kind)) return { hostname: true, port: true };
  if (['postgres','mysql','mssql','redis','mongodb','memcached'].includes(kind)) return { hostname: true, port: true };
  if (kind === 'ping')   return { hostname: true };
  if (kind === 'dns')    return { hostname: true, dns: true };
  if (kind === 'tls')    return { url: true };
  if (kind === 'domain') return { url: true };
  if (kind === 'websocket') return { url: true };
  if (kind === 'nats')      return { url: true };
  if (kind === 'ldap')      return { url: true };
  if (kind === 'amqp')      return { url: true };
  if (kind === 'doh')       return { url: true };
  if (kind === 'rdap')      return { url: true };
  if (kind === 'snmp')      return { hostname: true, port: true };
  if (kind === 'cassandra') return { hostname: true, port: true };
  if (kind === 'mdns')      return {};
  if (kind === 'ssdp')      return {};
  return {};
};

const DNS_RECORD_TYPES = ['A','AAAA','CNAME','MX','TXT','NS','SRV','CAA','SOA'];

const defaultPort = (kind) => ({
  tcp: 443, grpc: 443, mqtt: 1883, steam: 27015, kafka: 9092, radius: 1812,
  ssh: 22, smtp: 25, imap: 143, ftp: 21, pop3: 110,
  postgres: 5432, mysql: 3306, mssql: 1433, redis: 6379, mongodb: 27017, memcached: 11211, ntp: 123,
  snmp: 161, cassandra: 9042,
})[kind] || null;

// ── main ──────────────────────────────────────────────────────────────────
export default function NewMonitorWizard() {
  const [step, setStep] = useState(1);
  const [type, setType] = useState('http');

  const [name, setName] = useState('');
  const [url, setUrl]   = useState('https://');
  const [method, setMethod] = useState('GET');
  const [statuses, setStatuses] = useState('200, 201, 204');
  const [expectedHttpVersion, setExpectedHttpVersion] = useState('');
  const [hostname, setHostname] = useState('');
  const [port, setPort] = useState('');
  const [keyword, setKeyword] = useState('');
  const [jsonPath, setJsonPath] = useState('');
  const [jsonExpected, setJsonExpected] = useState('');
  const [rendererUrl, setRendererUrl] = useState('http://browserless:3000/content');
  const [dnsRecordType, setDnsRecordType] = useState('A');
  const [dnsResolver,   setDnsResolver]   = useState('');
  const [dnsExpected,   setDnsExpected]   = useState('');
  const [bannerExpect,  setBannerExpect]  = useState('');

  const [intervalSec, setIntervalSec] = useState('60');
  const [timeoutSec,  setTimeoutSec]  = useState('10');
  const [retries,     setRetries]     = useState(0);
  const [backoffStrategy, setBackoffStrategy] = useState('none');
  const [backoffBase,     setBackoffBase]     = useState('5');
  const [upsideDown,  setUpsideDown]  = useState(false);
  const [followRedir, setFollowRedir] = useState(true);
  const [ignoreTls,   setIgnoreTls]   = useState(false);
  const [proxyId,     setProxyId]     = useState('');

  // HTTP request headers as raw "Name: value" lines, one per line. Kept as
  // a textarea string (not a parsed map) so operators can paste a block;
  // parsed to a { name: value } object only at submit / preset-save time.
  const [headersText, setHeadersText] = useState('');

  // Available proxies for the picker. Polled rarely — these don't change
  // mid-flow. Falls back to [] on error so the form still renders.
  const proxiesState = useApi(() => api.proxies.list(), []);
  const proxies = proxiesState.data || [];

  // Reusable config presets (saved header sets / TLS posture). Bumped via
  // presetReload after a save so a freshly-created preset shows up without
  // a full page reload.
  const [presetReload, setPresetReload] = useState(0);
  const presetsState = useApi(() => api.monitorPresets.list(), [presetReload]);
  const presets = presetsState.data || [];
  const [savingPreset, setSavingPreset] = useState(false);

  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState(null);

  // Channels picked on step 3 — attached to the monitor after create.
  const channelsState = useApi(() => api.notifications.list(), []);
  const channels = channelsState.data || [];
  const [selectedChannels, setSelectedChannels] = useState(new Set());
  const toggleChannel = (id) => {
    setSelectedChannels(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const meta   = types.find(tp => tp.id === type);
  const fields = fieldsFor(type);

  const onPickType = (newType) => {
    setType(newType);
    // sensible default for port when switching to a kind that has one
    const dp = defaultPort(newType);
    if (dp && !port) setPort(String(dp));
  };

  const cancel = () => { window.location.hash = '#/'; };

  // Parse the "Name: value" textarea into a { name: value } object.
  // Blank lines and lines without a colon are skipped; the first colon
  // splits name from value so values containing colons survive.
  const parseHeaders = (text) => {
    const out = {};
    for (const line of text.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      const idx = trimmed.indexOf(':');
      if (idx <= 0) continue;
      const name = trimmed.slice(0, idx).trim();
      const value = trimmed.slice(idx + 1).trim();
      if (name) out[name] = value;
    }
    return out;
  };

  // Serialize a { name: value } map back into textarea lines.
  const headersToText = (map) =>
    Object.entries(map || {}).map(([k, v]) => `${k}: ${v}`).join('\n');

  // Apply a chosen preset: header presets fill the headers textarea, TLS
  // presets flip the ignore_tls toggle. No-op on an empty selection.
  const applyPreset = (presetId) => {
    const p = presets.find(x => x.id === presetId);
    if (!p) return;
    if (p.kind === 'http_headers') {
      setHeadersText(headersToText(p.data?.headers));
    } else if (p.kind === 'tls') {
      setIgnoreTls(!!p.data?.ignore_tls);
    }
  };

  // Save the headers currently typed into the wizard as a new reusable
  // preset the operator can apply on future monitors.
  const saveHeadersPreset = async () => {
    const headers = parseHeaders(headersText);
    if (Object.keys(headers).length === 0) {
      setErr(t('wizard.preset.empty'));
      return;
    }
    const name = prompt(t('wizard.preset.name_prompt'));
    if (!name || !name.trim()) return;
    setSavingPreset(true);
    try {
      await api.monitorPresets.create({ name: name.trim(), kind: 'http_headers', data: { headers } });
      setPresetReload(n => n + 1);
    } catch (e) {
      setErr(e.message || t('wizard.preset.save_failed'));
    } finally {
      setSavingPreset(false);
    }
  };

  const buildPayload = () => {
    const acceptedStatuses = statuses
      .split(',')
      .map(s => parseInt(s.trim(), 10))
      .filter(n => Number.isFinite(n));

    const config = {};
    if (fields.keyword  && keyword)  config.keyword = keyword;
    if (fields.renderer && rendererUrl) config.renderer_url = rendererUrl;
    if (fields.jsonPath && jsonPath) {
      config.json_path = jsonPath;
      if (jsonExpected) config.expected_value = jsonExpected;
    }
    if (fields.dns) {
      if (dnsRecordType && dnsRecordType !== 'A') config.record_type = dnsRecordType;
      if (dnsResolver.trim()) config.resolver = dnsResolver.trim();
      if (dnsExpected.trim()) config.expected = dnsExpected.trim();
    }
    if (fields.banner && bannerExpect.trim()) {
      config.expect = bannerExpect.trim();
    }
    // Optional negotiated-HTTP-version assertion (http1/http2/http3). Stored
    // in the freeform config JSONB; the rampart-checker HTTP probe reads it.
    if (fields.httpExtras && expectedHttpVersion) {
      config.expected_http_version = expectedHttpVersion;
    }
    // Optional retry-backoff curve. Only attach when the operator picked a
    // growth strategy — leaving it off keeps the scheduler's historical
    // (no inter-retry sleep) timing untouched.
    if (backoffStrategy !== 'none') {
      config.retry_backoff = {
        strategy:  backoffStrategy,
        base_secs: Math.max(1, parseInt(backoffBase, 10) || 5),
        max_secs:  60,
      };
    }

    const payload = {
      name: name.trim(),
      kind: type,
      interval_seconds: parseInt(intervalSec, 10) || 60,
      timeout_seconds:  parseInt(timeoutSec, 10)  || 10,
      max_retries:      Math.max(0, parseInt(retries, 10) || 0),
      upside_down:      upsideDown,
      config:           Object.keys(config).length ? config : undefined,
    };
    // HTTP-family extras are noise on every other kind; only attach them
    // when the form actually rendered the HTTP inputs.
    if (fields.httpExtras) {
      payload.follow_redirect  = followRedir;
      payload.http_method      = method;
      payload.accepted_statuses = acceptedStatuses.length ? acceptedStatuses : undefined;
      payload.ignore_tls       = ignoreTls;
      const headers = parseHeaders(headersText);
      if (Object.keys(headers).length) payload.http_headers = headers;
    }
    if (fields.url      && url)      payload.url      = url;
    if (fields.hostname && hostname) payload.hostname = hostname;
    if (fields.port     && port)     payload.port     = parseInt(port, 10);
    // Proxy only meaningful for HTTP-family kinds. Backend will ignore
    // it on other kinds anyway, but we don't send it.
    if (['http', 'keyword', 'json_query'].includes(type) && proxyId) {
      payload.proxy_id = proxyId;
    }
    // Drop undefined keys so the server defaults kick in.
    Object.keys(payload).forEach(k => payload[k] === undefined && delete payload[k]);
    return payload;
  };

  const submit = async () => {
    setErr(null);
    if (!name.trim()) { setErr(t('wizard.err_name')); return; }
    if (fields.url      && !url.trim())      { setErr(t('wizard.err_url')); return; }
    if (fields.hostname && !hostname.trim()) { setErr(t('wizard.err_hostname')); return; }
    setSubmitting(true);
    try {
      const created = await api.monitors.create(buildPayload());

      // Attach selected channels. Failures here aren't fatal — the monitor
      // exists; user can attach manually from the detail view. We surface
      // a soft warning instead of blocking.
      const attachErrors = [];
      for (const nid of selectedChannels) {
        try { await api.notifications.attach(created.id, nid); }
        catch (e) { attachErrors.push(e.message); }
      }
      if (attachErrors.length > 0) {
        // Land on the detail page either way; the user can retry attaching
        // from the sidebar card if needed.
        console.warn('Some channel attachments failed:', attachErrors);
      }

      window.location.hash = `#/monitor/${created.id}`;
    } catch (e) {
      setErr(e.message || t('wizard.err_create'));
      setSubmitting(false);
    }
  };

  const previewPayload = JSON.stringify(buildPayload(), null, 2);

  return (
    <div className="rampart">
      <style>{css}</style>

      {/* top bar */}
      <header style={{
        display: 'flex', alignItems: 'center', gap: 24,
        padding: '14px 24px', borderBottom: '1px solid var(--border)',
        background: 'var(--surface)', position: 'sticky', top: 0, zIndex: 10
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <ChevronLeft size={16} color="var(--text-2)" style={{ cursor: 'pointer' }} onClick={cancel}/>
          <a href="#/" style={{ fontSize: 14, color: 'var(--text-3)', textDecoration: 'none' }}>{t('wizard.breadcrumb')}</a>
          <span style={{ fontSize: 14, fontWeight: 500 }}>{t('wizard.new_monitor')}</span>
        </div>

        <div className="steps" style={{ marginLeft: 'auto' }}>
          {[
            { n: 1, label: t('wizard.step.type') },
            { n: 2, label: t('wizard.step.configure') },
            { n: 3, label: t('wizard.step.schedule') },
          ].map((s, i, arr) => (
            <React.Fragment key={s.n}>
              <div className={`step ${step === s.n ? 'active' : step > s.n ? 'done' : ''}`}>
                <span className="step-num">{step > s.n ? <Check size={12}/> : s.n}</span>
                <span style={{ fontSize: 12, color: step >= s.n ? 'var(--text)' : 'var(--text-3)', fontWeight: 500 }}>
                  {s.label}
                </span>
              </div>
              {i < arr.length - 1 && <div className="step-line"/>}
            </React.Fragment>
          ))}
        </div>

        <button className="btn" onClick={cancel}><X size={13}/> {t('common.cancel')}</button>
      </header>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 360px', minHeight: 'calc(100vh - 65px)' }}>
        {/* MAIN */}
        <div style={{ padding: '36px 48px', borderRight: '1px solid var(--border)' }}>

          {/* STEP 1: PICK TYPE */}
          {step === 1 && (
            <>
              <div style={{ marginBottom: 28 }}>
                <p style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em', margin: '0 0 8px' }}>{t('wizard.step1.eyebrow')}</p>
                <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 8px', letterSpacing: '-.02em' }}>{t('wizard.step1.title')}</h1>
                <p style={{ fontSize: 14, color: 'var(--text-2)', margin: 0 }}>
                  {t('wizard.step1.blurb')}
                </p>
              </div>

              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 10 }}>
                {types.map(tp => {
                  const Icon = tp.icon;
                  return (
                    <div key={tp.id} className={`type-card ${type === tp.id ? 'active' : ''}`} onClick={() => onPickType(tp.id)}>
                      {tp.badge && <span className={`badge badge-${tp.badge}`}>{tp.badge}</span>}
                      {!tp.badge && tp.stub && <span className="badge badge-stub">stub</span>}
                      <Icon size={18} color={type === tp.id ? 'var(--accent-2)' : 'var(--text-2)'} strokeWidth={1.75}/>
                      <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--text)', marginTop: 4 }}>{tp.name}</div>
                      <div style={{ fontSize: 11.5, color: 'var(--text-3)', lineHeight: 1.4 }}>{tp.desc}</div>
                      {tp.example && (
                        <div style={{ fontSize: 11, color: 'var(--text-2)', lineHeight: 1.45, marginTop: 6, fontStyle: 'italic', borderTop: '1px solid var(--border)', paddingTop: 6 }}>
                          {tp.example}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </>
          )}

          {/* STEP 2: CONFIGURE */}
          {step === 2 && (
            <>
              <div style={{ marginBottom: 28 }}>
                <p style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em', margin: '0 0 8px' }}>{t('wizard.step2.eyebrow')}</p>
                <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 8px', letterSpacing: '-.02em' }}>{t('wizard.step2.title')}</h1>
                <p style={{ fontSize: 14, color: 'var(--text-2)', margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
                  <meta.icon size={14} color="var(--text-3)"/> {meta.name} · {meta.desc}
                </p>
                {meta.example && (
                  <div style={{
                    marginTop: 14, padding: '10px 14px',
                    background: 'var(--accent-soft)', border: '1px solid #99f6e4',
                    borderRadius: 8, fontSize: 13, color: 'var(--text-2)',
                    display: 'flex', alignItems: 'center', gap: 8,
                  }}>
                    <span style={{ fontWeight: 600, color: 'var(--accent-2)', textTransform: 'uppercase', letterSpacing: '.05em', fontSize: 10 }}>{t('wizard.example')}</span>
                    <span>{meta.example}</span>
                  </div>
                )}
                {meta.stub && (
                  <div className="banner banner-warn" style={{ marginTop: 14 }}>
                    {t('wizard.stub_warning', { kind: type })}
                  </div>
                )}
              </div>

              <div style={{ maxWidth: 580 }}>
                <div className="field">
                  <label className="field-label">{t('wizard.field.display_name')}</label>
                  <input className="input" value={name} onChange={e => setName(e.target.value)} placeholder="api.example.com"/>
                  <div className="field-hint">{t('wizard.field.display_name_hint')}</div>
                </div>

                {fields.url && (
                  <div className="field">
                    <label className="field-label">{t('wizard.field.url')}</label>
                    <input className="input mono" value={url} onChange={e => setUrl(e.target.value)} placeholder={meta.placeholder?.url || 'https://example.com/health'}/>
                  </div>
                )}

                {fields.method && (
                  <div style={{ display: 'grid', gridTemplateColumns: '1fr 2fr', gap: 12 }}>
                    <div className="field">
                      <label className="field-label">{t('wizard.field.http_method')}</label>
                      <select className="select" value={method} onChange={e => setMethod(e.target.value)}>
                        <option>GET</option><option>POST</option><option>PUT</option>
                        <option>PATCH</option><option>DELETE</option><option>HEAD</option>
                      </select>
                    </div>
                    <div className="field">
                      <label className="field-label">{t('wizard.field.accepted_statuses')}</label>
                      <input className="input mono" value={statuses} onChange={e => setStatuses(e.target.value)} placeholder="200, 201, 204"/>
                    </div>
                  </div>
                )}

                {fields.httpExtras && (
                  <div className="field">
                    <label className="field-label">{t('wizard.field.expected_http_version')}</label>
                    <select className="select" value={expectedHttpVersion}
                      onChange={e => setExpectedHttpVersion(e.target.value)}>
                      <option value="">{t('wizard.field.expected_http_version_any')}</option>
                      <option value="http1">HTTP/1.1</option>
                      <option value="http2">HTTP/2</option>
                      <option value="http3">HTTP/3</option>
                    </select>
                    <div className="field-hint">{t('wizard.field.expected_http_version_hint')}</div>
                  </div>
                )}

                {fields.httpExtras && (
                  <div className="field">
                    <label className="field-label">{t('wizard.field.http_headers')}</label>
                    {/* Apply a saved preset to fill headers / TLS posture. */}
                    <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 6 }}>
                      <select className="select" style={{ flex: 1 }} value="__placeholder__"
                        disabled={presets.length === 0}
                        onChange={e => {
                          const v = e.target.value;
                          if (v === '__placeholder__') return;
                          applyPreset(v);
                          e.target.value = '__placeholder__';
                        }}>
                        <option value="__placeholder__">
                          {presets.length === 0 ? t('wizard.preset.none') : t('wizard.preset.apply')}
                        </option>
                        {presets.map(p => (
                          <option key={p.id} value={p.id}>
                            {p.name} · {p.kind === 'tls' ? t('wizard.preset.kind_tls') : t('wizard.preset.kind_headers')}
                          </option>
                        ))}
                      </select>
                      <button type="button" className="btn" disabled={savingPreset}
                        onClick={saveHeadersPreset}>
                        <Plus size={12}/> {t('wizard.preset.save')}
                      </button>
                    </div>
                    <textarea className="input mono" rows={3} value={headersText}
                      onChange={e => setHeadersText(e.target.value)}
                      placeholder={'Authorization: Bearer …\nX-Api-Version: 2'}/>
                    <div className="field-hint">{t('wizard.field.http_headers_hint')}</div>
                  </div>
                )}

                {fields.httpExtras && (
                  <label style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '4px 0 14px', fontSize: 13, cursor: 'pointer' }}>
                    <input type="checkbox" checked={ignoreTls} onChange={e => setIgnoreTls(e.target.checked)}/>
                    <span>
                      {t('wizard.field.ignore_tls')}
                      <span style={{ display: 'block', fontSize: 12, color: 'var(--text-3)' }}>{t('wizard.field.ignore_tls_hint')}</span>
                    </span>
                  </label>
                )}

                {fields.hostname && (
                  <div style={{ display: 'grid', gridTemplateColumns: fields.port ? '2fr 1fr' : '1fr', gap: 12 }}>
                    <div className="field">
                      <label className="field-label">{t('wizard.field.hostname')}</label>
                      <input className="input mono" value={hostname} onChange={e => setHostname(e.target.value)}
                        placeholder={meta.placeholder?.hostname || (type === 'ping' ? '8.8.8.8 or example.com' : 'db.internal')}/>
                    </div>
                    {fields.port && (
                      <div className="field">
                        <label className="field-label">{t('wizard.field.port')}</label>
                        <input className="input mono" value={port} onChange={e => setPort(e.target.value)}
                          placeholder={meta.placeholder?.port || String(defaultPort(type) ?? 443)}/>
                      </div>
                    )}
                  </div>
                )}

                {fields.keyword && (
                  <div className="field">
                    <label className="field-label">{t('wizard.field.keyword')}</label>
                    <input className="input mono" value={keyword} onChange={e => setKeyword(e.target.value)}
                      placeholder={meta.placeholder?.keyword || 'operational'}/>
                    <div className="field-hint">{t('wizard.field.keyword_hint')}</div>
                  </div>
                )}

                {fields.dns && (
                  <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
                    <div className="field">
                      <label className="field-label">{t('wizard.field.record_type')}</label>
                      <select className="select" value={dnsRecordType} onChange={e => setDnsRecordType(e.target.value)}>
                        {DNS_RECORD_TYPES.map(rt => <option key={rt}>{rt}</option>)}
                      </select>
                    </div>
                    <div className="field">
                      <label className="field-label">{t('wizard.field.resolver')}</label>
                      <input className="input mono" value={dnsResolver} onChange={e => setDnsResolver(e.target.value)}
                        placeholder="1.1.1.1"/>
                      <div className="field-hint">{t('wizard.field.resolver_hint')}</div>
                    </div>
                    <div className="field" style={{ gridColumn: '1 / -1' }}>
                      <label className="field-label">{t('wizard.field.dns_expected')}</label>
                      <input className="input mono" value={dnsExpected} onChange={e => setDnsExpected(e.target.value)}
                        placeholder="93.184.216.34"/>
                      <div className="field-hint">{t('wizard.field.dns_expected_hint')}</div>
                    </div>
                  </div>
                )}

                {fields.banner && (
                  <div className="field">
                    <label className="field-label">{t('wizard.field.banner')}</label>
                    <input className="input mono" value={bannerExpect} onChange={e => setBannerExpect(e.target.value)}
                      placeholder={
                        type === 'ssh' ? 'SSH-' :
                        type === 'smtp' ? '220' :
                        type === 'imap' ? '* OK' :
                        type === 'pop3' ? '+OK' :
                        type === 'ftp' ? '220' : ''
                      }/>
                    <div className="field-hint">
                      {t('wizard.field.banner_hint', { prefix:
                        type === 'ssh' ? '"SSH-"' :
                        type === 'smtp' || type === 'ftp' ? '"220"' :
                        type === 'imap' ? '"* OK"' :
                        type === 'pop3' ? '"+OK"' : t('wizard.field.banner_protocol_default') })}
                    </div>
                  </div>
                )}

                {fields.renderer && (
                  <div className="field">
                    <label className="field-label">{t('wizard.field.renderer')}</label>
                    <input className="input mono" value={rendererUrl} onChange={e => setRendererUrl(e.target.value)}
                      placeholder="http://browserless:3000/content"/>
                    <div className="field-hint">
                      {t('wizard.field.renderer_hint')}
                    </div>
                  </div>
                )}

                {fields.jsonPath && (
                  <div style={{ display: 'grid', gridTemplateColumns: '2fr 1fr', gap: 12 }}>
                    <div className="field">
                      <label className="field-label">{t('wizard.field.json_path')}</label>
                      <input className="input mono" value={jsonPath} onChange={e => setJsonPath(e.target.value)}
                        placeholder={meta.placeholder?.jsonPath || 'status.healthy'}/>
                      <div className="field-hint">{t('wizard.field.json_path_hint')}</div>
                    </div>
                    <div className="field">
                      <label className="field-label">{t('wizard.field.json_expected')}</label>
                      <input className="input mono" value={jsonExpected} onChange={e => setJsonExpected(e.target.value)}
                        placeholder={meta.placeholder?.jsonExpected || 'true'}/>
                    </div>
                  </div>
                )}
              </div>
            </>
          )}

          {/* STEP 3: SCHEDULE */}
          {step === 3 && (
            <>
              <div style={{ marginBottom: 28 }}>
                <p style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em', margin: '0 0 8px' }}>{t('wizard.step3.eyebrow')}</p>
                <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 8px', letterSpacing: '-.02em' }}>{t('wizard.step3.title')}</h1>
              </div>

              <div style={{ maxWidth: 580 }}>
                <div className="form-2col" style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
                  <div className="field">
                    <label className="field-label"><Clock size={12}/> {t('wizard.field.interval')}</label>
                    <select className="select" value={intervalSec} onChange={e => setIntervalSec(e.target.value)}>
                      <option value="10">{t('wizard.interval.10s')}</option>
                      <option value="30">{t('wizard.interval.30s')}</option>
                      <option value="60">{t('wizard.interval.1m')}</option>
                      <option value="300">{t('wizard.interval.5m')}</option>
                      <option value="1800">{t('wizard.interval.30m')}</option>
                      <option value="3600">{t('wizard.interval.1h')}</option>
                    </select>
                  </div>
                  <div className="field">
                    <label className="field-label">{t('wizard.field.timeout')}</label>
                    <select className="select" value={timeoutSec} onChange={e => setTimeoutSec(e.target.value)}>
                      <option value="5">{t('wizard.timeout.5s')}</option>
                      <option value="10">{t('wizard.timeout.10s')}</option>
                      <option value="16">{t('wizard.timeout.16s')}</option>
                      <option value="30">{t('wizard.timeout.30s')}</option>
                      <option value="60">{t('wizard.timeout.60s')}</option>
                    </select>
                  </div>
                </div>

                <div className="field">
                  <label className="field-label">{t('wizard.field.retries')}</label>
                  <input className="input mono" type="number" min="0" value={retries} onChange={e => setRetries(e.target.value)}/>
                  <div className="field-hint">{t('wizard.field.retries_hint')}</div>
                </div>

                <div className="form-2col" style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
                  <div className="field">
                    <label className="field-label">{t('wizard.field.retry_backoff')}</label>
                    <select className="select" value={backoffStrategy} onChange={e => setBackoffStrategy(e.target.value)}>
                      <option value="none">{t('wizard.backoff.none')}</option>
                      <option value="linear">{t('wizard.backoff.linear')}</option>
                      <option value="exponential">{t('wizard.backoff.exponential')}</option>
                    </select>
                    <div className="field-hint">{t('wizard.field.retry_backoff_hint')}</div>
                  </div>
                  {backoffStrategy !== 'none' && (
                    <div className="field">
                      <label className="field-label">{t('wizard.field.backoff_base')}</label>
                      <input className="input mono" type="number" min="1" value={backoffBase} onChange={e => setBackoffBase(e.target.value)}/>
                      <div className="field-hint">{t('wizard.field.backoff_base_hint')}</div>
                    </div>
                  )}
                </div>

                <div className="field">
                  <label className="field-label"><Bell size={12}/> {t('wizard.field.notifications')}</label>

                  {channelsState.loading && (
                    <div style={{ fontSize: 12, color: 'var(--text-3)' }}>{t('wizard.loading_channels')}</div>
                  )}

                  {!channelsState.loading && channels.length === 0 && (
                    <div className="banner banner-warn" style={{ marginBottom: 0 }}>
                      {t('wizard.no_channels')}{' '}
                      <a href="#/notifications" target="_blank" rel="noreferrer" style={{ color: '#92400e', fontWeight: 500 }}>
                        {t('wizard.add_channel_link')}
                      </a>
                      {' '}{t('wizard.no_channels_suffix')}
                    </div>
                  )}

                  {!channelsState.loading && channels.length > 0 && (
                    <>
                      <div className="field-hint" style={{ marginTop: 0, marginBottom: 8 }}>
                        {t('wizard.channels_hint')}
                      </div>
                      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                        {channels.map(c => {
                          const checked = selectedChannels.has(c.id);
                          return (
                            <label key={c.id} style={{
                              display: 'flex', alignItems: 'center', gap: 10,
                              padding: '9px 12px', border: '1px solid var(--border)',
                              borderRadius: 8, cursor: 'pointer',
                              background: checked ? 'var(--accent-soft)' : 'var(--surface)',
                              borderColor: checked ? 'var(--accent)' : 'var(--border)',
                            }}>
                              <input type="checkbox" checked={checked} onChange={() => toggleChannel(c.id)}/>
                              <span style={{ fontSize: 13, flex: 1 }}>{c.name}</span>
                              <span className="mono" style={{ fontSize: 10, color: 'var(--text-3)', textTransform: 'uppercase' }}>
                                {c.kind}
                              </span>
                            </label>
                          );
                        })}
                      </div>
                      <a href="#/notifications" target="_blank" rel="noreferrer"
                        style={{ display: 'inline-block', marginTop: 8, fontSize: 12, color: 'var(--accent)' }}>
                        {t('wizard.add_new_channel')}
                      </a>
                    </>
                  )}
                </div>

                <div className="field">
                  <label className="field-label">{t('wizard.field.advanced')}</label>
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                    <div style={{ display: 'flex', gap: 12, alignItems: 'flex-start', padding: '12px 14px', border: '1px solid var(--border)', borderRadius: 8 }}>
                      <div className={`toggle ${upsideDown ? 'on' : ''}`} onClick={() => setUpsideDown(v => !v)}/>
                      <div style={{ flex: 1 }}>
                        <div style={{ fontSize: 13, color: 'var(--text)', fontWeight: 500 }}>{t('wizard.upside_down')}</div>
                        <div className="field-hint" style={{ marginTop: 2 }}>
                          {t('wizard.upside_down_hint')}
                        </div>
                      </div>
                    </div>
                    {fields.url && (
                      <div style={{ display: 'flex', gap: 12, alignItems: 'flex-start', padding: '12px 14px', border: '1px solid var(--border)', borderRadius: 8 }}>
                        <div className={`toggle ${followRedir ? 'on' : ''}`} onClick={() => setFollowRedir(v => !v)}/>
                        <div style={{ flex: 1 }}>
                          <div style={{ fontSize: 13, color: 'var(--text)', fontWeight: 500 }}>{t('wizard.follow_redirects')}</div>
                          <div className="field-hint" style={{ marginTop: 2 }}>{t('wizard.follow_redirects_hint')}</div>
                        </div>
                      </div>
                    )}
                    {['http', 'keyword', 'json_query'].includes(type) && proxies.length > 0 && (
                      <div style={{ padding: '12px 14px', border: '1px solid var(--border)', borderRadius: 8 }}>
                        <div style={{ fontSize: 13, color: 'var(--text)', fontWeight: 500, marginBottom: 6 }}>{t('wizard.proxy')}</div>
                        <select className="input" value={proxyId} onChange={e => setProxyId(e.target.value)}>
                          <option value="">{t('wizard.proxy_none')}</option>
                          {proxies.filter(p => p.active).map(p => (
                            <option key={p.id} value={p.id}>{p.protocol}://{p.host}:{p.port}</option>
                          ))}
                        </select>
                        <div className="field-hint" style={{ marginTop: 4 }}>
                          {t('wizard.proxy_hint')} <a href="#/proxies" style={{ color: 'var(--accent)' }}>{t('proxies.title')}</a>.
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              </div>
            </>
          )}

          {/* error banner */}
          {err && (
            <div className="banner banner-err" style={{ marginTop: 18 }}>
              {err}
            </div>
          )}

          {/* nav buttons */}
          <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 36, paddingTop: 24, borderTop: '1px solid var(--border)' }}>
            <button className="btn" disabled={submitting}
              onClick={() => (step === 1 ? cancel() : setStep(s => s - 1))}>
              <ChevronLeft size={13}/> {t('wizard.back')}
            </button>
            {step < 3 ? (
              <button className="btn btn-accent" onClick={() => setStep(s => s + 1)} disabled={submitting}>
                {t('wizard.continue')} <ChevronRight size={13}/>
              </button>
            ) : (
              <button className="btn btn-accent" onClick={submit} disabled={submitting}>
                <Check size={13}/> {submitting ? t('wizard.creating') : t('wizard.create_monitor')}
              </button>
            )}
          </div>
        </div>

        {/* PREVIEW SIDE */}
        <aside style={{ padding: '32px 24px', background: 'var(--surface)', display: 'flex', flexDirection: 'column', gap: 16 }}>
          <div>
            <p style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em', margin: '0 0 12px' }}>{t('wizard.live_preview')}</p>
            <div className="card" style={{ padding: '16px 18px' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                <span style={{ width: 8, height: 8, borderRadius: '50%', background: 'var(--up)', boxShadow: '0 0 0 3px var(--up-soft)' }}/>
                <meta.icon size={14} color="var(--text-2)"/>
                <span style={{ fontSize: 14, fontWeight: 500 }}>{name.trim() || t('wizard.untitled')}</span>
              </div>
              <div style={{ fontSize: 11.5, color: 'var(--text-3)', lineHeight: 1.6 }}>
                <div>{meta.name} · {t('wizard.preview_schedule', { interval: intervalSec, timeout: timeoutSec })}</div>
                {fields.url      && url      && <div className="mono" style={{ color: 'var(--text-2)', wordBreak: 'break-all', marginTop: 4 }}>{url}</div>}
                {fields.hostname && hostname && <div className="mono" style={{ color: 'var(--text-2)', wordBreak: 'break-all', marginTop: 4 }}>{hostname}{port ? `:${port}` : ''}</div>}
              </div>
            </div>
          </div>

          <div style={{ marginTop: 'auto' }}>
            <p style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em', margin: '0 0 12px', display: 'flex', alignItems: 'center', gap: 6 }}>
              <Code size={11}/> {t('wizard.api_call')}
            </p>
            <pre className="mono" style={{
              padding: '12px 14px', background: 'var(--surface-2)',
              border: '1px solid var(--border)', borderRadius: 8,
              fontSize: 10.5, lineHeight: 1.55, color: 'var(--text-2)',
              margin: 0, overflow: 'auto', maxHeight: 320
            }}>
{`POST /v1/monitors\n${previewPayload}`}
            </pre>
          </div>
        </aside>
      </div>
    </div>
  );
}
