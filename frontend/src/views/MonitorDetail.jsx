import React, { useMemo, useState, useEffect, useRef } from 'react';
import {
  AreaChart, Area, XAxis, YAxis, ResponsiveContainer, Tooltip, ReferenceLine,
} from 'recharts';
import {
  ChevronLeft, Pause, Play, Edit3, Trash2, Bell, Plus, X, Send, Download,
  Globe, Server, Lock, AlertCircle, Activity, Hash, Radio, Database,
  MoreHorizontal, Calendar, ChevronDown, Copy, Check, Zap, Loader2, FileStack,
} from 'lucide-react';
import {
  api, useApi, formatRelative, offsetDateTimeArrayToDate, statusToClass,
} from '../lib/api.js';
import { canWrite } from '../lib/roles.js';
import { t } from '../lib/i18n.js';
import { toast, confirmDialog, promptDialog } from '../lib/notify.js';
import { useFocusTrap } from '../lib/useFocusTrap.js';
import { SyntheticSteps, configToUiSteps, uiToConfigSteps } from '../components/SyntheticSteps.jsx';

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

    /* Foreground text colours for the soft status backgrounds. Pulled out
       as their own tokens so the dark-theme override below can flip them —
       the literal #047857 / #b91c1c values are tuned for the LIGHT soft
       backgrounds and turn unreadable once --up-soft / --down-soft flip to
       their dark equivalents. */
    --up-text:#047857; --down-text:#b91c1c; --warn-text:#92400e; --maint-text:#4338ca;
    /* Result-panel tints for the "Test notifications" outcome banner. */
    --up-bg:#f0fdf4; --down-bg:#fef2f2;

    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    font-feature-settings: 'cv11','ss01';
    min-height: 100vh;
  }

  /* Dark-mode overrides for the tokens theme.js doesn't flip (it covers
     --up/--down + their -soft variants, but not --warn/--maint-soft nor the
     new -text / result-panel tints above). Higher specificity than the base
     .rampart block, lower churn than touching theme.js. */
  [data-theme="dark"] .rampart {
    --warn-soft:#78350f; --maint-soft:#3730a3;
    --up-text:#6ee7b7; --down-text:#fca5a5; --warn-text:#fde68a; --maint-text:#c7d2fe;
    --up-bg:#052e16; --down-bg:#450a0a;
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
  .pill-up     { background: var(--up-soft);    color: var(--up-text); }
  .pill-down   { background: var(--down-soft);  color: var(--down-text); }
  .pill-warn   { background: var(--warn-soft);  color: var(--warn-text); }
  .pill-maint  { background: var(--maint-soft); color: var(--maint-text); }
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

  /* Form controls — the .input / select.input / textarea.input rules were
     missing from this view's CSS block, so the EditModal's inputs fell
     through to browser-default styling. */
  .input {
    width: 100%; padding: 8px 11px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none;
    font-family: inherit; line-height: 1.4;
    transition: border-color .12s, box-shadow .12s;
  }
  .input::placeholder { color: var(--text-3); }
  .input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  select.input { padding-right: 28px; appearance: none; cursor: pointer; }
  textarea.input { resize: vertical; min-height: 56px; }

  /* Modal section header — quiet uppercase divider that groups related
     fields in the edit modal without adding a heavy <h3>. */
  .modal-section {
    font-size: 11px; font-weight: 600; color: var(--text-3);
    text-transform: uppercase; letter-spacing: .06em;
    margin: 18px 0 10px;
    padding-bottom: 8px; border-bottom: 1px solid var(--border);
  }
  .modal-section:first-of-type { margin-top: 4px; }
  .modal-hint {
    font-size: 11px; color: var(--text-3); margin-top: 4px;
  }

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
  if (t1 <= t0) return hbs.map((h, i) => latencyPoint(h, i, 0));

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
  // Pick a label precision based on the total span. < 1 hour → include
  // seconds so closely-bunched samples don't all read "22:02"; otherwise
  // minutes are enough.
  const span = t1 - t0;
  const fmt = span < 3600_000
    ? (d) => d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
    : (d) => d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  return bins.map((b, i) => ({
    t: i,
    label: b.count > 0
      ? fmt(new Date(b.t))
      : fmt(new Date(t0 + i * width)),
    latency: b.count > 0 ? Math.round(b.sum / b.count) : null,
  }));
}

function latencyPoint(h, i, spanMs) {
  const date = h.ts instanceof Array ? offsetDateTimeArrayToDate(h.ts) : new Date(h.ts);
  const opts = (spanMs != null && spanMs < 3600_000)
    ? { hour: '2-digit', minute: '2-digit', second: '2-digit' }
    : { hour: '2-digit', minute: '2-digit' };
  return {
    t: i,
    label: date.toLocaleTimeString('en-GB', opts),
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

// Like formatDuration but takes seconds (what the reliability endpoint
// returns) and renders the same compact two-part form used elsewhere on
// the page — "47h 23m", "4m 12s", "12s". Returns '—' for null/undefined
// so callers can pipe a possibly-missing value straight in.
function formatSecondsDuration(secs) {
  if (secs == null) return '—';
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h < 24) return `${h}h ${m}m`;
  const d = Math.floor(h / 24);
  return `${d}d ${h % 24}h`;
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
export default function MonitorDetail({ monitorId, user }) {
  // Readonly users can view everything (incl. CSV export) but get no
  // test / pause / edit / clone / delete affordances.
  const writable = canWrite(user);
  const [tab, setTab] = useState('overview');
  const [logFilter, setLogFilter] = useState('all');
  const [acting, setActing] = useState(false);
  const [editing, setEditing] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testingNotifs, setTestingNotifs] = useState(false);
  // null = no result panel. Otherwise { sent: number, failed: number }.
  const [notifResult, setNotifResult] = useState(null);
  const [testResult, setTestResult] = useState(null);
  // "Maintenance now" quick action. `maintPickerOpen` toggles the tiny
  // duration popover; `maintDuration` is the chosen length in seconds
  // (default 1h); `maintBusy` guards the in-flight create; `maintActive`
  // holds { until: Date } after a window is created so we can show a
  // confirmation banner with the active end time; `maintError` surfaces
  // a failed create.
  const [maintPickerOpen, setMaintPickerOpen] = useState(false);
  const [maintDuration, setMaintDuration]     = useState(3600);
  const [maintBusy, setMaintBusy]             = useState(false);
  const [maintActive, setMaintActive]         = useState(null);
  const [maintError, setMaintError]           = useState(null);

  const [monReload, setMonReload] = useState(0);
  const bumpMonitor = () => setMonReload((k) => k + 1);
  const monitorState   = useApi(() => monitorId ? api.monitors.get(monitorId)         : Promise.resolve(null), [monitorId, monReload], { pollMs: 15_000 });
  // 2000 = the backend's hard cap on this endpoint. We poll the newest
  // page every 10s and accumulate older pages on demand via the cursor
  // "Load more" button below (the heartbeat tab footer).
  const heartbeatState = useApi(() => monitorId ? api.monitors.heartbeats(monitorId, 2000) : Promise.resolve([]), [monitorId], { pollMs: 10_000 });
  const [olderHeartbeats, setOlderHeartbeats]   = useState([]);
  const [loadingMore,     setLoadingMore]       = useState(false);
  const [allLoaded,       setAllLoaded]         = useState(false);
  // Reset accumulated older pages when the user navigates to a different
  // monitor — otherwise old monitor's history bleeds into the next view.
  useEffect(() => {
    setOlderHeartbeats([]); setAllLoaded(false);
    // Clear the maintenance quick-action UI so a window started on one
    // monitor doesn't bleed its banner/picker into the next view.
    setMaintPickerOpen(false); setMaintActive(null); setMaintError(null);
    setMaintDuration(3600);
  }, [monitorId]);
  const summaryState   = useApi(() => api.monitors.summary(86400),       [], { pollMs: 15_000 });
  const summaryState30 = useApi(() => api.monitors.summary(2_592_000),   [], { pollMs: 60_000 });
  // MTBF / MTTR over a user-selected trailing window (7 / 30 / 90 days).
  // Default is 30 to match the pre-toggle widget. Slow to change, so we
  // poll only every 5min. Server walks heartbeat history directly so
  // values are exact (not derived from the heartbeats cache the client
  // has loaded). Changing `reliabilityWindow` triggers a re-fetch via
  // the dep list below.
  const [reliabilityWindow, setReliabilityWindow] = useState(30);
  const reliabilityState = useApi(
    () => monitorId ? api.monitors.reliability(monitorId, reliabilityWindow) : Promise.resolve(null),
    [monitorId, reliabilityWindow],
    { pollMs: 300_000 },
  );
  // SLO error-budget. Polls alongside the reliability card every 5 min.
  // Only fires when the operator has set an SLO target — the endpoint
  // returns 404 otherwise, so we short-circuit before issuing the
  // request to keep the network panel quiet on un-configured monitors.
  const sloTargetSet = monitorState.data?.slo_target_pct != null;
  const errorBudgetState = useApi(
    () => (monitorId && sloTargetSet) ? api.monitors.errorBudget(monitorId) : Promise.resolve(null),
    [monitorId, sloTargetSet],
    { pollMs: 300_000 },
  );
  // SLO error-budget burn-down — one point per day across a user-selected
  // trailing window (7 / 30 / 90 days). Defaults to the monitor's configured
  // `slo_window_days` until the operator picks a preset, matching the
  // pre-toggle behaviour. `null` means "not yet picked" — we send no
  // window_days param and let the backend resolve to the configured one.
  // Same 404 contract as the fuel gauge, so we short-circuit on
  // un-configured monitors and poll on the same 5-min cadence. Changing
  // `burndownWindow` triggers a re-fetch via the dep list below.
  const [burndownWindow, setBurndownWindow] = useState(null);
  // Reset the picked window when navigating to a different monitor so the
  // next monitor falls back to its own configured window.
  useEffect(() => { setBurndownWindow(null); }, [monitorId]);
  const burndownState = useApi(
    () => (monitorId && sloTargetSet)
      ? api.monitors.sloBurndown(monitorId, burndownWindow ?? undefined)
      : Promise.resolve(null),
    [monitorId, sloTargetSet, burndownWindow],
    { pollMs: 300_000 },
  );
  const groupsState    = useApi(() => api.monitorGroups.list(),          [], { pollMs: 60_000 });

  // Open escalation episode (or null). Polls on the same 15s cadence as
  // the monitor itself; `escReload` forces an immediate refetch after an
  // acknowledge so the banner flips without waiting for the next tick.
  const [escReload, setEscReload] = useState(0);
  const [acking,    setAcking]    = useState(false);
  const episodeState = useApi(
    () => monitorId ? api.escalation.episode(monitorId) : Promise.resolve(null),
    [monitorId, escReload],
    { pollMs: 15_000 },
  );
  const episode = episodeState.data;

  // Long-horizon uptime history — daily uptime% over a 30d / 90d / 1y range.
  // Reads from server-side rollups so it works past raw heartbeat retention,
  // complementing the 90-day strip (which is heartbeat-derived). Re-fetches
  // when the range selector changes. Reset to 30d when switching monitors.
  const [uptimeRange, setUptimeRange] = useState('30d');
  useEffect(() => { setUptimeRange('30d'); }, [monitorId]);
  const uptimeHistoryState = useApi(
    () => monitorId ? api.monitors.uptimeHistory(monitorId, uptimeRange) : Promise.resolve([]),
    [monitorId, uptimeRange],
    { pollMs: 300_000 },
  );

  const monitor = monitorState.data;
  // Combined view = freshly polled page + accumulated older pages.
  // Order is descending-ts (newest first) like the backend returns.
  const heartbeats = useMemo(
    () => [...(heartbeatState.data || []), ...olderHeartbeats],
    [heartbeatState.data, olderHeartbeats],
  );

  const loadMore = async () => {
    if (loadingMore || heartbeats.length === 0) return;
    const oldest = heartbeats[heartbeats.length - 1];
    // Backend serialises OffsetDateTime as a numeric array; convert back
    // to an ISO string for the cursor.
    const ts = Array.isArray(oldest.ts)
      ? offsetDateTimeArrayToDate(oldest.ts).toISOString()
      : new Date(oldest.ts).toISOString();
    setLoadingMore(true);
    try {
      const next = await api.monitors.heartbeats(monitor.id, 2000, ts);
      if (!next || next.length === 0) setAllLoaded(true);
      else setOlderHeartbeats(prev => [...prev, ...next]);
    } catch (e) { toast(`Load more failed: ${e.message}`, 'error'); }
    finally { setLoadingMore(false); }
  };
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
    } catch (e) { toast(`Failed: ${e.message}`, 'error'); }
    setActing(false);
  };
  const doDelete = async () => {
    if (!monitor || acting) return;
    if (!(await confirmDialog({ message: `Delete monitor "${monitor.name}"? This also drops all its heartbeats.` }))) return;
    setActing(true);
    try {
      await api.monitors.remove(monitor.id);
      window.location.hash = '#/';
    } catch (e) { toast(`Failed: ${e.message}`, 'error'); setActing(false); }
  };
  const doClone = async () => {
    if (!monitor || acting) return;
    setActing(true);
    try {
      const copy = await api.monitors.clone(monitor.id);
      window.location.hash = `#/monitor/${copy.id}`;
    } catch (e) { toast(`Failed: ${e.message}`, 'error'); setActing(false); }
  };
  // Capture this monitor's config into a reusable template. The `spec` is a
  // monitor-creation body: we lift the probe-shape fields off the monitor and
  // drop the per-instance bits (name, group, push token) the template library
  // fills in at instantiate time.
  const doSaveAsTemplate = async () => {
    if (!monitor || acting) return;
    const tplName = await promptDialog({
      title: t('templates.save.prompt'),
      defaultValue: t('templates.save.default_name', { name: monitor.name }),
    });
    if (tplName == null || !tplName.trim()) return;
    const spec = {
      kind: monitor.kind,
      interval_seconds: monitor.interval_seconds,
      timeout_seconds:  monitor.timeout_seconds,
    };
    if (monitor.url != null)               spec.url = monitor.url;
    if (monitor.hostname != null)          spec.hostname = monitor.hostname;
    if (monitor.port != null)              spec.port = monitor.port;
    if (monitor.http_method != null)       spec.http_method = monitor.http_method;
    if (monitor.accepted_statuses != null) spec.accepted_statuses = monitor.accepted_statuses;
    if (monitor.http_headers != null)      spec.http_headers = monitor.http_headers;
    if (monitor.follow_redirect != null)   spec.follow_redirect = monitor.follow_redirect;
    if (monitor.ignore_tls != null)        spec.ignore_tls = monitor.ignore_tls;
    if (monitor.max_retries != null)       spec.max_retries = monitor.max_retries;
    if (monitor.upside_down != null)       spec.upside_down = monitor.upside_down;
    if (monitor.config != null && Object.keys(monitor.config).length) spec.config = monitor.config;
    setActing(true);
    try {
      await api.monitorTemplates.create({ name: tplName.trim(), spec });
      if (await confirmDialog({ message: t('templates.save.ok_goto') })) window.location.hash = '#/templates';
    } catch (e) {
      toast(t('templates.save.failed', { msg: e.message }), 'error');
    } finally {
      setActing(false);
    }
  };
  const doTestNow = async () => {
    if (!monitor || testing) return;
    setTesting(true);
    setTestResult(null);
    try {
      const hb = await api.monitors.testNow(monitor.id);
      // useApi polls every 10s so the new heartbeat row + status appear on the
      // next tick. Surface the immediate probe outcome inline (no blocking
      // alert) so the operator sees it without waiting.
      setTestResult({ status: hb.status, latency_ms: hb.latency_ms, msg: hb.msg });
    } catch (e) { setTestResult({ error: e.message }); }
    finally { setTesting(false); }
  };
  const doTestNotifications = async () => {
    if (!monitor || testingNotifs) return;
    setTestingNotifs(true);
    setNotifResult(null);
    try {
      const res = await api.monitors.testNotifications(monitor.id);
      const sent = res?.sent ?? [];
      const failed = sent.filter((s) => !s.ok).length;
      setNotifResult({ sent: sent.length, failed });
    } catch (e) {
      setNotifResult({ error: e.message });
    } finally { setTestingNotifs(false); }
  };
  // Acknowledge the open escalation episode — stops further steps from
  // paging. The refetch flips the banner into its muted "acknowledged"
  // state until the monitor recovers.
  const doAckEscalation = async () => {
    if (!monitor || acking) return;
    setAcking(true);
    try { await api.escalation.ack(monitor.id); }
    catch (e) { toast(`Failed: ${e.message}`, 'error'); }
    finally { setAcking(false); setEscReload(k => k + 1); }
  };
  // Start a one-shot maintenance window covering [now, now+duration) for
  // THIS monitor only. Reuses the existing maintenance-create endpoint —
  // recurrence defaults to "none" so the window is single-shot, and we
  // attach just this monitor via monitor_ids. The picker closes on
  // success and a confirmation banner shows the active end time.
  const doMaintenanceNow = async () => {
    if (!monitor || maintBusy) return;
    setMaintBusy(true);
    setMaintError(null);
    const start = new Date();
    const end   = new Date(start.getTime() + maintDuration * 1000);
    try {
      await api.maintenance.create({
        name:        t('monitor.maint_now.window_name', { monitor: monitor.name }),
        start_at:    start.toISOString(),
        end_at:      end.toISOString(),
        monitor_ids: [monitor.id],
        recurrence:  { kind: 'none' },
      });
      setMaintActive({ until: end });
      setMaintPickerOpen(false);
    } catch (e) {
      setMaintError(e.message);
    } finally { setMaintBusy(false); }
  };

  // ── missing-id / loading / not-found ──────────────────────────────────
  if (!monitorId) {
    return (
      <div className="rampart">
        <style>{css}</style>
        <div style={{ padding: 80, textAlign: 'center' }}>
          <h2 style={{ fontSize: 18, fontWeight: 600 }}>{t('monitor.detail.none_selected')}</h2>
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
          <h2 style={{ fontSize: 18, fontWeight: 600 }}>{t('monitor.detail.not_found')}</h2>
          <p style={{ color: 'var(--text-2)', fontSize: 14, marginTop: 8 }}>{t('monitor.detail.likely_deleted')} <a href="#/" style={{ color: 'var(--accent)' }}>{t('monitor.detail.back_to_dashboard')}</a>.</p>
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
        <div style={{ fontSize: 12, color: 'var(--text-3)', display: 'flex', alignItems: 'center', gap: 6, marginBottom: 16, flexWrap: 'wrap' }}>
          <ChevronLeft size={14} style={{ cursor: 'pointer' }} onClick={() => { window.location.hash = '#/'; }}/>
          <a href="#/" style={{ cursor: 'pointer', color: 'var(--text-3)', textDecoration: 'none' }}>Monitors</a>
          {(() => {
            // Walk the folder ancestor chain so the breadcrumb shows where
            // a monitor lives in the (possibly nested) folder tree.
            const groups = groupsState.data || [];
            const byId   = new Map(groups.map(g => [g.id, g]));
            const chain  = [];
            let cur = monitor.group_id ? byId.get(monitor.group_id) : null;
            const seen = new Set();
            while (cur && !seen.has(cur.id)) {
              chain.unshift(cur); seen.add(cur.id);
              cur = cur.parent_id ? byId.get(cur.parent_id) : null;
            }
            return chain.map(g => (
              <React.Fragment key={g.id}>
                <span>/</span>
                <a href="#/folders" style={{ cursor: 'pointer', color: 'var(--text-3)', textDecoration: 'none' }}>{g.name}</a>
              </React.Fragment>
            ));
          })()}
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
              {monitor.current_status === 'pending'     && <span className="pill pill-pending">{t('monitor.status.pending_first')}</span>}
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
            {writable && monitor.kind !== 'push' && (
              <button className="btn" onClick={doTestNow} disabled={acting || testing}
                title={t('monitor.action.test_now_title')}>
                {testing ? <><Loader2 size={13} className="spin"/> {t('monitor.action.testing')}</> : <><Zap size={13}/> {t('monitor.action.test_now')}</>}
              </button>
            )}
            {writable && (
              <button className="btn" onClick={doTestNotifications} disabled={acting || testingNotifs}
                title={t('monitor.action.test_notifications_title')}>
                {testingNotifs ? <><Loader2 size={13} className="spin"/> {t('monitor.action.testing')}</> : <><Bell size={13}/> {t('monitor.action.test_notifications')}</>}
              </button>
            )}
            {writable && (
              <div style={{ position: 'relative' }}>
                <button className="btn" onClick={() => { setMaintError(null); setMaintPickerOpen(o => !o); }}
                  disabled={acting || maintBusy} title={t('monitor.maint_now.title')}>
                  <Calendar size={13}/> {t('monitor.maint_now.button')}
                </button>
                {maintPickerOpen && (
                  <div className="card" style={{
                    position: 'absolute', top: 'calc(100% + 6px)', right: 0, zIndex: 20,
                    width: 220, padding: 12, boxShadow: '0 8px 24px rgba(0,0,0,.12)',
                  }}>
                    <div style={{ fontSize: 12, color: 'var(--text-2)', marginBottom: 8 }}>
                      {t('monitor.maint_now.heading')} <strong style={{ color: 'var(--text)' }}>{monitor.name}</strong>
                    </div>
                    <div style={{ display: 'flex', gap: 4, marginBottom: 10 }}>
                      {[[3600, 'dur_1h'], [14400, 'dur_4h'], [86400, 'dur_24h']].map(([secs, key]) => (
                        <button key={secs} type="button"
                          className={`btn${maintDuration === secs ? ' btn-prim' : ''}`}
                          style={{ flex: 1, justifyContent: 'center', padding: '6px 4px', fontSize: 12 }}
                          onClick={() => setMaintDuration(secs)}>
                          {t(`monitor.maint_now.${key}`)}
                        </button>
                      ))}
                    </div>
                    {maintError && (
                      <div style={{ fontSize: 11, color: 'var(--down)', marginBottom: 8 }}>
                        {t('monitor.maint_now.error', { error: maintError })}
                      </div>
                    )}
                    <div style={{ display: 'flex', gap: 6 }}>
                      <button className="btn btn-accent" style={{ flex: 1, justifyContent: 'center' }}
                        onClick={doMaintenanceNow} disabled={maintBusy}>
                        {maintBusy
                          ? <><Loader2 size={13} className="spin"/> {t('monitor.maint_now.creating')}</>
                          : t('monitor.maint_now.start')}
                      </button>
                      <button className="btn" onClick={() => setMaintPickerOpen(false)} disabled={maintBusy}>
                        {t('monitor.maint_now.cancel')}
                      </button>
                    </div>
                  </div>
                )}
              </div>
            )}
            {writable && (
              <button className="btn" onClick={doPauseResume} disabled={acting}>
                {monitor.active ? <><Pause size={13}/> {t('common.pause')}</> : <><Play size={13}/> {t('common.resume')}</>}
              </button>
            )}
            {writable && <button className="btn" onClick={() => setEditing(true)} disabled={acting}><Edit3 size={13}/> {t('common.edit')}</button>}
            {writable && <button className="btn" onClick={doClone} disabled={acting} title={t('monitor.action.clone_title')}><Copy size={13}/> {t('common.clone')}</button>}
            {writable && <button className="btn" onClick={doSaveAsTemplate} disabled={acting} title={t('templates.save.title')}><FileStack size={13}/> {t('templates.save.button')}</button>}
            <a className="btn" href={`/v1/monitors/${monitor.id}/heartbeats.csv`} title={t('monitor.action.csv_title')} download>
              <Download size={13}/> {t('monitor.action.csv')}
            </a>
            {writable && <button className="btn btn-danger" onClick={doDelete} disabled={acting}><Trash2 size={13}/> {t('common.delete')}</button>}
          </div>
        </div>

        {testResult && (
          <div
            role="status"
            onClick={() => setTestResult(null)}
            style={{
              marginBottom: 12, padding: '8px 12px', borderRadius: 8, cursor: 'pointer',
              fontSize: 13, border: '1px solid var(--border)',
              background: testResult.error || testResult.status === 'down' ? 'var(--down-bg)' : 'var(--up-bg)',
              color: 'var(--text)',
            }}
            title={t('monitor.test_now.dismiss')}
          >
            {testResult.error
              ? t('monitor.test_now.error', { error: testResult.error })
              : t('monitor.test_now.ok', {
                  status: testResult.status,
                  detail: (testResult.latency_ms != null ? ` (${testResult.latency_ms}ms)` : '')
                        + (testResult.msg ? ` — ${testResult.msg}` : ''),
                })}
          </div>
        )}

        {notifResult && (
          <div
            role="status"
            onClick={() => setNotifResult(null)}
            style={{
              marginBottom: 12, padding: '8px 12px', borderRadius: 8, cursor: 'pointer',
              fontSize: 13, border: '1px solid var(--border)',
              background: notifResult.error || notifResult.failed > 0 ? 'var(--down-bg)' : 'var(--up-bg)',
              color: 'var(--text)',
            }}
            title={t('monitor.action.test_notifications_dismiss')}
          >
            {notifResult.error
              ? t('monitor.notif_test.error', { error: notifResult.error })
              : notifResult.sent === 0
                ? t('monitor.notif_test.none')
                : notifResult.failed > 0
                  ? t('monitor.notif_test.partial', { sent: notifResult.sent, failed: notifResult.failed })
                  : t('monitor.notif_test.ok', { sent: notifResult.sent })}
          </div>
        )}

        {maintActive && (
          <div
            role="status"
            onClick={() => setMaintActive(null)}
            style={{
              marginBottom: 12, padding: '8px 12px', borderRadius: 8, cursor: 'pointer',
              fontSize: 13, border: '1px solid var(--maint)',
              background: 'var(--maint-soft)', color: 'var(--maint-text)',
              display: 'flex', alignItems: 'center', gap: 8,
            }}
            title={t('monitor.action.test_notifications_dismiss')}
          >
            <Calendar size={14}/>
            {t('monitor.maint_now.active', { until: maintActive.until.toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' }) })}
          </div>
        )}

        {/* Open escalation episode. Loud while the ladder is still paging;
            muted once someone acknowledges (stays until the monitor
            recovers and the episode resolves server-side). */}
        {episode && !episode.resolved_at && (
          episode.acked_at ? (
            <div role="status" style={{
              marginBottom: 12, padding: '8px 12px', borderRadius: 8,
              fontSize: 13, border: '1px solid var(--border)',
              background: 'var(--surface-2)', color: 'var(--text-2)',
              display: 'flex', alignItems: 'center', gap: 8,
            }}>
              <Check size={14}/>
              {t('monitor.escalation.acked', { who: episode.acked_by || '—', when: formatRelative(episode.acked_at) })}
            </div>
          ) : (
            <div role="status" style={{
              marginBottom: 12, padding: '8px 12px', borderRadius: 8,
              fontSize: 13, border: '1px solid var(--down)',
              background: 'var(--down-soft)', color: 'var(--down-text)',
              display: 'flex', alignItems: 'center', gap: 8,
            }}>
              <Bell size={14}/>
              <span>
                <strong>{t('monitor.escalation.running', { n: episode.last_step })}</strong>
                {' · '}
                {t('monitor.escalation.started', { when: formatRelative(episode.started_at) })}
              </span>
              {writable && (
                <button className="btn" style={{ marginLeft: 'auto' }} onClick={doAckEscalation} disabled={acking}>
                  {acking
                    ? <><Loader2 size={13} className="spin"/> {t('monitor.escalation.acking')}</>
                    : <><Check size={13}/> {t('monitor.escalation.ack')}</>}
                </button>
              )}
            </div>
          )
        )}

        <div className="tabs" style={{ marginBottom: -1 }}>
          {['overview', 'heartbeats', 'config'].map(tabKey => (
            <button key={tabKey} className={tab === tabKey ? 'active' : ''} onClick={() => setTab(tabKey)}>
              {t(`monitor.tab.${tabKey}`)}
            </button>
          ))}
        </div>
      </header>

      <main className="dash-main" style={{ padding: '24px 32px', maxWidth: 1200, margin: '0 auto' }}>
        {/* KPI row */}
        <div className="kpi-grid" style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12, marginBottom: 20 }}>
          <Kpi
            label={t('monitor.kpi.uptime_24h')}
            value={uptime24h != null ? uptime24h.toFixed(2) : '—'}
            suffix={uptime24h != null ? '%' : null}
            sub={summary24h ? t('monitor.kpi.checks_ok', { up: summary24h.up, total: summary24h.total }) : t('monitor.kpi.no_data')}
            color={uptime24h != null && uptime24h < 99 ? 'var(--down)' : uptime24h != null && uptime24h < 99.9 ? 'var(--warn)' : 'var(--up)'}
          />
          <Kpi
            label={t('monitor.kpi.uptime_30d')}
            value={uptime30d != null ? uptime30d.toFixed(2) : '—'}
            suffix={uptime30d != null ? '%' : null}
            sub={summary30d ? t('monitor.kpi.checks_ok', { up: summary30d.up, total: summary30d.total }) : t('monitor.kpi.no_data')}
          />
          <Kpi
            label={t('monitor.kpi.avg_response')}
            value={avgLatency24h != null ? Math.round(avgLatency24h) : '—'}
            suffix={avgLatency24h != null ? 'ms' : null}
            sub={t('monitor.kpi.successful_24h')}
          />
          <Kpi
            label={t('monitor.kpi.interval')}
            value={monitor.interval_seconds}
            suffix="s"
            sub={t('monitor.kpi.timeout', { n: monitor.timeout_seconds })}
          />
        </div>

        {/* ─── OVERVIEW TAB ──────────────────────────────────────── */}
        {tab === 'overview' && <>

        {heartbeats[0]?.status === 'maintenance' && (
          <div style={{
            display: 'flex', alignItems: 'center', gap: 10,
            padding: '12px 16px', marginBottom: 20,
            background: 'var(--warn-soft)', border: '1px solid var(--warn)',
            borderRadius: 10, fontSize: 13, color: 'var(--warn-text)',
          }}>
            <Calendar size={14}/>
            <strong>Monitor is currently in maintenance.</strong>
            Checks and alerts are suppressed.
            <a href="#/maintenance" style={{ marginLeft: 'auto', color: 'var(--warn-text)', fontSize: 12 }}>Manage windows →</a>
          </div>
        )}

        {monitor.kind === 'push' && monitor.push_token && (
          <PushUrlCard monitorId={monitor.id} token={monitor.push_token} lastPushAt={monitor.last_push_at} interval={monitor.interval_seconds} config={monitor.config}/>
        )}

        {(monitor.kind === 'http' || monitor.kind === 'keyword' || monitor.kind === 'json_query')
          && (monitor.url || '').startsWith('https://')
          && monitor.cert_checked_at && (
          <CertCard monitor={monitor}/>
        )}

        {/* SLO card. Renders only when the operator has set a target;
            current% is pulled from the rolling 30d summary endpoint
            (close enough for the headline number — the precise
            window-day reading lives server-side and surfaces via
            breach alerts in a later iteration). */}
        {monitor.slo_target_pct != null && (
          <SloCard
            target={monitor.slo_target_pct}
            current={uptime30d}
            windowDays={monitor.slo_window_days}/>
        )}

        {/* Error-budget "fuel gauge". Shown directly under the SLO card
            so the operator sees target / current / remaining as one
            visual story. Polls the dedicated /slo/error-budget endpoint
            (5-min cadence) so the bar reflects heartbeat-derived down
            seconds for the configured window, not an interval-based
            approximation. */}
        {monitor.slo_target_pct != null && (
          <ErrorBudgetCard data={errorBudgetState.data}/>
        )}

        {/* Error budget burn-down. Sits directly under the fuel gauge:
            the gauge is the point-in-time reading, this is the same
            number plotted across the window so the operator sees the
            budget DEPLETING over time. Polls the /slo/burndown endpoint
            on the same 5-min cadence. */}
        {monitor.slo_target_pct != null && (
          <BurndownCard
            data={burndownState.data}
            windowDays={burndownWindow ?? monitor.slo_window_days}
            onWindowChange={setBurndownWindow}
          />
        )}

        {/* 90-day uptime strip */}
        <div className="card" style={{ padding: '20px 22px', marginBottom: 20 }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
            <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.card.90d_uptime')}</h3>
            <span className="tabular mono" style={{ fontSize: 12, color: 'var(--text-2)' }}>
              {incidentCount > 0 ? `${incidentCount} day${incidentCount === 1 ? '' : 's'} with downtime` : t('monitor.empty.no_recorded_downtime')}
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

        {/* Long-horizon uptime history — daily uptime% over 30d / 90d / 1y,
            fed from server-side rollups so it outlives raw heartbeat
            retention. Complements the heartbeat-derived strip above. */}
        <UptimeHistoryCard
          data={uptimeHistoryState.data}
          loading={uptimeHistoryState.loading}
          range={uptimeRange}
          onRangeChange={setUptimeRange}
        />

        {/* Reliability — MTBF / MTTR / downtime events over a user-selected
            trailing window (7 / 30 / 90 days). Backend walks the heartbeat
            history (server-side) so this stays accurate even before the
            client has lazy-loaded older heartbeats. Renders dashes until
            the first response lands. */}
        <ReliabilityCard
          data={reliabilityState.data}
          windowDays={reliabilityWindow}
          onWindowChange={setReliabilityWindow}
        />

        {/* response time chart */}
        <div className="card" style={{ padding: '20px 22px', marginBottom: 20 }}>
          <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', marginBottom: 18 }}>
            <div>
              <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.card.response_time')}</h3>
              <p style={{ fontSize: 12, color: 'var(--text-3)', margin: '4px 0 0' }}>
                {heartbeats.length > 0 ? `${heartbeats.length} samples · binned to ${responseData.length} points` : 'No samples yet'}
              </p>
            </div>
            <button className="btn"><Calendar size={13}/> {t('monitor.detail.all_samples')} <ChevronDown size={11}/></button>
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
                {t('monitor.empty.no_samples')}
              </div>
            )}
          </div>
        </div>

        {/* recent heartbeats + downtime side by side */}
        <div className="detail-grid" style={{ display: 'grid', gridTemplateColumns: '1.6fr 1fr', gap: 16 }}>
          <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
            <div style={{ padding: '16px 18px', display: 'flex', alignItems: 'center', justifyContent: 'space-between', borderBottom: '1px solid var(--border)' }}>
              <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
                <Activity size={14} color="var(--text-2)"/> {t('monitor.card.recent_heartbeats')}
              </h3>
              <div className="tabs">
                <button className={logFilter === 'all'  ? 'active' : ''} onClick={() => setLogFilter('all')}>{t('monitor.filter.all')}</button>
                <button className={logFilter === 'fail' ? 'active' : ''} onClick={() => setLogFilter('fail')}>{t('monitor.filter.failures')}</button>
              </div>
            </div>
            <div style={{
              display: 'grid', gridTemplateColumns: '90px 80px 80px 1fr 60px',
              gap: 16, padding: '10px 18px', fontSize: 11, fontWeight: 600,
              color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em',
              background: 'var(--surface-2)', borderBottom: '1px solid var(--border)'
            }}>
              <span>{t('monitor.col.time')}</span><span>{t('monitor.col.status')}</span><span style={{ textAlign: 'right' }}>{t('monitor.col.latency')}</span><span>{t('monitor.col.message')}</span><span style={{ textAlign: 'right' }}>{t('monitor.col.code')}</span>
            </div>
            {filteredLog.length === 0 ? (
              <div className="empty">{heartbeatState.loading ? t('common.loading') : t('monitor.empty.no_match')}</div>
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
            <div style={{ padding: '12px 18px', textAlign: 'center', borderTop: '1px solid var(--border)', fontSize: 11, color: 'var(--text-3)', display: 'flex', justifyContent: 'center', alignItems: 'center', gap: 12 }}>
              <span>showing {filteredLog.length} of {heartbeats.length} loaded</span>
              {!allLoaded && heartbeats.length > 0 && (
                <button className="btn" style={{ padding: '4px 10px', fontSize: 11 }}
                  onClick={loadMore} disabled={loadingMore}>
                  {loadingMore ? <><Loader2 size={11} className="spin"/> {t('common.loading')}</> : <>{t('monitor.load_older')}</>}
                </button>
              )}
              {allLoaded && <span style={{ fontStyle: 'italic' }}>{t('monitor.all_loaded')}</span>}
            </div>
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
            <div className="card" style={{ padding: '18px 20px' }}>
              <h3 style={{ fontSize: 14, fontWeight: 600, margin: '0 0 12px', display: 'flex', alignItems: 'center', gap: 8 }}>
                <AlertCircle size={14} color="var(--down)"/> {t('monitor.card.recent_downtime')}
              </h3>
              {downtime.length === 0 ? (
                <div className="empty" style={{ padding: 0 }}>{t('monitor.empty.no_downtime')}</div>
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
                <Activity size={14} color="var(--text-2)"/> {t('monitor.card.heartbeats')}
                <span style={{ fontSize: 11, color: 'var(--text-3)', fontWeight: 400, marginLeft: 6 }}>
                  · {heartbeats.length} loaded
                </span>
              </h3>
              <div className="tabs">
                <button className={logFilter === 'all'  ? 'active' : ''} onClick={() => setLogFilter('all')}>{t('monitor.filter.all')}</button>
                <button className={logFilter === 'fail' ? 'active' : ''} onClick={() => setLogFilter('fail')}>{t('monitor.filter.failures')}</button>
              </div>
            </div>
            <div style={{
              display: 'grid', gridTemplateColumns: '110px 90px 100px 1fr 70px',
              gap: 16, padding: '10px 22px', fontSize: 11, fontWeight: 600,
              color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em',
              background: 'var(--surface-2)', borderBottom: '1px solid var(--border)'
            }}>
              <span>{t('monitor.col.time')}</span><span>{t('monitor.col.status')}</span><span style={{ textAlign: 'right' }}>{t('monitor.col.latency')}</span><span>{t('monitor.col.message')}</span><span style={{ textAlign: 'right' }}>{t('monitor.col.code')}</span>
            </div>
            {(logFilter === 'all' ? heartbeats : heartbeats.filter(h => h.status !== 'up')).length === 0 ? (
              <div className="empty">{heartbeatState.loading ? t('common.loading') : t('monitor.empty.no_match')}</div>
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
            <div style={{ padding: '12px 22px', textAlign: 'center', borderTop: '1px solid var(--border)', fontSize: 11, color: 'var(--text-3)', display: 'flex', justifyContent: 'center', alignItems: 'center', gap: 12 }}>
              <span>Loaded {heartbeats.length} heartbeats — more land as the scheduler probes.</span>
              {!allLoaded && heartbeats.length > 0 && (
                <button className="btn" style={{ padding: '4px 10px', fontSize: 11 }}
                  onClick={loadMore} disabled={loadingMore}>
                  {loadingMore ? <><Loader2 size={11} className="spin"/> {t('common.loading')}</> : <>{t('monitor.load_older')}</>}
                </button>
              )}
              {allLoaded && <span style={{ fontStyle: 'italic' }}>{t('monitor.all_loaded')}</span>}
            </div>
          </div>
        )}

        {/* ─── CONFIG TAB ─────────────────────────────────────────── */}
        {tab === 'config' && (
          <ConfigPanel monitor={monitor}/>
        )}

        <div style={{ height: 40 }}/>
      </main>

      {editing && (
        <EditModal monitor={monitor} onCancel={() => setEditing(false)} onSaved={() => { bumpMonitor(); setEditing(false); }}/>
      )}
    </div>
  );
}

// ── Edit modal ────────────────────────────────────────────────────────────
function EditModal({ monitor, onCancel, onSaved }) {
  const [name,        setName]        = useState(monitor.name || '');
  const [intervalSec, setIntervalSec] = useState(String(monitor.interval_seconds));
  const [timeoutSec,  setTimeoutSec]  = useState(String(monitor.timeout_seconds));
  const [retries,     setRetries]     = useState(String(monitor.max_retries));
  const [resend,      setResend]      = useState(String(monitor.resend_interval_sec));
  const [url,         setUrl]         = useState(monitor.url || '');
  const [hostname,    setHostname]    = useState(monitor.hostname || '');
  const [port,        setPort]        = useState(monitor.port == null ? '' : String(monitor.port));
  const [method,      setMethod]      = useState(monitor.http_method || 'GET');
  const [statuses,    setStatuses]    = useState((monitor.accepted_statuses || []).join(', '));
  const [body,        setBody]        = useState(monitor.http_body || '');
  const [headers,     setHeaders]     = useState(monitor.http_headers ? JSON.stringify(monitor.http_headers, null, 2) : '');
  const [follow,      setFollow]      = useState(!!monitor.follow_redirect);
  const [ignoreTls,   setIgnoreTls]   = useState(!!monitor.ignore_tls);
  const [upside,      setUpside]      = useState(!!monitor.upside_down);
  const [config,      setConfig]      = useState(
    monitor.config && Object.keys(monitor.config).length
      ? JSON.stringify(monitor.config, null, 2)
      : '',
  );
  // Probe-config editor mode: structured form (default when the kind has known
  // fields) or raw JSON.
  const [configMode,  setConfigMode]  = useState(
    configFields(monitor.kind).length ? 'form' : 'json',
  );
  // Per-monitor SLO target. Both fields are optional — an empty string
  // is treated as "clear" so the operator can disable an SLO by erasing
  // either value.
  const [sloTarget,   setSloTarget]   = useState(
    monitor.slo_target_pct == null ? '' : String(monitor.slo_target_pct),
  );
  const [sloWindow,   setSloWindow]   = useState(
    monitor.slo_window_days == null ? '' : String(monitor.slo_window_days),
  );
  const [busy,        setBusy]        = useState(false);
  const [err,         setErr]         = useState(null);

  // Synthetic monitors get a structured step editor instead of raw-JSON config
  // surgery. State seeds from config.steps; non-step config keys are preserved
  // on save (see below).
  const isSynthetic = monitor.kind === 'synthetic';
  const [synthSteps, setSynthSteps] = useState(() => configToUiSteps(monitor.config?.steps));

  const isHttpFamily = ['http', 'keyword', 'json_query'].includes(monitor.kind);
  const isPush       = monitor.kind === 'push';
  const hasHostname  = monitor.hostname != null;
  const hasPort      = monitor.port != null;

  // Remote probe agent assignment. Fetched only while the modal is open
  // (this component mounts on open); on error or an empty list the select
  // simply doesn't render. Push monitors can't be assigned to an agent.
  const [agentId, setAgentId] = useState(monitor.agent_id || '');
  const agentsState = useApi(() => (isPush ? Promise.resolve([]) : api.agents.list()), []);
  const agents = agentsState.data || [];
  const showAgents = !isPush && agents.length > 0;

  // Escalation policy assignment. Same contract as the agent select: only
  // rendered when policies exist, and only included in the PATCH when it
  // rendered so a failed fetch can't silently detach an existing policy.
  const [policyId, setPolicyId] = useState(monitor.escalation_policy_id || '');
  const policiesState = useApi(() => api.escalation.list(), []);
  const policies = policiesState.data || [];
  const showPolicies = policies.length > 0;

  const save = async () => {
    setErr(null);
    if (!name.trim()) { setErr(t('validation.name_required')); return; }
    let headersJson = null;
    if (headers.trim()) {
      try { headersJson = JSON.parse(headers); }
      catch { setErr(t('validation.headers_json')); return; }
    }
    let configJson = {};
    if (config.trim()) {
      try { configJson = JSON.parse(config); }
      catch { setErr(t('validation.probe_json')); return; }
      if (typeof configJson !== 'object' || Array.isArray(configJson) || configJson === null) {
        setErr(t('validation.probe_json_object'));
        return;
      }
    }
    // Synthetic monitors: the structured step editor owns config.steps. Build
    // it from the editor and merge over any preserved non-step config keys.
    if (isSynthetic) {
      const steps = uiToConfigSteps(synthSteps);
      if (!steps.length) { setErr(t('wizard.err_synthetic')); return; }
      configJson = { ...configJson, steps };
    }
    const acceptedStatuses = statuses
      .split(',').map(s => parseInt(s.trim(), 10)).filter(Number.isFinite);

    const patch = {
      name:               name.trim(),
      interval_seconds:   parseInt(intervalSec, 10) || undefined,
      timeout_seconds:    parseInt(timeoutSec,  10) || undefined,
      max_retries:        parseInt(retries,     10) || 0,
      resend_interval_sec: parseInt(resend,     10) || 0,
      upside_down:        upside,
      config:             configJson,
    };
    if (monitor.url != null)         patch.url            = url || null;
    if (hasHostname)                 patch.hostname       = hostname || null;
    if (hasPort && port)             patch.port           = parseInt(port, 10);
    if (isHttpFamily) {
      patch.http_method       = method;
      patch.follow_redirect   = follow;
      patch.ignore_tls        = ignoreTls;
      patch.accepted_statuses = acceptedStatuses;
      patch.http_body         = body || null;
      patch.http_headers      = headersJson;
    }
    // Probe agent: empty string = local probing (null on the wire). Only
    // sent when the select rendered so a failed agents fetch can't clear
    // an existing assignment.
    if (showAgents) patch.agent_id = agentId || null;
    // Escalation policy: empty string = no policy (null detaches). Same
    // "only when rendered" guard as the agent select above.
    if (showPolicies) patch.escalation_policy_id = policyId || null;
    // SLO fields use the Option<Option<…>> double-option pattern on the
    // backend: send `null` to clear, omit to leave unchanged. We always
    // send both (the form lets the user clear either one) so the patch
    // reflects exactly what's in the form.
    {
      const t = sloTarget.trim();
      if (t === '') {
        patch.slo_target_pct = null;
      } else {
        const n = parseFloat(t);
        if (!Number.isFinite(n) || n < 90 || n > 100) {
          setErr(t('validation.slo_target_range'));
          return;
        }
        patch.slo_target_pct = n;
      }
      const w = sloWindow.trim();
      if (w === '') {
        patch.slo_window_days = null;
      } else {
        const n = parseInt(w, 10);
        if (!Number.isFinite(n) || n < 1 || n > 90) {
          setErr(t('validation.slo_window_range'));
          return;
        }
        patch.slo_window_days = n;
      }
    }
    Object.keys(patch).forEach(k => patch[k] === undefined && delete patch[k]);

    setBusy(true);
    try {
      await api.monitors.update(monitor.id, patch);
      onSaved?.();
    } catch (e) {
      setErr(e.message || 'Failed to save.');
      setBusy(false);
    }
  };

  const cardRef = useRef(null);
  useFocusTrap(cardRef, true, onCancel);
  return (
    <div onClick={onCancel} style={{
      position: 'fixed', inset: 0, background: 'rgba(0,0,0,.45)',
      display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 100,
      padding: 20,
    }}>
      <div ref={cardRef} role="dialog" aria-modal="true" tabIndex={-1} className="modal-card" onClick={e => e.stopPropagation()} style={{
        background: 'var(--surface)', borderRadius: 14,
        maxWidth: 760, width: '100%',
        maxHeight: '92vh', overflow: 'auto',
        boxShadow: '0 24px 64px rgba(0,0,0,.18)',
        border: '1px solid var(--border)',
      }}>
        {/* Header — sticky so the title + close stay visible while scrolling
            the long form. */}
        <div style={{
          position: 'sticky', top: 0, zIndex: 1,
          display: 'flex', justifyContent: 'space-between', alignItems: 'center',
          padding: '18px 24px',
          background: 'var(--surface)',
          borderBottom: '1px solid var(--border)',
        }}>
          <div>
            <div style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.06em', fontWeight: 600, marginBottom: 2 }}>
              {t('monitor.edit.eyebrow')}
            </div>
            <h3 style={{ fontSize: 17, fontWeight: 600, margin: 0, letterSpacing: '-.01em' }}>{monitor.name}</h3>
          </div>
          <button className="btn btn-ghost" onClick={onCancel} disabled={busy} aria-label={t('common.close')}>
            <X size={16}/>
          </button>
        </div>

        <div style={{ padding: '20px 24px 24px' }}>
          {err && (
            <div style={{ background: 'var(--down-soft)', color: 'var(--down-text)', border: '1px solid var(--down)', padding: '10px 14px', borderRadius: 8, fontSize: 13, marginBottom: 14 }}>
              {err}
            </div>
          )}

          {/* ── Basics ─────────────────────────────────────────────── */}
          <div className="modal-section">{t('monitor.edit.section.basics')}</div>

          <Field label={t('monitor.edit.name')}>
            <input className="input" value={name} onChange={e => setName(e.target.value)}/>
          </Field>

          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12 }}>
            <Field label={t('monitor.edit.interval')}><input className="input mono" value={intervalSec} onChange={e => setIntervalSec(e.target.value)}/></Field>
            <Field label={t('monitor.edit.timeout')}><input className="input mono" value={timeoutSec} onChange={e => setTimeoutSec(e.target.value)}/></Field>
            <Field label={t('monitor.edit.retries')}><input className="input mono" value={retries} onChange={e => setRetries(e.target.value)}/></Field>
            <Field label={t('monitor.edit.realert')}><input className="input mono" value={resend} onChange={e => setResend(e.target.value)}/></Field>
          </div>

          {/* SLO target + window. Both optional — clear either to disable
              the SLO. Server enforces the 90.0–100.0 / 1–90 ranges; we
              echo the ranges in the placeholders so the operator doesn't
              have to guess. */}
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 12 }}>
            <Field label={t('monitor.edit.slo_target')}>
              <input className="input mono" inputMode="decimal"
                value={sloTarget} onChange={e => setSloTarget(e.target.value)}
                placeholder="e.g. 99.9 (90.0 – 100.0)"/>
            </Field>
            <Field label={t('monitor.edit.slo_window')}>
              <input className="input mono" inputMode="numeric"
                value={sloWindow} onChange={e => setSloWindow(e.target.value)}
                placeholder="e.g. 30 (1 – 90)"/>
            </Field>
          </div>

          {/* ── Target ─────────────────────────────────────────────── */}
          {(monitor.url != null || hasHostname || hasPort) && (
            <>
              <div className="modal-section">{t('monitor.edit.section.target')}</div>
              {monitor.url != null && (
                <Field label={t('monitor.edit.url')}><input className="input mono" value={url} onChange={e => setUrl(e.target.value)}/></Field>
              )}
              {(hasHostname || hasPort) && (
                <div style={{ display: 'grid', gridTemplateColumns: hasHostname && hasPort ? '1fr 140px' : '1fr', gap: 12 }}>
                  {hasHostname && <Field label={t('monitor.edit.hostname')}><input className="input mono" value={hostname} onChange={e => setHostname(e.target.value)}/></Field>}
                  {hasPort     && <Field label={t('monitor.edit.port')}><input className="input mono" value={port} onChange={e => setPort(e.target.value)}/></Field>}
                </div>
              )}
            </>
          )}

          {/* ── HTTP request ──────────────────────────────────────── */}
          {isHttpFamily && (
            <>
              <div className="modal-section">{t('monitor.edit.section.http')}</div>
              <div style={{ display: 'grid', gridTemplateColumns: '140px 1fr', gap: 12 }}>
                <Field label={t('monitor.edit.method')}>
                  <select className="input" value={method} onChange={e => setMethod(e.target.value)}>
                    {['GET','HEAD','POST','PUT','PATCH','DELETE','OPTIONS'].map(m => <option key={m} value={m}>{m}</option>)}
                  </select>
                </Field>
                <Field label={t('monitor.edit.accepted_statuses')}>
                  <input className="input mono" value={statuses} onChange={e => setStatuses(e.target.value)} placeholder="200, 201, 204"/>
                </Field>
              </div>
              <Field label={t('monitor.edit.headers')}>
                <textarea className="input mono" rows={4} value={headers} onChange={e => setHeaders(e.target.value)} placeholder='{"Accept":"application/json"}' style={{ minHeight: 90 }}/>
              </Field>
              <Field label={t('monitor.edit.body')}>
                <textarea className="input mono" rows={4} value={body} onChange={e => setBody(e.target.value)} placeholder='Request body (raw)' style={{ minHeight: 80 }}/>
              </Field>
              <div className="modal-section">{t('monitor.edit.section.behaviour')}</div>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 10 }}>
                <Toggle label={t('monitor.edit.follow_redirects')} on={follow}     setOn={setFollow}/>
                <Toggle label={t('monitor.edit.ignore_tls')} on={ignoreTls} setOn={setIgnoreTls}/>
                <Toggle label={t('monitor.edit.upside_down')}       on={upside}    setOn={setUpside}/>
              </div>
              {showAgents && (
                <div style={{ marginTop: 14 }}>
                  <Field label={t('monitor.edit.agent')}>
                    <select className="input" value={agentId} onChange={e => setAgentId(e.target.value)}>
                      <option value="">{t('monitor.edit.agent_local')}</option>
                      {agents.map(a => (
                        <option key={a.id} value={a.id}>{a.name}{a.location ? ` — ${a.location}` : ''}</option>
                      ))}
                    </select>
                  </Field>
                </div>
              )}
              {showPolicies && (
                <div style={{ marginTop: 14 }}>
                  <Field label={t('monitor.edit.escalation_policy')}>
                    <select className="input" value={policyId} onChange={e => setPolicyId(e.target.value)}>
                      <option value="">{t('monitor.edit.escalation_none')}</option>
                      {policies.map(p => (
                        <option key={p.id} value={p.id}>{p.name}</option>
                      ))}
                    </select>
                    <div className="modal-hint">{t('monitor.edit.escalation_hint')}</div>
                  </Field>
                </div>
              )}
            </>
          )}

          {!isHttpFamily && (
            <>
              <div className="modal-section">{t('monitor.edit.section.behaviour')}</div>
              <Toggle label={t('monitor.edit.upside_down')} on={upside} setOn={setUpside}/>
              {showAgents && (
                <div style={{ marginTop: 14 }}>
                  <Field label={t('monitor.edit.agent')}>
                    <select className="input" value={agentId} onChange={e => setAgentId(e.target.value)}>
                      <option value="">{t('monitor.edit.agent_local')}</option>
                      {agents.map(a => (
                        <option key={a.id} value={a.id}>{a.name}{a.location ? ` — ${a.location}` : ''}</option>
                      ))}
                    </select>
                  </Field>
                </div>
              )}
              {showPolicies && (
                <div style={{ marginTop: 14 }}>
                  <Field label={t('monitor.edit.escalation_policy')}>
                    <select className="input" value={policyId} onChange={e => setPolicyId(e.target.value)}>
                      <option value="">{t('monitor.edit.escalation_none')}</option>
                      {policies.map(p => (
                        <option key={p.id} value={p.id}>{p.name}</option>
                      ))}
                    </select>
                    <div className="modal-hint">{t('monitor.edit.escalation_hint')}</div>
                  </Field>
                </div>
              )}
            </>
          )}

          {/* ── Probe config ──────────────────────────────────────── */}
          <div className="modal-section" style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <span>{t('monitor.edit.section.probe')}</span>
            {!isSynthetic && configFields(monitor.kind).length > 0 && (
              <div style={{ display: 'flex', gap: 4 }}>
                <button type="button" className={`btn ${configMode === 'form' ? 'btn-accent' : 'btn-ghost'}`}
                  style={{ padding: '3px 10px', fontSize: 12 }} onClick={() => setConfigMode('form')}>{t('monitor.config.mode_form')}</button>
                <button type="button" className={`btn ${configMode === 'json' ? 'btn-accent' : 'btn-ghost'}`}
                  style={{ padding: '3px 10px', fontSize: 12 }} onClick={() => setConfigMode('json')}>{t('monitor.config.mode_json')}</button>
              </div>
            )}
          </div>
          {isSynthetic ? (
            <SyntheticSteps steps={synthSteps} setSteps={setSynthSteps} />
          ) : configMode === 'form' && configFields(monitor.kind).length > 0 ? (
            <>
              <ProbeConfigForm kind={monitor.kind} config={config} setConfig={setConfig} />
              <div className="modal-hint">{configHint(monitor.kind)}</div>
            </>
          ) : (
            <Field label="">
              <textarea className="input mono" rows={6}
                value={config} onChange={e => setConfig(e.target.value)}
                placeholder={configPlaceholder(monitor.kind)}
                style={{ minHeight: 120 }}/>
              <div className="modal-hint">
                {configHint(monitor.kind)}
              </div>
            </Field>
          )}
        </div>

        {/* Footer — sticky so the action buttons are always reachable. */}
        <div style={{
          position: 'sticky', bottom: 0, zIndex: 1,
          display: 'flex', justifyContent: 'flex-end', gap: 8,
          padding: '14px 24px',
          background: 'var(--surface)',
          borderTop: '1px solid var(--border)',
        }}>
          <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
          <button className="btn btn-accent" onClick={save} disabled={busy}>
            {busy ? t('common.saving') : t('common.save')}
          </button>
        </div>
      </div>
    </div>
  );
}

// Per-kind placeholder + hint for the JSON config textarea in EditModal.
// The probe layer reads `monitor.config` as a free-form JSON object — these
// helpers surface the keys each probe actually reads so operators don't
// have to guess from source.
function configPlaceholder(kind) {
  switch (kind) {
    case 'dns':       return '{\n  "record_type": "A",\n  "resolver": "1.1.1.1",\n  "expected": "93.184.216.34"\n}';
    case 'keyword':   return '{\n  "keyword": "operational"\n}';
    case 'json_query':return '{\n  "json_path": "$.status",\n  "expected_value": "ok"\n}';
    case 'browser':   return '{\n  "renderer_url": "http://browserless:3000/content",\n  "keyword": "operational"\n}';
    case 'postgres':  case 'mysql': case 'mssql': case 'mongodb':
                      return '{\n  "user": "app",\n  "password": "secret",\n  "database": "app"\n}';
    case 'redis':     return '{\n  "user": "default",\n  "password": "secret",\n  "db": 0\n}';
    case 'ssh':       return '{\n  "expect": "SSH-"\n}';
    case 'smtp':      return '{\n  "expect": "220"\n}';
    case 'imap':      return '{\n  "expect": "* OK"\n}';
    case 'pop3':      return '{\n  "expect": "+OK"\n}';
    case 'ftp':       return '{\n  "expect": "220"\n}';
    default:          return '{}';
  }
}
// Known config keys per kind, for the structured ("Form") config editor. Keys
// not listed here are preserved untouched — switch to JSON mode to edit them.
function configFields(kind) {
  const latency = { key: 'max_latency_ms', label: 'Max latency (ms)', type: 'number', ph: 'e.g. 500' };
  switch (kind) {
    case 'http': case 'keyword': case 'json_query':
      return [
        { key: 'keyword', label: 'Keyword', type: 'text', ph: 'operational' },
        { key: 'json_path', label: 'JSON path', type: 'text', ph: '$.status' },
        { key: 'expected_value', label: 'Expected value', type: 'text', ph: 'ok' },
        latency,
      ];
    case 'dns':
      return [
        { key: 'record_type', label: 'Record type', type: 'text', ph: 'A' },
        { key: 'resolver', label: 'Resolver', type: 'text', ph: '1.1.1.1' },
        { key: 'expected', label: 'Expected (substring)', type: 'text', ph: '' },
      ];
    case 'postgres': case 'mysql': case 'mssql': case 'mongodb':
      return [
        { key: 'user', label: 'Username', type: 'text', ph: kind },
        { key: 'password', label: 'Password', type: 'password', ph: '' },
        { key: 'database', label: 'Database', type: 'text', ph: kind },
        { key: 'connection_string', label: 'Connection string', type: 'text', ph: '(overrides the above)' },
        latency,
      ];
    case 'redis':
      return [
        { key: 'user', label: 'Username (ACL)', type: 'text', ph: 'default' },
        { key: 'password', label: 'Password', type: 'password', ph: '' },
        { key: 'db', label: 'DB number', type: 'number', ph: '0' },
        { key: 'tls', label: 'TLS', type: 'bool' },
        latency,
      ];
    case 'mqtt':
      return [
        { key: 'username', label: 'Username', type: 'text', ph: '' },
        { key: 'password', label: 'Password', type: 'password', ph: '' },
        { key: 'client_id', label: 'Client ID', type: 'text', ph: 'rampart' },
        { key: 'tls', label: 'TLS', type: 'bool' },
        latency,
      ];
    case 'ldap':
      return [
        { key: 'bind_dn', label: 'Bind DN', type: 'text', ph: 'cn=monitor,dc=example,dc=com' },
        { key: 'bind_password', label: 'Bind password', type: 'password', ph: '' },
        latency,
      ];
    case 'ssh': case 'smtp': case 'imap': case 'pop3': case 'ftp':
      return [{ key: 'expect', label: 'Expect (greeting prefix)', type: 'text', ph: '220' }, latency];
    case 'browser':
      return [
        { key: 'renderer_url', label: 'Renderer URL', type: 'text', ph: 'http://browserless:3000/content' },
        { key: 'keyword', label: 'Keyword', type: 'text', ph: 'operational' },
      ];
    case 'tcp': case 'grpc': case 'mongodb': case 'memcached': case 'nats':
    case 'cassandra': case 'kafka': case 'amqp': case 'websocket': case 'snmp':
      return [latency];
    default:
      return [];
  }
}

// Structured editor for the known config keys. Parses the JSON string, edits
// the listed keys, preserves any unlisted keys, and writes back the serialized
// JSON (so save uses the same path as the raw editor).
function ProbeConfigForm({ kind, config, setConfig }) {
  const fields = configFields(kind);
  let obj = {};
  let parseErr = false;
  try {
    obj = config.trim() ? JSON.parse(config) : {};
    if (typeof obj !== 'object' || Array.isArray(obj)) { obj = {}; parseErr = true; }
  } catch { parseErr = true; }
  if (parseErr) {
    return <div className="modal-hint" style={{ color: 'var(--down)' }}>{t('monitor.config.invalid_json')}</div>;
  }
  const setKey = (key, val, type) => {
    const next = { ...obj };
    if (val === '' || val == null || val === false) delete next[key];
    else if (type === 'number') { const n = Number(val); if (Number.isFinite(n)) next[key] = n; else delete next[key]; }
    else next[key] = val;
    setConfig(Object.keys(next).length ? JSON.stringify(next, null, 2) : '');
  };
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
      {fields.map(f => (
        <Field key={f.key} label={f.label}>
          {f.type === 'bool'
            ? <label style={{ display: 'flex', gap: 8, alignItems: 'center', fontSize: 13 }}>
                <input type="checkbox" checked={!!obj[f.key]} onChange={e => setKey(f.key, e.target.checked, 'bool')}/> {t('common.enabled') || 'enabled'}
              </label>
            : <input className="input mono"
                type={f.type === 'password' ? 'password' : f.type === 'number' ? 'number' : 'text'}
                value={obj[f.key] ?? ''} placeholder={f.ph}
                autoComplete={f.type === 'password' ? 'new-password' : 'off'}
                onChange={e => setKey(f.key, e.target.value, f.type)}/>}
        </Field>
      ))}
    </div>
  );
}

function configHint(kind) {
  switch (kind) {
    case 'dns':       return 'Keys: record_type (A | AAAA | CNAME | MX | TXT | NS | SRV | CAA | SOA), resolver (IP), expected (substring).';
    case 'keyword':   return 'Key: keyword — substring the response body must contain.';
    case 'json_query':return 'Keys: json_path (JSONPath), expected_value (optional exact match).';
    case 'browser':   return 'Keys: renderer_url (URL of the headless renderer), keyword (substring required in rendered HTML).';
    case 'ssh':       case 'smtp': case 'imap': case 'pop3': case 'ftp':
                      return 'Key: expect — prefix the server greeting must start with. Empty uses the protocol default.';
    case 'postgres':  case 'mysql': case 'mssql': case 'mongodb':
                      return 'Auth keys: user, password, database. Or connection_string for a full URL. max_latency_ms marks a slow-but-up check down.';
    case 'redis':     return 'Auth keys: user (ACL, Redis 6+), password, db (number), tls (bool). Or connection_string. max_latency_ms marks a slow-but-up check down.';
    case 'mqtt':      return 'Auth keys: username, password, client_id, tls (bool). max_latency_ms marks a slow-but-up check down.';
    case 'ldap':      return 'Auth keys: bind_dn, bind_password (anonymous if omitted). max_latency_ms marks a slow-but-up check down.';
    default:          return 'Free-form JSON read by the probe runtime. Leave empty if this probe has no extra config.';
  }
}

function Field({ label, children }) {
  return (
    <div style={{ marginBottom: 14 }}>
      {label && (
        <label style={{ display: 'block', fontSize: 11, fontWeight: 600, color: 'var(--text-2)', letterSpacing: '.02em', marginBottom: 6 }}>
          {label}
        </label>
      )}
      {children}
    </div>
  );
}
function Toggle({ label, on, setOn }) {
  return (
    <label style={{
      display: 'flex', alignItems: 'center', gap: 9,
      padding: '10px 12px',
      border: '1px solid ' + (on ? 'var(--accent)' : 'var(--border)'),
      background: on ? 'var(--accent-soft)' : 'var(--surface)',
      borderRadius: 8, cursor: 'pointer',
      transition: 'all .12s',
    }}>
      <input type="checkbox" checked={on} onChange={e => setOn(e.target.checked)} style={{ accentColor: 'var(--accent)' }}/>
      <span style={{ fontSize: 12.5, color: on ? 'var(--accent-2)' : 'var(--text-2)', fontWeight: on ? 500 : 400 }}>{label}</span>
    </label>
  );
}

// ── Config panel ──────────────────────────────────────────────────────────
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
          <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.config.target')}</h3>
        </div>
        {row('Kind',       monitor.kind, true)}
        {row('Display name', monitor.name)}
        {row('URL',        monitor.url, true)}
        {row('Hostname',   monitor.hostname, true)}
        {row('Port',       monitor.port, true)}
      </div>

      <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
        <div style={{ padding: '16px 22px' }}>
          <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.config.schedule')}</h3>
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

      <TagsCard monitor={monitor} onChanged={bumpMonitor}/>
      <DependenciesCard monitor={monitor}/>

      {(monitor.kind === 'http' || monitor.kind === 'keyword' || monitor.kind === 'json_query') && (
        <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
          <div style={{ padding: '16px 22px' }}>
            <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.config.http')}</h3>
          </div>
          {row('Method',            monitor.http_method, true)}
          {row('Accepted statuses', acceptedStatuses, true)}
          {row('Follow redirects',  monitor.follow_redirect ? 'yes' : 'no')}
          {row('Ignore TLS errors', monitor.ignore_tls ? 'yes (insecure)' : 'no')}
          {row('Headers',           monitor.http_headers ? JSON.stringify(monitor.http_headers) : null, true)}
          {row('Body',              monitor.http_body, true)}
        </div>
      )}

      {monitor.kind === 'push' && monitor.config?.cron && (
        <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
          <div style={{ padding: '16px 22px' }}>
            <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.push.section')}</h3>
          </div>
          {row(t('monitor.push.schedule'), monitor.config.cron, true)}
          {row(t('monitor.push.grace'),    `${monitor.config.cron_grace_seconds ?? 300}s`, true)}
          {row(t('monitor.push.max_run'),  monitor.config.max_run_seconds ? `${monitor.config.max_run_seconds}s` : null, true)}
        </div>
      )}

      {monitor.config && Object.keys(monitor.config).length > 0 && (
        <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
          <div style={{ padding: '16px 22px' }}>
            <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.config.kind_specific')}</h3>
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
          <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.config.identity')}</h3>
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
  const all       = useApi(() => api.notifications.list(), [reloadKey]);
  const attached  = useApi(() => api.notifications.forMonitor(monitorId), [monitorId, reloadKey]);
  const effective = useApi(() => api.routing.effective(monitorId), [monitorId, reloadKey]);
  const excludes  = useApi(() => api.routing.excludes(monitorId),  [monitorId, reloadKey]);
  const [showPicker, setShowPicker] = useState(false);

  const byId        = new Map((all.data || []).map(c => [c.id, c]));
  const attachedIds = new Set((attached.data || []).map(c => c.id));
  const effectiveIds = effective.data || [];
  const excludeIds  = new Set(excludes.data || []);
  const available   = (all.data || []).filter(c => !attachedIds.has(c.id) && !effectiveIds.includes(c.id));

  const bounce = () => setReloadKey(k => k + 1);

  const attach = async (nid) => {
    try { await api.notifications.attach(monitorId, nid); bounce(); setShowPicker(false); } catch (e) { toast(e.message, 'error'); }
  };
  // Remove from this monitor: exclusion always wins, so it covers
  // tag/folder auto-routed channels too. Also drop a direct attach edge
  // if present, so the channel doesn't linger as "attached but muted".
  const remove = async (nid) => {
    try {
      if (attachedIds.has(nid)) await api.notifications.detach(monitorId, nid);
      await api.routing.addExclude(monitorId, nid);
      bounce();
    } catch (e) { toast(e.message, 'error'); }
  };
  const restore = async (nid) => {
    try { await api.routing.delExclude(monitorId, nid); bounce(); } catch (e) { toast(e.message, 'error'); }
  };
  const sendTest = async (nid) => {
    try { await api.notifications.test(nid); toast('Test sent. Check the channel.', 'error'); } catch (e) { toast(e.message, 'error'); }
  };

  const loading = effective.loading || all.loading;

  return (
    <div className="card" style={{ padding: '18px 20px' }}>
      <h3 style={{ fontSize: 14, fontWeight: 600, margin: '0 0 4px', display: 'flex', alignItems: 'center', gap: 8 }}>
        <Bell size={14} color="var(--text-2)"/> Notifications
      </h3>
      <div style={{ fontSize: 11, color: 'var(--text-3)', marginBottom: 10 }}>
        Channels that will fire for this monitor — direct attachments plus tag/folder auto-routing.
      </div>

      {!loading && effectiveIds.length === 0 && (
        <div className="empty" style={{ padding: 0, fontSize: 12, marginBottom: 10 }}>
          No channels will fire. Attach one below, or tag this monitor/its folder to match a channel.
        </div>
      )}

      {effectiveIds.map(nid => {
        const c = byId.get(nid);
        if (!c) return null;
        const auto = !attachedIds.has(nid);  // in effective but not directly attached → tag/folder
        return (
          <div key={nid} style={{
            display: 'grid', gridTemplateColumns: '1fr auto auto', alignItems: 'center', gap: 8,
            padding: '8px 0', borderTop: '1px solid var(--border)',
          }}>
            <div>
              <div style={{ fontSize: 12.5, fontWeight: 500, display: 'flex', alignItems: 'center', gap: 6 }}>
                {c.name}
                {auto && <span style={{ fontSize: 9, fontWeight: 600, color: 'var(--accent-2)', background: 'var(--accent-soft)', padding: '1px 5px', borderRadius: 999, textTransform: 'uppercase', letterSpacing: '.04em' }}>auto</span>}
              </div>
              <div style={{ fontSize: 10, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em' }}>{c.kind}</div>
            </div>
            <button className="btn" onClick={() => sendTest(nid)} title={t('monitor.send_test')} style={{ padding: '4px 8px', fontSize: 11 }}><Send size={11}/></button>
            <button className="btn btn-danger" onClick={() => remove(nid)} aria-label={t('common.remove')} title="Remove from this monitor" style={{ padding: '4px 8px', fontSize: 11 }}><X size={11}/></button>
          </div>
        );
      })}

      {/* Excluded — auto-routed channels the operator pulled off this monitor. */}
      {excludeIds.size > 0 && (
        <div style={{ marginTop: 12, paddingTop: 10, borderTop: '1px solid var(--border)' }}>
          <div style={{ fontSize: 10, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 600, marginBottom: 6 }}>Excluded</div>
          {[...excludeIds].map(nid => {
            const c = byId.get(nid);
            return (
              <div key={nid} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '5px 0' }}>
                <span style={{ fontSize: 12, color: 'var(--text-3)', textDecoration: 'line-through' }}>{c?.name || nid.slice(0, 8)}</span>
                <button className="btn btn-ghost" onClick={() => restore(nid)} style={{ padding: '3px 8px', fontSize: 11 }}>Restore</button>
              </div>
            );
          })}
        </div>
      )}

      {showPicker ? (
        <div style={{ marginTop: 12, padding: 10, border: '1px solid var(--border)', borderRadius: 8, background: 'var(--surface-2)' }}>
          <div style={{ fontSize: 11, color: 'var(--text-3)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 600 }}>{t('monitor.detail.attach_channel')}</div>
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

// Reliability rollup widget — MTBF, MTTR + downtime event count over a
// user-selected trailing window (7 / 30 / 90 days). The backend computes
// the values directly from the heartbeat history (so the figures don't
// depend on which pages the client has lazy-loaded). Renders dashes
// during the first fetch and when there's no downtime to compute MTBF /
// MTTR from. The segmented control above the three figures drives the
// window; clicking re-fetches via the parent component's state.
function ReliabilityCard({ data, windowDays, onWindowChange }) {
  // Tooltip text: industry-standard definitions so a new operator
  // browsing the dashboard isn't left guessing what the acronyms mean.
  const mtbfTip =
    'MTBF — Mean Time Between Failures. Average uptime between consecutive '
    + 'down events over the trailing window. Higher is better.';
  const mttrTip =
    'MTTR — Mean Time To Recovery. Average duration of a down event over '
    + 'the trailing window. Lower is better.';

  // Selection mirrors `windowDays` from the parent state — the response's
  // own `window_days` may briefly lag while a new fetch is in flight, so
  // the control should reflect the user's pick, not the stale payload.
  const selected = windowDays ?? 30;
  const mtbf     = formatSecondsDuration(data?.mtbf_secs);
  const mttr     = formatSecondsDuration(data?.mttr_secs);
  const events   = data?.downtime_events ?? 0;

  // Small inline-help icon. Browser default tooltip is enough — no extra
  // dependency, plays well with keyboard nav too.
  const HelpDot = ({ tip }) => (
    <span
      title={tip}
      style={{
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        width: 14, height: 14, marginLeft: 6, borderRadius: '50%',
        background: 'var(--surface-2)', color: 'var(--text-3)',
        fontSize: 10, fontWeight: 600, cursor: 'help', userSelect: 'none',
      }}>?</span>
  );

  // Segmented control — three equal segments, the selected one filled.
  // role=group / aria-label keeps screen-reader semantics; each option is
  // a real <button> so keyboard nav (tab + enter/space) works out of the
  // box. We keep the buttons disabled-no-op when already selected to
  // avoid a redundant re-fetch + spinner flash.
  const SegBtn = ({ days }) => {
    const isSel = selected === days;
    return (
      <button
        type="button"
        role="radio"
        aria-checked={isSel}
        aria-label={`${days}-day window`}
        onClick={() => { if (!isSel) onWindowChange?.(days); }}
        style={{
          flex: 1, padding: '4px 10px', fontSize: 12, fontWeight: 600,
          background: isSel ? 'var(--surface)' : 'transparent',
          color: isSel ? 'var(--text)' : 'var(--text-2)',
          border: 'none', borderRadius: 4,
          boxShadow: isSel ? '0 1px 2px rgba(0,0,0,0.06)' : 'none',
          cursor: isSel ? 'default' : 'pointer',
        }}>
        {days}d
      </button>
    );
  };

  return (
    <div className="card" style={{ padding: '20px 22px', marginBottom: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
        <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.card.reliability')}</h3>
        <div
          role="radiogroup"
          aria-label={t('monitor.aria.reliability_window')}
          style={{
            display: 'inline-flex', alignItems: 'center', gap: 2,
            background: 'var(--surface-2)', padding: 2, borderRadius: 6,
            width: 156,
          }}>
          <SegBtn days={7}/>
          <SegBtn days={30}/>
          <SegBtn days={90}/>
        </div>
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 14 }}>
        <div>
          <div className="kpi-label" style={{ display: 'flex', alignItems: 'center' }}>
            {t('monitor.reliability.mtbf')}<HelpDot tip={mtbfTip}/>
          </div>
          <div className="kpi-value tabular" style={{ marginTop: 8, color: 'var(--text)' }}>{mtbf}</div>
          <div style={{ fontSize: 12, color: 'var(--text-3)', marginTop: 4 }}>{t('monitor.reliability.between_failures')}</div>
        </div>
        <div>
          <div className="kpi-label" style={{ display: 'flex', alignItems: 'center' }}>
            {t('monitor.reliability.mttr')}<HelpDot tip={mttrTip}/>
          </div>
          <div className="kpi-value tabular" style={{ marginTop: 8, color: 'var(--text)' }}>{mttr}</div>
          <div style={{ fontSize: 12, color: 'var(--text-3)', marginTop: 4 }}>{t('monitor.reliability.to_recover')}</div>
        </div>
        <div>
          <div className="kpi-label">{t('monitor.reliability.downtime_events')}</div>
          <div
            className="kpi-value tabular"
            style={{ marginTop: 8, color: events > 0 ? 'var(--down)' : 'var(--text)' }}>
            {events}
          </div>
          <div style={{ fontSize: 12, color: 'var(--text-3)', marginTop: 4 }}>{t('monitor.reliability.transitions')}</div>
        </div>
      </div>
    </div>
  );
}

// ── Push-URL card. Shown on push monitors so the user can copy the
//    endpoint into their cron / CI / backup script. Server-side, the
//    token is what authenticates the heartbeat (so treat it like a
//    secret — anyone with the URL can mark this monitor up).
function CertCard({ monitor }) {
  const days = monitor.cert_days_left;
  const checked = monitor.cert_checked_at;
  const checkedDate = Array.isArray(checked) ? offsetDateTimeArrayToDate(checked) : new Date(checked);
  const tone = days == null ? 'unknown'
    : days < 0  ? 'expired'
    : days < 14 ? 'warn'
    : 'ok';
  const palette = {
    ok:      { bg: 'var(--up-soft)',   fg: 'var(--up-text)',   label: `${days} days left` },
    warn:    { bg: 'var(--warn-soft)', fg: 'var(--warn-text)', label: `expires in ${days} days` },
    expired: { bg: 'var(--down-soft)', fg: 'var(--down-text)', label: `expired ${Math.abs(days)} days ago` },
    unknown: { bg: 'var(--surface-2)', fg: 'var(--text-2)',    label: 'unknown' },
  }[tone];
  return (
    <div className="card" style={{ padding: '18px 22px', marginBottom: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
        <Lock size={14} color="var(--text-3)"/>
        <h3 style={{ fontSize: 13, fontWeight: 600, margin: 0 }}>{t('monitor.card.tls_cert')}</h3>
        <span style={{
          marginLeft: 'auto',
          background: palette.bg, color: palette.fg,
          fontSize: 11, fontWeight: 600, padding: '3px 9px', borderRadius: 999,
        }}>{palette.label}</span>
      </div>
      <div className="mono" style={{ fontSize: 12, color: 'var(--text-2)', wordBreak: 'break-all' }}>
        {monitor.cert_subject || '—'}
      </div>
      <div style={{ marginTop: 8, fontSize: 11, color: 'var(--text-3)' }}>
        Inspected {formatRelative(checkedDate)} · refreshed hourly while the monitor is active.
      </div>
    </div>
  );
}

// Compact SLO card. Mirrors CertCard's visual shape so the Overview tab
// stays consistent. Three states:
//   * "OK"       — current% >= target
//   * "At risk"  — current is within 0.1pp of target but still ≥ target
//   * "Breached" — current% < target
// "No data" when current is null. Window is informational — the precise
// uptime over the configured window is calculated server-side in the
// breach detector; we approximate here with the rolling 30d summary so
// the card stays cheap to render without an extra round-trip.
function SloCard({ target, current, windowDays }) {
  const hasData = current != null;
  let tone = 'unknown';
  let label = t('monitor.slo.no_data');
  if (hasData) {
    if (current >= target) {
      // "At risk" = within 0.1 percentage points of the target.
      // Renders amber so the operator knows error-budget is thin.
      tone = (current - target) < 0.1 ? 'risk' : 'ok';
      label = tone === 'risk' ? t('monitor.slo.at_risk') : t('monitor.slo.ok');
    } else {
      tone = 'breached';
      label = t('monitor.slo.breached');
    }
  }
  const palette = {
    ok:       { bg: 'var(--up-soft)',   fg: 'var(--up-text)' },
    risk:     { bg: 'var(--warn-soft)', fg: 'var(--warn-text)' },
    breached: { bg: 'var(--down-soft)', fg: 'var(--down-text)' },
    unknown:  { bg: 'var(--surface-2)', fg: 'var(--text-2)' },
  }[tone];
  const fmtPct = v => (v == null ? '—' : `${Number(v).toFixed(2)}%`);
  const windowLabel = windowDays ?? 30;
  return (
    <div className="card" style={{ padding: '18px 22px', marginBottom: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
        <Activity size={14} color="var(--text-3)"/>
        <h3 style={{ fontSize: 13, fontWeight: 600, margin: 0 }}>{t('monitor.card.slo')}</h3>
        <span style={{
          marginLeft: 'auto',
          background: palette.bg, color: palette.fg,
          fontSize: 11, fontWeight: 600, padding: '3px 9px', borderRadius: 999,
        }}>{label}</span>
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 16 }}>
        <div>
          <div style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 500 }}>
            {t('monitor.slo.target')}
          </div>
          <div className="mono tabular" style={{ fontSize: 18, fontWeight: 500, marginTop: 2 }}>
            {Number(target).toFixed(2)}%
          </div>
        </div>
        <div>
          <div style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 500 }}>
            {t('monitor.slo.current')}
          </div>
          <div className="mono tabular" style={{ fontSize: 18, fontWeight: 500, marginTop: 2, color: palette.fg }}>
            {fmtPct(current)}
          </div>
        </div>
      </div>
      <div style={{ marginTop: 8, fontSize: 11, color: 'var(--text-3)' }}>
        Rolling {windowLabel}-day window. Current is the headline 30-day uptime — the exact-window value surfaces through the breach alert when crossed.
      </div>
    </div>
  );
}

// Horizontal "fuel gauge" rendering the SLO error-budget. Two segments
// inside one rounded track:
//   * red    portion = used budget (down seconds in window)
//   * green  portion = remaining budget
// The numeric label under the bar reads
// "47 min remaining of 144 min budget", coloured by remaining_pct:
//   green when remaining_pct >= 50
//   amber when remaining_pct >= 10
//   red   when remaining_pct < 10
// Renders a placeholder bar while the first fetch is in flight so the
// card doesn't pop into existence after a delay.
function ErrorBudgetCard({ data }) {
  // Format a duration of seconds as a coarse "47 min" / "2.3 h" /
  // "1.2 d" — the fuel-gauge label is meant to be glanceable, so we
  // collapse precision once the number is large enough that exact
  // minutes don't matter.
  const fmtBudget = (secs) => {
    if (secs == null) return '—';
    const s = Math.max(0, Math.round(secs));
    if (s < 60) return `${s} sec`;
    const m = s / 60;
    if (m < 120) return `${Math.round(m)} min`;
    const h = m / 60;
    if (h < 48) return `${h.toFixed(1)} h`;
    const d = h / 24;
    return `${d.toFixed(1)} d`;
  };

  const loading  = !data;
  const allowed  = data?.allowed_downtime_secs ?? 0;
  const used     = data?.used_downtime_secs ?? 0;
  const remaining = data?.remaining_downtime_secs ?? 0;
  const remainingPct = data?.remaining_pct ?? 0;
  const windowDays   = data?.window_days ?? '';
  const targetPct    = data?.target_pct;

  // Width fractions for the two coloured segments. Clamp so a freshly
  // exhausted budget still shows a 100% red bar instead of a 0-width
  // sliver and so floating-point noise doesn't push past 100%.
  const denom = Math.max(allowed, used) || 1;
  const usedFrac      = Math.min(100, (used / denom) * 100);
  const remainingFrac = Math.max(0, 100 - usedFrac);

  // Threshold colour for the numeric label.
  const numColor = remainingPct >= 50 ? 'var(--up)'
    : remainingPct >= 10 ? 'var(--warn)'
    : 'var(--down)';

  return (
    <div className="card" style={{ padding: '18px 22px', marginBottom: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
        <Activity size={14} color="var(--text-3)"/>
        <h3 style={{ fontSize: 13, fontWeight: 600, margin: 0 }}>{t('monitor.card.error_budget')}</h3>
        <span className="tabular mono" style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--text-3)' }}>
          {windowDays ? `${windowDays}-day window` : ''}
          {targetPct != null ? ` · ${Number(targetPct).toFixed(2)}% target` : ''}
        </span>
      </div>

      {/* The bar itself. Two flex children with width-as-percent so the
          ratio stays correct at any container width. Rounded outer,
          square inner — the corners are clipped by overflow:hidden. */}
      <div
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(remainingPct)}
        style={{
          display: 'flex', width: '100%', height: 14,
          background: 'var(--surface-2)', borderRadius: 999, overflow: 'hidden',
          border: '1px solid var(--border)',
        }}>
        <div style={{ width: `${usedFrac}%`,      background: 'var(--down)', transition: 'width .25s ease' }}/>
        <div style={{ width: `${remainingFrac}%`, background: 'var(--up)',   transition: 'width .25s ease' }}/>
      </div>

      <div style={{
        marginTop: 10, display: 'flex', alignItems: 'baseline',
        justifyContent: 'space-between', gap: 12, flexWrap: 'wrap',
      }}>
        <div className="tabular" style={{ fontSize: 13 }}>
          <span style={{ color: numColor, fontWeight: 600 }}>
            {loading ? '—' : fmtBudget(remaining)}
          </span>
          <span style={{ color: 'var(--text-3)' }}>
            {' '}{t('monitor.budget.remaining_of', { budget: loading ? '—' : fmtBudget(allowed) })}
          </span>
        </div>
        <div className="tabular mono" style={{ fontSize: 11, color: 'var(--text-3)' }}>
          {loading ? '' : t('monitor.budget.used', { used: fmtBudget(used) })}
        </div>
      </div>
    </div>
  );
}

// SLO error-budget burn-down chart. Plots `budget_remaining_pct` per day
// across the configured window as a descending area (from 100% toward 0%
// as budget is consumed). Mirrors the response-time chart's recharts
// setup. Renders a loading/empty state until the first payload lands.
function BurndownCard({ data, windowDays, onWindowChange }) {
  const points = data?.points ?? [];
  const targetPct  = data?.target_pct;
  // Selection mirrors the `windowDays` prop (the monitor's configured window
  // until the operator picks a preset) rather than the payload's own
  // `window_days`, which may briefly lag while a new fetch is in flight.
  const selected = windowDays ?? data?.window_days ?? 30;

  // Segmented control — same three-equal-segment styling the reliability
  // card uses. Each option is a real <button> for keyboard nav; the
  // selected one is filled and a no-op to avoid a redundant re-fetch.
  const SegBtn = ({ days }) => {
    const isSel = selected === days;
    return (
      <button
        type="button"
        role="radio"
        aria-checked={isSel}
        aria-label={`${days}-day window`}
        onClick={() => { if (!isSel) onWindowChange?.(days); }}
        style={{
          flex: 1, padding: '4px 10px', fontSize: 12, fontWeight: 600,
          background: isSel ? 'var(--surface)' : 'transparent',
          color: isSel ? 'var(--text)' : 'var(--text-2)',
          border: 'none', borderRadius: 4,
          boxShadow: isSel ? '0 1px 2px rgba(0,0,0,0.06)' : 'none',
          cursor: isSel ? 'default' : 'pointer',
        }}>
        {days}d
      </button>
    );
  };

  // Map server points → chart rows. `day` is an ISO date string from the
  // serialised `time::Date`; show a compact "Jun 9" tick label and keep
  // the raw values for the tooltip.
  const chart = useMemo(() => points.map((p) => {
    const d = new Date(`${p.day}T00:00:00Z`);
    const label = isNaN(d.getTime())
      ? String(p.day)
      : d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', timeZone: 'UTC' });
    return {
      label,
      date: String(p.day),
      remainingPct: p.budget_remaining_pct,
      remainingSecs: p.budget_remaining_secs,
    };
  }), [points]);

  const renderTooltip = ({ active, payload }) => {
    if (!active || !payload || !payload.length) return null;
    const row = payload[0].payload;
    return (
      <div style={{
        background: 'var(--surface)', border: '1px solid var(--border)',
        borderRadius: 8, fontSize: 12, padding: '8px 10px', boxShadow: '0 4px 12px rgba(0,0,0,.08)',
      }}>
        <div style={{ color: 'var(--text-2)', marginBottom: 4 }}>{row.date}</div>
        <div className="tabular">
          <strong>{row.remainingPct.toFixed(1)}%</strong>
          <span style={{ color: 'var(--text-3)' }}> · {Math.round(row.remainingSecs / 60)} min left</span>
        </div>
      </div>
    );
  };

  return (
    <div className="card" style={{ padding: '20px 22px', marginBottom: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 14 }}>
        <Activity size={14} color="var(--text-3)"/>
        <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.detail.burndown_heading')}</h3>
        <span className="tabular mono" style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--text-3)' }}>
          {targetPct != null ? `${Number(targetPct).toFixed(2)}% target` : ''}
        </span>
        <div
          role="radiogroup"
          aria-label={t('monitor.aria.burndown_window')}
          style={{
            display: 'inline-flex', alignItems: 'center', gap: 2,
            background: 'var(--surface-2)', padding: 2, borderRadius: 6,
            width: 156,
          }}>
          <SegBtn days={7}/>
          <SegBtn days={30}/>
          <SegBtn days={90}/>
        </div>
      </div>
      <div style={{ height: 200 }}>
        {chart.length > 0 ? (
          <ResponsiveContainer>
            <AreaChart data={chart} margin={{ top: 5, right: 5, left: -10, bottom: 0 }}>
              <defs>
                <linearGradient id="burndown" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%"   stopColor="var(--up)" stopOpacity={0.3}/>
                  <stop offset="100%" stopColor="var(--up)" stopOpacity={0}/>
                </linearGradient>
              </defs>
              <XAxis dataKey="label" stroke="var(--text-3)" tick={{ fontSize: 11, fontFamily: 'JetBrains Mono' }}
                interval={Math.max(0, Math.floor(chart.length / 8))} tickLine={false}
                axisLine={{ stroke: 'var(--border)' }}/>
              <YAxis stroke="var(--text-3)" tick={{ fontSize: 11, fontFamily: 'JetBrains Mono' }}
                domain={[0, 100]} tickLine={false} axisLine={false} tickFormatter={v => `${v}%`}/>
              <Tooltip content={renderTooltip}/>
              <ReferenceLine y={0} stroke="var(--down)" strokeWidth={1.2}
                label={{ value: 'budget exhausted', fill: 'var(--down)', fontSize: 10, position: 'insideTopRight' }}/>
              <Area type="monotone" dataKey="remainingPct" stroke="var(--up)" strokeWidth={1.8}
                fill="url(#burndown)" connectNulls isAnimationActive={false}/>
            </AreaChart>
          </ResponsiveContainer>
        ) : (
          <div className="empty" style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
            {data ? 'No data in the window yet' : '—'}
          </div>
        )}
      </div>
    </div>
  );
}

// ── long-horizon uptime history (hand-rolled SVG bars) ─────────────────────
// Daily uptime% across 30d / 90d / 1y, coloured by threshold. Built as a bare
// SVG (no charting dep) to match the dependency-graph / sparkline approach.
// Each bar carries a <title> tooltip with the day, pct and sample count.
function UptimeHistoryCard({ data, loading, range, onRangeChange }) {
  const rows = Array.isArray(data) ? data : [];

  // Colour a bar by uptime threshold: green ≥ 99.9, amber ≥ 99, red below.
  // A day with no samples renders as a faint "no-data" stub.
  const colourFor = (pct) =>
    pct == null ? 'var(--border)'
    : pct >= 99.9 ? 'var(--up)'
    : pct >= 99   ? 'var(--warn)'
    :               'var(--down)';

  const RANGES = ['30d', '90d', '1y'];
  const SegBtn = ({ r }) => {
    const isSel = range === r;
    return (
      <button
        type="button" role="radio" aria-checked={isSel}
        aria-label={t('monitor.uptime_history.range_label', { range: r })}
        onClick={() => { if (!isSel) onRangeChange?.(r); }}
        style={{
          flex: 1, padding: '4px 10px', fontSize: 12, fontWeight: 600,
          background: isSel ? 'var(--surface)' : 'transparent',
          color: isSel ? 'var(--text)' : 'var(--text-2)',
          border: 'none', borderRadius: 4,
          boxShadow: isSel ? '0 1px 2px rgba(0,0,0,0.06)' : 'none',
          cursor: isSel ? 'default' : 'pointer',
        }}>
        {r}
      </button>
    );
  };

  // SVG geometry — a fixed viewBox scaled to width via CSS. Bars are evenly
  // spaced across the full range; the y-axis is zoomed to [floor, 100] so
  // day-to-day variation near 100% stays legible (uptime rarely dips far).
  const H = 120, padTop = 6, padBottom = 16, plotH = H - padTop - padBottom;
  const n = rows.length;
  const minPct = rows.reduce((lo, r) => (r.uptime_pct != null && r.uptime_pct < lo ? r.uptime_pct : lo), 100);
  // Floor the y-axis a touch below the worst day (never above 90) so a 100%
  // wall of bars still shows height, but a bad day visibly drops.
  const yFloor = Math.min(90, Math.floor(minPct));
  const span = Math.max(1, 100 - yFloor);
  const barW = n > 0 ? 100 / n : 0; // percentage units in a 0..100 viewBox X
  const fmtDay = (d) => {
    const dt = new Date(`${d}T00:00:00Z`);
    return isNaN(dt.getTime()) ? String(d)
      : dt.toLocaleDateString(undefined, { month: 'short', day: 'numeric', timeZone: 'UTC' });
  };
  const firstLabel = n > 0 ? fmtDay(rows[0].day) : '';
  const lastLabel  = n > 0 ? fmtDay(rows[n - 1].day) : '';

  return (
    <div className="card" style={{ padding: '20px 22px', marginBottom: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 14 }}>
        <Activity size={14} color="var(--text-3)"/>
        <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t('monitor.uptime_history.title')}</h3>
        <div role="radiogroup" aria-label={t('monitor.uptime_history.title')}
          style={{
            marginLeft: 'auto', display: 'inline-flex', alignItems: 'center', gap: 2,
            background: 'var(--surface-2)', padding: 2, borderRadius: 6, width: 156,
          }}>
          {RANGES.map(r => <SegBtn key={r} r={r}/>)}
        </div>
      </div>

      {loading && n === 0 ? (
        <div className="empty" style={{ height: 120, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          {t('common.loading')}
        </div>
      ) : n === 0 ? (
        <div className="empty" style={{ height: 120, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          {t('monitor.uptime_history.empty')}
        </div>
      ) : (
        <>
          <svg viewBox={`0 0 100 ${H}`} preserveAspectRatio="none"
            style={{ width: '100%', height: 120, display: 'block', overflow: 'visible' }}
            role="img" aria-label={t('monitor.uptime_history.title')}>
            {rows.map((r, i) => {
              const pct = r.uptime_pct;
              const hasData = pct != null && (r.sample_count == null || r.sample_count > 0);
              const norm = hasData ? Math.max(0, Math.min(1, (pct - yFloor) / span)) : 0;
              const barH = hasData ? Math.max(1, norm * plotH) : plotH;
              const x = i * barW;
              const y = padTop + (plotH - barH);
              return (
                <rect key={i}
                  x={x + barW * 0.12} y={y}
                  width={barW * 0.76} height={barH}
                  rx={0.4}
                  fill={hasData ? colourFor(pct) : 'var(--border)'}
                  opacity={hasData ? 0.9 : 0.4}>
                  <title>
                    {hasData
                      ? t('monitor.uptime_history.tooltip', {
                          day: fmtDay(r.day),
                          pct: Number(pct).toFixed(2),
                          samples: r.sample_count ?? 0,
                        })
                      : t('monitor.uptime_history.tooltip_nodata', { day: fmtDay(r.day) })}
                  </title>
                </rect>
              );
            })}
          </svg>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 8, fontSize: 11, color: 'var(--text-3)' }}>
            <span>{firstLabel}</span>
            <span className="mono tabular">{t('monitor.uptime_history.floor', { pct: yFloor })}</span>
            <span>{lastLabel}</span>
          </div>
        </>
      )}
    </div>
  );
}

function PushUrlCard({ monitorId, token, lastPushAt, interval, config }) {
  const [currentToken, setCurrentToken] = useState(token);
  const [copiedKey, setCopiedKey] = useState(null);
  const [rotating, setRotating] = useState(false);
  const base = `${window.location.origin}/push/${currentToken}`;
  const cron = config?.cron;

  const copy = async (key, text) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedKey(key);
      setTimeout(() => setCopiedKey(null), 1600);
    } catch { /* clipboard refused — silently no-op */ }
  };

  const regenerate = async () => {
    if (!(await confirmDialog({ message: 'Regenerate the push token? The current URL stops working immediately — '
      + 'any cron / CI job still using it will start receiving 404 responses '
      + 'until you give them the new URL.' }))) return;
    setRotating(true);
    try {
      const r = await api.monitors.regeneratePushToken(monitorId);
      setCurrentToken(r.push_token);
    } catch (e) {
      toast(e.message || 'Failed to regenerate token.', 'error');
    } finally { setRotating(false); }
  };

  // /run and /complete bracket the job; the trailing || /fail pages
  // immediately on a non-zero exit anywhere in the chain.
  const crontab = `${cron || '0 3 * * *'} curl -fsS -m 10 ${base}/run && backup.sh && curl -fsS -m 10 ${base}/complete || curl -fsS -m 10 ${base}/fail`;

  const rows = [
    { key: 'ping',     label: t('monitor.push.ping_url'),        value: base },
    { key: 'run',      label: t('monitor.push.run_url'),         value: `${base}/run` },
    { key: 'complete', label: t('monitor.push.complete_url'),    value: `${base}/complete` },
    { key: 'fail',     label: t('monitor.push.fail_url'),        value: `${base}/fail` },
    { key: 'crontab',  label: t('monitor.push.crontab_example'), value: crontab },
  ];

  const lastPushDate = lastPushAt ? offsetDateTimeArrayToDate(lastPushAt) : null;
  return (
    <div className="card" style={{ padding: '18px 22px', marginBottom: 20, borderLeft: '3px solid var(--accent)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
        <Zap size={14} color="var(--accent)"/>
        <h3 style={{ fontSize: 13, fontWeight: 600, margin: 0 }}>{t('monitor.push.title')}</h3>
        <button className="btn btn-ghost" onClick={regenerate} disabled={rotating} style={{ marginLeft: 'auto', padding: '0 12px', fontSize: 12 }}
          title={t('monitor.token_reissue_tip')}>
          {rotating ? 'Rotating…' : 'Regenerate'}
        </button>
      </div>
      <p style={{ fontSize: 12, color: 'var(--text-2)', margin: '0 0 12px', lineHeight: 1.5 }}>
        {cron
          ? <>Wrap your job: hit /run when it starts, /complete on success, /fail on error.
              Scheduled <code className="mono">{cron}</code> (UTC) — a missed or overrunning run flips the monitor to Down.</>
          : <>Have your cron / CI / backup job call this URL on each successful run.
              If we don't hear from it within {interval}s (plus grace), the monitor flips to Down.</>}
      </p>
      {rows.map(r => (
        <div key={r.key} style={{ marginBottom: 8 }}>
          <div style={{ fontSize: 11, color: 'var(--text-3)', marginBottom: 3 }}>{r.label}</div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'stretch', flexWrap: 'wrap' }}>
            <code className="mono" style={{
              flex: '1 1 320px', padding: '8px 10px', fontSize: 12, background: 'var(--surface-2)',
              border: '1px solid var(--border)', borderRadius: 6, color: 'var(--text)',
              overflow: 'auto', whiteSpace: 'nowrap',
            }}>{r.value}</code>
            <button className="btn btn-ghost" onClick={() => copy(r.key, r.value)} style={{ padding: '0 12px', fontSize: 12 }}>
              {copiedKey === r.key ? <><Check size={12}/> Copied</> : <><Copy size={12}/> Copy</>}
            </button>
          </div>
        </div>
      ))}
      <div style={{ marginTop: 10, fontSize: 11, color: 'var(--text-3)' }}>
        Last push: {lastPushDate ? formatRelative(lastPushDate) : 'never'}
      </div>
    </div>
  );
}

// Inline tag editor. Lists currently-attached tags as chips with × to
// detach; offers a dropdown of all known tags to attach; opens a tiny
// inline form for creating a new tag on the fly. Reloads the page on
// any mutation — the dashboard polls the monitor list so the chips
// over there refresh on the next tick anyway.
function TagsCard({ monitor, onChanged }) {
  const allTags = useApi(() => api.tags.list(), []);
  const [busy,     setBusy]     = useState(false);
  const [creating, setCreating] = useState(false);
  const [newName,  setNewName]  = useState('');
  const [newColor, setNewColor] = useState('#14b8a6');

  const attached = monitor.tags || [];
  const attachedIds = new Set(attached.map(t => t.id));
  const candidates = (allTags.data || []).filter(t => !attachedIds.has(t.id));

  const detach = async (tagId) => {
    setBusy(true);
    try { await api.tags.detach(monitor.id, tagId); onChanged?.(); }
    finally { setBusy(false); }
  };
  const attach = async (tagId) => {
    setBusy(true);
    try { await api.tags.attach(monitor.id, tagId); onChanged?.(); }
    finally { setBusy(false); }
  };
  const createAndAttach = async () => {
    if (!newName.trim()) return;
    setBusy(true);
    try {
      const t = await api.tags.create(newName.trim(), newColor);
      await api.tags.attach(monitor.id, t.id);
      onChanged?.();
    } finally { setBusy(false); }
  };

  return (
    <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
      <div style={{ padding: '16px 22px' }}>
        <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>Tags</h3>
      </div>
      <div style={{ padding: '14px 22px', borderTop: '1px solid var(--border)', display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center' }}>
        {attached.length === 0 && <span style={{ fontSize: 12, color: 'var(--text-3)' }}>No tags attached.</span>}
        {attached.map(t => (
          <span key={t.id} style={{
            display: 'inline-flex', alignItems: 'center', gap: 5,
            background: t.color, color: '#fff',
            fontSize: 11, fontWeight: 500,
            padding: '3px 4px 3px 9px', borderRadius: 999,
          }}>
            {t.name}
            <button disabled={busy} onClick={() => detach(t.id)} title={t('common.detach')} style={{
              background: 'rgba(255,255,255,.2)', border: 'none', color: '#fff',
              borderRadius: '50%', width: 16, height: 16, cursor: 'pointer',
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            }}>
              <X size={9}/>
            </button>
          </span>
        ))}
        {candidates.length > 0 && (
          <select disabled={busy} className="select" style={{ fontSize: 12, padding: '4px 8px', width: 'auto' }}
            value="" onChange={e => e.target.value && attach(e.target.value)}>
            <option value="">+ Attach existing…</option>
            {candidates.map(t => <option key={t.id} value={t.id}>{t.name}</option>)}
          </select>
        )}
        {!creating ? (
          <button className="btn btn-ghost" disabled={busy} onClick={() => setCreating(true)} style={{ padding: '3px 8px', fontSize: 11 }}>
            <Plus size={11}/> New tag
          </button>
        ) : (
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <input autoFocus className="input" placeholder="tag name" value={newName} onChange={e => setNewName(e.target.value)} style={{ width: 130, padding: '4px 8px', fontSize: 12 }}/>
            <input type="color" value={newColor} onChange={e => setNewColor(e.target.value)} title={t('common.color')}/>
            <button className="btn btn-accent" disabled={busy || !newName.trim()} onClick={createAndAttach} style={{ padding: '4px 8px', fontSize: 11 }}>Create</button>
            <button className="btn btn-ghost" disabled={busy} onClick={() => { setCreating(false); setNewName(''); }} aria-label={t('common.cancel')} style={{ padding: '4px 6px' }}><X size={11}/></button>
          </span>
        )}
      </div>
    </div>
  );
}

function DependenciesCard({ monitor }) {
  const [reloadKey, setReloadKey] = useState(0);
  const depsState = useApi(() => api.dependencies.list(monitor.id), [monitor.id, reloadKey]);
  const monitors  = useApi(() => api.monitors.list(), []);
  const [busy, setBusy] = useState(false);
  const [err,  setErr]  = useState(null);

  const parents  = depsState.data?.parents  || [];
  const children = depsState.data?.children || [];
  const byId = new Map((monitors.data || []).map(m => [m.id, m]));
  const candidates = (monitors.data || []).filter(m =>
    m.id !== monitor.id && !parents.includes(m.id)
  );

  // Refetch the dependency list in place rather than reloading the whole page —
  // a full reload bounced the operator back to the overview after each add, so
  // attaching several dependencies meant re-navigating every time.
  const attach = async (parentId) => {
    setErr(null); setBusy(true);
    try { await api.dependencies.attach(monitor.id, parentId); setReloadKey(k => k + 1); }
    catch (e) { setErr(e.message || 'attach failed'); }
    finally { setBusy(false); }
  };
  const detach = async (parentId) => {
    setErr(null); setBusy(true);
    try { await api.dependencies.detach(monitor.id, parentId); setReloadKey(k => k + 1); }
    catch (e) { setErr(e.message || 'detach failed'); }
    finally { setBusy(false); }
  };

  return (
    <div className="card" style={{ padding: 0, overflow: 'hidden', marginBottom: 20 }}>
      <div style={{ padding: '16px 22px' }}>
        <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>Dependencies</h3>
        <div style={{ fontSize: 12, color: 'var(--text-3)', marginTop: 4 }}>
          When any parent is Down, alerts on this monitor are suppressed
          — keeps a root cause from paging every dependent.
        </div>
      </div>
      <div style={{ padding: '14px 22px', borderTop: '1px solid var(--border)' }}>
        <div style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', marginBottom: 6 }}>{t('monitor.detail.depends_on')}</div>
        {err && <div className="banner-err" style={{ marginBottom: 10 }}>{err}</div>}
        {parents.length === 0 && <div style={{ fontSize: 12, color: 'var(--text-3)', marginBottom: 8 }}>No upstream dependencies.</div>}
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginBottom: 10 }}>
          {parents.map(pid => {
            const p = byId.get(pid);
            return (
              <span key={pid} style={{
                display: 'inline-flex', alignItems: 'center', gap: 6,
                background: 'var(--surface-2)', color: 'var(--text)',
                fontSize: 12, padding: '4px 6px 4px 10px', borderRadius: 999,
              }}>
                {p?.name || pid.slice(0, 8)}
                <button disabled={busy} onClick={() => detach(pid)} title={t('common.detach')} style={{
                  background: 'var(--border-2)', border: 'none', color: 'var(--text-2)',
                  borderRadius: '50%', width: 16, height: 16, cursor: 'pointer',
                  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
                }}>
                  <X size={9}/>
                </button>
              </span>
            );
          })}
          {candidates.length > 0 && (
            <select disabled={busy} className="select" style={{ fontSize: 12, padding: '4px 8px', width: 'auto' }}
              value="" onChange={e => e.target.value && attach(e.target.value)}>
              <option value="">+ Add parent…</option>
              {candidates.map(m => <option key={m.id} value={m.id}>{m.name}</option>)}
            </select>
          )}
        </div>

        {children.length > 0 && (
          <>
            <div style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', marginBottom: 6 }}>Dependents</div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
              {children.map(cid => (
                <span key={cid} style={{
                  fontSize: 12, color: 'var(--text-2)',
                  background: 'var(--surface-2)', padding: '3px 8px', borderRadius: 999,
                }}>
                  {byId.get(cid)?.name || cid.slice(0, 8)}
                </span>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
