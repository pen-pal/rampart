// Unauthenticated public status page.
// Rendered at #/s/:slug. No login chrome, no auth gating in App.jsx.
//
// Layout (top → bottom):
//   - Brand row (page title + auto-refresh indicator)
//   - Hero status banner with overall state + the "all systems operational"
//     line OR an active-incident summary
//   - KPI row: total components, components down, 90d uptime, last incident
//   - Active incidents (if any), inline cards
//   - Components list — one row per monitor, each carrying a 90-day daily
//     status bar (one cell per day, color-coded) + uptime% + avg-latency
//   - Subscribe widget (email-only, single opt-in)
//   - Footer: brand attribution + last-updated relative time
//
// The page polls the API every 30 seconds; the auto-refresh dot in the
// brand row pulses on each refresh so visitors can see the page is alive.

import React, { useEffect, useMemo, useState } from 'react';
import {
  CheckCircle2, AlertCircle, AlertTriangle, Calendar, Loader2,
  Activity, Bell, Wrench, Shield, Mail, Rss, Link2, Check, X, History,
} from 'lucide-react';
import { api, offsetDateTimeArrayToDate } from '../lib/api.js';

// Timestamps from the API are `time::OffsetDateTime` → serde renders them
// as a numeric array, which `new Date()` can't parse ("Invalid Date").
// Coerce arrays via the helper; fall back to Date for ISO strings.
function toDate(v) {
  return Array.isArray(v) ? offsetDateTimeArrayToDate(v) : new Date(v);
}

// Friendly relative time like "2 minutes ago" / "yesterday" without
// pulling in a date library.
function relative(date) {
  const now = Date.now();
  const ms = now - date.getTime();
  const s = Math.round(ms / 1000);
  if (s < 5)        return 'just now';
  if (s < 60)       return `${s} seconds ago`;
  const m = Math.round(s / 60);
  if (m < 60)       return `${m} minute${m === 1 ? '' : 's'} ago`;
  const h = Math.round(m / 60);
  if (h < 24)       return `${h} hour${h === 1 ? '' : 's'} ago`;
  const d = Math.round(h / 24);
  if (d === 1)      return 'yesterday';
  if (d < 30)       return `${d} days ago`;
  return date.toLocaleDateString();
}

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap');

  .public {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    --warn:#f59e0b; --warn-soft:#fef3c7;
    --maint:#6366f1; --maint-soft:#e0e7ff;
    --none:#e7e5e4;

    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
    font-feature-settings: 'cv11','ss01';
  }
  .public.dark {
    --bg:#0c0a09; --surface:#1c1917; --surface-2:#292524;
    --border:#292524; --border-2:#44403c;
    --text:#fafaf9; --text-2:#d6d3d1; --text-3:#78716c;
    --accent:#2dd4bf; --accent-2:#14b8a6; --accent-soft:#134e4a;
    --up:#34d399; --up-soft:#064e3b;
    --down:#f87171; --down-soft:#7f1d1d;
    --warn:#fbbf24; --warn-soft:#78350f;
    --maint:#818cf8; --maint-soft:#3730a3;
    --none:#292524;
  }
  .public * { box-sizing: border-box; }

  .card {
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 14px;
  }

  .row {
    display: grid; grid-template-columns: 1fr auto;
    column-gap: 18px;
    padding: 18px 22px; border-top: 1px solid var(--border);
    align-items: center;
  }
  .row:first-child { border-top: none; }

  /* ── Daily uptime strip ───────────────────────────────────────── */
  .strip {
    display: flex; gap: 2px; height: 34px;
    margin: 10px 0 4px;
  }
  .strip > div {
    flex: 1; min-width: 2px; border-radius: 3px;
    transition: transform .12s, opacity .12s;
    cursor: default;
    position: relative;
  }
  .strip > div:hover { transform: scaleY(1.15); }
  .s-u { background: var(--up); opacity: .9; }
  .s-d { background: var(--down); }
  .s-w { background: var(--warn); }
  .s-m { background: var(--maint); opacity: .7; }
  .s-n { background: var(--none); }

  .strip-axis {
    display: flex; justify-content: space-between;
    font-size: 10.5px; color: var(--text-3); margin-top: 2px;
    letter-spacing: .04em;
  }

  /* ── Badges ───────────────────────────────────────────────────── */
  .badge {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 5px 10px; border-radius: 999px;
    font-size: 12px; font-weight: 600;
    white-space: nowrap;
  }
  .badge-up      { background: var(--up-soft);    color: #047857; }
  .badge-down    { background: var(--down-soft);  color: #b91c1c; }
  .badge-warn    { background: var(--warn-soft);  color: #92400e; }
  .badge-paused  { background: var(--surface-2);  color: var(--text-2); }
  .badge-pending { background: var(--surface-2);  color: var(--text-3); }
  .badge-maint   { background: var(--maint-soft); color: #4338ca; }

  .public.dark .badge-up   { color: #6ee7b7; }
  .public.dark .badge-down { color: #fca5a5; }
  .public.dark .badge-warn { color: #fde68a; }
  .public.dark .badge-maint{ color: #c7d2fe; }

  /* ── Hero status banner ───────────────────────────────────────── */
  .hero {
    padding: 32px 32px; border-radius: 18px;
    display: flex; align-items: center; gap: 18px;
    font-size: 22px; font-weight: 600; letter-spacing: -.01em;
    border: 1px solid transparent;
    box-shadow: 0 1px 2px rgba(0,0,0,.03);
  }
  .hero-up   { background: var(--up-soft);   color: #047857; border-color: var(--up); }
  .hero-down { background: var(--down-soft); color: #b91c1c; border-color: var(--down); }
  .hero-warn { background: var(--warn-soft); color: #92400e; border-color: var(--warn); }
  .hero-maint{ background: var(--maint-soft); color: #4338ca; border-color: var(--maint); }
  .hero-icon {
    width: 52px; height: 52px; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    background: rgba(255,255,255,.55); flex-shrink: 0;
  }
  .public.dark .hero { box-shadow: none; }
  .public.dark .hero-up   { color: #6ee7b7; }
  .public.dark .hero-down { color: #fca5a5; }
  .public.dark .hero-warn { color: #fde68a; }
  .public.dark .hero-maint{ color: #c7d2fe; }
  .public.dark .hero-icon { background: rgba(0,0,0,.3); }

  /* ── KPI row ──────────────────────────────────────────────────── */
  .kpis {
    display: grid; grid-template-columns: repeat(4, 1fr); gap: 14px;
  }
  .kpi {
    padding: 16px 18px;
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 12px;
  }
  .kpi-label {
    font-size: 11px; color: var(--text-3); font-weight: 600;
    text-transform: uppercase; letter-spacing: .06em;
    display: flex; align-items: center; gap: 6px;
  }
  .kpi-value {
    font-size: 24px; font-weight: 600; letter-spacing: -.02em;
    margin-top: 6px; line-height: 1;
  }
  .kpi-sub {
    font-size: 11px; color: var(--text-3); margin-top: 4px;
  }

  /* ── Subscribe ────────────────────────────────────────────────── */
  .subscribe-card {
    padding: 22px 26px;
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 14px;
    display: flex; align-items: center; gap: 18px; flex-wrap: wrap;
  }
  .subscribe-card h3 {
    margin: 0 0 4px; font-size: 15px; font-weight: 600;
  }
  .subscribe-card p {
    margin: 0; font-size: 12.5px; color: var(--text-3);
  }
  .sub-input {
    flex: 1; min-width: 240px;
    padding: 10px 14px; border-radius: 9px;
    background: var(--surface); border: 1px solid var(--border);
    color: var(--text); font-size: 13.5px; font-family: inherit; outline: none;
    transition: border-color .12s, box-shadow .12s;
  }
  .sub-input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .sub-input::placeholder { color: var(--text-3); }
  .sub-btn {
    padding: 10px 16px; border-radius: 9px;
    background: var(--accent); color: white; border: 1px solid var(--accent);
    font-size: 13.5px; font-weight: 500; cursor: pointer; font-family: inherit;
    transition: background .12s;
  }
  .sub-btn:hover:not([disabled]) { background: var(--accent-2); }
  .sub-btn[disabled] { opacity: .6; cursor: not-allowed; }

  /* ── Section header ───────────────────────────────────────────── */
  .section-h {
    font-size: 12px; font-weight: 600; color: var(--text-2);
    text-transform: uppercase; letter-spacing: .06em;
    margin: 30px 0 12px;
    display: flex; align-items: center; gap: 8px;
  }

  /* ── Brand row ────────────────────────────────────────────────── */
  .brand-row {
    display: flex; align-items: center; justify-content: space-between;
    margin-bottom: 24px;
  }
  .brand-row h1 {
    font-size: 26px; font-weight: 600; margin: 0; letter-spacing: -.02em;
  }
  .live-dot {
    display: inline-flex; align-items: center; gap: 7px;
    font-size: 11.5px; color: var(--text-3); font-weight: 500;
  }
  .live-dot::before {
    content: ''; width: 7px; height: 7px; border-radius: 50%;
    background: var(--up); box-shadow: 0 0 0 3px var(--up-soft);
    animation: pulse 2s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1;   transform: scale(1);   }
    50%      { opacity: .55; transform: scale(.85); }
  }

  /* ── Footer ───────────────────────────────────────────────────── */
  .footer {
    margin-top: 40px; padding-top: 24px;
    border-top: 1px solid var(--border);
    display: flex; justify-content: space-between; align-items: center;
    font-size: 11.5px; color: var(--text-3);
  }
  .footer a { color: var(--text-2); text-decoration: none; }
  .footer a:hover { color: var(--text); }

  /* ── Legend pills under the strip ─────────────────────────────── */
  .legend {
    display: flex; gap: 14px; flex-wrap: wrap;
    font-size: 11px; color: var(--text-3);
    margin-top: 18px;
  }
  .legend-dot {
    display: inline-block; width: 10px; height: 10px;
    border-radius: 2px; margin-right: 6px; vertical-align: -1px;
  }

  @media (max-width: 640px) {
    .kpis { grid-template-columns: repeat(2, 1fr); }
    .hero { padding: 22px 20px; font-size: 18px; }
  }
`;

const STATUS_LABEL = {
  up:          'Operational',
  down:        'Down',
  warn:        'Degraded',
  paused:      'Paused',
  pending:     'Pending',
  maintenance: 'Maintenance',
};

function statusIcon(status, size = 14) {
  if (status === 'down')        return <AlertCircle  size={size}/>;
  if (status === 'warn')        return <AlertTriangle size={size}/>;
  if (status === 'maintenance') return <Wrench size={size}/>;
  return <CheckCircle2 size={size}/>;
}

// Rolled-up overall: worst-wins. Maintenance is treated as informational
// (overrides Up, but Down wins over Maintenance — a real outage still surfaces).
function overall(monitors) {
  if (monitors.some(m => m.current_status === 'down')) return 'down';
  if (monitors.some(m => m.current_status === 'warn')) return 'warn';
  if (monitors.some(m => m.current_status === 'maintenance')) return 'maintenance';
  return 'up';
}

// Avg of `uptime_90d` across all monitors, weighted equally.
function avgUptime(monitors) {
  const have = monitors.filter(m => m.uptime_90d != null);
  if (have.length === 0) return null;
  return have.reduce((s, m) => s + m.uptime_90d, 0) / have.length;
}

export default function StatusPageView({ slug }) {
  const [data,    setData]    = useState(null);
  const [error,   setError]   = useState(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const r = await api.statusPages.publicView(slug);
        if (!cancelled) { setData(r); setError(null); setLoading(false); }
      } catch (e) {
        if (!cancelled) { setError(e); setLoading(false); }
      }
    };
    load();
    // Light polling so the page updates without a manual refresh.
    const t = setInterval(load, 30_000);
    return () => { cancelled = true; clearInterval(t); };
  }, [slug]);

  if (loading) {
    return (
      <div className="public">
        <style>{css}</style>
        <div style={{ maxWidth: 720, margin: '0 auto', padding: '120px 24px', textAlign: 'center', color: 'var(--text-3)' }}>
          <Loader2 size={28} style={{ animation: 'spin 1s linear infinite' }}/>
        </div>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="public">
        <style>{css}</style>
        <div style={{ maxWidth: 560, margin: '0 auto', padding: '120px 24px', textAlign: 'center' }}>
          <AlertCircle size={36} color="var(--text-3)" style={{ marginBottom: 14 }}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: '0 0 8px' }}>Page not found</h1>
          <p style={{ fontSize: 14, color: 'var(--text-2)', margin: 0 }}>
            No status page is published at <code style={{ background: 'var(--surface-2)', padding: '2px 6px', borderRadius: 4 }}>/{slug}</code>.
          </p>
        </div>
      </div>
    );
  }

  const themeClass = data.theme === 'dark' ? 'public dark' : 'public';
  const monitors = data.monitors || [];
  const incidents = data.incidents || [];
  const hasIncidents = incidents.length > 0;

  let status = overall(monitors);
  if (status === 'up' && hasIncidents) status = 'warn';

  const hero = {
    up:          { title: 'All Systems Operational', sub: 'Every component is reporting healthy.', cls: 'hero-up' },
    warn:        { title: hasIncidents ? 'Active Incident' : 'Partial Service Degradation', sub: hasIncidents ? 'See details below.' : 'One or more components are degraded.', cls: 'hero-warn' },
    down:        { title: 'Service Disruption', sub: 'One or more components are down. Engineers are looking into it.', cls: 'hero-down' },
    maintenance: { title: 'Scheduled Maintenance', sub: 'A planned maintenance window is in effect.', cls: 'hero-maint' },
  }[status];

  const downCount = monitors.filter(m => m.current_status === 'down').length;
  const warnCount = monitors.filter(m => m.current_status === 'warn').length;
  const upCount   = monitors.filter(m => m.current_status === 'up').length;
  const overallUptime = avgUptime(monitors);

  return (
    <div className={themeClass}>
      <style>{css}</style>

      <div style={{ maxWidth: 880, margin: '0 auto', padding: '40px 24px 24px' }}>

        {/* ── Brand row ──────────────────────────────────────────── */}
        <div className="brand-row">
          <div>
            <h1>{data.title}</h1>
            {data.description && (
              <p style={{ fontSize: 13.5, color: 'var(--text-3)', margin: '4px 0 0' }}>{data.description}</p>
            )}
          </div>
          <div className="live-dot">live · auto-refresh 30s</div>
        </div>

        {/* ── Hero status banner ─────────────────────────────────── */}
        <div className={`hero ${hero.cls}`}>
          <div className="hero-icon">{statusIcon(status, 26)}</div>
          <div>
            <div>{hero.title}</div>
            <div style={{ fontSize: 13, fontWeight: 400, opacity: .85, marginTop: 4 }}>{hero.sub}</div>
          </div>
        </div>

        {/* ── KPIs ───────────────────────────────────────────────── */}
        <div className="kpis" style={{ marginTop: 18 }}>
          <Kpi icon={<Activity size={12}/>} label="Components" value={String(monitors.length)} sub={`${upCount} up · ${warnCount} degraded · ${downCount} down`}/>
          <Kpi icon={<Shield size={12}/>}   label="90d uptime"  value={overallUptime == null ? '—' : `${overallUptime.toFixed(2)}%`} sub="across all components"/>
          <Kpi icon={<Bell size={12}/>}     label="Active incidents" value={String(incidents.length)} sub={incidents.length ? 'see below' : 'none right now'}/>
          <Kpi icon={<Calendar size={12}/>} label="Last update"   value={data.generated_at ? relative(toDate(data.generated_at)) : '—'} sub="refreshes every 30s"/>
        </div>

        {/* ── Active incidents ──────────────────────────────────── */}
        {hasIncidents && (
          <>
            <div className="section-h"><Bell size={13}/> Active incidents</div>
            <div>
              {incidents.map((inc, i) => <Incident key={i} incident={inc}/>)}
            </div>
          </>
        )}

        {/* ── Components ───────────────────────────────────────── */}
        <div className="section-h"><Activity size={13}/> Components</div>
        <div className="card" style={{ overflow: 'hidden' }}>
          {monitors.length === 0 ? (
            <div style={{ padding: 36, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
              No components attached to this page yet.
            </div>
          ) : monitors.map((m, i) => <Component key={i} monitor={m}/>)}
        </div>

        {/* ── Legend ───────────────────────────────────────────── */}
        <div className="legend">
          <span><span className="legend-dot" style={{ background: 'var(--up)' }}/> Operational</span>
          <span><span className="legend-dot" style={{ background: 'var(--warn)' }}/> Degraded</span>
          <span><span className="legend-dot" style={{ background: 'var(--down)' }}/> Down</span>
          <span><span className="legend-dot" style={{ background: 'var(--maint)' }}/> Maintenance</span>
          <span><span className="legend-dot" style={{ background: 'var(--none)' }}/> No data</span>
        </div>

        {/* ── Incident history ────────────────────────────────── */}
        {(data.incident_history || []).length > 0 && (
          <>
            <div className="section-h"><History size={13}/> Incident history</div>
            <div>
              {(data.incident_history || []).map((inc, i) => <ResolvedIncident key={i} incident={inc}/>)}
            </div>
          </>
        )}

        {/* ── Subscribe ────────────────────────────────────────── */}
        <div className="section-h"><Bell size={13}/> Subscribe to updates</div>
        <SubscribeBox slug={slug}/>

        {/* ── Footer ───────────────────────────────────────────── */}
        <div className="footer">
          <span>Powered by <a href="https://github.com/pen-pal/rampart" target="_blank" rel="noreferrer">Rampart</a></span>
          <span>Last updated {data.generated_at ? relative(toDate(data.generated_at)) : '—'}</span>
        </div>
      </div>
    </div>
  );
}

function Kpi({ icon, label, value, sub }) {
  return (
    <div className="kpi">
      <div className="kpi-label">{icon} {label}</div>
      <div className="kpi-value">{value}</div>
      {sub && <div className="kpi-sub">{sub}</div>}
    </div>
  );
}

function Component({ monitor }) {
  const status = monitor.current_status;
  const statusKey = status === 'maintenance' ? 'maint' : status;
  // Daily-status strip carries 90 chars. If the backend hasn't backfilled
  // a row yet it might be shorter — pad with 'n' so the strip always has
  // 90 cells (otherwise it'd render at variable widths between monitors).
  const strip = (monitor.daily_status_90d || '').padStart(90, 'n').slice(-90);

  return (
    <div className="row">
      <div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 2 }}>
          <span style={{ fontSize: 15, fontWeight: 500 }}>{monitor.name}</span>
        </div>
        <div className="strip" title="One cell per day, oldest left, today right">
          {strip.split('').map((c, i) => (
            <div
              key={i}
              className={`s-${c}`}
              title={cellTitle(c, 90 - 1 - i)}
            />
          ))}
        </div>
        <div className="strip-axis">
          <span>90 days ago</span>
          <span>
            {monitor.uptime_90d == null
              ? 'no data yet'
              : `${monitor.uptime_90d.toFixed(2)}% uptime`}
            {monitor.avg_latency_ms_24h != null && (
              <> · avg {Math.round(monitor.avg_latency_ms_24h)} ms</>
            )}
          </span>
          <span>today</span>
        </div>
      </div>
      <span className={`badge badge-${statusKey}`}>
        {statusIcon(status)} {STATUS_LABEL[status] || status}
      </span>
    </div>
  );
}

function cellTitle(c, daysAgo) {
  const label = {
    u: 'Operational',
    d: 'Down',
    w: 'Degraded',
    m: 'Maintenance',
    n: 'No data',
  }[c] || 'Unknown';
  if (daysAgo === 0)         return `${label} · today`;
  if (daysAgo === 1)         return `${label} · yesterday`;
  return `${label} · ${daysAgo} days ago`;
}

function SubscribeBox({ slug }) {
  // Tabs: 'email' | 'rss' | 'atom' | 'webhook'
  const [tab, setTab] = useState('email');

  const tabs = [
    { id: 'email',   label: 'Email',   icon: Mail },
    { id: 'rss',     label: 'RSS',     icon: Rss },
    { id: 'atom',    label: 'Atom',    icon: Rss },
    { id: 'webhook', label: 'Webhook', icon: Link2 },
  ];

  return (
    <div className="subscribe-card" style={{ display: 'block', padding: '6px 6px 22px' }}>
      <div style={{
        display: 'flex', gap: 4, padding: 6,
        background: 'var(--surface-2)', borderRadius: 10,
        marginBottom: 18,
      }}>
        {tabs.map(t => {
          const Icon = t.icon;
          const active = tab === t.id;
          return (
            <button key={t.id} type="button" onClick={() => setTab(t.id)}
              style={{
                flex: 1, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 6,
                padding: '9px 12px', borderRadius: 7,
                background: active ? 'var(--surface)' : 'transparent',
                color: active ? 'var(--text)' : 'var(--text-3)',
                border: 'none', cursor: 'pointer', font: 'inherit',
                fontSize: 12.5, fontWeight: 500,
                boxShadow: active ? '0 1px 2px rgba(0,0,0,.05)' : 'none',
                transition: 'all .12s',
              }}>
              <Icon size={13}/> {t.label}
            </button>
          );
        })}
      </div>
      <div style={{ padding: '0 18px' }}>
        {tab === 'email'   && <EmailTab slug={slug}/>}
        {tab === 'rss'     && <FeedTab slug={slug} kind="rss"/>}
        {tab === 'atom'    && <FeedTab slug={slug} kind="atom"/>}
        {tab === 'webhook' && <WebhookTab/>}
      </div>
    </div>
  );
}

function EmailTab({ slug }) {
  const [email, setEmail] = useState('');
  const [state, setState] = useState('idle');
  const [err,   setErr]   = useState(null);

  const submit = async (e) => {
    e.preventDefault();
    if (!email.includes('@')) { setErr('Enter a valid email.'); setState('err'); return; }
    setState('sending'); setErr(null);
    try {
      await api.subscribers.subscribe(slug, email.trim());
      setState('ok');
    } catch (e2) {
      setErr(e2.message || 'Subscribe failed.');
      setState('err');
    }
  };

  if (state === 'ok') {
    return (
      <div style={{
        background: 'var(--up-soft)', color: '#047857',
        padding: '14px 18px', borderRadius: 9, fontSize: 13.5, fontWeight: 500,
        display: 'flex', alignItems: 'center', gap: 10,
      }}>
        <Check size={16}/> Subscribed. We'll email you on every incident posted.
      </div>
    );
  }

  return (
    <>
      <p style={{ margin: '0 0 12px', fontSize: 13, color: 'var(--text-2)', lineHeight: 1.5 }}>
        Get notified by email when an incident is posted and again on every update + when it's resolved.
      </p>
      <form onSubmit={submit} style={{ display: 'flex', gap: 8 }}>
        <input className="sub-input" type="email" value={email}
          onChange={e => setEmail(e.target.value)}
          placeholder="you@example.com"
          required/>
        <button className="sub-btn" type="submit" disabled={state === 'sending'}>
          {state === 'sending' ? 'Subscribing…' : 'Subscribe'}
        </button>
      </form>
      {state === 'err' && err && (
        <div style={{ fontSize: 12, color: 'var(--down)', marginTop: 8, fontWeight: 500 }}>{err}</div>
      )}
    </>
  );
}

function CopyButton({ value }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      // Older browsers / non-secure contexts — fall back to a temp
      // textarea selectall+copy. Quiet fallback; no toast.
      const el = document.createElement('textarea');
      el.value = value;
      document.body.appendChild(el);
      el.select();
      try { document.execCommand('copy'); setCopied(true); setTimeout(() => setCopied(false), 1800); } catch {}
      document.body.removeChild(el);
    }
  };
  return (
    <button onClick={copy} className="sub-btn" type="button" style={{ padding: '8px 12px' }}>
      {copied ? <><Check size={13}/> Copied</> : <><Link2 size={13}/> Copy</>}
    </button>
  );
}

function FeedTab({ slug, kind }) {
  // URL is relative to the current origin so it works whether the page
  // is served from `rampart.example.com` or behind a path-prefix proxy.
  const url = `${window.location.origin}/v1/public/status-pages/${slug}/feed.${kind}`;
  const label = kind === 'atom' ? 'Atom 1.0' : 'RSS 2.0';
  return (
    <>
      <p style={{ margin: '0 0 12px', fontSize: 13, color: 'var(--text-2)', lineHeight: 1.5 }}>
        Drop this {label} feed into any reader (Feedly, NetNewsWire, Reeder, Slack's <code style={{ background: 'var(--surface-2)', padding: '1px 5px', borderRadius: 4 }}>/feed</code>, etc.) to get a new entry per incident + a refresh on every update.
      </p>
      <div style={{ display: 'flex', gap: 8 }}>
        <input className="sub-input" readOnly value={url}
          onFocus={e => e.target.select()}
          style={{ fontFamily: 'JetBrains Mono, monospace', fontSize: 12.5 }}/>
        <CopyButton value={url}/>
        <a className="sub-btn" href={url} target="_blank" rel="noreferrer" style={{ textDecoration: 'none', display: 'inline-flex', alignItems: 'center', gap: 6 }}>
          <Rss size={13}/> Open
        </a>
      </div>
    </>
  );
}

function WebhookTab() {
  return (
    <>
      <p style={{ margin: '0 0 10px', fontSize: 13, color: 'var(--text-2)', lineHeight: 1.5 }}>
        Webhook subscriptions are configured on the operator side from <strong>Notifications → Channels</strong> inside the Rampart admin. Drop a Slack / Discord / Microsoft Teams / Generic Webhook there and tag it for this status page.
      </p>
      <p style={{ margin: 0, fontSize: 12, color: 'var(--text-3)' }}>
        If you don't run the Rampart instance hosting this page, ask the operator to wire your endpoint as a notification channel.
      </p>
    </>
  );
}

const STYLE_TO_BG = {
  info:    ['#dbeafe', '#1e40af'],
  warning: ['#fef3c7', '#92400e'],
  danger:  ['#fee2e2', '#b91c1c'],
  primary: ['#e0e7ff', '#4338ca'],
  success: ['#d1fae5', '#047857'],
};

/** A short "47 minutes" / "2 days" style duration. */
function duration(start, end) {
  const ms = end.getTime() - start.getTime();
  const m = Math.round(ms / 60000);
  if (m < 1)    return 'under a minute';
  if (m < 60)   return `${m} minute${m === 1 ? '' : 's'}`;
  const h = Math.floor(m / 60);
  const rm = m % 60;
  if (h < 24)   return rm === 0 ? `${h} hour${h === 1 ? '' : 's'}` : `${h}h ${rm}m`;
  const d = Math.floor(h / 24);
  const rh = h % 24;
  return rh === 0 ? `${d} day${d === 1 ? '' : 's'}` : `${d}d ${rh}h`;
}

function ResolvedIncident({ incident }) {
  const created = toDate(incident.created_at);
  const resolved = toDate(incident.resolved_at);
  const [bg, fg] = STYLE_TO_BG[incident.style] || STYLE_TO_BG.success;
  return (
    <div style={{
      padding: '14px 18px', marginBottom: 10,
      borderRadius: 12, background: 'var(--surface)',
      border: '1px solid var(--border)',
      display: 'grid', gridTemplateColumns: '6px 1fr', gap: 14, alignItems: 'flex-start',
    }}>
      <div style={{ width: 6, alignSelf: 'stretch', borderRadius: 3, background: bg }}/>
      <div>
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 10, flexWrap: 'wrap', marginBottom: 4 }}>
          <strong style={{ fontSize: 14 }}>{incident.title}</strong>
          <span style={{ fontSize: 11, color: fg, fontWeight: 500, background: bg, padding: '2px 8px', borderRadius: 999 }}>
            Resolved
          </span>
        </div>
        <div style={{ fontSize: 12, color: 'var(--text-3)', marginBottom: 6 }}>
          {created.toLocaleDateString()} · lasted {duration(created, resolved)}
        </div>
        <div style={{ fontSize: 13, color: 'var(--text-2)', whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{incident.content}</div>
      </div>
    </div>
  );
}

function Incident({ incident }) {
  const [bg, fg] = STYLE_TO_BG[incident.style] || STYLE_TO_BG.warning;
  return (
    <div style={{
      padding: '18px 22px', marginBottom: 12,
      borderRadius: 12, background: bg, color: fg,
      border: `1px solid ${fg}33`,
    }}>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 10, marginBottom: 6, flexWrap: 'wrap' }}>
        <strong style={{ fontSize: 15 }}>{incident.title}</strong>
        <span style={{ fontSize: 11, opacity: .75 }}>
          posted {relative(toDate(incident.created_at))}
        </span>
      </div>
      <div style={{ fontSize: 13.5, whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{incident.content}</div>
      {(incident.updates || []).length > 0 && (
        <div style={{ marginTop: 14, paddingTop: 12, borderTop: `1px solid ${fg}33` }}>
          {(incident.updates || []).map((u, i) => (
            <div key={i} style={{
              marginTop: i === 0 ? 0 : 10,
              paddingLeft: 12, borderLeft: `2px solid ${fg}80`,
              fontSize: 13, opacity: .9,
            }}>
              <div style={{ fontSize: 11, opacity: .8, marginBottom: 2 }}>
                {relative(toDate(u.posted_at))}
              </div>
              <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{u.message}</div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
