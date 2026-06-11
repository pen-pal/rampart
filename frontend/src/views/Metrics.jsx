import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, AlertCircle, Loader2, X, Pencil, Check, Copy,
  Activity, BellRing, Flame, LineChart,
} from 'lucide-react';
import { api, useApi, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { canWrite } from '../lib/roles.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
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
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .input, .select, textarea.input {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input.mono { font-family: 'JetBrains Mono', monospace; font-size: 12.5px; }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); display: block; margin-bottom: 6px; }
  .field-hint { font-size: 11.5px; color: var(--text-3); margin-top: 6px; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
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
  .row {
    display: grid; grid-template-columns: 1fr auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .series-row { cursor: pointer; }
  .series-row:hover { background: var(--surface-2); }
  .series-row.selected { background: var(--accent-soft); }
  .chip {
    display: inline-flex; align-items: center;
    font-family: 'JetBrains Mono', monospace;
    font-size: 10.5px; padding: 2px 7px; border-radius: 999px; font-weight: 500;
    background: var(--surface-2); color: var(--text-2);
  }
  .pill {
    display: inline-flex; align-items: center; gap: 4px;
    font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500;
  }
  .pill-fire { background: var(--down-soft); color: #b91c1c; }
  .pill-on   { background: var(--up-soft);   color: #047857; }
  .pill-off  { background: var(--surface-2); color: var(--text-3); }
  .seg {
    display: inline-flex; align-items: center; gap: 2px;
    background: var(--surface-2); padding: 2px; border-radius: 6px;
  }
  .seg button {
    border: none; padding: 4px 10px; border-radius: 4px; cursor: pointer;
    font-size: 12px; font-weight: 600; font-family: inherit;
    background: transparent; color: var(--text-2);
  }
  .seg button.active {
    background: var(--surface); color: var(--text);
    box-shadow: 0 1px 2px rgba(0,0,0,.06); cursor: default;
  }
  pre.snippet {
    font-family: 'JetBrains Mono', monospace; font-size: 11.5px; text-align: left;
    background: var(--surface-2); border: 1px solid var(--border); border-radius: 8px;
    padding: 12px 14px; white-space: pre-wrap; word-break: break-all;
    color: var(--text-2); margin: 0;
  }
`;

const tsToDate = (ts) => (Array.isArray(ts) ? offsetDateTimeArrayToDate(ts) : new Date(ts));

// Range picker → query window + bucket size (seconds).
const RANGES = [
  { id: '1h',  secs: 3_600,    step: 60 },
  { id: '24h', secs: 86_400,   step: 300 },
  { id: '7d',  secs: 604_800,  step: 3_600 },
];

const OP_SYMBOL = { gt: '>', lt: '<', gte: '≥', lte: '≤' };

// Compact value formatting for axis labels / tooltips.
const fmtNum = (v) => {
  if (v == null || !Number.isFinite(v)) return '—';
  if (Math.abs(v) >= 1000) return v.toLocaleString(undefined, { maximumFractionDigits: 0 });
  return String(Number(v.toFixed(2)));
};

// "queue_depth{queue=emails}" — shared by the series list and rule rows.
const seriesLabel = (name, labels) => {
  const entries = Object.entries(labels || {});
  if (entries.length === 0) return name;
  return `${name}{${entries.map(([k, v]) => `${k}=${v}`).join(',')}}`;
};

export default function Metrics({ user }) {
  const writable = canWrite(user);
  const [tab, setTab] = useState('explore');  // 'explore' | 'rules'
  const [reloadKey, setReloadKey] = useState(0);

  const series   = useApi(() => api.metrics.series(),    [reloadKey]);
  const rules    = useApi(() => api.metrics.listRules(), [reloadKey]);
  const channels = useApi(() => api.notifications.list(), []);

  const reload = () => setReloadKey(k => k + 1);
  const seriesRows = series.data || [];
  const ruleRows   = rules.data || [];

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 980, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('metrics.back')}
        </a>

        <div style={{ marginBottom: 22 }}>
          <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('metrics.title')}</h1>
          <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('metrics.subtitle')}</p>
        </div>

        <div className="tabs">
          <button className={tab === 'explore' ? 'active' : ''} onClick={() => setTab('explore')}>
            <LineChart size={12}/> {t('metrics.tab.explore')} {seriesRows.length > 0 && <span className="mono" style={{ opacity: .7 }}>{seriesRows.length}</span>}
          </button>
          <button className={tab === 'rules' ? 'active' : ''} onClick={() => setTab('rules')}>
            <BellRing size={12}/> {t('metrics.tab.rules')} {ruleRows.length > 0 && <span className="mono" style={{ opacity: .7 }}>{ruleRows.length}</span>}
          </button>
        </div>

        {tab === 'explore' && <ExplorePanel state={series} onRetry={reload}/>}
        {tab === 'rules' && (
          <RulesPanel
            state={rules}
            seriesRows={seriesRows}
            channels={channels.data || []}
            writable={writable}
            reload={reload}
          />
        )}
      </div>
    </div>
  );
}

// ─── Explore tab ─────────────────────────────────────────────────────────────

function ExplorePanel({ state, onRetry }) {
  // Selected series keyed by name+labels so re-fetched lists keep the pick.
  const [selKey, setSelKey] = useState(null);
  const [range,  setRange]  = useState('24h');

  const rows = state.data || [];
  const keyOf = (s) => seriesLabel(s.name, s.labels);
  const selected = rows.find(s => keyOf(s) === selKey) || null;

  if (state.error) {
    return (
      <div className="banner-err">
        <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>
        {t('metrics.error')}{' '}
        <button className="btn btn-ghost" style={{ marginLeft: 8 }} onClick={onRetry}>{t('metrics.retry')}</button>
      </div>
    );
  }

  if (state.loading) {
    return (
      <div className="card" style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
        <Loader2 size={16}/> {t('metrics.loading')}
      </div>
    );
  }

  if (rows.length === 0) return <IngestHelpCard/>;

  return (
    <>
      {selected && (
        <div className="card" style={{ padding: '18px 20px', marginBottom: 18 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 12 }}>
            <Activity size={14} color="var(--text-3)"/>
            <span className="mono" style={{ fontSize: 13, fontWeight: 500 }}>{seriesLabel(selected.name, selected.labels)}</span>
            <div className="seg" style={{ marginLeft: 'auto' }}>
              {RANGES.map(r => (
                <button key={r.id} className={range === r.id ? 'active' : ''}
                  onClick={() => setRange(r.id)}>{r.id}</button>
              ))}
            </div>
            <button className="btn btn-ghost" onClick={() => setSelKey(null)} title={t('common.close')}><X size={14}/></button>
          </div>
          <SeriesChart name={selected.name} labels={selected.labels} range={range}/>
        </div>
      )}
      {!selected && (
        <p style={{ fontSize: 12.5, color: 'var(--text-3)', margin: '0 0 12px' }}>{t('metrics.series.pick')}</p>
      )}

      <div className="card" style={{ overflow: 'hidden' }}>
        {rows.map(s => {
          const key = keyOf(s);
          return (
            <div key={key}
              className={`row series-row${key === selKey ? ' selected' : ''}`}
              onClick={() => setSelKey(key === selKey ? null : key)}>
              <div style={{ minWidth: 0 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', marginBottom: 3 }}>
                  <span className="mono" style={{ fontSize: 13, fontWeight: 500 }}>{s.name}</span>
                  {Object.entries(s.labels || {}).map(([k, v]) => (
                    <span key={k} className="chip">{k}={v}</span>
                  ))}
                </div>
                <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                  {formatRelative(tsToDate(s.last_ts))}
                  {' · '}
                  {t('metrics.series.samples', { n: s.samples })}
                </div>
              </div>
              <LineChart size={14} color="var(--text-3)"/>
            </div>
          );
        })}
      </div>
    </>
  );
}

// Empty-state card with a copyable curl one-liner showing how to push metrics.
function IngestHelpCard() {
  const [copied, setCopied] = useState(false);
  const snippet = `echo "my_metric 42" | curl --data-binary @- -H "Authorization: Bearer <api key>" ${window.location.origin}/v1/metrics/ingest`;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(snippet);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch { /* clipboard unavailable — user can select manually */ }
  };
  return (
    <div className="card" style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
      <Activity size={28} style={{ marginBottom: 10, opacity: .5 }}/>
      <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('metrics.empty.title')}</div>
      <div style={{ fontSize: 12.5, marginBottom: 14 }}>{t('metrics.empty.body')}</div>
      <pre className="snippet" style={{ maxWidth: 680, margin: '0 auto' }}>{snippet}</pre>
      <button className="btn" style={{ marginTop: 12 }} onClick={copy}>
        {copied ? <><Check size={13}/> {t('metrics.empty.copied')}</> : <><Copy size={13}/> {t('metrics.empty.copy')}</>}
      </button>
    </div>
  );
}

// Hand-rolled SVG line chart: avg as a polyline over a translucent min–max
// band. Matches the bare-SVG approach of the uptime-history card — no chart
// dep. Each bucket carries a <title> tooltip with avg/min/max.
function SeriesChart({ name, labels, range }) {
  const r = RANGES.find(x => x.id === range) || RANGES[1];
  const state = useApi(() => {
    const to   = new Date();
    const from = new Date(to.getTime() - r.secs * 1000);
    return api.metrics.query(name, {
      labels,
      from: from.toISOString(),
      to: to.toISOString(),
      stepSeconds: r.step,
    });
  }, [name, JSON.stringify(labels || {}), range]);

  if (state.loading) {
    return <div style={{ height: 180, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-3)', fontSize: 13 }}>
      <Loader2 size={14} style={{ marginRight: 6 }}/> {t('common.loading')}
    </div>;
  }
  if (state.error) {
    return <div className="banner-err" style={{ marginBottom: 0 }}>{t('metrics.error')}</div>;
  }

  const buckets = (state.data || [])
    .map(p => ({ ...p, date: tsToDate(p.ts) }))
    .filter(p => !isNaN(p.date.getTime()) && p.avg != null)
    .sort((a, b) => a.date - b.date);

  if (buckets.length === 0) {
    return <div style={{ height: 180, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-3)', fontSize: 13 }}>
      {t('metrics.chart.empty')}
    </div>;
  }

  // Geometry — fixed viewBox, scaled to width via CSS. X is time-scaled
  // between the first and last bucket; Y spans [min, max] across the window
  // with a little headroom so a flat line doesn't hug the frame.
  const W = 640, H = 180, padL = 48, padR = 10, padT = 10, padB = 22;
  const plotW = W - padL - padR, plotH = H - padT - padB;
  const t0 = buckets[0].date.getTime();
  const t1 = buckets[buckets.length - 1].date.getTime();
  const span = Math.max(1, t1 - t0);
  let lo = Math.min(...buckets.map(p => (p.min ?? p.avg)));
  let hi = Math.max(...buckets.map(p => (p.max ?? p.avg)));
  if (hi === lo) { hi += 1; lo -= 1; }
  const pad = (hi - lo) * 0.08;
  lo -= pad; hi += pad;

  const X = (d) => padL + ((d.getTime() - t0) / span) * plotW;
  const Y = (v) => padT + (1 - (v - lo) / (hi - lo)) * plotH;

  const avgPts = buckets.map(p => `${X(p.date).toFixed(1)},${Y(p.avg).toFixed(1)}`).join(' ');
  // Band polygon: max values forward, min values backward.
  const bandPts = [
    ...buckets.map(p => `${X(p.date).toFixed(1)},${Y(p.max ?? p.avg).toFixed(1)}`),
    ...[...buckets].reverse().map(p => `${X(p.date).toFixed(1)},${Y(p.min ?? p.avg).toFixed(1)}`),
  ].join(' ');

  const fmtTime = (d) => range === '7d'
    ? d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
    : d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  const mid = (lo + hi) / 2;
  const hoverW = buckets.length > 1 ? plotW / (buckets.length - 1) : plotW;

  return (
    <>
      <svg viewBox={`0 0 ${W} ${H}`} style={{ width: '100%', height: 180, display: 'block' }}
        role="img" aria-label={name}>
        {/* y-axis gridlines + labels */}
        {[hi - pad, mid, lo + pad].map((v, i) => (
          <g key={i}>
            <line x1={padL} x2={W - padR} y1={Y(v)} y2={Y(v)} stroke="var(--border)" strokeWidth="0.5"/>
            <text x={padL - 6} y={Y(v) + 3} textAnchor="end"
              style={{ fontSize: 9, fill: 'var(--text-3)', fontFamily: "'JetBrains Mono', monospace" }}>
              {fmtNum(v)}
            </text>
          </g>
        ))}
        <polygon points={bandPts} fill="var(--accent)" opacity="0.12"/>
        <polyline points={avgPts} fill="none" stroke="var(--accent)" strokeWidth="1.8"
          strokeLinejoin="round" strokeLinecap="round"/>
        {/* invisible hover targets — one per bucket, native <title> tooltip */}
        {buckets.map((p, i) => (
          <rect key={i}
            x={X(p.date) - hoverW / 2} y={padT} width={hoverW} height={plotH}
            fill="transparent">
            <title>{`${fmtTime(p.date)} — ${t('metrics.chart.avg')} ${fmtNum(p.avg)} (${fmtNum(p.min)}…${fmtNum(p.max)}, n=${p.samples})`}</title>
          </rect>
        ))}
      </svg>
      <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 4, fontSize: 11, color: 'var(--text-3)' }}>
        <span>{fmtTime(buckets[0].date)}</span>
        <span>{t('metrics.chart.band')}</span>
        <span>{fmtTime(buckets[buckets.length - 1].date)}</span>
      </div>
    </>
  );
}

// ─── Rules tab ───────────────────────────────────────────────────────────────

function RulesPanel({ state, seriesRows, channels, writable, reload }) {
  const [creating, setCreating] = useState(false);
  const [editing,  setEditing]  = useState(null);  // rule id being edited
  const [busy,     setBusy]     = useState(null);
  const [err,      setErr]      = useState(null);

  const ruleRows = state.data || [];
  // De-duplicated metric names feeding the form's <datalist>.
  const metricNames = [...new Set(seriesRows.map(s => s.name))];

  const remove = async (id) => {
    if (!confirm(t('metrics.rules.delete_confirm'))) return;
    setBusy(id); setErr(null);
    try { await api.metrics.removeRule(id); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const toggle = async (rule) => {
    setBusy(rule.id); setErr(null);
    try { await api.metrics.updateRule(rule.id, { enabled: !rule.enabled }); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  return (
    <>
      {writable && (
        <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 14 }}>
          <button className="btn btn-accent" onClick={() => { setEditing(null); setCreating(true); }}>
            <Plus size={14}/> {t('metrics.rules.new')}
          </button>
        </div>
      )}

      {(err || state.error) && (
        <div className="banner-err">
          <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>
          {err || t('metrics.error')}
          {state.error && (
            <button className="btn btn-ghost" style={{ marginLeft: 8 }} onClick={reload}>{t('metrics.retry')}</button>
          )}
        </div>
      )}

      {creating && (
        <RuleForm metricNames={metricNames} channels={channels}
          onCancel={() => setCreating(false)} onSaved={reload}/>
      )}

      <div className="card" style={{ overflow: 'hidden' }}>
        {state.loading ? (
          <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
            <Loader2 size={16}/> {t('metrics.loading')}
          </div>
        ) : ruleRows.length === 0 ? (
          <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
            <BellRing size={28} style={{ marginBottom: 10, opacity: .5 }}/>
            <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('metrics.rules.empty.title')}</div>
            <div style={{ fontSize: 12.5 }}>{t('metrics.rules.empty.cta')}</div>
          </div>
        ) : ruleRows.map(r => (
          editing === r.id ? (
            <div key={r.id} style={{ padding: 18, borderTop: '1px solid var(--border)' }}>
              <RuleForm rule={r} metricNames={metricNames} channels={channels}
                onCancel={() => setEditing(null)} onSaved={reload}/>
            </div>
          ) : (
            <div className="row" key={r.id}>
              <div style={{ minWidth: 0 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', marginBottom: 3 }}>
                  <span style={{ fontSize: 14, fontWeight: 600 }}>{r.name}</span>
                  {r.firing_at != null && (
                    <span className="pill pill-fire"><Flame size={11}/> {t('metrics.rules.firing')}</span>
                  )}
                  <span className={`pill ${r.enabled ? 'pill-on' : 'pill-off'}`}>
                    {r.enabled ? t('metrics.rules.enabled') : t('metrics.rules.disabled')}
                  </span>
                </div>
                <div className="mono" style={{ fontSize: 11.5, color: 'var(--text-2)', wordBreak: 'break-all' }}>
                  {seriesLabel(r.metric, r.labels)} {OP_SYMBOL[r.op] || r.op} {fmtNum(r.threshold)}{' '}
                  {t('metrics.rules.for', { n: r.for_seconds ?? 0 })}
                </div>
                <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 3 }}>
                  {t('metrics.rules.channels_count', { n: (r.channel_ids || []).length })}
                </div>
              </div>
              {writable && (
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <button className="btn btn-ghost" onClick={() => toggle(r)} disabled={busy === r.id}>
                    {r.enabled ? t('metrics.rules.disable') : t('metrics.rules.enable')}
                  </button>
                  <button className="btn btn-ghost" onClick={() => { setCreating(false); setEditing(r.id); }} disabled={busy === r.id}>
                    <Pencil size={13}/> {t('common.edit')}
                  </button>
                  <button className="btn btn-ghost btn-danger" onClick={() => remove(r.id)} disabled={busy === r.id}>
                    <Trash2 size={13}/>
                  </button>
                </div>
              )}
            </div>
          )
        ))}
      </div>
    </>
  );
}

// Shared create / edit rule form. With `rule` it PATCHes in place; without,
// it POSTs a new rule.
function RuleForm({ rule, metricNames, channels, onCancel, onSaved }) {
  const editing = !!rule;
  const [name,       setName]       = useState(rule?.name || '');
  const [metric,     setMetric]     = useState(rule?.metric || '');
  const [labelsText, setLabelsText] = useState(
    rule && Object.keys(rule.labels || {}).length ? JSON.stringify(rule.labels) : '',
  );
  const [op,         setOp]         = useState(rule?.op || 'gt');
  const [threshold,  setThreshold]  = useState(rule?.threshold ?? '');
  const [forSecs,    setForSecs]    = useState(rule?.for_seconds ?? 0);
  const [channelIds, setChannelIds] = useState(rule?.channel_ids || []);
  const [enabled,    setEnabled]    = useState(rule?.enabled ?? true);
  const [busy,       setBusy]       = useState(false);
  const [err,        setErr]        = useState(null);

  const toggleChannel = (id) =>
    setChannelIds(ids => (ids.includes(id) ? ids.filter(x => x !== id) : [...ids, id]));

  const submit = async () => {
    setErr(null);
    if (!name.trim())   { setErr(t('metrics.rules.err_name'));   return; }
    if (!metric.trim()) { setErr(t('metrics.rules.err_metric')); return; }
    // Labels: empty → {}; otherwise must parse to a plain JSON object.
    let labels = {};
    if (labelsText.trim()) {
      try {
        const parsed = JSON.parse(labelsText);
        if (parsed == null || Array.isArray(parsed) || typeof parsed !== 'object') throw new Error();
        for (const [k, v] of Object.entries(parsed)) labels[k] = String(v);
      } catch {
        setErr(t('metrics.rules.err_labels'));
        return;
      }
    }
    const thr = Number(threshold);
    if (threshold === '' || !Number.isFinite(thr)) { setErr(t('metrics.rules.err_threshold')); return; }
    const body = {
      name: name.trim(),
      metric: metric.trim(),
      labels,
      op,
      threshold: thr,
      for_seconds: Math.max(0, Number(forSecs) || 0),
      enabled,
      channel_ids: channelIds,
    };
    setBusy(true);
    try {
      if (editing) await api.metrics.updateRule(rule.id, body);
      else         await api.metrics.createRule(body);
      onSaved();
    } catch (e) {
      setErr(e.message || t('metrics.rules.err_save'));
      setBusy(false);
    }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: editing ? 0 : 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>
          {editing ? t('metrics.rules.edit_title') : t('metrics.rules.new')}
        </h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}><X size={14}/></button>
      </div>
      {err && <div className="banner-err">{err}</div>}

      <div style={{ marginBottom: 14 }}>
        <label className="field-label">{t('metrics.rules.field.name')}</label>
        <input className="input" value={name} onChange={e => setName(e.target.value)}
          placeholder={t('metrics.rules.field.name_placeholder')}/>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12, marginBottom: 14 }}>
        <div>
          <label className="field-label">{t('metrics.rules.field.metric')}</label>
          <input className="input mono" list="metrics-rule-names" value={metric}
            onChange={e => setMetric(e.target.value)} placeholder="queue_depth"/>
          <datalist id="metrics-rule-names">
            {metricNames.map(n => <option key={n} value={n}/>)}
          </datalist>
        </div>
        <div>
          <label className="field-label">{t('metrics.rules.field.labels')}</label>
          <input className="input mono" value={labelsText}
            onChange={e => setLabelsText(e.target.value)} placeholder='{"queue":"emails"}'/>
          <div className="field-hint">{t('metrics.rules.field.labels_hint')}</div>
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '160px 1fr 1fr', gap: 12, marginBottom: 14 }}>
        <div>
          <label className="field-label">{t('metrics.rules.field.op')}</label>
          <select className="select" value={op} onChange={e => setOp(e.target.value)}>
            <option value="gt">{t('metrics.rules.op.gt')}</option>
            <option value="lt">{t('metrics.rules.op.lt')}</option>
            <option value="gte">{t('metrics.rules.op.gte')}</option>
            <option value="lte">{t('metrics.rules.op.lte')}</option>
          </select>
        </div>
        <div>
          <label className="field-label">{t('metrics.rules.field.threshold')}</label>
          <input className="input mono" type="number" step="any" value={threshold}
            onChange={e => setThreshold(e.target.value)} placeholder="40"/>
        </div>
        <div>
          <label className="field-label">{t('metrics.rules.field.for_seconds')}</label>
          <input className="input mono" type="number" min="0" step="1" value={forSecs}
            onChange={e => setForSecs(e.target.value)} placeholder="300"/>
          <div className="field-hint">{t('metrics.rules.field.for_hint')}</div>
        </div>
      </div>

      <div style={{ marginBottom: 14 }}>
        <label className="field-label">{t('metrics.rules.field.channels')}</label>
        {channels.length === 0 ? (
          <div className="field-hint" style={{ marginTop: 0 }}>{t('metrics.rules.field.channels_empty')}</div>
        ) : (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px 16px' }}>
            {channels.map(c => (
              <label key={c.id} style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 13, cursor: 'pointer' }}>
                <input type="checkbox" checked={channelIds.includes(c.id)} onChange={() => toggleChannel(c.id)}/>
                {c.name}
              </label>
            ))}
          </div>
        )}
      </div>

      <label style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 13, cursor: 'pointer', marginBottom: 16 }}>
        <input type="checkbox" checked={enabled} onChange={e => setEnabled(e.target.checked)}/>
        {t('metrics.rules.field.enabled')}
      </label>

      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>
          {busy
            ? <><Loader2 size={13}/> {t('common.saving')}</>
            : <><Check size={13}/> {editing ? t('metrics.rules.save') : t('metrics.rules.create')}</>}
        </button>
      </div>
    </div>
  );
}
