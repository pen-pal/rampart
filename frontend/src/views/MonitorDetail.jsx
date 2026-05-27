import React, { useMemo, useState } from 'react';
import {
  AreaChart, Area, XAxis, YAxis, ResponsiveContainer, Tooltip, ReferenceLine,
} from 'recharts';
import {
  ChevronLeft, Pause, Play, Edit3, Trash2, Bell, Plus, X, Send,
  Globe, Server, Lock, AlertCircle, Activity, Hash, Radio, Database,
  MoreHorizontal, Calendar, ChevronDown, Copy, Check, Zap,
} from 'lucide-react';
import {
  api, useApi, formatRelative, offsetDateTimeArrayToDate, statusToClass,
} from '../lib/api.js';

// ── shared design system (matches dashboard v2) ───────────────────────────
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
    --maint:#6366f1; --maint-soft:#e0e7ff;
    --paused:#a8a29e;

    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    font-feature-settings: 'cv11','ss01';
    min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', monospace; font-feature-settings: 'zero'; }
  .tabular { font-variant-numeric: tabular-nums; }

  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 12px; border-radius: 8px; cursor: pointer;
    font-size: 13px; font-weight: 500; line-height: 1;
    background: var(--surface); border: 1px solid var(--border); color: var(--text-2);
    transition: all .12s;
  }
  .btn:hover { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .btn-prim { background: var(--text); color: var(--surface); border-color: var(--text); }
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-danger { color: var(--down); }
  .btn-danger:hover { background: var(--down-soft); border-color: var(--down); }
  .btn-ghost { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .btn[disabled] { opacity: .5; cursor: not-allowed; }

  .pill {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 3px 9px; border-radius: 999px;
    font-size: 11px; font-weight: 500; line-height: 1.4;
  }
  .pill-up     { background: var(--up-soft);    color: #047857; }
  .pill-down   { background: var(--down-soft);  color: #b91c1c; }
  .pill-warn   { background: var(--warn-soft);  color: #b45309; }
  .pill-maint  { background: var(--maint-soft); color: #4338ca; }
  .pill-paused { background: var(--surface-2);  color: var(--text-2); }
  .pill-pending{ background: var(--surface-2);  color: var(--text-2); }

  .dot { width: 10px; height: 10px; border-radius: 50%; display: inline-block; flex-shrink: 0; }
  .dot.up     { background: var(--up);   box-shadow: 0 0 0 4px var(--up-soft); }
  .dot.down   { background: var(--down); box-shadow: 0 0 0 4px var(--down-soft); }
  .dot.warn   { background: var(--warn); box-shadow: 0 0 0 4px var(--warn-soft); }
  .dot.maint  { background: var(--maint); box-shadow: 0 0 0 4px var(--maint-soft); }
  .dot.paused { background: var(--paused); }

  .tabs { display: inline-flex; gap: 2px; padding: 3px; background: var(--surface-2); border-radius: 8px; border: 1px solid var(--border); }
  .tabs button {
    background: transparent; border: none; padding: 6px 14px; border-radius: 6px;
    font-size: 12px; font-weight: 500; color: var(--text-2); cursor: pointer;
    font-family: inherit;
  }
  .tabs button:hover { color: var(--text); }
  .tabs button.active { background: var(--surface); color: var(--text); box-shadow: 0 1px 2px rgba(0,0,0,.04); }

  .kpi-label { font-size: 11px; font-weight: 500; color: var(--text-3); text-transform: uppercase; letter-spacing: .04em; }
  .kpi-value { font-size: 28px; font-weight: 600; line-height: 1; letter-spacing: -.02em; }

  .uptime-bar { display: flex; gap: 2px; height: 32px; }
  .uptime-bar > div { flex: 1; border-radius: 2px; min-width: 2px; cursor: pointer; transition: transform .1s; }
  .uptime-bar > div:hover { transform: scaleY(1.1); }
  .ub-up    { background: var(--up); opacity: .85; }
  .ub-up:hover { opacity: 1; }
  .ub-down  { background: var(--down); }
  .ub-warn  { background: var(--warn); }
  .ub-maint { background: var(--maint); opacity: .7; }
  .ub-none  { background: var(--border); }

  .hb-row {
    display: grid; grid-template-columns: 90px 80px 80px 1fr 60px;
    gap: 16px; padding: 12px 18px; font-size: 12px;
    border-top: 1px solid var(--border); align-items: center;
  }
  .hb-row:hover { background: var(--surface-2); }

  .empty {
    padding: 24px 18px; text-align: center;
    color: var(--text-3); font-size: 13px;
  }
`;

// ── derived data helpers ──────────────────────────────────────────────────
function bucketByDay(heartbeats, days = 90) {
  // Returns `days` cells, oldest -> newest. Each cell: ub-up / ub-down /
  // ub-warn based on the worst status that day. ub-none for days with no
  // heartbeats (most days, given the project is new).
  const now = Date.now();
  const out = Array.from({ length: days }, () => 'ub-none');
  for (const h of heartbeats) {
    const t = h.ts instanceof Array ? offsetDateTimeArrayToDate(h.ts).getTime() : new Date(h.ts).getTime();
    const ageDays = Math.floor((now - t) / 86_400_000);
    if (ageDays < 0 || ageDays >= days) continue;
    const idx = days - 1 - ageDays;
    const cur = out[idx];
    if (h.status === 'down')              out[idx] = 'ub-down';
    else if (h.status === 'warn' && cur !== 'ub-down') out[idx] = 'ub-warn';
    else if (cur === 'ub-none')           out[idx] = 'ub-up';
  }
  return out;
}

function bucketLatency(heartbeats, buckets = 144) {
  // Reverse so heartbeats are oldest-first.
  const hbs = [...heartbeats].reverse();
  if (hbs.length === 0) return [];
  const first = hbs[0];
  const last  = hbs[hbs.length - 1];
  const t0 = (first.ts instanceof Array ? offsetDateTimeArrayToDate(first.ts) : new Date(first.ts)).getTime();
  const t1 = (last.ts  instanceof Array ? offsetDateTimeArrayToDate(last.ts)  : new Date(last.ts)).getTime();
  if (t1 <= t0) return hbs.map((h, i) => latencyPoint(h, i));

  const width = (t1 - t0) / buckets;
  const bins = Array.from({ length: buckets }, () => ({ sum: 0, count: 0, t: 0 }));
  for (const h of hbs) {
    if (h.status !== 'up' || h.latency_ms == null) continue;
    const t = (h.ts instanceof Array ? offsetDateTimeArrayToDate(h.ts) : new Date(h.ts)).getTime();
    const i = Math.min(buckets - 1, Math.max(0, Math.floor((t - t0) / width)));
    bins[i].sum   += h.latency_ms;
    bins[i].count += 1;
    bins[i].t      = t;
  }
  return bins.map((b, i) => ({
    t: i,
    label: b.count > 0
      ? new Date(b.t).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })
      : new Date(t0 + i * width).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' }),
    latency: b.count > 0 ? Math.round(b.sum / b.count) : null,
  }));
}

function latencyPoint(h, i) {
  const date = h.ts instanceof Array ? offsetDateTimeArrayToDate(h.ts) : new Date(h.ts);
  return {
    t: i,
    label: date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' }),
    latency: h.status === 'up' ? (h.latency_ms ?? null) : null,
  };
}

// Group consecutive `down` heartbeats into a single downtime span.
function downtimeSpans(heartbeats) {
  const hbs = [...heartbeats].reverse(); // oldest -> newest
  const spans = [];
  let cur = null;
  for (const h of hbs) {
    const t = h.ts instanceof Array ? offsetDateTimeArrayToDate(h.ts).getTime() : new Date(h.ts).getTime();
    if (h.status === 'down') {
      if (!cur) cur = { from: t, until: t, msg: h.msg || 'down', count: 1 };
      else      { cur.until = t; cur.count += 1; cur.msg = cur.msg || h.msg; }
    } else if (cur) {
      spans.push(cur); cur = null;
    }
  }
  if (cur) spans.push(cur);
  return spans.reverse().slice(0, 6).map(s => ({
    cause:  s.msg || 'down',
    dur:    formatDuration(s.until - s.from),
    from:   new Date(s.from),
  }));
}

function formatDuration(ms) {
  const sec = Math.max(0, Math.round(ms / 1000));
  if (sec < 60)  return `${sec}s`;
  if (sec < 3600) return `${Math.floor(sec / 60)}m ${sec % 60}s`;
  return `${Math.floor(sec / 3600)}h ${Math.floor((sec % 3600) / 60)}m`;
}

// ── components ────────────────────────────────────────────────────────────
function Kpi({ label, value, suffix, sub, color }) {
  return (
    <div className="card" style={{ padding: '18px 20px' }}>
      <div className="kpi-label">{label}</div>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 4, marginTop: 10 }}>
        <span className="kpi-value tabular" style={{ color: color || 'var(--text)' }}>{value}</span>
        {suffix && <span style={{ fontSize: 13, color: 'var(--text-3)', fontWeight: 500 }}>{suffix}</span>}
      </div>
      {sub && <div style={{ fontSize: 12, color: 'var(--text-3)', marginTop: 6 }}>{sub}</div>}
    </div>
  );
}

const KIND_LABEL = {
  http: 'HTTP', keyword: 'HTTP keyword', json_query: 'HTTP JSON',
  tcp: 'TCP', ping: 'Ping', dns: 'DNS', push: 'Push', grpc: 'gRPC',
  tls: 'TLS', docker: 'Docker', steam: 'Steam', mqtt: 'MQTT', radius: 'RADIUS',
  kafka: 'Kafka', postgres: 'Postgres', mysql: 'MySQL', mssql: 'MSSQL',
  redis: 'Redis', mongodb: 'MongoDB', domain: 'Domain expiry',
};

// ── main ──────────────────────────────────────────────────────────────────
export default function MonitorDetail({ monitorId }) {
  const [tab, setTab] = useState('overview');
  const [logFilter, setLogFilter] = useState('all');
  const [acting, setActing] = useState(false);

  const monitorState   = useApi(() => monitorId ? api.monitors.get(monitorId)         : Promise.resolve(null), [monitorId], { pollMs: 15_000 });
  const heartbeatState = useApi(() => monitorId ? api.monitors.heartbeats(monitorId, 500) : Promise.resolve([]),   [monitorId], { pollMs: 10_000 });
  const summaryState   = useApi(() => api.monitors.summary(86400),       [], { pollMs: 15_000 });
  const summaryState30 = useApi(() => api.monitors.summary(2_592_000),   [], { pollMs: 60_000 });

  const monitor = monitorState.data;
  const heartbeats = heartbeatState.data || [];
  const summary24h = (summaryState.data   || []).find(s => s.monitor_id === monitorId);
  const summary30d = (summaryState30.data || []).find(s => s.monitor_id === monitorId);

  const uptime24h = summary24h?.uptime_pct;
  const uptime30d = summary30d?.uptime_pct;
  const avgLatency24h = summary24h?.avg_latency_ms;
  const sample90 = useMemo(() => bucketByDay(heartbeats, 90), [heartbeats]);
  const incidentCount = sample90.filter(c => c === 'ub-down').length;
  const responseData = useMemo(() => bucketLatency(heartbeats, 144), [heartbeats]);
  const downtime = useMemo(() => downtimeSpans(heartbeats), [heartbeats]);

  const filteredLog = (logFilter === 'all' ? heartbeats : heartbeats.filter(h => h.status !== 'up')).slice(0, 50);

  // ── actions ────────────────────────────────────────────────────────────
  const doPauseResume = async () => {
    if (!monitor || acting) return;
    setActing(true);
    try {
      if (monitor.active) await api.monitors.pause(monitor.id);
      else                await api.monitors.resume(monitor.id);
      monitorState.data && (monitorState.data.active = !monitorState.data.active); // optimistic, the next poll corrects
    } catch (e) { alert(`Failed: ${e.message}`); }
    setActing(false);
  };
  const doDelete = async () => {
    if (!monitor || acting) return;
    if (!confirm(`Delete monitor "${monitor.name}"? This also drops all its heartbeats.`)) return;
    setActing(true);
    try {
      await api.monitors.remove(monitor.id);
      window.location.hash = '#/';
    } catch (e) { alert(`Failed: ${e.message}`); setActing(false); }
  };

  // ── missing-id / loading / not-found ──────────────────────────────────
  if (!monitorId) {
    return (
      <div className="rampart">
        <style>{css}</style>
        <div style={{ padding: 80, textAlign: 'center' }}>
          <h2 style={{ fontSize: 18, fontWeight: 600 }}>No monitor selected</h2>
          <p style={{ color: 'var(--text-2)', fontSize: 14, marginTop: 8 }}>Open one from the dashboard, or <a href="#/" style={{ color: 'var(--accent)' }}>go back</a>.</p>
        </div>
      </div>
    );
  }
  if (monitorState.error?.status === 404) {
    return (
      <div className="rampart">
        <style>{css}</style>
        <div style={{ padding: 80, textAlign: 'center' }}>
          <h2 style={{ fontSize: 18, fontWeight: 600 }}>Monitor not found</h2>
          <p style={{ color: 'var(--text-2)', fontSize: 14, marginTop: 8 }}>Likely deleted. <a href="#/" style={{ color: 'var(--accent)' }}>Back to dashboard</a>.</p>
        </div>
      </div>
    );
  }
  if (monitorState.loading && !monitor) {
    return (
      <div className="rampart">
        <style>{css}</style>
        <div className="empty" style={{ padding: 80 }}>Loading monitor…</div>
      </div>
    );
  }
  if (!monitor) return null;

  const statusCls = statusToClass(monitor.current_status);
  const statusPillCls = monitor.current_status === 'maintenance' ? 'maint' : monitor.current_status;
  const lastTs = summary24h?.last_ts || heartbeats[0]?.ts;

  return (
    <div className="rampart">
      <style>{css}</style>

      {/* header */}
      <header style={{
        background: 'var(--surface)', borderBottom: '1px solid var(--border)',
        padding: '14px 24px 0', position: 'sticky', top: 0, zIndex: 10
      }}>
        <div style={{ fontSize: 12, color: 'var(--text-3)', display: 'flex', alignItems: 'center', gap: 6, marginBottom: 16 }}>
          <ChevronLeft size={14} style={{ cursor: 'pointer' }} onClick={() => { window.location.hash = '#/'; }}/>
          <a href="#/" style={{ cursor: 'pointer', color: 'var(--text-3)', textDecoration: 'none' }}>Monitors</a>
          <span>/</span>
          <span style={{ color: 'var(--text)', fontWeight: 500 }}>{monitor.name}</span>
        </div>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 18 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 8 }}>
              <span className={`dot ${statusCls}`}/>
              <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{monitor.name}</h1>
              {monitor.current_status === 'down'        && <span className="pill pill-down"><AlertCircle size={11}/> Outage</span>}
              {monitor.current_status === 'warn'        && <span className="pill pill-warn">Degraded</span>}
              {monitor.current_status === 'up'          && <span className="pill pill-up">Healthy</span>}
              {monitor.current_status === 'paused'      && <span className="pill pill-paused"><Pause size={11}/> Paused</span>}
              {monitor.current_status === 'pending'     && <span className="pill pill-pending">Pending first check</span>}
              {monitor.current_status === 'maintenance' && <span className="pill pill-maint">Maintenance</span>}
            </div>
            <div style={{ display: 'flex', gap: 14, fontSize: 13, color: 'var(--text-2)', flexWrap: 'wrap' }}>
              <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <Globe size={13} color="var(--text-3)"/> {KIND_LABEL[monitor.kind] || monitor.kind.toUpperCase()}
                {monitor.kind === 'http' && monitor.http_method && ` · ${monitor.http_method}`}
              </span>
              {monitor.url && <span className="mono" style={{ color: 'var(--text-2)' }}>{monitor.url}</span>}
              {monitor.hostname && (
                <span className="mono" style={{ color: 'var(--text-2)' }}>
                  {monitor.hostname}{monitor.port ? `:${monitor.port}` : ''}
                </span>
              )}
              <span style={{ color: 'var(--text-3)' }}>·</span>
              <span>Every {monitor.interval_seconds}s</span>
              <span style={{ color: 'var(--text-3)' }}>·</span>
              <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                <span style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--up)' }}/>
                checked {formatRelative(lastTs)}
              </span>
            </div>
          </div>

          <div style={{ display: 'flex', gap: 6 }}>
            <button className="btn" onClick={doPauseResume} disabled={acting}>
              {monitor.active ? <><Pause size={13}/> Pause</> : <><Play size={13}/> Resume</>}
            </button>
            <button className="btn" disabled title="Editing isn't wired yet"><Edit3 size={13}/> Edit</button>
            <button className="btn btn-danger" onClick={doDelete} disabled={acting}><Trash2 size={13}/> Delete</button>
          </div>
        </div>

        <div className="tabs" style={{ marginBottom: -1 }}>
          {['overview', 'heartbeats', 'config'].map(t => (
            <button key={t} className={tab === t ? 'active' : ''} onClick={() => setTab(t)}>
              {t.charAt(0).toUpperCase() + t.slice(1)}
            </button>
          ))}
        </div>
      </header>

      <main style={{ padding: '24px 32px', maxWidth: 1200, margin: '0 auto' }}>
        {/* KPI row */}
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12, marginBottom: 20 }}>
          <Kpi
            label="Uptime · 24h"
            value={uptime24h != null ? uptime24h.toFixed(2) : '—'}
            suffix={uptime24h != null ? '%' : null}
            sub={summary24h ? `${summary24h.up}/${summary24h.total} checks ok` : 'no data yet'}
            color={uptime24h != null && uptime24h < 99 ? 'var(--down)' : uptime24h != null && uptime24h < 99.9 ? 'var(--warn)' : 'var(--up)'}
          />
          <Kpi
            label="Uptime · 30d"
            value={uptime30d != null ? uptime30d.toFixed(2) : '—'}
            suffix={uptime30d != null ? '%' : null}
            sub={summary30d ? `${summary30d.up}/${summary30d.total} checks ok` : 'no data yet'}
          />
          <Kpi
            label="Avg response"
            value={avgLatency24h != null ? Math.round(avgLatency24h) : '—'}
            suffix={avgLatency24h != null ? 'ms' : null}
            sub="successful checks · 24h"
          />
          <Kpi
            label="Interval"
            value={monitor.interval_seconds}
            suffix="s"
            sub={`timeout ${monitor.timeout_seconds}s`}
          />
        </div>

        {/* ─── OVERVIEW TAB ──────────────────────────────────────── */}
        {tab === 'overview' && <>

        {monitor.kind === 'push' && monitor.push_token && (
          <PushUrlCard token={monitor.push_token} lastPushAt={monitor.last_push_at} interval={monitor.interval_seconds}/>
        )}

        {/* 90-day uptime strip */}
        <div className="card" style={{ padding: '20px 22px', marginBottom: 20 }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
            <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>90-day uptime</h3>
            <span className="tabular mono" style={{ fontSize: 12, color: 'var(--text-2)' }}>
              {incidentCount > 0 ? `${incidentCount} day${incidentCount === 1 ? '' : 's'} with downtime` : 'No recorded downtime'}
            </span>
          </div>
          <div className="uptime-bar">
            {sample90.map((c, i) => <div key={i} className={c} title={`${90 - i} day${90 - i === 1 ? '' : 's'} ago: ${c.replace('ub-','')}`}/>)}
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 8, fontSize: 11, color: 'var(--text-3)' }}>
            <span>90 days ago</span>
            <span>Today</span>
          </div>
        </div>

        {/* response time chart */}
        <div className="card" style={{ padding: '20px 22px', marginBottom: 20 }}>
          <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', marginBottom: 18 }}>
            <div>
              <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>Response time</h3>
              <p style={{ fontSize: 12, color: 'var(--text-3)', margin: '4px 0 0' }}>
                {heartbeats.length > 0 ? `${heartbeats.length} samples · binned to ${responseData.length} points` : 'No samples yet'}
              </p>
            </div>
            <button className="btn"><Calendar size={13}/> All samples <ChevronDown size={11}/></button>
          </div>
          <div style={{ height: 220 }}>
            {responseData.length > 0 ? (
              <ResponsiveContainer>
                <AreaChart data={responseData} margin={{ top: 5, right: 5, left: -10, bottom: 0 }}>
                  <defs>
                    <linearGradient id="lat" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%"   stopColor="var(--accent)" stopOpacity={0.3}/>
                      <stop offset="100%" stopColor="var(--accent)" stopOpacity={0}/>
                    </linearGradient>
                  </defs>
                  <XAxis dataKey="label" stroke="var(--text-3)" tick={{ fontSize: 11, fontFamily: 'JetBrains Mono' }}
                    interval={Math.max(1, Math.floor(responseData.length / 8))} tickLine={false}
                    axisLine={{ stroke: 'var(--border)' }}/>
                  <YAxis stroke="var(--text-3)" tick={{ fontSize: 11, fontFamily: 'JetBrains Mono' }}
                    tickLine={false} axisLine={false} tickFormatter={v => `${v}ms`}/>
                  <Tooltip contentStyle={{
                    background: 'var(--surface)', border: '1px solid var(--border)',
                    borderRadius: 8, fontSize: 12, boxShadow: '0 4px 12px rgba(0,0,0,.08)'
                  }}/>
                  <ReferenceLine y={500} stroke="var(--warn)" strokeDasharray="3 4"
                    label={{ value: 'slow', fill: 'var(--warn)', fontSize: 10, position: 'right' }}/>
                  <Area type="monotone" dataKey="latency" stroke="var(--accent)" strokeWidth={1.8}
                    fill="url(#lat)" connectNulls isAnimationActive={false}/>
                </AreaChart>
              </ResponsiveContainer>
            ) : (
              <div className="empty" style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                No samples yet — wait for the next probe.
              </div>
            )}
          </div>
        </div>

        {/* recent heartbeats + downtime side by side */}
        <div style={{ display: 'grid', gridTemplateColumns: '1.6fr 1fr', gap: 16 }}>
          <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
            <div style={{ padding: '16px 18px', display: 'flex', alignItems: 'center', justifyContent: 'space-between', borderBottom: '1px solid var(--border)' }}>
              <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
                <Activity size={14} color="var(--text-2)"/> Recent heartbeats
              </h3>
              <div className="tabs">
                <button className={logFilter === 'all'  ? 'active' : ''} onClick={() => setLogFilter('all')}>All</button>
                <button className={logFilter === 'fail' ? 'active' : ''} onClick={() => setLogFilter('fail')}>Failures</button>
              </div>
            </div>
            <div style={{
              display: 'grid', gridTemplateColumns: '90px 80px 80px 1fr 60px',
              gap: 16, padding: '10px 18px', fontSize: 11, fontWeight: 600,
              color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em',
              background: 'var(--surface-2)', borderBottom: '1px solid var(--border)'
            }}>
              <span>Time</span><span>Status</span><span style={{ textAlign: 'right' }}>Latency</span><span>Message</span><span style={{ textAlign: 'right' }}>Code</span>
            </div>
            {filteredLog.length === 0 ? (
              <div className="empty">{heartbeatState.loading ? 'Loading…' : 'No heartbeats match this filter.'}</div>
            ) : filteredLog.map((h, i) => {
              const date = h.ts instanceof Array ? offsetDateTimeArrayToDate(h.ts) : new Date(h.ts);
              const t = date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
              return (
                <div key={`${h.monitor_id}-${i}`} className="hb-row">
                  <span className="mono" style={{ color: 'var(--text-2)' }}>{t}</span>
                  <span><span className={`pill pill-${h.status === 'maintenance' ? 'maint' : h.status}`}>{h.status}</span></span>
                  <span className="mono tabular" style={{ textAlign: 'right', color: h.status === 'up' ? 'var(--text-2)' : 'var(--down)' }}>
                    {h.latency_ms == null ? '—' : h.latency_ms >= 1000 ? `${(h.latency_ms / 1000).toFixed(1)}s` : `${h.latency_ms}ms`}
                  </span>
                  <span style={{ color: h.status === 'up' ? 'var(--text-3)' : 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {h.msg || (h.status === 'up' ? 'OK' : '')}
                  </span>
                  <span className="mono tabular" style={{ textAlign: 'right', color: 'var(--text-2)' }}>{h.status_code ?? '—'}</span>
                </div>
              );
            })}
            <div style={{ padding: '12px 18px', textAlign: 'center', borderTop: '1px solid var(--border)', fontSize: 11, color: 'var(--text-3)' }}>
              showing {filteredLog.length} of {heartbeats.length} loaded
            </div>
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
            <div className="card" style={{ padding: '18px 20px' }}>
              <h3 style={{ fontSize: 14, fontWeight: 600, margin: '0 0 12px', display: 'flex', alignItems: 'center', gap: 8 }}>
                <AlertCircle size={14} color="var(--down)"/> Recent downtime
              </h3>
              {downtime.length === 0 ? (
                <div className="empty" style={{ padding: 0 }}>No downtime in loaded history.</div>
              ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                  {downtime.map((d, i) => (
                    <div key={i} style={{ paddingBottom: 12, borderBottom: i < downtime.length - 1 ? '1px solid var(--border)' : 'none' }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
                        <span style={{ fontSize: 12.5, fontWeight: 500 }}>{d.cause}</span>
                        <span className="mono tabular" style={{ fontSize: 12, color: 'var(--down)', fontWeight: 500 }}>{d.dur}</span>
                      </div>
                      <div style={{ fontSize: 11, color: 'var(--text-3)' }}>
                        {d.from.toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' })}
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>

            <MonitorChannels monitorId={monitor.id} />
          </div>
        </div>

        </>}{/* end overview tab */}

        {/* ─── HEARTBEATS TAB ─────────────────────────────────────── */}
        {tab === 'heartbeats' && (
          <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
            <div style={{ padding: '16px 18px', display: 'flex', alignItems: 'center', justifyContent: 'space-between', borderBottom: '1px solid var(--border)' }}>
              <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
                <Activity size={14} color="var(--text-2)"/> Heartbeats
                <span style={{ fontSize: 11, color: 'var(--text-3)', fontWeight: 400, marginLeft: 6 }}>
                  · {heartbeats.length} loaded
                </span>
              </h3>
              <div className="tabs">
                <button className={logFilter === 'all'  ? 'active' : ''} onClick={() => setLogFilter('all')}>All</button>
                <button className={logFilter === 'fail' ? 'active' : ''} onClick={() => setLogFilter('fail')}>Failures</button>
              </div>
            </div>
            <div style={{
              display: 'grid', gridTemplateColumns: '110px 90px 100px 1fr 70px',
              gap: 16, padding: '10px 22px', fontSize: 11, fontWeight: 600,
              color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em',
              background: 'var(--surface-2)', borderBottom: '1px solid var(--border)'
            }}>
              <span>Time</span><span>Status</span><span style={{ textAlign: 'right' }}>Latency</span><span>Message</span><span style={{ textAlign: 'right' }}>Code</span>
            </div>
            {(logFilter === 'all' ? heartbeats : heartbeats.filter(h => h.status !== 'up')).length === 0 ? (
              <div className="empty">{heartbeatState.loading ? 'Loading…' : 'No heartbeats match this filter.'}</div>
            ) : (logFilter === 'all' ? heartbeats : heartbeats.filter(h => h.status !== 'up')).map((h, i) => {
              const date = h.ts instanceof Array ? offsetDateTimeArrayToDate(h.ts) : new Date(h.ts);
              const t = date.toLocaleString('en-GB', { dateStyle: 'short', timeStyle: 'medium' });
              return (
                <div key={`${h.monitor_id}-${i}`} style={{
                  display: 'grid', gridTemplateColumns: '110px 90px 100px 1fr 70px',
                  gap: 16, padding: '12px 22px', fontSize: 12,
                  borderTop: '1px solid var(--border)', alignItems: 'center',
                }}>
                  <span className="mono" style={{ color: 'var(--text-2)' }}>{t}</span>
                  <span><span className={`pill pill-${h.status === 'maintenance' ? 'maint' : h.status}`}>{h.status}</span></span>
                  <span className="mono tabular" style={{ textAlign: 'right', color: h.status === 'up' ? 'var(--text-2)' : 'var(--down)' }}>
                    {h.latency_ms == null ? '—' : h.latency_ms >= 1000 ? `${(h.latency_ms / 1000).toFixed(1)}s` : `${h.latency_ms}ms`}
                  </span>
                  <span style={{ color: h.status === 'up' ? 'var(--text-3)' : 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {h.msg || (h.status === 'up' ? 'OK' : '')}
                  </span>
                  <span className="mono tabular" style={{ textAlign: 'right', color: 'var(--text-2)' }}>{h.status_code ?? '—'}</span>
                </div>
              );
            })}
            <div style={{ padding: '12px 22px', textAlign: 'center', borderTop: '1px solid var(--border)', fontSize: 11, color: 'var(--text-3)' }}>
              Loaded the most recent {heartbeats.length} heartbeats — more land as the scheduler probes.
            </div>
          </div>
        )}

        {/* ─── CONFIG TAB ─────────────────────────────────────────── */}
        {tab === 'config' && (
          <ConfigPanel monitor={monitor}/>
        )}

        <div style={{ height: 40 }}/>
      </main>
    </div>
  );
}

// ── Config panel ──────────────────────────────────────────────────────────
// Read-only display of the monitor's current configuration. Edit is wired
// to a "not implemented" stub on the header for now — a real edit form
// would land here.
function ConfigPanel({ monitor }) {
  const row = (label, value, mono = false) => (
    <div style={{
      display: 'grid', gridTemplateColumns: '180px 1fr',
      gap: 16, padding: '12px 22px', alignItems: 'baseline',
      borderTop: '1px solid var(--border)',
    }}>
      <span style={{ fontSize: 12, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 500 }}>
        {label}
      </span>
      <span className={mono ? 'mono' : ''} style={{ fontSize: 13, color: value == null || value === '' ? 'var(--text-3)' : 'var(--text)', wordBreak: 'break-all' }}>
        {value == null || value === '' ? '—' : value}
      </span>
    </div>
  );

  const acceptedStatuses = Array.isArray(monitor.accepted_statuses) && monitor.accepted_statuses.length > 0
    ? monitor.accepted_statuses.join(', ')
    : null;

  return (
    <>
      <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
        <div style={{ padding: '16px 22px' }}>
          <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>Target</h3>
        </div>
        {row('Kind',       monitor.kind, true)}
        {row('Display name', monitor.name)}
        {row('URL',        monitor.url, true)}
        {row('Hostname',   monitor.hostname, true)}
        {row('Port',       monitor.port, true)}
      </div>

      <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
        <div style={{ padding: '16px 22px' }}>
          <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>Schedule</h3>
        </div>
        {row('Interval',         `${monitor.interval_seconds}s`, true)}
        {row('Timeout',          `${monitor.timeout_seconds}s`,  true)}
        {row('Max retries',      monitor.max_retries,             true)}
        {row('Retry interval',   `${monitor.retry_interval_sec}s`, true)}
        {row('Re-alert every',   monitor.resend_interval_sec > 0 ? `${monitor.resend_interval_sec}s` : 'once', true)}
        {row('Upside-down mode', monitor.upside_down ? 'yes (failed checks count as up)' : 'no')}
        {row('Active',           monitor.active ? 'yes' : 'paused')}
        {row('Current status',   monitor.current_status, true)}
      </div>

      {(monitor.kind === 'http' || monitor.kind === 'keyword' || monitor.kind === 'json_query') && (
        <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
          <div style={{ padding: '16px 22px' }}>
            <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>HTTP</h3>
          </div>
          {row('Method',            monitor.http_method, true)}
          {row('Accepted statuses', acceptedStatuses, true)}
          {row('Follow redirects',  monitor.follow_redirect ? 'yes' : 'no')}
          {row('Ignore TLS errors', monitor.ignore_tls ? 'yes (insecure)' : 'no')}
          {row('Headers',           monitor.http_headers ? JSON.stringify(monitor.http_headers) : null, true)}
          {row('Body',              monitor.http_body, true)}
        </div>
      )}

      {monitor.config && Object.keys(monitor.config).length > 0 && (
        <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
          <div style={{ padding: '16px 22px' }}>
            <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>Kind-specific config</h3>
          </div>
          <pre className="mono" style={{
            margin: 0, padding: '14px 22px', fontSize: 12,
            background: 'var(--surface-2)', overflow: 'auto', borderTop: '1px solid var(--border)',
          }}>
{JSON.stringify(monitor.config, null, 2)}
          </pre>
        </div>
      )}

      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <div style={{ padding: '16px 22px' }}>
          <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>Identity</h3>
        </div>
        {row('Monitor ID', monitor.id, true)}
        {row('Created at', monitor.created_at instanceof Array
          ? offsetDateTimeArrayToDate(monitor.created_at).toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' })
          : new Date(monitor.created_at).toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' }), true)}
        {row('Updated at', monitor.updated_at instanceof Array
          ? offsetDateTimeArrayToDate(monitor.updated_at).toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' })
          : new Date(monitor.updated_at).toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' }), true)}
      </div>
    </>
  );
}

// ── Notifications card on the monitor-detail sidebar ──────────────────────
// Lists channels attached to this monitor, lets you attach/detach existing
// channels (created in /#/notifications), and send a test.
function MonitorChannels({ monitorId }) {
  const [reloadKey, setReloadKey] = useState(0);
  const attached = useApi(() => api.notifications.forMonitor(monitorId), [monitorId, reloadKey]);
  const all = useApi(() => api.notifications.list(), [reloadKey]);
  const [showPicker, setShowPicker] = useState(false);

  const attachedIds = new Set((attached.data || []).map(c => c.id));
  const available = (all.data || []).filter(c => !attachedIds.has(c.id));

  const bounce = () => setReloadKey(k => k + 1);

  const attach = async (nid) => {
    try { await api.notifications.attach(monitorId, nid); bounce(); setShowPicker(false); }
    catch (e) { alert(e.message); }
  };
  const detach = async (nid) => {
    try { await api.notifications.detach(monitorId, nid); bounce(); }
    catch (e) { alert(e.message); }
  };
  const sendTest = async (nid) => {
    try { await api.notifications.test(nid); alert('Test sent. Check the channel.'); }
    catch (e) { alert(e.message); }
  };

  return (
    <div className="card" style={{ padding: '18px 20px' }}>
      <h3 style={{ fontSize: 14, fontWeight: 600, margin: '0 0 12px', display: 'flex', alignItems: 'center', gap: 8 }}>
        <Bell size={14} color="var(--text-2)"/> Notifications
      </h3>

      {(attached.data || []).length === 0 && !attached.loading && (
        <div className="empty" style={{ padding: 0, fontSize: 12, marginBottom: 10 }}>
          No channels attached. Create channels at <a href="#/notifications" style={{ color: 'var(--accent)' }}>Notifications</a>, then attach them here.
        </div>
      )}

      {(attached.data || []).map(c => (
        <div key={c.id} style={{
          display: 'grid', gridTemplateColumns: '1fr auto auto', alignItems: 'center', gap: 8,
          padding: '8px 0', borderTop: '1px solid var(--border)',
        }}>
          <div>
            <div style={{ fontSize: 12.5, fontWeight: 500 }}>{c.name}</div>
            <div style={{ fontSize: 10, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em' }}>{c.kind}</div>
          </div>
          <button className="btn" onClick={() => sendTest(c.id)} title="Send test" style={{ padding: '4px 8px', fontSize: 11 }}>
            <Send size={11}/>
          </button>
          <button className="btn btn-danger" onClick={() => detach(c.id)} title="Detach" style={{ padding: '4px 8px', fontSize: 11 }}>
            <X size={11}/>
          </button>
        </div>
      ))}

      {showPicker ? (
        <div style={{ marginTop: 12, padding: 10, border: '1px solid var(--border)', borderRadius: 8, background: 'var(--surface-2)' }}>
          <div style={{ fontSize: 11, color: 'var(--text-3)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 600 }}>Attach an existing channel</div>
          {available.length === 0 ? (
            <div className="empty" style={{ padding: '6px 0', fontSize: 12 }}>
              No more channels to attach. <a href="#/notifications" style={{ color: 'var(--accent)' }}>Create one →</a>
            </div>
          ) : (
            available.map(c => (
              <div key={c.id} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '6px 0' }}>
                <div>
                  <div style={{ fontSize: 12.5 }}>{c.name}</div>
                  <div style={{ fontSize: 10, color: 'var(--text-3)', textTransform: 'uppercase' }}>{c.kind}</div>
                </div>
                <button className="btn btn-accent" onClick={() => attach(c.id)} style={{ padding: '4px 10px', fontSize: 11 }}>Attach</button>
              </div>
            ))
          )}
          <button className="btn btn-ghost" onClick={() => setShowPicker(false)} style={{ marginTop: 6, padding: '4px 8px', fontSize: 11 }}>Cancel</button>
        </div>
      ) : (
        <button className="btn btn-ghost" onClick={() => setShowPicker(true)} style={{ marginTop: 12, width: '100%', justifyContent: 'center', fontSize: 12 }}>
          <Plus size={11}/> Attach channel
        </button>
      )}
    </div>
  );
}

// ── Push-URL card. Shown on push monitors so the user can copy the
//    endpoint into their cron / CI / backup script. Server-side, the
//    token is what authenticates the heartbeat (so treat it like a
//    secret — anyone with the URL can mark this monitor up).
function PushUrlCard({ token, lastPushAt, interval }) {
  const [copied, setCopied] = useState(false);
  const url = `${window.location.origin}/push/${token}?status=up&msg=ok`;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch { /* clipboard refused — silently no-op */ }
  };

  const lastPushDate = lastPushAt ? offsetDateTimeArrayToDate(lastPushAt) : null;
  return (
    <div className="card" style={{ padding: '18px 22px', marginBottom: 20, borderLeft: '3px solid var(--accent)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
        <Zap size={14} color="var(--accent)"/>
        <h3 style={{ fontSize: 13, fontWeight: 600, margin: 0 }}>Push endpoint</h3>
      </div>
      <p style={{ fontSize: 12, color: 'var(--text-2)', margin: '0 0 12px', lineHeight: 1.5 }}>
        Have your cron / CI / backup job call this URL on each successful run.
        If we don't hear from it within {interval}s (plus grace), the monitor flips to Down.
      </p>
      <div style={{ display: 'flex', gap: 8, alignItems: 'stretch' }}>
        <code className="mono" style={{
          flex: 1, padding: '8px 10px', fontSize: 12, background: 'var(--surface-2)',
          border: '1px solid var(--border)', borderRadius: 6, color: 'var(--text)',
          overflow: 'auto', whiteSpace: 'nowrap',
        }}>{url}</code>
        <button className="btn btn-ghost" onClick={copy} style={{ padding: '0 12px', fontSize: 12 }}>
          {copied ? <><Check size={12}/> Copied</> : <><Copy size={12}/> Copy</>}
        </button>
      </div>
      <div style={{ marginTop: 10, fontSize: 11, color: 'var(--text-3)' }}>
        Last push: {lastPushDate ? formatRelative(lastPushDate) : 'never'}
      </div>
    </div>
  );
}
