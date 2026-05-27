import React, { useMemo, useState } from 'react';
import {
  LineChart, Line, XAxis, YAxis,
  ResponsiveContainer, Tooltip,
} from 'recharts';
import {
  Search, Plus, Bell, Sun, ChevronDown, ChevronRight, Activity,
  AlertCircle, Pause, MoreHorizontal, Calendar,
  Tag, ArrowUpRight, Wrench, Zap, Globe, Server,
  Database, Radio, Lock, Hash, Filter,
} from 'lucide-react';
import {
  api, useApi, formatRelative, offsetDateTimeArrayToDate, statusToClass,
} from '../lib/api.js';

// ──────────────────────────────────────────────────────────────────────────
// DESIGN SYSTEM v2 — friendly, modern, operator-focused
// Different from Rampart v1's terminal aesthetic. Aimed at indie devs,
// homelabs, and small teams. Inter sans, teal accent, rounded.
// ──────────────────────────────────────────────────────────────────────────
const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');

  .rampart {
    --bg:        #fafaf9;
    --surface:   #ffffff;
    --surface-2: #f5f5f4;
    --border:    #e7e5e4;
    --border-2:  #d6d3d1;
    --text:      #1c1917;
    --text-2:    #57534e;
    --text-3:    #a8a29e;

    --accent:      #14b8a6;
    --accent-2:    #0d9488;
    --accent-soft: #ccfbf1;

    --up:        #10b981;
    --up-soft:   #d1fae5;
    --down:      #ef4444;
    --down-soft: #fee2e2;
    --warn:      #f59e0b;
    --warn-soft: #fef3c7;
    --maint:     #6366f1;
    --maint-soft:#e0e7ff;
    --paused:    #a8a29e;

    background: var(--bg);
    color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    font-feature-settings: 'cv11','ss01';
    min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', ui-monospace, monospace; font-feature-settings: 'zero'; }
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
  .btn-prim {
    background: var(--text); color: var(--surface); border-color: var(--text);
  }
  .btn-prim:hover { background: #000; color: var(--surface); }
  .btn-accent {
    background: var(--accent); color: white; border-color: var(--accent);
  }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-ghost { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }

  .pill {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 2px 8px; border-radius: 999px;
    font-size: 11px; font-weight: 500; line-height: 1.4;
  }
  .pill-up    { background: var(--up-soft);    color: #047857; }
  .pill-down  { background: var(--down-soft);  color: #b91c1c; }
  .pill-warn  { background: var(--warn-soft);  color: #b45309; }
  .pill-maint { background: var(--maint-soft); color: #4338ca; }
  .pill-paused{ background: var(--surface-2);  color: var(--text-2); }

  .dot { width: 8px; height: 8px; border-radius: 50%; display: inline-block; flex-shrink: 0; }
  .dot.up     { background: var(--up); box-shadow: 0 0 0 3px var(--up-soft); }
  .dot.down   { background: var(--down); box-shadow: 0 0 0 3px var(--down-soft); }
  .dot.warn   { background: var(--warn); box-shadow: 0 0 0 3px var(--warn-soft); }
  .dot.maint  { background: var(--maint); box-shadow: 0 0 0 3px var(--maint-soft); }
  .dot.paused { background: var(--paused); }

  .mon-row {
    display: grid; grid-template-columns: 16px 1fr auto auto;
    align-items: center; gap: 10px;
    padding: 10px 12px; border-radius: 8px; cursor: pointer;
    transition: background .1s;
  }
  .mon-row:hover { background: var(--surface-2); }
  .mon-row.active { background: var(--accent-soft); }

  /* 60-cell mini history bar */
  .uptime-bar { display: flex; gap: 2px; height: 22px; }
  .uptime-bar > div { flex: 1; border-radius: 2px; min-width: 2px; }
  .ub-up    { background: var(--up); opacity: .9; }
  .ub-up:hover { opacity: 1; cursor: pointer; }
  .ub-down  { background: var(--down); }
  .ub-warn  { background: var(--warn); }
  .ub-maint { background: var(--maint); opacity: .7; }
  .ub-none  { background: var(--border); }

  input.search {
    width: 100%; padding: 9px 12px 9px 36px;
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 8px; font-size: 13px; color: var(--text); outline: none;
    transition: border-color .12s;
  }
  input.search:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }

  .kbd {
    padding: 1px 5px; border-radius: 4px; background: var(--surface-2);
    border: 1px solid var(--border); font-size: 10px; color: var(--text-2);
    font-family: 'JetBrains Mono', monospace;
  }

  .group-head {
    display: flex; align-items: center; gap: 8px;
    padding: 12px 12px 6px;
    font-size: 11px; font-weight: 600; color: var(--text-3);
    text-transform: uppercase; letter-spacing: .05em; cursor: pointer;
  }
  .group-head:hover { color: var(--text-2); }

  .empty {
    padding: 24px 18px; text-align: center;
    color: var(--text-3); font-size: 13px;
  }
`;

// ─── kind → icon ─────────────────────────────────────────────────────────
const KIND_ICON = {
  http:       Globe,
  keyword:    Globe,
  json_query: Globe,
  tcp:        Server,
  ping:       Radio,
  dns:        Hash,
  push:       Zap,
  grpc:       Server,
  tls:        Lock,
  docker:     Server,
  steam:      Server,
  mqtt:       Radio,
  radius:     Server,
  kafka:      Database,
  postgres:   Database,
  mysql:      Database,
  mssql:      Database,
  redis:      Database,
  mongodb:    Database,
  domain:     Lock,
};
const kindIcon = (k) => KIND_ICON[k] || Activity;

// ─── 60-cell history derived from real heartbeats ────────────────────────
// Heartbeats arrive oldest-first per monitor (the backend ORDERs BY ts ASC).
// We map status -> css class and left-pad with `ub-none` so monitors with
// short history still take 60 columns.
function heartbeatsToCells(hbs, paused) {
  const cls = (s) => {
    if (paused)            return 'ub-none';
    if (s === 'up')        return 'ub-up';
    if (s === 'down')      return 'ub-down';
    if (s === 'warn')      return 'ub-warn';
    if (s === 'maintenance') return 'ub-maint';
    return 'ub-none';
  };
  const cells = (hbs || []).slice(-60).map(h => cls(h.status));
  while (cells.length < 60) cells.unshift('ub-none');
  return cells;
}

// ─── monitor row in sidebar ───────────────────────────────────────────────
function MonitorRow({ m, active, onClick, uptimePct }) {
  const Icon = kindIcon(m.kind);
  const cls = statusToClass(m.current_status);
  return (
    <div className={`mon-row ${active ? 'active' : ''}`} onClick={onClick}>
      <span className={`dot ${cls}`}/>
      <div style={{ minWidth: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
        <Icon size={13} color="var(--text-3)" strokeWidth={1.8}/>
        <span style={{ fontSize: 13, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{m.name}</span>
      </div>
      <span className="mono tabular" style={{ fontSize: 11, color: m.current_status === 'down' ? 'var(--down)' : m.current_status === 'paused' ? 'var(--text-3)' : 'var(--text-2)' }}>
        {uptimePct != null ? `${uptimePct.toFixed(2)}%` : '—'}
      </span>
      <MoreHorizontal size={14} color="var(--text-3)" style={{ opacity: 0 }} className="row-action"/>
    </div>
  );
}

// ─── trend chart payload from heartbeats ─────────────────────────────────
// Picks up to 4 monitors with the most heartbeats and plots their latencies
// against a common time axis (most-recent-window samples). Each line is a
// monitor; we key by short name so the legend stays readable.
function buildTrend(historyById, monitorsById) {
  const ids = [...historyById.keys()]
    .map(id => ({ id, count: historyById.get(id).length }))
    .sort((a, b) => b.count - a.count)
    .slice(0, 4)
    .map(x => x.id);
  if (ids.length === 0) return { rows: [], series: [] };

  const series = ids.map((id, i) => {
    const name = monitorsById.get(id)?.name || id.slice(0, 6);
    return { id, name, color: SERIES_COLORS[i % SERIES_COLORS.length] };
  });

  // Use the longest history for time labels, oldest -> newest.
  const longest = ids.reduce((a, b) => (historyById.get(a).length >= historyById.get(b).length ? a : b));
  const ref = historyById.get(longest);

  const rows = ref.map((h, idx) => {
    const t = h.ts instanceof Array ? offsetDateTimeArrayToDate(h.ts) : new Date(h.ts);
    const row = { t: idx, label: t.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' }) };
    series.forEach(s => {
      const hb = historyById.get(s.id)?.[idx];
      // Treat down samples as gaps so the line doesn't dive to 0.
      row[s.name] = (hb && hb.status === 'up') ? (hb.latency_ms ?? null) : null;
    });
    return row;
  });
  return { rows, series };
}
const SERIES_COLORS = ['#14b8a6', '#6366f1', '#10b981', '#ef4444'];

// ─── main component ───────────────────────────────────────────────────────
export default function Dashboard({ user, onLogout } = {}) {
  const monitorsState = useApi(() => api.monitors.list(),         [], { pollMs: 10_000 });
  const summaryState  = useApi(() => api.monitors.summary(86400), [], { pollMs: 15_000 });
  const historyState  = useApi(() => api.monitors.history(60),    [], { pollMs: 10_000 });
  const countsState   = useApi(() => api.notifications.counts(),  [], { pollMs: 30_000 });
  const channelCount  = useMemo(() => {
    const m = new Map();
    (countsState.data || []).forEach(r => m.set(r.monitor_id, r.count));
    return m;
  }, [countsState.data]);

  const monitors = monitorsState.data || [];
  const summaryById = useMemo(() => {
    const m = new Map();
    (summaryState.data || []).forEach(s => m.set(s.monitor_id, s));
    return m;
  }, [summaryState.data]);
  const historyById = useMemo(() => {
    const m = new Map();
    (historyState.data || []).forEach(h => {
      if (!m.has(h.monitor_id)) m.set(h.monitor_id, []);
      m.get(h.monitor_id).push(h);
    });
    return m;
  }, [historyState.data]);
  const monitorsById = useMemo(() => {
    const m = new Map();
    monitors.forEach(x => m.set(x.id, x));
    return m;
  }, [monitors]);

  const [openGroup, setOpenGroup] = useState(true);
  const [query, setQuery] = useState('');
  const [tagFilter, setTagFilter] = useState(new Set()); // tag IDs to require

  // All tags currently in use across the visible monitors. We don't fetch
  // /v1/tags separately — the hydrated tags on each monitor are enough
  // to render the filter bar.
  const tagsInUse = useMemo(() => {
    const seen = new Map();
    for (const m of monitors) for (const t of (m.tags || [])) seen.set(t.id, t);
    return Array.from(seen.values()).sort((a, b) => a.name.localeCompare(b.name));
  }, [monitors]);

  const toggleTagFilter = (id) => {
    setTagFilter(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const matchesTagFilter = (m) => {
    if (tagFilter.size === 0) return true;
    const ids = new Set((m.tags || []).map(t => t.id));
    for (const need of tagFilter) if (!ids.has(need)) return false;
    return true;
  };

  const filtered = monitors
    .filter(m => m.name.toLowerCase().includes(query.toLowerCase()))
    .filter(matchesTagFilter);

  const counts = monitors.reduce((acc, m) => {
    const k = m.current_status === 'maintenance' ? 'maint' : m.current_status;
    acc[k] = (acc[k] || 0) + 1; return acc;
  }, {});
  const upCount     = counts.up      || 0;
  const downCount   = counts.down    || 0;
  const warnCount   = counts.warn    || 0;
  const pausedCount = counts.paused  || 0;
  const anyDown   = downCount > 0;
  const anyWarn   = warnCount > 0;
  const heroTitle = monitors.length === 0 ? 'No monitors yet'
                  : anyDown ? `${downCount} service${downCount > 1 ? 's' : ''} down`
                  : anyWarn ? 'Some services are degraded'
                  : 'All systems operational';
  const heroColor = monitors.length === 0 ? 'var(--text-3)'
                  : anyDown ? 'var(--down)'
                  : anyWarn ? 'var(--warn)'
                  : 'var(--up)';
  const heroSoft  = monitors.length === 0 ? 'var(--surface-2)'
                  : anyDown ? 'var(--down-soft)'
                  : anyWarn ? 'var(--warn-soft)'
                  : 'var(--up-soft)';

  const trend = useMemo(() => buildTrend(historyById, monitorsById), [historyById, monitorsById]);

  const openMonitor    = (id) => { window.location.hash = `#/monitor/${id}`; };
  const goToNewMonitor = ()   => { window.location.hash = '#/new-monitor'; };
  const goToStatusPage = ()   => { window.location.hash = '#/status-page'; };

  // Stub data we don't have endpoints for yet. The empty states render cleanly.
  const recentIncidents = [];
  const upcomingMaint   = [];

  return (
    <div className="rampart">
      <style>{css}</style>

      {/* ─── top bar ─────────────────────────────────────────────── */}
      <header style={{
        display: 'flex', alignItems: 'center', gap: 16,
        padding: '12px 20px', borderBottom: '1px solid var(--border)',
        background: 'var(--surface)', position: 'sticky', top: 0, zIndex: 10
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div style={{
            width: 28, height: 28, borderRadius: 8,
            background: 'linear-gradient(135deg, var(--accent), var(--accent-2))',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            color: 'white', boxShadow: '0 2px 8px rgba(20,184,166,.35)'
          }}>
            <Activity size={15} strokeWidth={2.4}/>
          </div>
          <span style={{ fontSize: 15, fontWeight: 600, letterSpacing: '-.01em' }}>Rampart</span>
          <span className="pill" style={{ background: 'var(--surface-2)', color: 'var(--text-3)' }}>v0.4.0</span>
        </div>

        <div style={{ position: 'relative', flex: 1, maxWidth: 420 }}>
          <Search size={14} color="var(--text-3)" style={{ position: 'absolute', left: 12, top: '50%', transform: 'translateY(-50%)' }}/>
          <input className="search" placeholder="Search monitors…" value={query} onChange={e => setQuery(e.target.value)}/>
          <span className="kbd" style={{ position: 'absolute', right: 10, top: '50%', transform: 'translateY(-50%)' }}>⌘K</span>
        </div>

        <div style={{ display: 'flex', gap: 8, marginLeft: 'auto' }}>
          <button className="btn btn-ghost" title="Notifications"><Bell size={14}/></button>
          <button className="btn btn-ghost" title="Toggle theme"><Sun size={14}/></button>
          <button className="btn" onClick={goToStatusPage}><Wrench size={13}/> Status page</button>
          <button className="btn btn-accent" onClick={goToNewMonitor}><Plus size={13} strokeWidth={2.4}/> Add monitor</button>
          {user && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <div title={user.email} style={{
                width: 30, height: 30, borderRadius: '50%',
                background: 'linear-gradient(135deg, #fb923c, #ea580c)',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                color: 'white', fontSize: 12, fontWeight: 600, textTransform: 'uppercase',
              }}>
                {(user.name || user.email || '?').charAt(0)}
              </div>
              <button className="btn btn-ghost" onClick={onLogout} title="Sign out" style={{ fontSize: 12 }}>
                Sign out
              </button>
            </div>
          )}
        </div>
      </header>

      {/* ─── layout ──────────────────────────────────────────────── */}
      <div style={{ display: 'grid', gridTemplateColumns: '320px 1fr' }}>

        {/* ─── sidebar ───────────────────────────────────────────── */}
        <aside style={{
          borderRight: '1px solid var(--border)', background: 'var(--surface)',
          padding: '16px 12px', height: 'calc(100vh - 57px)', overflowY: 'auto',
          position: 'sticky', top: 57
        }}>
          {/* status summary card */}
          <div className="card" style={{ padding: 14, marginBottom: 16 }}>
            <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', marginBottom: 12 }}>
              <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em' }}>
                Status · last 24h
              </span>
              <span className="mono tabular" style={{ fontSize: 11, color: 'var(--text-3)' }}>{monitors.length} total</span>
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 8 }}>
              {[
                { label: 'up',     v: upCount,     color: 'var(--up)' },
                { label: 'warn',   v: warnCount,   color: 'var(--warn)' },
                { label: 'down',   v: downCount,   color: 'var(--down)' },
                { label: 'paused', v: pausedCount, color: 'var(--paused)' },
              ].map(s => (
                <div key={s.label} style={{ textAlign: 'center' }}>
                  <div className="tabular" style={{ fontSize: 22, fontWeight: 600, color: s.color, lineHeight: 1 }}>{s.v}</div>
                  <div style={{ fontSize: 10, color: 'var(--text-3)', marginTop: 4, textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 500 }}>{s.label}</div>
                </div>
              ))}
            </div>
          </div>

          {/* single "All monitors" group — backend has no group/folder concept */}
          {monitors.length > 0 ? (
            <div style={{ marginBottom: 4 }}>
              <div className="group-head" onClick={() => setOpenGroup(g => !g)}>
                {openGroup ? <ChevronDown size={11}/> : <ChevronRight size={11}/>}
                <span>monitors</span>
                <span style={{ marginLeft: 'auto', color: 'var(--text-3)', fontWeight: 500 }}>{filtered.length}</span>
              </div>
              {openGroup && filtered.map(m => (
                <MonitorRow
                  key={m.id}
                  m={m}
                  active={false}
                  onClick={() => openMonitor(m.id)}
                  uptimePct={summaryById.get(m.id)?.uptime_pct}
                />
              ))}
            </div>
          ) : !monitorsState.loading && (
            <div className="empty" style={{ paddingTop: 32 }}>
              No monitors yet.<br/>
              <button className="btn btn-accent" onClick={goToNewMonitor} style={{ marginTop: 12 }}>
                <Plus size={13}/> Create the first one
              </button>
            </div>
          )}

          {monitorsState.loading && monitors.length === 0 && (
            <div className="empty">Loading monitors…</div>
          )}
        </aside>

        {/* ─── main panel ────────────────────────────────────────── */}
        <main style={{ padding: '28px 36px', maxWidth: 1100 }}>

          {/* hero */}
          <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', marginBottom: 28 }}>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 6 }}>
                <span className="dot" style={{ background: heroColor, boxShadow: `0 0 0 5px ${heroSoft}` }}/>
                <h1 style={{ fontSize: 26, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{heroTitle}</h1>
              </div>
              <p style={{ fontSize: 14, color: 'var(--text-2)', margin: '0 0 0 20px' }}>
                {monitors.length === 0 && <>Create your first monitor to start receiving heartbeats.</>}
                {monitors.length > 0 && anyDown && <>{downCount} unreachable{warnCount > 0 ? `, ${warnCount} degraded` : ''} — auto-refreshing every 10s</>}
                {monitors.length > 0 && !anyDown && anyWarn && <>{warnCount} degraded — auto-refreshing every 10s</>}
                {monitors.length > 0 && !anyDown && !anyWarn && <>{monitors.length} monitor{monitors.length > 1 ? 's' : ''} healthy — auto-refreshing every 10s</>}
              </p>
            </div>
            <div style={{ display: 'flex', gap: 8 }}>
              <button className="btn"><Filter size={13}/> Filter</button>
              <button className="btn"><Calendar size={13}/> Last 24h <ChevronDown size={11}/></button>
            </div>
          </div>

          {/* response trend chart */}
          <div className="card" style={{ padding: '20px 22px', marginBottom: 20 }}>
            <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', marginBottom: 18 }}>
              <div>
                <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>Response time</h3>
                <p style={{ fontSize: 12, color: 'var(--text-3)', margin: '4px 0 0' }}>
                  {trend.series.length > 0 ? `Top ${trend.series.length} monitor${trend.series.length > 1 ? 's' : ''} · most recent samples` : 'No samples yet'}
                </p>
              </div>
              <div style={{ display: 'flex', gap: 14 }}>
                {trend.series.map(s => (
                  <span key={s.id} style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12, color: 'var(--text-2)' }}>
                    <span style={{ width: 8, height: 8, borderRadius: 2, background: s.color }}/>
                    {s.name}
                  </span>
                ))}
              </div>
            </div>
            <div style={{ height: 240 }}>
              {trend.rows.length > 0 ? (
                <ResponsiveContainer>
                  <LineChart data={trend.rows} margin={{ top: 5, right: 5, left: -10, bottom: 0 }}>
                    <XAxis dataKey="label" stroke="var(--text-3)"
                      tick={{ fontSize: 11, fontFamily: 'JetBrains Mono' }}
                      interval={Math.max(1, Math.floor(trend.rows.length / 8))} tickLine={false} axisLine={{ stroke: 'var(--border)' }}/>
                    <YAxis stroke="var(--text-3)"
                      tick={{ fontSize: 11, fontFamily: 'JetBrains Mono' }}
                      tickLine={false} axisLine={false}
                      tickFormatter={v => `${v}ms`}/>
                    <Tooltip
                      contentStyle={{
                        background: 'var(--surface)', border: '1px solid var(--border)',
                        borderRadius: 8, fontSize: 12, fontFamily: 'Inter',
                        boxShadow: '0 4px 12px rgba(0,0,0,.08)'
                      }}/>
                    {trend.series.map(s => (
                      <Line key={s.id} type="monotone" dataKey={s.name} stroke={s.color}
                        strokeWidth={1.8} dot={false} connectNulls isAnimationActive={false}/>
                    ))}
                  </LineChart>
                </ResponsiveContainer>
              ) : (
                <div className="empty" style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                  No heartbeats yet — create a monitor to see latency trends.
                </div>
              )}
            </div>
          </div>

          {/* two column: incidents + upcoming maintenance */}
          <div style={{ display: 'grid', gridTemplateColumns: '1.6fr 1fr', gap: 16, marginBottom: 20 }}>
            <div className="card" style={{ padding: '18px 20px' }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
                <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
                  <AlertCircle size={14} color="var(--down)"/> Recent incidents
                </h3>
                <button className="btn btn-ghost" style={{ padding: '4px 8px' }}>View all <ArrowUpRight size={11}/></button>
              </div>
              {recentIncidents.length === 0 ? (
                <div className="empty">No incidents recorded yet.</div>
              ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                  {recentIncidents.map(i => (
                    <div key={i.id} style={{
                      display: 'grid', gridTemplateColumns: '90px 1fr auto',
                      gap: 12, padding: '10px 0', borderTop: '1px solid var(--border)',
                      alignItems: 'center'
                    }}>
                      <span className="mono" style={{ fontSize: 11, color: 'var(--text-3)' }}>{i.id}</span>
                      <div style={{ minWidth: 0 }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 2 }}>
                          <span className={`pill pill-${i.sev}`}>{i.sev === 'down' ? 'outage' : 'degraded'}</span>
                          <span style={{ fontSize: 13, fontWeight: 500 }}>{i.monitor}</span>
                        </div>
                        <div style={{ fontSize: 12, color: 'var(--text-2)' }}>{i.note}</div>
                      </div>
                      <div style={{ textAlign: 'right' }}>
                        <div className="mono tabular" style={{ fontSize: 11, color: 'var(--text-2)' }}>{i.dur}</div>
                        <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 2 }}>{i.when}</div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div className="card" style={{ padding: '18px 20px' }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
                <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
                  <Wrench size={14} color="var(--maint)"/> Maintenance
                </h3>
                <button className="btn btn-ghost" style={{ padding: '4px 8px' }}><Plus size={11}/></button>
              </div>
              {upcomingMaint.length === 0 ? (
                <div className="empty">No upcoming maintenance scheduled.</div>
              ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                  {upcomingMaint.map(m => (
                    <div key={m.id} style={{ padding: '10px 12px', border: '1px solid var(--border)', borderRadius: 8, background: 'var(--surface-2)' }}>
                      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', marginBottom: 4 }}>
                        <span style={{ fontSize: 13, fontWeight: 500 }}>{m.title}</span>
                        {m.recurring && <span className="pill pill-maint">Recurring</span>}
                      </div>
                      <div className="mono" style={{ fontSize: 11, color: 'var(--text-2)' }}>{m.when}</div>
                      <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 3 }}>{m.dur} · affects {m.monitors} monitor{m.monitors > 1 ? 's' : ''}</div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* all monitors table with inline uptime history */}
          <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
            <div style={{ padding: '16px 22px', display: 'flex', alignItems: 'center', justifyContent: 'space-between', borderBottom: '1px solid var(--border)', gap: 10, flexWrap: 'wrap' }}>
              <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>All monitors</h3>
              {tagsInUse.length > 0 && (
                <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
                  <Tag size={11} color="var(--text-3)"/>
                  {tagsInUse.map(t => {
                    const on = tagFilter.has(t.id);
                    return (
                      <button key={t.id} onClick={() => toggleTagFilter(t.id)} style={{
                        display: 'inline-flex', alignItems: 'center', gap: 5,
                        padding: '3px 9px', borderRadius: 999,
                        fontSize: 11, fontWeight: 500, cursor: 'pointer',
                        background: on ? t.color : 'var(--surface-2)',
                        color:      on ? '#fff'   : 'var(--text-2)',
                        border: `1px solid ${on ? t.color : 'var(--border)'}`,
                      }}>
                        {t.name}
                      </button>
                    );
                  })}
                  {tagFilter.size > 0 && (
                    <button className="btn btn-ghost" onClick={() => setTagFilter(new Set())} style={{ padding: '2px 7px', fontSize: 11 }}>
                      Clear
                    </button>
                  )}
                </div>
              )}
            </div>

            <div style={{
              display: 'grid',
              gridTemplateColumns: '24px 1.4fr 70px 70px 1.5fr 60px',
              gap: 16, padding: '10px 22px',
              fontSize: 11, fontWeight: 600, color: 'var(--text-3)',
              textTransform: 'uppercase', letterSpacing: '.04em',
              background: 'var(--surface-2)', borderBottom: '1px solid var(--border)'
            }}>
              <span/>
              <span>Monitor</span>
              <span>Type</span>
              <span style={{ textAlign: 'right' }}>p50</span>
              <span>Last 60 checks</span>
              <span style={{ textAlign: 'right' }}>Uptime</span>
            </div>

            {monitors.length === 0 ? (
              <div className="empty" style={{ padding: '40px 18px' }}>
                {monitorsState.loading ? 'Loading…' : (
                  <>
                    No monitors yet.{' '}
                    <a href="#/new-monitor" style={{ color: 'var(--accent)', textDecoration: 'none', fontWeight: 500 }}>
                      Create the first one →
                    </a>
                  </>
                )}
              </div>
            ) : filtered.length === 0 ? (
              <div className="empty" style={{ padding: '40px 18px' }}>
                No monitors match the current filter.
              </div>
            ) : filtered.map(m => {
              const hist = heartbeatsToCells(historyById.get(m.id), m.current_status === 'paused');
              const summary = summaryById.get(m.id);
              const p50 = summary?.avg_latency_ms;
              const uptime = summary?.uptime_pct;
              const cls = statusToClass(m.current_status);
              return (
                <div key={m.id} onClick={() => openMonitor(m.id)} style={{
                  display: 'grid',
                  gridTemplateColumns: '24px 1.4fr 70px 70px 1.5fr 60px',
                  gap: 16, padding: '14px 22px',
                  borderBottom: '1px solid var(--border)', alignItems: 'center',
                  cursor: 'pointer',
                }}>
                  <span className={`dot ${cls}`}/>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    <span style={{ fontSize: 13, fontWeight: 500 }}>{m.name}</span>
                    {(m.tags || []).map(t => (
                      <span key={t.id} title={`tag: ${t.name}`} style={{
                        display: 'inline-flex', alignItems: 'center',
                        fontSize: 10, fontWeight: 500,
                        padding: '1px 6px', borderRadius: 999,
                        background: t.color, color: '#fff',
                      }}>{t.name}</span>
                    ))}
                    {m.current_status === 'down'        && <span className="pill pill-down">Outage</span>}
                    {m.current_status === 'warn'        && <span className="pill pill-warn">Degraded</span>}
                    {m.current_status === 'maintenance' && <span className="pill pill-maint">Maintenance</span>}
                    {m.current_status === 'paused'      && <span className="pill pill-paused"><Pause size={9}/> Paused</span>}
                    {m.current_status === 'pending'     && <span className="pill pill-paused">Pending</span>}
                    {(() => {
                      const n = channelCount.get(m.id) || 0;
                      return n > 0 ? (
                        <span title={`${n} notification channel${n > 1 ? 's' : ''} attached`}
                          style={{
                            display: 'inline-flex', alignItems: 'center', gap: 3,
                            fontSize: 10.5, color: 'var(--accent-2)',
                            background: 'var(--accent-soft)', padding: '2px 7px',
                            borderRadius: 999, fontWeight: 500,
                          }}>
                          <Bell size={9}/> {n}
                        </span>
                      ) : (
                        <span title="No notification channels attached — flips will record but won't alert anyone"
                          style={{
                            display: 'inline-flex', alignItems: 'center',
                            fontSize: 10.5, color: 'var(--text-3)',
                            background: 'var(--surface-2)', padding: '2px 6px',
                            borderRadius: 999,
                          }}>
                          <Bell size={9} style={{ opacity: .6 }}/>
                        </span>
                      );
                    })()}
                  </div>
                  <span style={{ fontSize: 12, color: 'var(--text-2)', textTransform: 'uppercase', fontWeight: 500, letterSpacing: '.04em' }}>{m.kind}</span>
                  <span className="mono tabular" style={{ fontSize: 12, color: m.current_status === 'down' ? 'var(--down)' : 'var(--text-2)', textAlign: 'right' }}>
                    {p50 != null ? `${Math.round(p50)}ms` : '—'}
                  </span>
                  <div className="uptime-bar">
                    {hist.map((c, i) => <div key={i} className={c}/>)}
                  </div>
                  <span className="mono tabular" style={{ fontSize: 12, color: uptime === 100 ? 'var(--up)' : uptime != null && uptime < 99 ? 'var(--down)' : 'var(--text)', textAlign: 'right', fontWeight: 500 }}>
                    {uptime != null ? `${uptime.toFixed(2)}%` : '—'}
                  </span>
                </div>
              );
            })}
          </div>

          {/* error footer */}
          {(monitorsState.error || summaryState.error || historyState.error) && (
            <div style={{ marginTop: 20, padding: 12, background: 'var(--down-soft)', color: '#b91c1c', borderRadius: 8, fontSize: 13 }}>
              API error: {(monitorsState.error || summaryState.error || historyState.error)?.message}
            </div>
          )}

          <div style={{ height: 40 }}/>
        </main>
      </div>
    </div>
  );
}
