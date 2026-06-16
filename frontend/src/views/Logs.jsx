import React, { useState, useEffect } from 'react';
import { ChevronLeft, Loader2, AlertCircle, ScrollText, Search, RefreshCw, Download, Bookmark, X } from 'lucide-react';
import { api, useApi, formatClock, formatRelative } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { promptDialog } from '../lib/notify.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1; --down:#ef4444;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif; min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', monospace; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn { display: inline-flex; align-items: center; gap: 6px; padding: 7px 12px; border-radius: 8px; cursor: pointer; font-size: 13px; font-weight: 500; line-height: 1; background: var(--surface); border: 1px solid var(--border); color: var(--text-2); font-family: inherit; }
  .btn:hover { background: var(--surface-2); color: var(--text); }
  .btn-ghost { background: transparent; border-color: transparent; }
  .input, .select { padding: 8px 10px; border-radius: 8px; background: var(--surface); border: 1px solid var(--border); font-size: 13px; color: var(--text); outline: none; font-family: inherit; }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .banner-err { background: #fee2e2; color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .log-row { display: grid; grid-template-columns: 84px 64px 130px 1fr; gap: 10px; align-items: baseline; padding: 6px 12px; border-top: 1px solid var(--border); font-size: 12.5px; cursor: pointer; }
  .log-row:first-child { border-top: none; }
  .log-row:hover { background: var(--surface-2); }
  .lvl { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: .03em; }
  .lvl-trace,.lvl-debug { color: var(--text-3); }
  .lvl-info { color: var(--accent-2); }
  .lvl-warn { color: #b45309; }
  .lvl-error,.lvl-fatal { color: var(--down); }
  .log-body { white-space: pre-wrap; word-break: break-word; }
  .log-attrs { grid-column: 1 / -1; margin-top: 6px; padding: 8px 10px; background: var(--surface-2); border-radius: 6px; font-size: 11.5px; }
`;

const LEVELS = ['', 'trace', 'debug', 'info', 'warn', 'error', 'fatal'];

// ELK-style log-volume histogram: one bar per time bucket, error-level volume
// stacked in red at the base. Heights scale to the busiest bucket in view.
function LogHistogram({ buckets }) {
  if (!buckets || buckets.length < 2) return null;
  const max = Math.max(1, ...buckets.map(b => b.total));
  return (
    <div style={{ display: 'flex', alignItems: 'flex-end', gap: 1, height: 52, marginBottom: 14 }}>
      {buckets.map((b, i) => (
        <div key={i} title={`${b.total} logs · ${b.errors} error+`}
          style={{ flex: 1, height: `${Math.max((b.total / max) * 100, b.total ? 3 : 0)}%`, minHeight: b.total ? 2 : 0,
                   background: 'var(--text-3)', borderRadius: 1, display: 'flex', flexDirection: 'column-reverse', opacity: .85 }}>
          {b.errors > 0 && <div style={{ height: `${(b.errors / b.total) * 100}%`, background: 'var(--down)', borderRadius: 1 }}/>}
        </div>
      ))}
    </div>
  );
}

export default function Logs({ traceId }) {
  const [service, setService] = useState('');
  const [level, setLevel] = useState('');
  const [winHours, setWinHours] = useState(24); // histogram + level-count window
  const [q, setQ] = useState('');
  const [applied, setApplied] = useState({ service: '', level: '', q: '' });
  const [reloadKey, setReloadKey] = useState(0);
  const [expanded, setExpanded] = useState(null);
  const [live, setLive] = useState(false);

  // Live tail: poll the filtered query every 3s while enabled. Polling (vs an
  // SSE stream) reads from the DB, so it works across replicas without a
  // per-process log broadcast.
  useEffect(() => {
    if (!live) return undefined;
    const h = setInterval(() => setReloadKey(k => k + 1), 3000);
    return () => clearInterval(h);
  }, [live]);

  const svcState = useApi(() => api.logs.services(), []);
  const logsState = useApi(
    () => api.logs.query({ service: applied.service, level: applied.level, q: applied.q, trace_id: traceId || undefined, limit: 300 }),
    [applied, reloadKey, traceId],
  );
  const levelsState = useApi(() => api.logs.levels(applied.service, winHours), [applied.service, winHours, reloadKey]);
  const histState = useApi(
    () => api.logs.histogram({ service: applied.service, level: applied.level, q: applied.q, hours: winHours }),
    [applied, winHours, reloadKey],
  );
  const services = svcState.data || [];
  const logs = logsState.data || [];
  const levelCounts = levelsState.data || [];
  const hist = histState.data || [];

  const apply = () => setApplied({ service, level, q });

  // Saved searches — per-user, stored in the shared prefs blob under
  // `log_searches` (the dashboard owns `saved_views`; patchPrefs merges so the
  // two never clobber each other). Only offered on the main view, not the
  // single-trace drill-down.
  const [saved, setSaved] = useState([]);
  useEffect(() => {
    if (traceId) return;
    let off = false;
    api.me.getPrefs()
      .then(p => { if (!off) setSaved(Array.isArray(p?.log_searches) ? p.log_searches : []); })
      .catch(() => {});
    return () => { off = true; };
  }, [traceId]);

  const persistSaved = (next) => { setSaved(next); api.me.patchPrefs({ log_searches: next }).catch(() => {}); };
  const saveCurrent = async () => {
    const name = (await promptDialog({ message: t('logs.save_prompt') }));
    if (!name || !name.trim()) return;
    persistSaved([...saved, { id: Date.now().toString(36), name: name.trim(), ...applied }]);
  };
  const applySaved = (s) => {
    setService(s.service || ''); setLevel(s.level || ''); setQ(s.q || '');
    setApplied({ service: s.service || '', level: s.level || '', q: s.q || '' });
  };
  const deleteSaved = (id) => persistSaved(saved.filter(s => s.id !== id));

  // CSV export honours the currently-applied filters (server clamps the row
  // count). Cookie-auth + same-origin, so a plain download link carries the
  // session — no fetch/blob dance needed.
  const exportUrl = (() => {
    const p = new URLSearchParams();
    if (applied.service) p.set('service', applied.service);
    if (applied.level) p.set('level', applied.level);
    if (applied.q) p.set('q', applied.q);
    if (traceId) p.set('trace_id', traceId);
    const qs = p.toString();
    return `/v1/logs/export.csv${qs ? '?' + qs : ''}`;
  })();

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 1040, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}><ChevronLeft size={14}/> {t('logs.back')}</a>
        <div style={{ marginBottom: 16 }}>
          <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('logs.title')}</h1>
          <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('logs.subtitle')}</p>
        </div>

        {traceId && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14, padding: '8px 12px', background: 'var(--accent-soft)', borderRadius: 8, fontSize: 12.5 }}>
            <span>{t('logs.for_trace')} <span className="mono">{traceId}</span></span>
            <a href={`#/traces/${traceId}`} style={{ color: 'var(--accent-2)', fontWeight: 500 }}>{t('logs.open_trace')}</a>
            <a href="#/logs" style={{ color: 'var(--text-3)', marginLeft: 'auto' }}>{t('logs.clear_trace')}</a>
          </div>
        )}

        <div style={{ display: 'flex', gap: 8, marginBottom: 14, flexWrap: 'wrap', alignItems: 'center' }}>
          <select className="select" value={service} onChange={e => setService(e.target.value)}>
            <option value="">{t('logs.all_services')}</option>
            {services.map(s => <option key={s} value={s}>{s}</option>)}
          </select>
          <select className="select" value={level} onChange={e => setLevel(e.target.value)}>
            {LEVELS.map(l => <option key={l} value={l}>{l ? t(`logs.level.${l}`) : t('logs.any_level')}</option>)}
          </select>
          <select className="select" value={winHours} onChange={e => setWinHours(Number(e.target.value))} title={t('logs.window_hint')}>
            <option value={1}>1h</option>
            <option value={24}>24h</option>
            <option value={168}>7d</option>
          </select>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, flex: '1 1 220px' }}>
            <Search size={14} style={{ color: 'var(--text-3)' }}/>
            <input className="input" style={{ flex: 1 }} placeholder={t('logs.search_ph')}
              value={q} onChange={e => setQ(e.target.value)} onKeyDown={e => e.key === 'Enter' && apply()}/>
          </div>
          <button className="btn" onClick={apply}>{t('logs.apply')}</button>
          <button className="btn btn-ghost" onClick={() => setReloadKey(k => k + 1)} title={t('logs.refresh')}><RefreshCw size={14}/></button>
          <a className="btn btn-ghost" href={exportUrl} download title={t('logs.export_hint')}><Download size={14}/> {t('logs.export')}</a>
          {!traceId && <button className="btn btn-ghost" onClick={saveCurrent} title={t('logs.save_hint')}><Bookmark size={14}/> {t('logs.save')}</button>}
          <button className={`btn ${live ? 'btn-accent' : 'btn-ghost'}`} onClick={() => setLive(v => !v)} title={t('logs.live_hint')}>
            <span style={{ display: 'inline-block', width: 7, height: 7, borderRadius: '50%', background: live ? '#fff' : 'var(--text-3)', marginRight: 6, verticalAlign: 'middle' }}/>
            {t('logs.live')}
          </button>
        </div>

        {!traceId && saved.length > 0 && (
          <div style={{ display: 'flex', gap: 6, marginBottom: 12, flexWrap: 'wrap', alignItems: 'center' }}>
            <span style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em' }}>{t('logs.saved')}</span>
            {saved.map(s => (
              <span key={s.id} style={{ display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 12, padding: '3px 9px', borderRadius: 999, background: 'var(--surface-2, #f5f5f4)', border: '1px solid var(--border, #e7e5e4)' }}>
                <span style={{ cursor: 'pointer' }} onClick={() => applySaved(s)} title={[s.service, s.level, s.q].filter(Boolean).join(' · ') || t('logs.all_services')}>{s.name}</span>
                <X size={11} style={{ cursor: 'pointer', opacity: 0.6 }} onClick={() => deleteSaved(s.id)}/>
              </span>
            ))}
          </div>
        )}

        {levelCounts.length > 0 && (
          <div style={{ display: 'flex', gap: 8, marginBottom: 12, flexWrap: 'wrap', alignItems: 'center' }}>
            <span style={{ fontSize: 11, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.04em' }}>{t('logs.levels_in_window')}</span>
            {levelCounts.map(([lvl, count]) => (
              <button key={lvl} className={`lvl lvl-${lvl}`}
                style={{ cursor: 'pointer', border: 'none', font: 'inherit' }}
                title={t('logs.filter_to_level')}
                onClick={() => { setLevel(lvl); setApplied(a => ({ ...a, level: lvl })); }}>
                {lvl} {count}
              </button>
            ))}
          </div>
        )}

        {!traceId && <LogHistogram buckets={hist} />}

        {logsState.error && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{t('logs.load_error')}</div>}

        <div className="card" style={{ overflow: 'hidden' }}>
          {logsState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('logs.loading')}</div>
          ) : logs.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <ScrollText size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('logs.empty.title')}</div>
              <div style={{ fontSize: 12.5 }}>{t('logs.empty.cta')}</div>
            </div>
          ) : logs.map(l => (
            <div className="log-row" key={l.id} onClick={() => setExpanded(expanded === l.id ? null : l.id)}>
              <span className="mono" style={{ color: 'var(--text-3)' }} title={formatRelative(l.ts)}>{formatClock(l.ts)}</span>
              <span className={`lvl lvl-${l.level}`}>{l.level}</span>
              <span style={{ color: 'var(--text-2)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{l.service_name}</span>
              <span className="log-body mono">{l.body}</span>
              {expanded === l.id && (
                <div className="log-attrs mono">
                  {l.trace_id && <div>trace_id: <a href={`#/traces/${l.trace_id}`} style={{ color: 'var(--accent-2)' }} onClick={e => e.stopPropagation()}>{l.trace_id}</a>{l.span_id ? ` · span_id: ${l.span_id}` : ''}</div>}
                  {l.severity_text && <div>severity: {l.severity_text} ({l.severity})</div>}
                  {l.attributes && Object.keys(l.attributes).length > 0 &&
                    <div style={{ marginTop: 4 }}>{JSON.stringify(l.attributes, null, 0)}</div>}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
