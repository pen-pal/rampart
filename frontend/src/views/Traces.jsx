import React, { useState, useEffect } from 'react';
import {
  ChevronLeft, Loader2, AlertCircle, Activity, Network, Gauge, Download, Bookmark, X,
} from 'lucide-react';
import { api, useApi, formatRelative } from '../lib/api.js';
import { t } from '../lib/i18n.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --down:#ef4444; --down-soft:#fee2e2;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif; min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', monospace; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn {
    display: inline-flex; align-items: center; gap: 6px; padding: 7px 12px; border-radius: 8px;
    cursor: pointer; font-size: 13px; font-weight: 500; line-height: 1;
    background: var(--surface); border: 1px solid var(--border); color: var(--text-2); font-family: inherit;
  }
  .btn:hover { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); display: block; margin-bottom: 6px; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .row { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 14px; padding: 14px 18px; border-top: 1px solid var(--border); }
  .row:first-child { border-top: none; }
  .pill { display: inline-flex; align-items: center; font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500; background: var(--accent-soft); color: var(--accent-2); }
  .pill-down { background: var(--down-soft); color: #b91c1c; }
  .pill-muted { background: var(--surface-2); color: var(--text-3); }
  .wf-row { display: grid; grid-template-columns: 340px 1fr; gap: 12px; align-items: center; height: 26px; font-size: 12px; border-radius: 4px; }
  .wf-row:hover { background: var(--surface-2); }
  .wf-name { display: flex; align-items: center; min-width: 0; height: 100%; }
  .wf-guide { width: 14px; align-self: stretch; flex: none; border-left: 1px solid var(--border); }
  .wf-caret { width: 16px; height: 16px; display: inline-flex; align-items: center; justify-content: center; cursor: pointer; color: var(--text-3); border-radius: 3px; flex: none; font-size: 9px; }
  .wf-caret:hover { background: var(--border-2); color: var(--text); }
  .wf-caret.leaf { visibility: hidden; }
  .wf-swatch { width: 8px; height: 8px; border-radius: 2px; flex: none; margin-right: 6px; }
  .wf-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .wf-track { position: relative; height: 100%; }
  .wf-track-bg { position: absolute; left: 0; right: 0; top: 50%; transform: translateY(-50%); height: 18px; border-radius: 3px; background: var(--surface-2);
                 background-image: linear-gradient(to right, var(--border) 1px, transparent 1px); background-size: 25% 100%; }
  .wf-bar { position: absolute; top: 50%; transform: translateY(-50%); height: 14px; border-radius: 3px; min-width: 2px; box-shadow: inset 0 0 0 1px rgba(0,0,0,.08); }
  .wf-bar.err { background: var(--down) !important; }
  .wf-dur { position: absolute; top: 50%; transform: translateY(-50%); font-size: 10px; color: var(--text-2); white-space: nowrap; }
  .wf-ruler { display: grid; grid-template-columns: 340px 1fr; gap: 12px; margin-bottom: 6px; position: sticky; top: 0; z-index: 2; background: var(--surface); padding-bottom: 5px; border-bottom: 1px solid var(--border); }
  .wf-ruler-track { position: relative; height: 14px; }
  .wf-tick { position: absolute; top: 0; font-size: 10px; color: var(--text-3); transform: translateX(-50%); white-space: nowrap; }
  .wf-tick:first-child { transform: none; }
  .wf-tick:last-child { transform: translateX(-100%); }
  .wf-detail { margin: 0 0 6px; padding: 8px 12px; background: var(--bg); border: 1px solid var(--border); border-radius: 8px; font-size: 12px; }
`;

const SPAN_KIND = { 0: '', 1: 'internal', 2: 'server', 3: 'client', 4: 'producer', 5: 'consumer' };

function fmtMs(ms) {
  if (ms == null) return '—';
  if (ms < 1) return `${(ms * 1000).toFixed(0)}µs`;
  if (ms < 1000) return `${ms.toFixed(1)}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

// Flatten the spans into parent→child DFS order with a depth, so the waterfall
// reads as a real call-tree breakdown. Spans whose parent isn't in the trace
// (and any unreachable ones) are treated as roots. Subtrees whose root span_id
// is in `collapsed` are not recursed into, so the caller can fold them away.
function flattenSpans(spans, collapsed) {
  const ids = new Set(spans.map(s => s.span_id));
  const byParent = new Map();
  for (const s of spans) {
    const key = s.parent_span_id && ids.has(s.parent_span_id) ? s.parent_span_id : '__root';
    if (!byParent.has(key)) byParent.set(key, []);
    byParent.get(key).push(s);
  }
  for (const arr of byParent.values()) arr.sort((a, b) => a.start_ns - b.start_ns);
  const out = [];
  const visit = (key, depth) => {
    for (const s of byParent.get(key) || []) {
      const kids = byParent.get(s.span_id) || [];
      const folded = collapsed.has(s.span_id);
      out.push({ s, depth, childCount: kids.length, collapsed: folded });
      if (kids.length && !folded) visit(s.span_id, depth + 1);
    }
  };
  visit('__root', 0);
  // Orphans (cycle / missing parent) — show them flat rather than dropping.
  if (out.length === 0 && spans.length) {
    for (const s of spans) out.push({ s, depth: 0, childCount: 0, collapsed: false });
  }
  return out;
}

// Stable per-service color so the waterfall groups by service visually.
const SVC_COLORS = ['#6366f1', '#0ea5e9', '#10b981', '#f59e0b', '#ec4899', '#8b5cf6', '#14b8a6', '#ef4444'];
function svcColor(name) {
  let h = 0;
  for (let i = 0; i < (name || '').length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
  return SVC_COLORS[Math.abs(h) % SVC_COLORS.length];
}

export default function Traces({ openTraceId }) {
  const [traceId, setTraceId] = useState(openTraceId || null);
  const [tab, setTab] = useState('traces'); // traces | map

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 1000, margin: '0 auto', padding: '32px 32px 64px' }}>
        {traceId ? (
          <TraceDetail traceId={traceId} onBack={() => setTraceId(null)} />
        ) : (
          <>
            <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}><ChevronLeft size={14}/> {t('traces.back')}</a>
            <div style={{ marginBottom: 18 }}>
              <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('traces.title')}</h1>
              <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('traces.subtitle')}</p>
            </div>
            <div style={{ display: 'flex', gap: 8, marginBottom: 14 }}>
              <button className={`btn ${tab === 'traces' ? 'btn-accent' : ''}`} onClick={() => setTab('traces')}><Activity size={13}/> {t('traces.tab_traces')}</button>
              <button className={`btn ${tab === 'ops' ? 'btn-accent' : ''}`} onClick={() => setTab('ops')}><Gauge size={13}/> {t('traces.tab_ops')}</button>
              <button className={`btn ${tab === 'map' ? 'btn-accent' : ''}`} onClick={() => setTab('map')}><Network size={13}/> {t('traces.tab_map')}</button>
            </div>
            {tab === 'traces' ? <TraceList onOpen={setTraceId} /> : tab === 'ops' ? <OperationsTable /> : <ServiceMap />}
          </>
        )}
      </div>
    </div>
  );
}

function TraceList({ onOpen }) {
  const [service, setService] = useState('');
  const [q, setQ] = useState('');
  const [minDur, setMinDur] = useState('');
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [applied, setApplied] = useState({ service: '', q: '', min_duration_ms: '', errors_only: false });
  const state = useApi(
    () => api.traces.list({ limit: 100, ...applied }),
    [applied],
  );
  const traces = state.data || [];
  const apply = () => setApplied({ service, q, min_duration_ms: minDur, errors_only: errorsOnly });

  // Saved searches — per-user, in the shared prefs blob under `trace_searches`
  // (patchPrefs merges, so the dashboard's saved_views are untouched).
  const [saved, setSaved] = useState([]);
  useEffect(() => {
    let off = false;
    api.me.getPrefs()
      .then(p => { if (!off) setSaved(Array.isArray(p?.trace_searches) ? p.trace_searches : []); })
      .catch(() => {});
    return () => { off = true; };
  }, []);
  const persistSaved = (next) => { setSaved(next); api.me.patchPrefs({ trace_searches: next }).catch(() => {}); };
  const saveCurrent = () => {
    const name = window.prompt(t('traces.save_prompt'));
    if (!name || !name.trim()) return;
    persistSaved([...saved, { id: Date.now().toString(36), name: name.trim(), ...applied }]);
  };
  const applySaved = (s) => {
    setService(s.service || ''); setQ(s.q || ''); setMinDur(s.min_duration_ms || ''); setErrorsOnly(!!s.errors_only);
    setApplied({ service: s.service || '', q: s.q || '', min_duration_ms: s.min_duration_ms || '', errors_only: !!s.errors_only });
  };
  const deleteSaved = (id) => persistSaved(saved.filter(s => s.id !== id));

  // CSV export of the currently-applied trace filters (cookie-auth, same-origin).
  const exportUrl = (() => {
    const p = new URLSearchParams();
    if (applied.service) p.set('service', applied.service);
    if (applied.q) p.set('q', applied.q);
    if (applied.min_duration_ms) p.set('min_duration_ms', applied.min_duration_ms);
    if (applied.errors_only) p.set('errors_only', 'true');
    const qs = p.toString();
    return `/v1/traces/export.csv${qs ? '?' + qs : ''}`;
  })();
  return (
    <>
      <div style={{ display: 'flex', gap: 8, marginBottom: 12, flexWrap: 'wrap', alignItems: 'center' }}>
        <input className="input" style={{ flex: '1 1 180px' }} placeholder={t('traces.filter.search')}
          value={q} onChange={e => setQ(e.target.value)} onKeyDown={e => e.key === 'Enter' && apply()}/>
        <input className="input" style={{ width: 150 }} placeholder={t('traces.filter.service')}
          value={service} onChange={e => setService(e.target.value)} onKeyDown={e => e.key === 'Enter' && apply()}/>
        <input className="input" style={{ width: 130 }} type="number" placeholder={t('traces.filter.min_ms')}
          value={minDur} onChange={e => setMinDur(e.target.value)} onKeyDown={e => e.key === 'Enter' && apply()}/>
        <button className={`btn ${errorsOnly ? 'btn-accent' : ''}`}
          onClick={() => { const v = !errorsOnly; setErrorsOnly(v); setApplied(a => ({ ...a, errors_only: v })); }}>
          {t('traces.filter.errors_only')}
        </button>
        <button className="btn" onClick={apply}>{t('traces.filter.apply')}</button>
        <a className="btn btn-ghost" href={exportUrl} download title={t('traces.export_hint')}><Download size={14}/> {t('traces.export')}</a>
        <button className="btn btn-ghost" onClick={saveCurrent} title={t('traces.save_hint')}><Bookmark size={14}/> {t('traces.save')}</button>
      </div>
      {saved.length > 0 && (
        <div style={{ display: 'flex', gap: 6, marginBottom: 12, flexWrap: 'wrap', alignItems: 'center' }}>
          <span style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em' }}>{t('traces.saved')}</span>
          {saved.map(s => (
            <span key={s.id} className="pill" style={{ gap: 5 }}>
              <span style={{ cursor: 'pointer' }} onClick={() => applySaved(s)} title={[s.service, s.q, s.min_duration_ms && `≥${s.min_duration_ms}ms`, s.errors_only && 'errors'].filter(Boolean).join(' · ')}>{s.name}</span>
              <X size={11} style={{ cursor: 'pointer', opacity: 0.6 }} onClick={() => deleteSaved(s.id)}/>
            </span>
          ))}
        </div>
      )}
      {state.error && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{t('traces.load_error')}</div>}
      <div className="card" style={{ overflow: 'hidden' }}>
        {state.loading ? (
          <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('traces.loading')}</div>
        ) : traces.length === 0 ? (
          <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
            <Activity size={28} style={{ marginBottom: 10, opacity: .5 }}/>
            <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('traces.empty.title')}</div>
            <div style={{ fontSize: 12.5 }}>{t('traces.empty.cta')}</div>
          </div>
        ) : traces.map(tr => (
          <div className="row" key={tr.trace_id} style={{ cursor: 'pointer' }} onClick={() => onOpen(tr.trace_id)}>
            <div style={{ minWidth: 0 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
                <span className="pill pill-muted">{tr.root_service}</span>
                <span style={{ fontSize: 13.5, fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{tr.root_name || '(root)'}</span>
              </div>
              <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                {t('traces.span_count', { n: tr.span_count })} · {(tr.services || []).length} {t('traces.services')} · {formatRelative(tr.started_at)}
              </div>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              {tr.error_count > 0 && <span className="pill pill-down">{t('traces.errors', { n: tr.error_count })}</span>}
              <span className="pill">{fmtMs(tr.duration_ms)}</span>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

function OperationsTable() {
  const state = useApi(() => api.traces.operations(null, 24), []);
  const ops = state.data || [];
  return (
    <>
      {state.error && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{t('traces.load_error')}</div>}
      <div className="card" style={{ overflow: 'hidden' }}>
        {state.loading ? (
          <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('traces.loading')}</div>
        ) : ops.length === 0 ? (
          <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>{t('traces.ops.empty')}</div>
        ) : (
          <>
            <div className="row" style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em', fontWeight: 600 }}>
              <div style={{ flex: 1 }}>{t('traces.ops.operation')}</div>
              <div style={{ display: 'flex', gap: 18, fontVariantNumeric: 'tabular-nums' }}>
                <span style={{ width: 56, textAlign: 'right' }}>{t('traces.ops.calls')}</span>
                <span style={{ width: 56, textAlign: 'right' }}>{t('traces.ops.err')}</span>
                <span style={{ width: 64, textAlign: 'right' }}>p50</span>
                <span style={{ width: 64, textAlign: 'right' }}>p95</span>
                <span style={{ width: 64, textAlign: 'right' }}>p99</span>
              </div>
            </div>
            {ops.map((o, i) => (
              <div className="row" key={i}>
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span className="pill pill-muted">{o.service}</span>
                    <span style={{ fontSize: 13, fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{o.operation}</span>
                  </div>
                </div>
                <div style={{ display: 'flex', gap: 18, fontSize: 12.5, fontVariantNumeric: 'tabular-nums', color: 'var(--text-2)' }}>
                  <span style={{ width: 56, textAlign: 'right' }}>{o.calls}</span>
                  <span style={{ width: 56, textAlign: 'right', color: o.error_rate > 0 ? 'var(--down)' : 'var(--text-3)' }}>{o.error_rate.toFixed(1)}%</span>
                  <span style={{ width: 64, textAlign: 'right' }}>{fmtMs(o.p50_ms)}</span>
                  <span style={{ width: 64, textAlign: 'right' }}>{fmtMs(o.p95_ms)}</span>
                  <span style={{ width: 64, textAlign: 'right', fontWeight: 600 }}>{fmtMs(o.p99_ms)}</span>
                </div>
              </div>
            ))}
          </>
        )}
      </div>
    </>
  );
}

function ServiceMap() {
  const state = useApi(() => api.traces.serviceMap(24), []);
  const edges = state.data || [];
  return (
    <>
      {state.error && <div className="banner-err">{t('traces.load_error')}</div>}
      <div className="card" style={{ overflow: 'hidden' }}>
        {state.loading ? (
          <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('traces.loading')}</div>
        ) : edges.length === 0 ? (
          <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>{t('traces.no_edges')}</div>
        ) : edges.map((e, i) => (
          <div className="row" key={i}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, fontSize: 13 }}>
              <span className="pill pill-muted">{e.from_service}</span>
              <span style={{ color: 'var(--text-3)' }}>→</span>
              <span className="pill pill-muted">{e.to_service}</span>
            </div>
            <span className="pill">{t('traces.calls', { n: e.calls })}</span>
          </div>
        ))}
      </div>
    </>
  );
}

function TraceDetail({ traceId, onBack }) {
  const state = useApi(() => api.traces.detail(traceId), [traceId]);
  const spans = state.data || [];
  const [openSpan, setOpenSpan] = useState(null);
  const [collapsed, setCollapsed] = useState(() => new Set());

  if (state.loading) return <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('traces.loading')}</div>;
  if (state.error || spans.length === 0) {
    return <><button className="btn btn-ghost" onClick={onBack}><ChevronLeft size={14}/> {t('traces.back_list')}</button><div className="banner-err" style={{ marginTop: 16 }}>{t('traces.load_error')}</div></>;
  }

  const traceStart = Math.min(...spans.map(s => s.start_ns));
  const traceEnd = Math.max(...spans.map(s => s.end_ns));
  const total = Math.max(1, traceEnd - traceStart);
  const root = spans.find(s => !s.parent_span_id) || spans[0];

  // span_ids that have at least one child in the trace — the collapsible set.
  const parentIds = new Set(spans.map(s => s.parent_span_id).filter(Boolean));
  const toggleCollapse = (id) => setCollapsed(prev => {
    const next = new Set(prev);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });
  const rows = flattenSpans(spans, collapsed);

  return (
    <>
      <button className="btn btn-ghost" style={{ marginBottom: 18 }} onClick={onBack}><ChevronLeft size={14}/> {t('traces.back_list')}</button>
      <div style={{ marginBottom: 6 }}>
        <span className="pill pill-muted">{root.service_name}</span>
        <span style={{ fontSize: 18, fontWeight: 600, marginLeft: 8 }}>{root.name}</span>
      </div>
      <div style={{ fontSize: 12, color: 'var(--text-3)', marginBottom: 16 }}>
        {t('traces.span_count', { n: spans.length })} · {fmtMs(total / 1e6)} · <span className="mono">{traceId}</span>
        {' · '}
        <a href={`#/profiling?service=${encodeURIComponent(root.service_name)}`} style={{ color: 'var(--accent)' }}>{t('traces.open_profile')}</a>
      </div>

      <div className="card" style={{ padding: 16, overflow: 'auto', maxHeight: '72vh' }}>
        <div className="wf-ruler">
          <div style={{ display: 'flex', alignItems: 'flex-end', gap: 10 }}>
            <button className="btn btn-ghost" style={{ padding: '2px 6px', fontSize: 11 }}
              onClick={() => setCollapsed(new Set(parentIds))} disabled={parentIds.size === 0}>{t('traces.collapse_all')}</button>
            <button className="btn btn-ghost" style={{ padding: '2px 6px', fontSize: 11 }}
              onClick={() => setCollapsed(new Set())} disabled={collapsed.size === 0}>{t('traces.expand_all')}</button>
          </div>
          <div className="wf-ruler-track">
            {[0, 0.25, 0.5, 0.75, 1].map(f => (
              <span key={f} className="wf-tick" style={{ left: `${f * 100}%` }}>{fmtMs((total * f) / 1e6)}</span>
            ))}
          </div>
        </div>
        {rows.map(({ s, depth, childCount, collapsed: folded }) => {
          const left = ((s.start_ns - traceStart) / total) * 100;
          const width = Math.max(0.4, ((s.end_ns - s.start_ns) / total) * 100);
          const err = s.status_code === 2;
          const isOpen = openSpan === s.span_id;
          const attrs = s.attributes && typeof s.attributes === 'object' ? Object.entries(s.attributes) : [];
          const hasDetail = attrs.length > 0 || s.status_message;
          // Put the duration after the bar; flip it before when the bar hugs the right edge.
          const labelRight = left + width > 82;
          return (
            <div key={s.span_id}>
              <div className="wf-row">
                <div className="wf-name" title={`${s.service_name}: ${s.name}`}>
                  {Array.from({ length: depth }).map((_, i) => <span key={i} className="wf-guide"/>)}
                  <span className={`wf-caret${childCount ? '' : ' leaf'}`}
                    onClick={() => childCount && toggleCollapse(s.span_id)}>{childCount ? (folded ? '▶' : '▼') : '•'}</span>
                  <span className="wf-swatch" style={{ background: err ? 'var(--down)' : svcColor(s.service_name) }}/>
                  <span className="wf-label" style={{ cursor: hasDetail ? 'pointer' : 'default' }}
                    onClick={() => hasDetail && setOpenSpan(isOpen ? null : s.span_id)}>
                    <span style={{ color: 'var(--text-3)', fontSize: 11 }}>{s.service_name}</span>{' '}
                    <span style={{ fontWeight: 500, color: err ? 'var(--down)' : 'inherit' }}>{s.name}</span>
                    {folded && childCount > 0 && <span className="pill pill-muted" style={{ marginLeft: 6 }}>+{childCount}</span>}
                    {SPAN_KIND[s.kind] && <span className="pill pill-muted" style={{ marginLeft: 6 }}>{SPAN_KIND[s.kind]}</span>}
                    {hasDetail && <span style={{ color: 'var(--text-3)', fontSize: 9, marginLeft: 5 }}>{isOpen ? '▾' : '▸'}</span>}
                  </span>
                </div>
                <div className="wf-track">
                  <div className="wf-track-bg"/>
                  <div className={`wf-bar${err ? ' err' : ''}`} style={{ left: `${left}%`, width: `${width}%`, background: svcColor(s.service_name) }} title={fmtMs(s.duration_ms)}/>
                  <span className="wf-dur" style={labelRight ? { right: `${100 - left}%`, paddingRight: 4 } : { left: `${left + width}%`, paddingLeft: 4 }}>{fmtMs(s.duration_ms)}</span>
                </div>
              </div>
              {isOpen && (
                <div className="wf-detail" style={{ marginLeft: depth * 14 + 24 }}>
                  <div style={{ marginBottom: 8 }}>
                    <a href={`#/profiling?service=${encodeURIComponent(s.service_name)}&from_ms=${Math.floor(s.start_ns / 1e6)}&to_ms=${Math.ceil(s.end_ns / 1e6)}`}
                      style={{ color: 'var(--accent)', fontSize: 12 }}>{t('traces.profile_span')}</a>
                  </div>
                  {s.status_message && (
                    <div style={{ color: 'var(--down)', marginBottom: attrs.length ? 8 : 0 }}>⚠ {s.status_message}</div>
                  )}
                  {attrs.map(([k, v]) => (
                    <div key={k} style={{ display: 'flex', gap: 8, padding: '2px 0' }}>
                      <span style={{ color: 'var(--text-3)', flexShrink: 0, minWidth: 130, fontFamily: 'monospace' }}>{k}</span>
                      <span style={{ fontFamily: 'monospace', whiteSpace: 'pre-wrap', wordBreak: 'break-word', minWidth: 0 }}>
                        {typeof v === 'object' ? JSON.stringify(v) : String(v)}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>

      <TraceLogs traceId={traceId} />
      <TraceErrors traceId={traceId} />
    </>
  );
}

// Correlated errors — error issues touched by this trace (reverse of the
// error → trace context link). The trace → errors side of the join.
function TraceErrors({ traceId }) {
  const state = useApi(() => api.errorIssues.byTrace(traceId), [traceId]);
  const issues = state.data || [];
  if (state.loading || issues.length === 0) return null;
  return (
    <div style={{ marginTop: 24 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', marginBottom: 8 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-2)' }}>{t('traces.correlated_errors', { n: issues.length })}</span>
        <a href="#/errors" style={{ fontSize: 12, color: 'var(--accent)' }}>{t('traces.open_in_errors')}</a>
      </div>
      <div className="card" style={{ padding: '4px 12px' }}>
        {issues.map(i => (
          <a key={i.issue_id} href={`#/errors/${i.issue_id}`} className="row"
            style={{ display: 'flex', justifyContent: 'space-between', padding: '8px 0', textDecoration: 'none', color: 'inherit' }}>
            <span style={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              <span style={{ color: i.level === 'error' || i.level === 'fatal' ? 'var(--down)' : 'var(--text-3)', fontWeight: 600, fontSize: 11 }}>{i.level}</span>{' '}
              {i.title}
            </span>
            <span className="pill pill-muted">{i.count}</span>
          </a>
        ))}
      </div>
    </div>
  );
}

// Correlated logs — logs the apps emitted carrying this trace_id. The cross-tier
// link: jump from a span waterfall to the matching log lines.
function TraceLogs({ traceId }) {
  const state = useApi(() => api.logs.query({ trace_id: traceId, limit: 100 }), [traceId]);
  const logs = state.data || [];
  if (state.loading || logs.length === 0) return null;
  return (
    <div style={{ marginTop: 24 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', marginBottom: 8 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-2)' }}>{t('traces.correlated_logs', { n: logs.length })}</span>
        <a href={`#/logs/trace/${traceId}`} style={{ fontSize: 12, color: 'var(--accent)' }}>{t('traces.open_in_logs')}</a>
      </div>
      <div className="card mono" style={{ padding: '8px 12px', fontSize: 12, maxHeight: 240, overflowY: 'auto' }}>
        {logs.map(l => (
          <div key={l.id} style={{ padding: '3px 0', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
            <span style={{ color: l.level === 'error' || l.level === 'fatal' ? 'var(--down)' : 'var(--text-3)', fontWeight: 600 }}>{l.level}</span>{' '}
            <span style={{ color: 'var(--text-3)' }}>{l.service_name}</span>{' '}{l.body}
          </div>
        ))}
      </div>
    </div>
  );
}
