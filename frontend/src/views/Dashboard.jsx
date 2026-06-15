import React, { useMemo, useState, useRef, useEffect } from 'react';
import {
  LineChart, Line, XAxis, YAxis,
  ResponsiveContainer, Tooltip,
} from 'recharts';
import {
  Search, Plus, Bell, ChevronDown, ChevronRight, Activity,
  AlertCircle, Pause, MoreHorizontal, Calendar,
  Tag, ArrowUpRight, Wrench, Zap, Globe, Server,
  Database, Radio, Lock, Hash,
  Menu, Folder, Tag as TagIcon, Calendar as CalIcon, Network, Key, ScrollText, Users as UsersIcon, Mail, Database as DbIcon, Settings, Upload, FileStack,
  Bookmark, Star, Check, Trash2, X, Copy, Share2, Download, RotateCcw,
} from 'lucide-react';
import {
  api, useApi, formatRelative, offsetDateTimeArrayToDate, statusToClass,
} from '../lib/api.js';
import { useHeartbeatStream, useDebouncedTick } from '../lib/sse.js';
import { ThemeToggle } from '../components/ThemeToggle.jsx';
import { canWrite } from '../lib/roles.js';
import { t } from '../lib/i18n.js';

// ─── shareable saved-view (de)serialisation ─────────────────────────────────
// A saved view is pure filter state — { tags, folder, search } — so it can be
// shared as an opaque, URL-safe token with no backend involvement. We JSON the
// minimal shape, UTF-8 encode, then base64url it (so it survives a URL hash
// param without escaping). encodeView/decodeView are pure + inverse.
function encodeView(view) {
  const payload = {
    tags: Array.isArray(view?.tags) ? view.tags : [],
    folder: view?.folder ?? null,
    search: view?.search || '',
  };
  const json = JSON.stringify(payload);
  // btoa wants a binary string; round-trip UTF-8 through encodeURIComponent so
  // non-ASCII names/searches survive. Then make it base64url (URL-hash safe).
  const b64 = btoa(unescape(encodeURIComponent(json)));
  return b64.replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}
function decodeView(token) {
  if (!token) return null;
  try {
    let b64 = String(token).replace(/-/g, '+').replace(/_/g, '/');
    while (b64.length % 4) b64 += '=';
    const json = decodeURIComponent(escape(atob(b64)));
    const p = JSON.parse(json);
    if (!p || typeof p !== 'object') return null;
    return {
      tags: Array.isArray(p.tags) ? p.tags.filter(x => typeof x === 'string') : [],
      folder: typeof p.folder === 'string' ? p.folder : null,
      search: typeof p.search === 'string' ? p.search : '',
    };
  } catch { return null; }
}
// Pull a ?view=<token> param out of the hash (e.g. "#/?view=abc"). Returns the
// raw token string or null. We parse the query portion of the hash ourselves
// because the app is hash-routed and `location.search` is empty.
function viewTokenFromHash() {
  const h = window.location.hash || '';
  const q = h.indexOf('?');
  if (q === -1) return null;
  try { return new URLSearchParams(h.slice(q + 1)).get('view'); }
  catch { return null; }
}

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
  /* drag-to-folder affordance: grab cursor + a faint move handle on hover,
     and a dim ghost while the row is being dragged. */
  .mon-row.draggable { cursor: grab; }
  .mon-row.draggable:active { cursor: grabbing; }
  .mon-row.dragging { opacity: .45; }
  /* drop highlight on the folder header being hovered during a drag */
  .group-head.drop-target {
    background: var(--accent-soft); color: var(--accent-2);
    border-radius: 8px; outline: 1px dashed var(--accent);
  }

  /* per-row clone affordance — only visible on row hover to keep the list calm */
  .clone-action { opacity: 0; transition: opacity .1s; background: none; border: none; cursor: pointer; padding: 4px; border-radius: 6px; color: var(--text-3); display: inline-flex; }
  .activity-row:hover .clone-action { opacity: 1; }
  .clone-action:hover { background: var(--surface-2); color: var(--text); }

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
function MonitorRow({ m, active, onClick, uptimePct, draggable, dragging, onDragStart, onDragEnd }) {
  const Icon = kindIcon(m.kind);
  const cls = statusToClass(m.current_status);
  return (
    <div
      className={`mon-row ${active ? 'active' : ''}${draggable ? ' draggable' : ''}${dragging ? ' dragging' : ''}`}
      onClick={onClick}
      draggable={draggable || undefined}
      onDragStart={draggable ? onDragStart : undefined}
      onDragEnd={draggable ? onDragEnd : undefined}
      aria-grabbed={draggable ? (dragging ? true : false) : undefined}
      title={draggable ? t('monitor.move.hint') : undefined}>
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

// Render a bulk-edit preview value (from/to) compactly. Handles null (cleared
// group), booleans (enabled), arrays (tag sets) and scalars.
function fmtBulkVal(v) {
  if (v === null || v === undefined) return '—';
  if (Array.isArray(v)) return v.length ? `[${v.length}]` : '[]';
  if (typeof v === 'boolean') return v ? 'on' : 'off';
  return String(v);
}

// Compact SLO error-budget health for the dashboard sidebar. Self-contained —
// fetches its own list, hides entirely when no SLOs are defined.
function sloBudgetColor(r) {
  if (r == null) return 'var(--text-3)';
  if (r <= 0) return 'var(--down)';
  if (r < 25) return 'var(--warn)';
  return 'var(--up)';
}
function SloWidget() {
  const state = useApi(() => api.slos.list(), [], { pollMs: 60_000 });
  const slos = state.data || [];
  if (state.loading || slos.length === 0) return null;
  const breaching = slos.filter(s => s.snapshot && s.snapshot.breaching).length;
  // Worst budgets first so the sidebar shows what's at risk.
  const sorted = [...slos].sort((a, b) =>
    (a.snapshot?.remaining_pct ?? 100) - (b.snapshot?.remaining_pct ?? 100));
  return (
    <div className="card" style={{ padding: 14, marginBottom: 16 }}>
      <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', marginBottom: 12 }}>
        <a href="#/slos" style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em', textDecoration: 'none' }}>
          {t('slos.title')}
        </a>
        {breaching > 0
          ? <span className="mono" style={{ fontSize: 11, color: 'var(--down)', fontWeight: 600 }}>{breaching} {t('slos.breaching').toLowerCase()}</span>
          : <span className="mono" style={{ fontSize: 11, color: 'var(--up)' }}>{t('slos.healthy').toLowerCase()}</span>}
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 9 }}>
        {sorted.slice(0, 5).map(s => {
          const r = s.snapshot?.remaining_pct;
          return (
            <a key={s.id} href="#/slos" style={{ textDecoration: 'none', color: 'inherit' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11.5, marginBottom: 3 }}>
                <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 150 }}>{s.name}</span>
                <span className="mono" style={{ color: 'var(--text-3)' }}>{r == null ? '—' : `${Math.round(r)}%`}</span>
              </div>
              <div style={{ height: 6, borderRadius: 999, background: 'var(--surface-2)', overflow: 'hidden' }}>
                <div style={{ width: `${r == null ? 0 : Math.max(0, Math.min(100, r))}%`, height: '100%', background: sloBudgetColor(r) }}/>
              </div>
            </a>
          );
        })}
      </div>
    </div>
  );
}

// Recent open error issues across all projects — dashboard sidebar feed.
function errLevelColor(lvl) {
  if (lvl === 'fatal' || lvl === 'error') return 'var(--down)';
  if (lvl === 'warning' || lvl === 'warn') return 'var(--warn)';
  return 'var(--text-3)';
}
function ErrorsWidget() {
  const state = useApi(() => api.errorIssues.recent(), [], { pollMs: 60_000 });
  const issues = state.data || [];
  if (state.loading || issues.length === 0) return null;
  return (
    <div className="card" style={{ padding: 14, marginBottom: 16 }}>
      <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', marginBottom: 12 }}>
        <a href="#/errors" style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em', textDecoration: 'none' }}>
          {t('dashboard.errors.title')}
        </a>
        <span className="mono" style={{ fontSize: 11, color: 'var(--text-3)' }}>{issues.length}</span>
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 9 }}>
        {issues.map(i => (
          <a key={i.id} href={`#/errors/${i.id}`} style={{ textDecoration: 'none', color: 'inherit', display: 'block' }}>
            <div style={{ display: 'flex', gap: 7, alignItems: 'baseline' }}>
              <span style={{ width: 6, height: 6, borderRadius: 3, background: errLevelColor(i.level), flexShrink: 0, alignSelf: 'center' }}/>
              <span style={{ fontSize: 12, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }} title={i.title}>{i.title}</span>
              <span className="mono" style={{ fontSize: 10.5, color: 'var(--text-3)' }}>×{i.times_seen}</span>
            </div>
            {i.culprit && <div style={{ fontSize: 10.5, color: 'var(--text-3)', marginLeft: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{i.culprit}</div>}
          </a>
        ))}
      </div>
    </div>
  );
}

// ─── main component ───────────────────────────────────────────────────────
export default function Dashboard({ user, onLogout } = {}) {
  // Whether the current user may mutate (admin/editor). Readonly users see
  // the data but no create / bulk / pause / delete affordances. The backend
  // is the real gate (it 403s); this is UX so they don't hit dead buttons.
  const writable = canWrite(user);
  // Live heartbeat overlay — SSE pushes mutate `liveHb` immediately so
  // status pulses don't wait for the next poll. The polled hooks below
  // still run as the source of truth + reconnect fallback.
  const [liveHb, setLiveHb] = useState(() => new Map());
  const stream = useHeartbeatStream(hb => {
    setLiveHb(prev => {
      const next = new Map(prev);
      next.set(hb.monitor_id, hb);
      return next;
    });
  });
  // Debounced tick lets polled hooks refetch on activity bursts without
  // hammering — coalesces N heartbeats per ~500ms window into one bump.
  const liveTick = useDebouncedTick(stream.lastEventAt, 500);

  // Manual refetch trigger. Bumping it re-runs the monitor/group fetches
  // without a full page reload — used by bulk-edit (so the undo bar can
  // survive the refresh) and the drag-to-folder move.
  const [reloadKey, setReloadKey] = useState(0);
  const bumpReload = () => setReloadKey(k => k + 1);

  const monitorsState = useApi(() => api.monitors.list(),         [liveTick, reloadKey], { pollMs: 30_000 });
  const groupsState   = useApi(() => api.monitorGroups.list(),     [reloadKey],          { pollMs: 60_000 });
  const channelsState = useApi(() => api.notifications.list(),     [],         { pollMs: 60_000 });
  const tagsState     = useApi(() => api.tags.list(),              [],         { pollMs: 60_000 });
  const [windowSec, setWindowSec] = useState(86400); // 1h | 24h | 7d | 30d
  // small helper so the summary-card label stays in sync with the picker.
  // eslint-disable-next-line no-inner-declarations
  function windowLabel(s) {
    return s === 3600 ? 'last 1h'
         : s === 86400 ? 'last 24h'
         : s === 604800 ? 'last 7d'
         : s === 2592000 ? 'last 30d'
         : `last ${s}s`;
  }
  // Backend version — read once at mount off `/healthz`. The api binary
  // bakes `CARGO_PKG_VERSION` (= workspace.package.version) into the
  // response, so the header pill always reflects the running build and
  // can't drift like the hard-coded "v0.4.0" string it used to be.
  const [version, setVersion] = useState(null);
  useEffect(() => {
    let cancelled = false;
    api.health.live()
      .then(d => { if (!cancelled) setVersion(d?.version || null); })
      .catch(() => {});
    return () => { cancelled = true; };
  }, []);

  const summaryState  = useApi(() => api.monitors.summary(windowSec), [windowSec, liveTick], { pollMs: 30_000 });
  const historyState  = useApi(() => api.monitors.history(60),    [liveTick], { pollMs: 30_000 });
  const countsState   = useApi(() => api.notifications.counts(),  [], { pollMs: 60_000 });
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

  // Per-group expand/collapse state — keyed by group id (or 'ungrouped').
  // Default-open so existing users see all monitors without a click.
  // Folder collapse state lives in localStorage so toggles survive a reload.
  // Default-open semantics: absent key = open; only explicit `false` collapses.
  const [openGroups, setOpenGroups] = useState(() => {
    try {
      const raw = localStorage.getItem('rampart_open_groups');
      const parsed = raw ? JSON.parse(raw) : null;
      if (parsed && typeof parsed === 'object') return parsed;
    } catch { /* fall through */ }
    return { ungrouped: true };
  });
  const toggleGroup = (key) => setOpenGroups(s => {
    const next = { ...s, [key]: !(s[key] ?? true) };
    try { localStorage.setItem('rampart_open_groups', JSON.stringify(next)); } catch { /* ignore quota */ }
    return next;
  });

  // Bulk-selection state for the activity table.
  const [selected, setSelected] = useState(() => new Set());
  const [bulkBusy, setBulkBusy] = useState(false);
  // Inline bulk-edit form: null when closed, else the working form state.
  const [bulkEdit, setBulkEdit] = useState(null);
  // Dry-run preview returned by bulk-edit?dry_run=true: null when not
  // previewing, else { would_update, would_skip, preview: [...] }. Shown in a
  // confirm panel before the operator commits the real edit.
  const [bulkPreview, setBulkPreview] = useState(null);
  // Undo payload captured from the last real bulk-edit:
  // { undo: { ids, patch }, undo_partial, n }. POST it straight back to revert.
  const [bulkUndo, setBulkUndo] = useState(null);
  // Clone-into-folder dialog: null when closed, else { monitor, group, busy }.
  // `group` is the chosen target group id ('' = inherit source, '__ungroup__'
  // = ungrouped, else a group id).
  const [cloneState, setCloneState] = useState(null);
  const submitClone = async () => {
    if (!cloneState || cloneState.busy) return;
    setCloneState(s => ({ ...s, busy: true }));
    const g = cloneState.group;
    const opts = {};
    if (g === '__ungroup__') opts.group_id = null;
    else if (g) opts.group_id = g;
    try {
      await api.monitors.clone(cloneState.monitor.id, opts);
      setCloneState(null);
      window.location.reload();
    } catch (e) {
      alert(t("dashboard.clone.failed", { msg: e.message }));
      setCloneState(s => s && ({ ...s, busy: false }));
    }
  };
  const toggleSelect = (id) => setSelected(prev => {
    const next = new Set(prev);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });

  // ── drag a monitor between folders (sidebar tree) ────────────────────────
  // `dragMon` holds the id of the monitor currently being dragged; `dropTarget`
  // is the bucket key being hovered ('ungrouped' → null group). Editor/admin
  // only. The existing folder-assignment UI (bulk-edit, per-monitor config)
  // stays as the keyboard / non-drag path.
  const [dragMon, setDragMon] = useState(null);
  const [dropTarget, setDropTarget] = useState(null);
  const moveMonitorToGroup = async (monitorId, groupKey) => {
    if (!monitorId) return;
    // 'ungrouped' bucket → clear the folder (null); any other key is a group id.
    const targetId = groupKey === 'ungrouped' ? null : groupKey;
    const mon = monitors.find(m => m.id === monitorId);
    // No-op when the monitor is already in the target folder.
    if (mon && (mon.group_id || null) === targetId) return;
    try {
      await api.monitors.update(monitorId, { group_id: targetId });
      bumpReload();
    } catch (e) {
      alert(t("monitor.move.failed", { msg: e.message }));
    }
  };
  const runBulk = async (action, confirmMsg) => {
    if (selected.size === 0 || bulkBusy) return;
    if (confirmMsg && !confirm(confirmMsg)) return;
    setBulkBusy(true);
    try {
      await api.monitors.bulk(Array.from(selected), action);
      setSelected(new Set());
      window.location.reload();
    } catch (e) {
      alert(`Bulk action failed: ${e.message}`);
      setBulkBusy(false);
    }
  };
  // Build the patch per the bulk-edit contract: only include fields the
  // operator actually touched. `enabled` is a tri-state ('' = leave alone),
  // `group` is '' (leave) / '__ungroup__' (clear → null) / a group id, and
  // `tags` is a full replace of the tag set when the toggle is on. Returns
  // null when nothing was touched.
  const buildBulkPatch = () => {
    if (!bulkEdit) return null;
    const patch = {};
    const interval = bulkEdit.interval.trim();
    const timeout = bulkEdit.timeout.trim();
    if (interval !== '') patch.interval_secs = Number(interval);
    if (timeout !== '') patch.timeout_secs = Number(timeout);
    if (bulkEdit.enabled === 'enabled') patch.enabled = true;
    else if (bulkEdit.enabled === 'paused') patch.enabled = false;
    if (bulkEdit.group === '__ungroup__') patch.group_id = null;
    else if (bulkEdit.group) patch.group_id = bulkEdit.group;
    if (bulkEdit.setTagsOn) patch.tags = Array.from(bulkEdit.tags);
    return Object.keys(patch).length === 0 ? null : patch;
  };

  // Dry run: ask the backend what it WOULD change, then show the per-monitor
  // diff in a confirm panel before the operator commits. No mutation here.
  const previewBulkEdit = async () => {
    if (selected.size === 0 || bulkBusy || !bulkEdit) return;
    const patch = buildBulkPatch();
    if (!patch) { alert(t("dashboard.bulk.edit_empty")); return; }
    setBulkBusy(true);
    try {
      const res = await api.monitors.bulkEditPreview(Array.from(selected), patch);
      setBulkPreview(res);
    } catch (e) {
      alert(t("dashboard.bulk.preview_failed", { msg: e.message }));
    } finally {
      setBulkBusy(false);
    }
  };

  const runBulkEdit = async () => {
    if (selected.size === 0 || bulkBusy || !bulkEdit) return;
    const patch = buildBulkPatch();
    // Nothing to do — guard so we don't fire a no-op request.
    if (!patch) {
      alert(t("dashboard.bulk.edit_empty"));
      return;
    }
    setBulkBusy(true);
    try {
      const res = await api.monitors.bulkEdit(Array.from(selected), patch);
      setBulkEdit(null);
      setBulkPreview(null);
      setSelected(new Set());
      // Capture the inverse request so we can offer a one-click undo. We
      // refetch the lists rather than reload the page so the undo bar can
      // persist across the refresh.
      if (res?.undo) {
        setBulkUndo({ undo: res.undo, undo_partial: !!res.undo_partial, n: res.updated ?? 0 });
      }
      bumpReload();
    } catch (e) {
      alert(t("dashboard.bulk.edit_failed", { msg: e.message }));
    } finally {
      setBulkBusy(false);
    }
  };

  // Replay the inverse request the backend handed back. `undo` is already a
  // ready-to-POST { ids, patch } body.
  const undoBulkEdit = async () => {
    if (!bulkUndo || bulkBusy) return;
    setBulkBusy(true);
    try {
      await api.monitors.bulkEdit(bulkUndo.undo.ids, bulkUndo.undo.patch);
      setBulkUndo(null);
      bumpReload();
    } catch (e) {
      alert(t("dashboard.bulk.undo_failed", { msg: e.message }));
    } finally {
      setBulkBusy(false);
    }
  };
  // Pause/resume every monitor carrying a tag — used by the tag-filter
  // chips' "pause all / resume all" actions. Confirmed first since it can
  // touch monitors that aren't currently on screen (the tag may apply to
  // more than the filtered/visible set).
  const runBulkByTag = async (tagId, tagName, action) => {
    if (bulkBusy) return;
    const verb = action === 'pause' ? t('dashboard.bulk_tag.pause') : t('dashboard.bulk_tag.resume');
    if (!confirm(t('dashboard.bulk_tag.confirm', { verb, tag: tagName }))) return;
    setBulkBusy(true);
    try {
      await api.monitors.bulkByTag(tagId, action);
      window.location.reload();
    } catch (e) {
      alert(t('dashboard.bulk_tag.failed', { msg: e.message }));
      setBulkBusy(false);
    }
  };

  const [query, setQuery] = useState('');
  // tag IDs to require — persisted to localStorage so the chosen filter
  // survives reloads (operators often filter to "prod" and want it sticky).
  const [tagFilter, setTagFilter] = useState(() => {
    try {
      const raw = localStorage.getItem('rampart_tag_filter');
      const arr = raw ? JSON.parse(raw) : null;
      if (Array.isArray(arr)) return new Set(arr);
    } catch { /* fall through */ }
    return new Set();
  });

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
      try { localStorage.setItem('rampart_tag_filter', JSON.stringify([...next])); } catch { /* ignore quota */ }
      return next;
    });
  };

  const matchesTagFilter = (m) => {
    if (tagFilter.size === 0) return true;
    const ids = new Set((m.tags || []).map(t => t.id));
    for (const need of tagFilter) if (!ids.has(need)) return false;
    return true;
  };

  // ── saved views + default folder (server-backed per-user prefs) ──────────
  // When signed in we round-trip a single opaque blob through /v1/me/prefs:
  //   { saved_views: [{ id, name, tags: [id], folder: id|null, search }],
  //     default_folder_id: id|null }
  // The blob is the source of truth; we mirror it into React state so the
  // dropdown re-renders. Signed-out users keep the old localStorage-only
  // behaviour (tagFilter + openGroups already persist there), so this layer
  // is purely additive and degrades cleanly.
  const loggedIn = !!user;
  // `folderFilter` scopes the visible monitors to one folder (null = all).
  // It's part of what a view captures and what the default folder restores.
  const [folderFilter, setFolderFilter] = useState(null);
  const [savedViews, setSavedViews] = useState([]);
  const [defaultFolderId, setDefaultFolderId] = useState(null);
  // Guard so we only seed from prefs once, and don't echo that seed back as a
  // write (which would race the initial GET).
  const prefsLoaded = useRef(false);

  useEffect(() => {
    if (!loggedIn) { prefsLoaded.current = true; return; }
    let cancelled = false;
    api.me.getPrefs()
      .then(p => {
        if (cancelled) return;
        const views = Array.isArray(p?.saved_views) ? p.saved_views : [];
        setSavedViews(views);
        const def = p?.default_folder_id ?? null;
        setDefaultFolderId(def);
        // Open to the default folder on load (only if nothing's been picked).
        if (def) setFolderFilter(prev => prev ?? def);
      })
      .catch(() => { /* fall back to localStorage-only behaviour */ })
      .finally(() => { if (!cancelled) prefsLoaded.current = true; });
    return () => { cancelled = true; };
  }, [loggedIn]);

  // Persist the views + default folder back to the server. Best-effort: a
  // failed write surfaces via the returned promise so callers can alert.
  const persistPrefs = async (next) => {
    const payload = {
      saved_views: next.savedViews ?? savedViews,
      default_folder_id: next.defaultFolderId !== undefined ? next.defaultFolderId : defaultFolderId,
    };
    if (!loggedIn) return; // signed-out: nothing server-side to persist
    await api.me.setPrefs(payload);
  };

  const saveCurrentView = async () => {
    const name = (prompt(t("dashboard.views.save_prompt")) || '').trim();
    if (!name) return;
    const view = {
      id: (crypto?.randomUUID?.() || String(Date.now())),
      name,
      tags: [...tagFilter],
      folder: folderFilter,
      search: query,
    };
    const next = [...savedViews, view];
    setSavedViews(next);
    try { await persistPrefs({ savedViews: next }); }
    catch (e) { alert(t("dashboard.views.save_failed", { msg: e.message })); }
  };

  const applyView = (view) => {
    setTagFilter(new Set(view.tags || []));
    try { localStorage.setItem('rampart_tag_filter', JSON.stringify(view.tags || [])); } catch { /* ignore quota */ }
    setFolderFilter(view.folder ?? null);
    setQuery(view.search || '');
  };

  const deleteView = async (view) => {
    if (!confirm(t("dashboard.views.delete_confirm", { name: view.name }))) return;
    const next = savedViews.filter(v => v.id !== view.id);
    setSavedViews(next);
    try { await persistPrefs({ savedViews: next }); } catch { /* keep optimistic state */ }
  };

  // Toggle a folder as the dashboard's default landing folder. Clicking the
  // current default clears it.
  const toggleDefaultFolder = async (folderId) => {
    const next = defaultFolderId === folderId ? null : folderId;
    setDefaultFolderId(next);
    try { await persistPrefs({ defaultFolderId: next }); } catch { /* keep optimistic state */ }
  };

  // ── share / export saved views ───────────────────────────────────────────
  // Serialise a view (saved or the current filter state) to an opaque token
  // and a ready-to-share deep link. No backend: a view is just filter state.
  const exportView = (view) => {
    const token = encodeView(view ?? { tags: [...tagFilter], folder: folderFilter, search: query });
    const base = `${window.location.origin}${window.location.pathname}`;
    return { token, url: `${base}#/?view=${token}` };
  };

  // Apply an imported view to the live filters. When `save` is true we also
  // persist it into the user's saved views (reusing the existing setPrefs
  // round-trip) so a shared link can become a permanent personal view.
  const importView = async (token, { name, save } = {}) => {
    const decoded = decodeView(token);
    if (!decoded) { alert(t("dashboard.views.import_invalid")); return false; }
    applyView(decoded);
    if (save) {
      const view = {
        id: (crypto?.randomUUID?.() || String(Date.now())),
        name: (name || '').trim() || t("dashboard.views.imported_name"),
        tags: decoded.tags,
        folder: decoded.folder,
        search: decoded.search,
      };
      const next = [...savedViews, view];
      setSavedViews(next);
      try { await persistPrefs({ savedViews: next }); }
      catch (e) { alert(t("dashboard.views.save_failed", { msg: e.message })); }
    }
    return true;
  };

  // On load, if the URL hash carries a ?view= token, apply it once. This lets
  // a shared deep link land the recipient straight into the captured filters.
  const viewHashApplied = useRef(false);
  useEffect(() => {
    if (viewHashApplied.current) return;
    const token = viewTokenFromHash();
    if (!token) { viewHashApplied.current = true; return; }
    const decoded = decodeView(token);
    if (decoded) applyView(decoded);
    viewHashApplied.current = true;
    // Strip the ?view= param so a later "save view" / reload doesn't re-apply
    // it, but keep the user on the dashboard route.
    try { window.history.replaceState(null, '', `${window.location.pathname}#/`); } catch { /* ignore */ }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const matchesFolderFilter = (m) => {
    if (!folderFilter) return true;
    return m.group_id === folderFilter;
  };

  const filtered = monitors
    .filter(m => m.name.toLowerCase().includes(query.toLowerCase()))
    .filter(matchesTagFilter)
    .filter(matchesFolderFilter);

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
      <header className="dash-topbar" style={{
        display: 'flex', alignItems: 'center', gap: 16,
        padding: '12px 20px', borderBottom: '1px solid var(--border)',
        background: 'var(--surface)', position: 'sticky', top: 0, zIndex: 10
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          {/* Brand mark — inline copy of docs/assets/logo.svg (slate shield
              + orange ECG pulse). Kept inline rather than fetched via an
              <img> so it inherits text colour transitions and renders
              without an extra HTTP request from the embedded bundle. */}
          <svg width="28" height="28" viewBox="0 0 24 24" role="img" aria-label="Rampart">
            <path fill="#3b414c" d="M12 2 L20 4 V12 C20 17 17 21 12 22 C7 21 4 17 4 12 V4 Z"/>
            <path fill="none" stroke="#d27a3c" strokeWidth="1.6"
                  strokeLinecap="round" strokeLinejoin="round"
                  d="M6 13 H9 L11 9 L13 16 L15 7 L17 13 H18"/>
          </svg>
          <span style={{ fontSize: 15, fontWeight: 600, letterSpacing: '-.01em' }}>Rampart</span>
          <span className="pill" title={version ? `Running rampart-api v${version}` : 'Version unavailable'}
                style={{ background: 'var(--surface-2)', color: 'var(--text-3)' }}>
            {version ? `v${version}` : '·'}
          </span>
        </div>

        <div style={{ position: 'relative', flex: 1, maxWidth: 420 }}>
          <Search size={14} color="var(--text-3)" style={{ position: 'absolute', left: 12, top: '50%', transform: 'translateY(-50%)' }}/>
          <input className="search" placeholder={t("dashboard.search_placeholder")} value={query} onChange={e => setQuery(e.target.value)}/>
          <span className="kbd" style={{ position: 'absolute', right: 10, top: '50%', transform: 'translateY(-50%)' }}>⌘K</span>
        </div>

        <div style={{ display: 'flex', gap: 8, marginLeft: 'auto', alignItems: 'center' }}>
          <span title={stream.connected ? 'Live · receiving heartbeat stream' : 'Live stream offline · falling back to polling'}
            style={{
              width: 8, height: 8, borderRadius: '50%',
              background: stream.connected ? 'var(--up)' : 'var(--text-3)',
              boxShadow: stream.connected ? '0 0 0 3px var(--up-soft)' : 'none',
              transition: 'background .2s',
            }}/>
          <ThemeToggle/>
          <button className="btn btn-ghost" title="Menu" aria-label="Open navigation"
            onClick={() => window.dispatchEvent(new Event('rampart:nav-open'))}>
            <Menu size={16}/>
          </button>
          <a className="btn btn-ghost" title="Notification channels" href="#/notifications" style={{ textDecoration: 'none' }}>
            <Bell size={14}/>
          </a>
          <ViewsMenu
            loggedIn={loggedIn}
            views={savedViews}
            onSave={saveCurrentView}
            onApply={applyView}
            onDelete={deleteView}
            onExport={exportView}
            onImport={importView}
          />
          <button className="btn" onClick={goToStatusPage}><Wrench size={13}/> {t("dashboard.status_page")}</button>
          {writable && <a className="btn btn-ghost" href="#/import" style={{ textDecoration: 'none' }} title={t("import.link_title")}><Upload size={13}/> {t("import.link")}</a>}
          {writable && <button className="btn btn-accent" onClick={goToNewMonitor}><Plus size={13} strokeWidth={2.4}/> {t("dashboard.add_monitor")}</button>}
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
              <button className="btn btn-ghost" onClick={onLogout} title={t("dashboard.sign_out")} style={{ fontSize: 12 }}>
                {t("dashboard.sign_out")}
              </button>
            </div>
          )}
        </div>
      </header>

      {/* ─── layout ──────────────────────────────────────────────── */}
      <div className="dash-shell" style={{ display: 'grid', gridTemplateColumns: '320px 1fr' }}>

        {/* ─── sidebar ───────────────────────────────────────────── */}
        <aside className="dash-sidebar" style={{
          borderRight: '1px solid var(--border)', background: 'var(--surface)',
          padding: '16px 12px', height: 'calc(100vh - 57px)', overflowY: 'auto',
          position: 'sticky', top: 57
        }}>
          {/* status summary card */}
          <div className="card" style={{ padding: 14, marginBottom: 16 }}>
            <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', marginBottom: 12 }}>
              <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em' }}>
                {t("dashboard.kpi.eyebrow", { window: windowLabel(windowSec) })}
              </span>
              <span className="mono tabular" style={{ fontSize: 11, color: 'var(--text-3)' }}>{t("dashboard.kpi.total", { n: monitors.length })}</span>
            </div>
            <div className="kpi-grid" style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 8 }}>
              {[
                { label: t("dashboard.kpi.up"),     v: upCount,     color: 'var(--up)' },
                { label: t("dashboard.kpi.warn"),   v: warnCount,   color: 'var(--warn)' },
                { label: t("dashboard.kpi.down"),   v: downCount,   color: 'var(--down)' },
                { label: t("dashboard.kpi.paused"), v: pausedCount, color: 'var(--paused)' },
              ].map(s => (
                <div key={s.label} style={{ textAlign: 'center' }}>
                  <div className="tabular" style={{ fontSize: 22, fontWeight: 600, color: s.color, lineHeight: 1 }}>{s.v}</div>
                  <div style={{ fontSize: 10, color: 'var(--text-3)', marginTop: 4, textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 500 }}>{s.label}</div>
                </div>
              ))}
            </div>
          </div>

          <SloWidget />
          <ErrorsWidget />

          {monitors.length > 0 ? (() => {
            const groups = groupsState.data || [];
            // Group folders by parent so we can walk the tree in DFS order.
            // A folder's own count is just its DIRECT monitors — nested
            // folders render as their own indented bucket below.
            const byParent = new Map();
            for (const g of groups) {
              const k = g.parent_id || '__root__';
              if (!byParent.has(k)) byParent.set(k, []);
              byParent.get(k).push(g);
            }
            const buckets = [];
            // Track each bucket's ancestor chain so we can hide a folder
            // when ANY ancestor is collapsed (cascading collapse).
            const walk = (key, depth, ancestors) => {
              for (const g of (byParent.get(key) || [])) {
                buckets.push({
                  key:  g.id,
                  name: g.name,
                  rows: filtered.filter(m => m.group_id === g.id),
                  depth,
                  ancestors,
                });
                walk(g.id, depth + 1, [...ancestors, g.id]);
              }
            };
            walk('__root__', 0, []);
            const ungrouped = filtered.filter(m => !m.group_id);
            buckets.push({ key: 'ungrouped', name: 'Ungrouped', rows: ungrouped, depth: 0, ancestors: [] });
            // Skip empty buckets unless the bucket is the only one. Folders
            // with no direct monitors but with non-empty descendants stay
            // visible so the hierarchy reads correctly. Then hide anything
            // beneath a collapsed ancestor — collapsing "Production" should
            // also hide "Databases" + its children, not leave them dangling.
            const hasDescendantWithRows = (gid) => {
              for (const child of (byParent.get(gid) || [])) {
                if (filtered.some(m => m.group_id === child.id)) return true;
                if (hasDescendantWithRows(child.id)) return true;
              }
              return false;
            };
            const visible = buckets
              .filter(b =>
                b.rows.length > 0 || (b.key !== 'ungrouped' && hasDescendantWithRows(b.key))
              )
              .filter(b =>
                // openGroups[key] defaults to true (open) when absent.
                b.ancestors.every(a => openGroups[a] ?? true)
              );
            const display = visible.length === 0 ? [{ key:'ungrouped', name:'Monitors', rows: filtered, depth: 0 }] : visible;
            return display.map(b => {
              const open = openGroups[b.key] ?? true;
              // Folder headers (and the Ungrouped bucket) are drop zones when a
              // monitor is being dragged. Editor/admin only.
              const isDropZone = writable && b.key !== undefined;
              const dropHandlers = isDropZone ? {
                onDragOver: (e) => { if (dragMon) { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; } },
                onDragEnter: (e) => { if (dragMon) { e.preventDefault(); setDropTarget(b.key); } },
                onDragLeave: () => setDropTarget(prev => (prev === b.key ? null : prev)),
                onDrop: (e) => {
                  e.preventDefault();
                  const id = dragMon || e.dataTransfer.getData('text/plain');
                  setDropTarget(null);
                  setDragMon(null);
                  if (id) moveMonitorToGroup(id, b.key);
                },
              } : {};
              return (
                <div key={b.key} style={{ marginBottom: 4 }}>
                  <div
                    className={`group-head${isDropZone && dropTarget === b.key ? ' drop-target' : ''}`}
                    style={{ paddingLeft: 12 + (b.depth || 0) * 14 }}
                    onClick={() => toggleGroup(b.key)}
                    {...dropHandlers}>
                    {open ? <ChevronDown size={11}/> : <ChevronRight size={11}/>}
                    <span>{b.name}</span>
                    {loggedIn && b.key !== 'ungrouped' && (
                      <button
                        title={defaultFolderId === b.key ? t("dashboard.views.is_default") : t("dashboard.views.default_folder")}
                        onClick={(e) => { e.stopPropagation(); toggleDefaultFolder(b.key); }}
                        style={{
                          marginLeft: 'auto', background: 'transparent', border: 'none',
                          cursor: 'pointer', padding: 2, display: 'inline-flex', alignItems: 'center',
                          color: defaultFolderId === b.key ? 'var(--warn)' : 'var(--text-3)',
                        }}>
                        <Star size={11} fill={defaultFolderId === b.key ? 'var(--warn)' : 'none'}/>
                      </button>
                    )}
                    <span style={{ marginLeft: (loggedIn && b.key !== 'ungrouped') ? 8 : 'auto', color: 'var(--text-3)', fontWeight: 500 }}>{b.rows.length}</span>
                  </div>
                  {open && b.rows.map(m => (
                    <MonitorRow
                      key={m.id}
                      m={m}
                      active={false}
                      onClick={() => openMonitor(m.id)}
                      uptimePct={summaryById.get(m.id)?.uptime_pct}
                      draggable={writable}
                      dragging={dragMon === m.id}
                      onDragStart={(e) => {
                        setDragMon(m.id);
                        e.dataTransfer.effectAllowed = 'move';
                        e.dataTransfer.setData('text/plain', m.id);
                      }}
                      onDragEnd={() => { setDragMon(null); setDropTarget(null); }}
                    />
                  ))}
                </div>
              );
            });
          })() : !monitorsState.loading && (
            <div className="empty" style={{ paddingTop: 32 }}>
              {t("dashboard.empty.title")}<br/>
              {writable && (
                <button className="btn btn-accent" onClick={goToNewMonitor} style={{ marginTop: 12 }}>
                  <Plus size={13}/> {t("dashboard.empty.create_first")}
                </button>
              )}
            </div>
          )}

          {monitorsState.loading && monitors.length === 0 && (
            <div className="empty">{t("dashboard.loading_monitors")}</div>
          )}
        </aside>

        {/* ─── main panel ────────────────────────────────────────── */}
        <main className="dash-main" style={{ padding: '28px 36px', maxWidth: 1500 }}>

          {/* hero — health-reactive banner: accent edge in the health
              colour, alert tint when something's down/degraded, calm
              surface when all clear. */}
          <div style={{
            display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between',
            marginBottom: 28, padding: '18px 22px', borderRadius: 12,
            border: '1px solid var(--border)', borderLeft: `4px solid ${heroColor}`,
            background: (anyDown || anyWarn) ? heroSoft : 'var(--surface)',
            transition: 'background .25s, border-color .25s',
          }}>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 6 }}>
                <span className="dot" style={{ background: heroColor, boxShadow: `0 0 0 5px ${heroSoft}` }}/>
                <h1 style={{ fontSize: 26, fontWeight: 600, margin: 0, letterSpacing: '-.02em', color: (anyDown || anyWarn) ? heroColor : 'var(--text)' }}>{heroTitle}</h1>
              </div>
              <p style={{ fontSize: 14, color: 'var(--text-2)', margin: '0 0 0 20px' }}>
                {monitors.length === 0 && <>{t("dashboard.hero.first_monitor")}</>}
                {monitors.length > 0 && anyDown && <>{downCount} unreachable{warnCount > 0 ? `, ${warnCount} degraded` : ''} — auto-refreshing every 10s</>}
                {monitors.length > 0 && !anyDown && anyWarn && <>{warnCount} degraded — auto-refreshing every 10s</>}
                {monitors.length > 0 && !anyDown && !anyWarn && <>{monitors.length} monitor{monitors.length > 1 ? 's' : ''} healthy — auto-refreshing every 10s</>}
              </p>
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <Calendar size={13} color="var(--text-3)"/>
              <select className="btn" value={windowSec}
                onChange={e => setWindowSec(parseInt(e.target.value, 10))}
                style={{ paddingRight: 26, cursor: 'pointer', appearance: 'auto' }}
                title="Rollup window for uptime + latency above">
                <option value={3600}>Last 1h</option>
                <option value={86400}>Last 24h</option>
                <option value={604800}>Last 7d</option>
                <option value={2592000}>Last 30d</option>
              </select>
            </div>
          </div>

          {/* response trend chart */}
          <div className="card" style={{ padding: '20px 22px', marginBottom: 20 }}>
            <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', marginBottom: 18 }}>
              <div>
                <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t("dashboard.response_time")}</h3>
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
            <div style={{ height: trend.rows.length > 0 ? 240 : 88 }}>
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
                  {t("dashboard.empty.samples")}
                </div>
              )}
            </div>
          </div>

          {/* two column: incidents + upcoming maintenance */}
          <div className="hero-split" style={{ display: 'grid', gridTemplateColumns: '1.6fr 1fr', gap: 16, marginBottom: 20 }}>
            <div className="card" style={{ padding: '18px 20px' }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
                <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
                  <AlertCircle size={14} color="var(--down)"/> {t("dashboard.recent_incidents")}
                </h3>
                <button className="btn btn-ghost" style={{ padding: '4px 8px' }}
                        onClick={() => { window.location.hash = '#/status-page'; }}
                        title="Manage incidents on the status-page builder">
                  {t("dashboard.view_all")} <ArrowUpRight size={11}/>
                </button>
              </div>
              {recentIncidents.length === 0 ? (
                <div className="empty">{t("dashboard.empty.incidents")}</div>
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
                  <Wrench size={14} color="var(--maint)"/> {t("dashboard.maintenance")}
                </h3>
                <button className="btn btn-ghost" style={{ padding: '4px 8px' }}
                        onClick={() => { window.location.hash = '#/maintenance'; }}
                        title="Schedule a maintenance window">
                  <Plus size={11}/>
                </button>
              </div>
              {upcomingMaint.length === 0 ? (
                <div className="empty">{t("dashboard.empty.maintenance")}</div>
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
              <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t("dashboard.all_monitors")}</h3>
              {folderFilter && (
                <button onClick={() => setFolderFilter(null)} title={t("common.clear")}
                  style={{
                    display: 'inline-flex', alignItems: 'center', gap: 5,
                    padding: '3px 9px', borderRadius: 999, fontSize: 11, fontWeight: 500,
                    cursor: 'pointer', background: 'var(--accent-soft)', color: 'var(--accent-2)',
                    border: '1px solid var(--accent)',
                  }}>
                  <Folder size={11}/>
                  {(groupsState.data || []).find(g => g.id === folderFilter)?.name || 'Folder'}
                  <X size={11}/>
                </button>
              )}
              {tagsInUse.length > 0 && (
                <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
                  <Tag size={11} color="var(--text-3)"/>
                  {tagsInUse.map(tg => {
                    const on = tagFilter.has(tg.id);
                    return (
                      <span key={tg.id} style={{ display: 'inline-flex', alignItems: 'center', gap: 2 }}>
                        <button onClick={() => toggleTagFilter(tg.id)} style={{
                          display: 'inline-flex', alignItems: 'center', gap: 5,
                          padding: '3px 9px', borderRadius: writable ? '999px 0 0 999px' : 999,
                          fontSize: 11, fontWeight: 500, cursor: 'pointer',
                          background: on ? tg.color : 'var(--surface-2)',
                          color:      on ? '#fff'   : 'var(--text-2)',
                          border: `1px solid ${on ? tg.color : 'var(--border)'}`,
                        }}>
                          {tg.name}
                        </button>
                        {writable && (
                          <>
                            <button className="btn btn-ghost" disabled={bulkBusy}
                              title={t('dashboard.bulk_tag.pause_title', { tag: tg.name })}
                              onClick={() => runBulkByTag(tg.id, tg.name, 'pause')}
                              style={{ padding: '3px 5px', borderRadius: 0 }}>
                              <Pause size={11}/>
                            </button>
                            <button className="btn btn-ghost" disabled={bulkBusy}
                              title={t('dashboard.bulk_tag.resume_title', { tag: tg.name })}
                              onClick={() => runBulkByTag(tg.id, tg.name, 'resume')}
                              style={{ padding: '3px 5px', borderRadius: '0 999px 999px 0' }}>
                              <Activity size={11}/>
                            </button>
                          </>
                        )}
                      </span>
                    );
                  })}
                  {tagFilter.size > 0 && (
                    <button className="btn btn-ghost" onClick={() => setTagFilter(new Set())} style={{ padding: '2px 7px', fontSize: 11 }}>
                      {t("common.clear")}
                    </button>
                  )}
                </div>
              )}
            </div>

            {selected.size > 0 && (
              <div style={{
                display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap',
                padding: '10px 22px', background: 'var(--accent-soft)',
                borderBottom: '1px solid var(--border)', fontSize: 12.5,
              }}>
                <strong>{t("dashboard.bulk.selected", { n: selected.size })}</strong>
                <button className="btn" disabled={bulkBusy} onClick={() => runBulk({ action: 'pause' })}><Pause size={12}/> {t("dashboard.bulk.pause")}</button>
                <button className="btn" disabled={bulkBusy} onClick={() => runBulk({ action: 'resume' })}><Activity size={12}/> {t("dashboard.bulk.resume")}</button>
                <select className="select" style={{ width: 'auto', padding: '4px 8px', fontSize: 12 }} disabled={bulkBusy}
                  value="__placeholder__"
                  onChange={e => {
                    const v = e.target.value;
                    if (v === '__placeholder__') return;
                    runBulk({ action: 'set_group', group_id: v === '__ungroup__' ? null : v });
                  }}>
                  <option value="__placeholder__">{t("dashboard.bulk.move_to_group")}</option>
                  {(() => {
                    // Render the folder tree as flat <option>s, prefixing
                    // depth with "── " so nested folders are visible in
                    // the picker.
                    const all = groupsState.data || [];
                    const byParent = new Map();
                    for (const g of all) {
                      const k = g.parent_id || '__root__';
                      if (!byParent.has(k)) byParent.set(k, []);
                      byParent.get(k).push(g);
                    }
                    const out = [];
                    const walk = (key, depth) => {
                      for (const g of (byParent.get(key) || [])) {
                        out.push(
                          <option key={g.id} value={g.id}>
                            {'  '.repeat(depth)}{depth > 0 ? '↳ ' : ''}{g.name}
                          </option>
                        );
                        walk(g.id, depth + 1);
                      }
                    };
                    walk('__root__', 0);
                    return out;
                  })()}
                  <option value="__ungroup__">{t("dashboard.bulk.ungrouped")}</option>
                </select>
                <select className="select" style={{ width: 'auto', padding: '4px 8px', fontSize: 12 }} disabled={bulkBusy}
                  value="__placeholder__"
                  onChange={e => {
                    const v = e.target.value;
                    if (v === '__placeholder__') return;
                    if (v.startsWith('detach:')) {
                      runBulk({ action: 'detach_channel', notification_id: v.slice('detach:'.length) });
                    } else {
                      runBulk({ action: 'attach_channel', notification_id: v });
                    }
                  }}>
                  <option value="__placeholder__">{t("dashboard.bulk.channel")}</option>
                  <optgroup label={t("dashboard.bulk.attach")}>
                    {(channelsState.data || []).map(c => (
                      <option key={`a-${c.id}`} value={c.id}>{c.name}</option>
                    ))}
                  </optgroup>
                  <optgroup label={t("dashboard.bulk.detach")}>
                    {(channelsState.data || []).map(c => (
                      <option key={`d-${c.id}`} value={`detach:${c.id}`}>{c.name}</option>
                    ))}
                  </optgroup>
                </select>
                {writable && (
                  <button className="btn" disabled={bulkBusy}
                    onClick={() => setBulkEdit(prev => prev
                      ? null
                      : { interval: '', timeout: '', enabled: '', group: '', setTagsOn: false, tags: new Set() })}>
                    <Settings size={12}/> {t("dashboard.bulk.edit")}
                  </button>
                )}
                <button className="btn btn-danger" disabled={bulkBusy}
                  onClick={() => runBulk({ action: 'delete' }, t("dashboard.bulk.delete_confirm", { n: selected.size }))}>
                  <AlertCircle size={12}/> {t("dashboard.bulk.delete")}
                </button>
                <button className="btn btn-ghost" disabled={bulkBusy} onClick={() => { setSelected(new Set()); setBulkEdit(null); setBulkPreview(null); }} style={{ marginLeft: 'auto' }}>{t("dashboard.bulk.clear")}</button>

                {writable && bulkEdit && (
                  <div style={{
                    flexBasis: '100%', display: 'flex', flexWrap: 'wrap', alignItems: 'flex-end',
                    gap: 14, marginTop: 6, paddingTop: 10, borderTop: '1px solid var(--border)',
                  }}>
                    <strong style={{ flexBasis: '100%', marginBottom: 2 }}>
                      {t("dashboard.bulk.edit_title", { n: selected.size })}
                    </strong>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                      <span style={{ color: 'var(--text-3)' }}>{t("dashboard.bulk.interval_label")}</span>
                      <input className="input" type="number" min="10" max="86400"
                        style={{ width: 150, padding: '4px 8px', fontSize: 12 }}
                        placeholder={t("dashboard.bulk.interval_ph")} disabled={bulkBusy}
                        value={bulkEdit.interval}
                        onChange={e => setBulkEdit(s => ({ ...s, interval: e.target.value }))}/>
                    </label>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                      <span style={{ color: 'var(--text-3)' }}>{t("dashboard.bulk.timeout_label")}</span>
                      <input className="input" type="number" min="1" max="600"
                        style={{ width: 150, padding: '4px 8px', fontSize: 12 }}
                        placeholder={t("dashboard.bulk.timeout_ph")} disabled={bulkBusy}
                        value={bulkEdit.timeout}
                        onChange={e => setBulkEdit(s => ({ ...s, timeout: e.target.value }))}/>
                    </label>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                      <span style={{ color: 'var(--text-3)' }}>{t("dashboard.bulk.enabled_label")}</span>
                      <select className="select" style={{ width: 150, padding: '4px 8px', fontSize: 12 }} disabled={bulkBusy}
                        value={bulkEdit.enabled}
                        onChange={e => setBulkEdit(s => ({ ...s, enabled: e.target.value }))}>
                        <option value="">{t("dashboard.bulk.leave_alone")}</option>
                        <option value="enabled">{t("dashboard.bulk.set_enabled")}</option>
                        <option value="paused">{t("dashboard.bulk.set_paused")}</option>
                      </select>
                    </label>
                    <label style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                      <span style={{ color: 'var(--text-3)' }}>{t("dashboard.bulk.group_label")}</span>
                      <select className="select" style={{ width: 180, padding: '4px 8px', fontSize: 12 }} disabled={bulkBusy}
                        value={bulkEdit.group}
                        onChange={e => setBulkEdit(s => ({ ...s, group: e.target.value }))}>
                        <option value="">{t("dashboard.bulk.leave_alone")}</option>
                        <option value="__ungroup__">{t("dashboard.bulk.ungrouped")}</option>
                        {(groupsState.data || []).map(g => (
                          <option key={g.id} value={g.id}>{g.name}</option>
                        ))}
                      </select>
                    </label>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                      <label style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text-3)', cursor: 'pointer' }}>
                        <input type="checkbox" disabled={bulkBusy} checked={bulkEdit.setTagsOn}
                          onChange={e => setBulkEdit(s => ({ ...s, setTagsOn: e.target.checked }))}/>
                        {t("dashboard.bulk.set_tags_label")}
                      </label>
                      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5, maxWidth: 320, opacity: bulkEdit.setTagsOn ? 1 : .4 }}>
                        {(tagsState.data || []).length === 0
                          ? <span style={{ color: 'var(--text-3)' }}>{t("dashboard.bulk.no_tags")}</span>
                          : (tagsState.data || []).map(tag => {
                              const on = bulkEdit.tags.has(tag.id);
                              return (
                                <button key={`set-${tag.id}`} type="button" disabled={bulkBusy || !bulkEdit.setTagsOn}
                                  style={{
                                    display: 'inline-flex', alignItems: 'center', gap: 5,
                                    padding: '3px 9px', borderRadius: 999,
                                    fontSize: 11, fontWeight: 500, cursor: bulkEdit.setTagsOn ? 'pointer' : 'default',
                                    background: on ? (tag.color || 'var(--accent)') : 'var(--surface-2)',
                                    color:      on ? '#fff' : 'var(--text-2)',
                                    border: `1px solid ${on ? (tag.color || 'var(--accent)') : 'var(--border)'}`,
                                  }}
                                  onClick={() => setBulkEdit(s => {
                                    const tags = new Set(s.tags);
                                    if (tags.has(tag.id)) tags.delete(tag.id);
                                    else tags.add(tag.id);
                                    return { ...s, tags };
                                  })}>
                                  {on ? '✓ ' : '+ '}{tag.name}
                                </button>
                              );
                            })}
                      </div>
                    </div>
                    <div style={{ display: 'flex', gap: 8, marginLeft: 'auto' }}>
                      <button className="btn" disabled={bulkBusy} onClick={previewBulkEdit}>
                        {t("dashboard.bulk.preview.button")}
                      </button>
                      <button className="btn btn-accent" disabled={bulkBusy} onClick={runBulkEdit}>
                        {t("dashboard.bulk.apply")}
                      </button>
                      <button className="btn btn-ghost" disabled={bulkBusy} onClick={() => { setBulkEdit(null); setBulkPreview(null); }}>
                        {t("dashboard.bulk.cancel")}
                      </button>
                    </div>

                    {/* Dry-run preview: per-monitor field diffs the real edit
                        WOULD apply. Confirming here commits the same patch. */}
                    {bulkPreview && (
                      <div style={{
                        flexBasis: '100%', marginTop: 8, padding: 12, borderRadius: 8,
                        background: 'var(--surface)', border: '1px solid var(--border)',
                      }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                          <strong style={{ fontSize: 12.5 }}>{t("dashboard.bulk.preview.title")}</strong>
                          <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                            {t("dashboard.bulk.preview.summary", {
                              update: bulkPreview.would_update ?? (bulkPreview.preview || []).length,
                              skip: bulkPreview.would_skip ?? 0,
                            })}
                          </span>
                          <button className="btn btn-ghost" style={{ marginLeft: 'auto', padding: '2px 6px' }}
                            onClick={() => setBulkPreview(null)}><X size={12}/></button>
                        </div>
                        {(bulkPreview.preview || []).length === 0 ? (
                          <div style={{ fontSize: 12, color: 'var(--text-3)' }}>{t("dashboard.bulk.preview.no_changes")}</div>
                        ) : (
                          <div style={{ maxHeight: 200, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 6 }}>
                            {(bulkPreview.preview || []).map(row => (
                              <div key={row.id} style={{ fontSize: 12 }}>
                                <span style={{ fontWeight: 600 }}>{row.name}</span>
                                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 2 }}>
                                  {Object.entries(row.changes || {}).map(([field, ch]) => (
                                    <span key={field} className="mono" style={{ fontSize: 11, color: 'var(--text-2)' }}>
                                      {field}: <span style={{ color: 'var(--text-3)' }}>{fmtBulkVal(ch.from)}</span>
                                      {' → '}<span style={{ color: 'var(--accent-2)' }}>{fmtBulkVal(ch.to)}</span>
                                    </span>
                                  ))}
                                </div>
                              </div>
                            ))}
                          </div>
                        )}
                        <div style={{ display: 'flex', gap: 8, marginTop: 10, justifyContent: 'flex-end' }}>
                          <button className="btn btn-ghost" disabled={bulkBusy} onClick={() => setBulkPreview(null)}>
                            {t("dashboard.bulk.preview.dismiss")}
                          </button>
                          <button className="btn btn-accent" disabled={bulkBusy} onClick={runBulkEdit}>
                            {t("dashboard.bulk.preview.confirm")}
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}

            {/* Undo bar — appears after a real bulk-edit. POSTs the inverse
                request the backend handed back. Disabled-annotated when the
                tag part couldn't be inverted as one set. */}
            {bulkUndo && (
              <div style={{
                display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap',
                padding: '10px 22px', background: 'var(--surface-2)',
                borderBottom: '1px solid var(--border)', fontSize: 12.5,
              }}>
                <strong>{t("dashboard.bulk.undo.done", { n: bulkUndo.n })}</strong>
                {bulkUndo.undo_partial && (
                  <span style={{ color: 'var(--warn)', display: 'inline-flex', alignItems: 'center', gap: 5 }}>
                    <AlertCircle size={13}/> {t("dashboard.bulk.undo.partial")}
                  </span>
                )}
                <button className="btn" disabled={bulkBusy} onClick={undoBulkEdit}>
                  <RotateCcw size={12}/> {t("dashboard.bulk.undo.button")}
                </button>
                <button className="btn btn-ghost" disabled={bulkBusy} onClick={() => setBulkUndo(null)} style={{ marginLeft: 'auto' }}>
                  {t("dashboard.bulk.undo.dismiss")}
                </button>
              </div>
            )}

            <div className="activity-row" style={{
              display: 'grid',
              gridTemplateColumns: '24px 1.4fr 70px 70px 1.5fr 60px',
              gap: 16, padding: '10px 22px',
              fontSize: 11, fontWeight: 600, color: 'var(--text-3)',
              textTransform: 'uppercase', letterSpacing: '.04em',
              background: 'var(--surface-2)', borderBottom: '1px solid var(--border)'
            }}>
              <span/>
              <span>{t("dashboard.table.monitor")}</span>
              <span>{t("dashboard.table.type")}</span>
              <span style={{ textAlign: 'right' }}>{t("dashboard.table.p50")}</span>
              <span>{t("dashboard.table.last_checks")}</span>
              <span style={{ textAlign: 'right' }}>{t("dashboard.table.uptime")}</span>
            </div>

            {monitors.length === 0 ? (
              <div className="empty" style={{ padding: '40px 18px' }}>
                {monitorsState.loading ? t("dashboard.loading") : (
                  <>
                    {t("dashboard.empty.title")}{' '}
                    <a href="#/new-monitor" style={{ color: 'var(--accent)', textDecoration: 'none', fontWeight: 500 }}>
                      {t("dashboard.empty.create_first")} →
                    </a>
                  </>
                )}
              </div>
            ) : filtered.length === 0 ? (
              <div className="empty" style={{ padding: '40px 18px' }}>
                {t("dashboard.no_match")}
              </div>
            ) : filtered.map(m => {
              const hist = heartbeatsToCells(historyById.get(m.id), m.current_status === 'paused');
              const summary = summaryById.get(m.id);
              const p50 = summary?.avg_latency_ms;
              const uptime = summary?.uptime_pct;
              const cls = statusToClass(m.current_status);
              return (
                <div key={m.id} onClick={() => openMonitor(m.id)} className="activity-row" style={{
                  display: 'grid',
                  gridTemplateColumns: '24px 1.4fr 70px 70px 1.5fr 60px',
                  gap: 16, padding: '14px 22px',
                  borderBottom: '1px solid var(--border)', alignItems: 'center',
                  cursor: 'pointer',
                  background: selected.has(m.id) ? 'var(--accent-soft)' : 'transparent',
                }}>
                  <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
                    {writable && (
                      <input type="checkbox" checked={selected.has(m.id)}
                        onClick={e => e.stopPropagation()}
                        onChange={() => toggleSelect(m.id)}
                        title="Select for bulk action"/>
                    )}
                    <span className={`dot ${cls}`}/>
                  </span>
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
                  <span style={{ display: 'inline-flex', alignItems: 'center', justifyContent: 'flex-end', gap: 6 }}>
                    <span className="mono tabular" style={{ fontSize: 12, color: uptime === 100 ? 'var(--up)' : uptime != null && uptime < 99 ? 'var(--down)' : 'var(--text)', textAlign: 'right', fontWeight: 500 }}>
                      {uptime != null ? `${uptime.toFixed(2)}%` : '—'}
                    </span>
                    {writable && (
                      <button className="clone-action" title={t("monitor.action.clone_into_title")}
                        onClick={e => { e.stopPropagation(); setCloneState({ monitor: m, group: '', busy: false }); }}>
                        <Copy size={13}/>
                      </button>
                    )}
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

      {/* ─── clone-into-folder dialog ──────────────────────────────── */}
      {cloneState && (
        <div onClick={() => !cloneState.busy && setCloneState(null)} style={{
          position: 'fixed', inset: 0, zIndex: 200, background: 'rgba(0,0,0,.35)',
          display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 20,
        }}>
          <div className="card" onClick={e => e.stopPropagation()} style={{ width: 420, maxWidth: '100%', padding: 20 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
              <Copy size={16}/>
              <strong style={{ fontSize: 15 }}>{t("dashboard.clone.title")}</strong>
            </div>
            <div style={{ fontSize: 12.5, color: 'var(--text-3)', marginBottom: 14 }}>
              {t("dashboard.clone.subtitle", { name: cloneState.monitor.name })}
            </div>
            <label style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
              <span style={{ fontSize: 12, color: 'var(--text-2)', fontWeight: 500 }}>{t("dashboard.clone.target_label")}</span>
              <select className="select" value={cloneState.group} disabled={cloneState.busy}
                onChange={e => setCloneState(s => ({ ...s, group: e.target.value }))}>
                <option value="">{t("dashboard.clone.same_group")}</option>
                <option value="__ungroup__">{t("dashboard.bulk.ungrouped")}</option>
                {(() => {
                  const all = groupsState.data || [];
                  const byParent = new Map();
                  for (const g of all) {
                    const k = g.parent_id || '__root__';
                    if (!byParent.has(k)) byParent.set(k, []);
                    byParent.get(k).push(g);
                  }
                  const out = [];
                  const walk = (key, depth) => {
                    for (const g of (byParent.get(key) || [])) {
                      out.push(
                        <option key={g.id} value={g.id}>
                          {'  '.repeat(depth)}{depth > 0 ? '↳ ' : ''}{g.name}
                        </option>
                      );
                      walk(g.id, depth + 1);
                    }
                  };
                  walk('__root__', 0);
                  return out;
                })()}
              </select>
            </label>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 18 }}>
              <button className="btn btn-ghost" disabled={cloneState.busy} onClick={() => setCloneState(null)}>
                {t("dashboard.bulk.cancel")}
              </button>
              <button className="btn btn-accent" disabled={cloneState.busy} onClick={submitClone}>
                <Copy size={13}/> {t("dashboard.clone.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// Saved-views dropdown. Lists the user's saved filter combos (tags + folder +
// search) and lets them save the current one, apply/delete a saved one, and
// — purely client-side — export a view to a shareable link/token or import
// one pasted from elsewhere. Export/import need no backend: a view is just
// filter state, and importing-with-save reuses the existing prefs round-trip.
function ViewsMenu({ loggedIn, views = [], onSave, onApply, onDelete, onExport, onImport }) {
  const [open, setOpen] = useState(false);
  // The view currently being exported (saved view or the live filters), plus
  // its serialised token/url — null when the share panel is closed.
  const [share, setShare] = useState(null);
  // Import panel: null when closed, else the working { token } string.
  const [importing, setImporting] = useState(null);
  const [copied, setCopied] = useState(false);
  const ref = useRef(null);
  useEffect(() => {
    if (!open) return;
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) { setOpen(false); setShare(null); setImporting(null); } };
    const onKey = (e) => { if (e.key === 'Escape') { setOpen(false); setShare(null); setImporting(null); } };
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    return () => { document.removeEventListener('mousedown', onDoc); document.removeEventListener('keydown', onKey); };
  }, [open]);

  const doExport = (view) => { setShare({ view, ...onExport(view) }); setImporting(null); };
  const copyShare = async () => {
    if (!share) return;
    try { await navigator.clipboard.writeText(share.url); setCopied(true); setTimeout(() => setCopied(false), 1500); }
    catch { /* clipboard blocked — the field is selectable as a fallback */ }
  };
  const doImport = async (save) => {
    const token = (importing?.token || '').trim();
    if (!token) return;
    // Accept either a bare token or a full URL with ?view=… in it.
    let tok = token;
    const qi = token.indexOf('?view=');
    if (qi !== -1) { try { tok = new URLSearchParams(token.slice(qi + 1)).get('view') || token; } catch { /* keep raw */ } }
    const ok = await onImport(tok, { save });
    if (ok) { setImporting(null); setOpen(false); }
  };

  return (
    <div ref={ref} style={{ position: 'relative' }}>
      <button className="btn btn-ghost" onClick={() => setOpen(o => !o)} title={t("dashboard.views.menu")}>
        <Bookmark size={14}/> {t("dashboard.views.menu")} <ChevronDown size={13}/>
      </button>
      {open && (
        <div className="card" style={{
          position: 'absolute', right: 0, top: 'calc(100% + 6px)', zIndex: 50,
          minWidth: 280, maxWidth: 360, padding: 8, boxShadow: 'var(--shadow, 0 8px 24px rgba(0,0,0,.12))',
        }}>
          {!loggedIn && (
            <div style={{ fontSize: 11.5, color: 'var(--text-3)', padding: '4px 8px 8px' }}>
              {t("dashboard.views.signed_out")}
            </div>
          )}
          <div style={{ maxHeight: 240, overflowY: 'auto' }}>
            {views.length === 0 ? (
              <div style={{ fontSize: 12, color: 'var(--text-3)', padding: '6px 8px' }}>{t("dashboard.views.none")}</div>
            ) : views.map(v => (
              <div key={v.id} style={{ display: 'flex', alignItems: 'center', gap: 4, padding: '2px 0' }}>
                <button className="btn btn-ghost" style={{ flex: 1, justifyContent: 'flex-start', fontSize: 12.5 }}
                  onClick={() => { onApply(v); setOpen(false); }} title={t("dashboard.views.apply_title")}>
                  {v.name}
                </button>
                <button className="btn btn-ghost" style={{ padding: '4px 6px' }} title={t("dashboard.views.export")}
                  onClick={() => doExport(v)}><Share2 size={13}/></button>
                <button className="btn btn-ghost" style={{ padding: '4px 6px' }} title={t("dashboard.views.delete")}
                  onClick={() => onDelete(v)}><Trash2 size={13}/></button>
              </div>
            ))}
          </div>
          <div style={{ borderTop: '1px solid var(--border)', marginTop: 6, paddingTop: 6, display: 'flex', flexDirection: 'column', gap: 4 }}>
            <button className="btn btn-ghost" style={{ justifyContent: 'flex-start', fontSize: 12.5 }}
              onClick={() => { onSave(); setOpen(false); }}><Bookmark size={13}/> {t("dashboard.views.save_current")}</button>
            <button className="btn btn-ghost" style={{ justifyContent: 'flex-start', fontSize: 12.5 }}
              onClick={() => doExport(null)}><Share2 size={13}/> {t("dashboard.views.export_current")}</button>
            <button className="btn btn-ghost" style={{ justifyContent: 'flex-start', fontSize: 12.5 }}
              onClick={() => { setImporting({ token: '' }); setShare(null); }}><Download size={13}/> {t("dashboard.views.import")}</button>
          </div>

          {share && (
            <div style={{ borderTop: '1px solid var(--border)', marginTop: 6, paddingTop: 8 }}>
              <div style={{ fontSize: 11, color: 'var(--text-3)', marginBottom: 4 }}>{t("dashboard.views.export_hint")}</div>
              <input className="input" readOnly value={share.url} onFocus={e => e.target.select()}
                style={{ width: '100%', fontSize: 11.5, padding: '5px 8px', fontFamily: 'var(--mono, monospace)' }}/>
              <div style={{ display: 'flex', gap: 6, marginTop: 6 }}>
                <button className="btn btn-accent" style={{ fontSize: 12 }} onClick={copyShare}>
                  {copied ? <><Check size={12}/> {t("dashboard.views.copied")}</> : <><Copy size={12}/> {t("dashboard.views.copy")}</>}
                </button>
                <button className="btn btn-ghost" style={{ fontSize: 12 }} onClick={() => setShare(null)}>{t("common.clear")}</button>
              </div>
            </div>
          )}

          {importing && (
            <div style={{ borderTop: '1px solid var(--border)', marginTop: 6, paddingTop: 8 }}>
              <div style={{ fontSize: 11, color: 'var(--text-3)', marginBottom: 4 }}>{t("dashboard.views.import_hint")}</div>
              <input className="input" autoFocus value={importing.token}
                placeholder={t("dashboard.views.import_ph")}
                onChange={e => setImporting({ token: e.target.value })}
                style={{ width: '100%', fontSize: 11.5, padding: '5px 8px', fontFamily: 'var(--mono, monospace)' }}/>
              <div style={{ display: 'flex', gap: 6, marginTop: 6 }}>
                <button className="btn btn-accent" style={{ fontSize: 12 }} disabled={!importing.token.trim()}
                  onClick={() => doImport(false)}>{t("dashboard.views.import_apply")}</button>
                {loggedIn && (
                  <button className="btn" style={{ fontSize: 12 }} disabled={!importing.token.trim()}
                    onClick={() => doImport(true)}>{t("dashboard.views.import_save")}</button>
                )}
                <button className="btn btn-ghost" style={{ fontSize: 12 }} onClick={() => setImporting(null)}>{t("common.clear")}</button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// Header nav menu — single discoverable entry point to every admin page.
// Previously these were only reachable via the dev-only floating switcher.
function NavMenu({ writable } = {}) {
  const [open, setOpen] = useState(false);
  const ref = useRef(null);
  useEffect(() => {
    if (!open) return;
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    const onKey = (e) => { if (e.key === 'Escape') setOpen(false); };
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    return () => { document.removeEventListener('mousedown', onDoc); document.removeEventListener('keydown', onKey); };
  }, [open]);

  const items = [
    { href: '#/folders',          label: 'Folders',        Icon: Folder },
    { href: '#/tags',             label: 'Tags',           Icon: TagIcon },
    ...(writable ? [{ href: '#/templates', label: t('templates.nav'), Icon: FileStack }] : []),
    ...(writable ? [{ href: '#/alert-rules', label: t('alertrules.nav'), Icon: Bell }] : []),
    { href: '#/maintenance',      label: 'Maintenance',    Icon: CalIcon },
    { href: '#/proxies',          label: 'Proxies',        Icon: Network },
    { href: '#/api-keys',         label: 'API keys',       Icon: Key },
    { href: '#/audit',            label: 'Audit log',      Icon: ScrollText },
    { href: '#/users',            label: 'Users',          Icon: UsersIcon },
    { href: '#/security',         label: 'Security / 2FA', Icon: Lock },
    { sep: true },
    { href: '#/settings/smtp',      label: 'SMTP settings',    Icon: Mail },
    { href: '#/settings/retention', label: 'Retention',        Icon: DbIcon },
    { href: '#/settings/ingest',    label: 'Ingest token',     Icon: Key },
  ];

  return (
    <div ref={ref} style={{ position: 'relative' }}>
      <button className="btn btn-ghost" title={t("dashboard.nav.title")} onClick={() => setOpen(o => !o)}>
        <Menu size={15}/>
      </button>
      {open && (
        <div style={{
          position: 'absolute', top: 'calc(100% + 6px)', right: 0, zIndex: 60, minWidth: 190,
          background: 'var(--surface)', border: '1px solid var(--border-2)', borderRadius: 10,
          boxShadow: '0 12px 32px rgba(0,0,0,.22)', padding: 6,
        }}>
          {items.map((it, i) => it.sep
            ? <div key={i} style={{ height: 1, background: 'var(--border)', margin: '6px 4px' }}/>
            : (
              <a key={it.href} href={it.href} onClick={() => setOpen(false)} style={{
                display: 'flex', alignItems: 'center', gap: 9, padding: '8px 10px', borderRadius: 7,
                color: 'var(--text)', textDecoration: 'none', fontSize: 13,
              }}
                onMouseEnter={e => e.currentTarget.style.background = 'var(--surface-2)'}
                onMouseLeave={e => e.currentTarget.style.background = 'transparent'}>
                <it.Icon size={14} color="var(--text-3)"/> {it.label}
              </a>
            ))}
        </div>
      )}
    </div>
  );
}

// `ThemeToggle` lives in src/components/ThemeToggle.jsx so every view can
// reach it. The dashboard header imports the inline variant; App.jsx
// mounts a FloatingThemeToggle for views without their own chrome.
