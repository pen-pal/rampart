import React, { useState } from 'react';
import {
  ChevronLeft, Loader2, AlertCircle, Activity, Network, Gauge,
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
  .wf-row { display: grid; grid-template-columns: 260px 1fr; gap: 10px; align-items: center; padding: 3px 0; font-size: 12px; }
  .wf-track { position: relative; height: 16px; background: var(--surface-2); border-radius: 4px; }
  .wf-bar { position: absolute; top: 0; height: 16px; border-radius: 4px; background: var(--accent); min-width: 2px; }
  .wf-bar.err { background: var(--down); }
`;

const SPAN_KIND = { 0: '', 1: 'internal', 2: 'server', 3: 'client', 4: 'producer', 5: 'consumer' };

function fmtMs(ms) {
  if (ms == null) return '—';
  if (ms < 1) return `${(ms * 1000).toFixed(0)}µs`;
  if (ms < 1000) return `${ms.toFixed(1)}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
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
      </div>
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

  if (state.loading) return <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('traces.loading')}</div>;
  if (state.error || spans.length === 0) {
    return <><button className="btn btn-ghost" onClick={onBack}><ChevronLeft size={14}/> {t('traces.back_list')}</button><div className="banner-err" style={{ marginTop: 16 }}>{t('traces.load_error')}</div></>;
  }

  const traceStart = Math.min(...spans.map(s => s.start_ns));
  const traceEnd = Math.max(...spans.map(s => s.end_ns));
  const total = Math.max(1, traceEnd - traceStart);
  const root = spans.find(s => !s.parent_span_id) || spans[0];

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

      <div className="card" style={{ padding: 16 }}>
        {spans.map(s => {
          const left = ((s.start_ns - traceStart) / total) * 100;
          const width = Math.max(0.4, ((s.end_ns - s.start_ns) / total) * 100);
          const err = s.status_code === 2;
          return (
            <div className="wf-row" key={s.span_id}>
              <div style={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={`${s.service_name}: ${s.name}`}>
                <span style={{ color: 'var(--text-3)', fontSize: 11 }}>{s.service_name}</span>{' '}
                <span style={{ fontWeight: 500 }}>{s.name}</span>
                {SPAN_KIND[s.kind] && <span className="pill pill-muted" style={{ marginLeft: 6 }}>{SPAN_KIND[s.kind]}</span>}
              </div>
              <div className="wf-track">
                <div className={`wf-bar${err ? ' err' : ''}`} style={{ left: `${left}%`, width: `${width}%` }} title={fmtMs(s.duration_ms)}/>
                <span style={{ position: 'absolute', left: `${Math.min(left, 80)}%`, top: 0, fontSize: 10, color: 'var(--text-3)', paddingLeft: 4, lineHeight: '16px' }}>{fmtMs(s.duration_ms)}</span>
              </div>
            </div>
          );
        })}
      </div>

      <TraceLogs traceId={traceId} />
    </>
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
