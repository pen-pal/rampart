import React, { useEffect, useState } from 'react';
import { ChevronLeft, Plus, X, LayoutGrid, RefreshCw } from 'lucide-react';
import { api, useApi } from '../lib/api.js';
import { t } from '../lib/i18n.js';

// Custom dashboards ("boards"): user-defined grids of widgets, persisted per
// user in the shared prefs blob under `boards` (no backend/schema — patchPrefs
// merges, so the dashboard's saved_views etc. are untouched). Each widget reads
// an existing API; nothing new server-side.

const WIDGET_TYPES = [
  { type: 'monitors', label: 'Monitor status' },
  { type: 'metric',   label: 'Metric chart' },
  { type: 'rum',      label: 'Web vitals (RUM)' },
  { type: 'note',     label: 'Note / text' },
];

const css = `
  .rampart { font-family: Inter, system-ui, sans-serif; color: var(--text); background: var(--bg); min-height: 100vh; }
  .btn { display: inline-flex; align-items: center; gap: 6px; padding: 7px 12px; border-radius: 8px; border: 1px solid var(--border); background: var(--surface); color: var(--text); font-size: 13px; cursor: pointer; text-decoration: none; }
  .btn-accent { background: var(--accent); border-color: var(--accent); color: #fff; }
  .btn-ghost { background: transparent; }
  .input, .select { padding: 7px 10px; border-radius: 8px; border: 1px solid var(--border); background: var(--bg); color: var(--text); font-size: 13px; }
  .tab { padding: 6px 12px; border-radius: 8px; cursor: pointer; font-size: 13px; border: 1px solid transparent; }
  .tab-active { background: var(--surface-2, #f5f5f4); border-color: var(--border); font-weight: 600; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 14px; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 16px; }
  .wtitle { font-size: 12px; font-weight: 600; text-transform: uppercase; letter-spacing: .04em; color: var(--text-3); margin: 0 0 10px; display: flex; justify-content: space-between; align-items: center; }
  .big { font-size: 30px; font-weight: 700; letter-spacing: -.02em; }
  .muted { color: var(--text-3); font-size: 12.5px; }
  .x { cursor: pointer; opacity: .5; }
  .x:hover { opacity: 1; }
`;

const uid = () => Date.now().toString(36) + Math.floor(Math.random() * 1e4).toString(36);

export default function Dashboards() {
  const [boards, setBoards] = useState([]);
  const [active, setActive] = useState(null);
  const [loaded, setLoaded] = useState(false);
  const [adding, setAdding] = useState(false);

  useEffect(() => {
    api.me.getPrefs()
      .then(p => {
        const b = Array.isArray(p?.boards) ? p.boards : [];
        setBoards(b);
        setActive(b[0]?.id ?? null);
      })
      .catch(() => {})
      .finally(() => setLoaded(true));
  }, []);

  const persist = (next) => {
    setBoards(next);
    api.me.patchPrefs({ boards: next }).catch(() => {});
  };

  const addBoard = () => {
    const name = window.prompt(t('boards.name_prompt'));
    if (!name || !name.trim()) return;
    const b = { id: uid(), name: name.trim(), widgets: [] };
    const next = [...boards, b];
    persist(next);
    setActive(b.id);
  };
  const deleteBoard = (id) => {
    if (!window.confirm(t('boards.delete_confirm'))) return;
    const next = boards.filter(b => b.id !== id);
    persist(next);
    setActive(next[0]?.id ?? null);
  };
  const updateBoard = (id, fn) => persist(boards.map(b => (b.id === id ? fn(b) : b)));
  const addWidget = (w) => { updateBoard(active, b => ({ ...b, widgets: [...b.widgets, w] })); setAdding(false); };
  const removeWidget = (wid) => updateBoard(active, b => ({ ...b, widgets: b.widgets.filter(w => w.id !== wid) }));

  const board = boards.find(b => b.id === active) || null;

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 1100, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}><ChevronLeft size={14}/> {t('common.dashboard')}</a>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <LayoutGrid size={20}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{t('boards.title')}</h1>
        </div>
        <p className="muted" style={{ margin: '0 0 20px' }}>{t('boards.subtitle')}</p>

        {!loaded ? (
          <div className="muted" style={{ padding: 24, textAlign: 'center' }}>…</div>
        ) : (
          <>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 18, flexWrap: 'wrap' }}>
              {boards.map(b => (
                <span key={b.id} className={`tab ${b.id === active ? 'tab-active' : ''}`} onClick={() => setActive(b.id)}>
                  {b.name}
                  {b.id === active && <X size={12} className="x" style={{ marginLeft: 8, verticalAlign: '-1px' }} onClick={(e) => { e.stopPropagation(); deleteBoard(b.id); }}/>}
                </span>
              ))}
              <button className="btn btn-ghost" onClick={addBoard}><Plus size={13}/> {t('boards.new')}</button>
            </div>

            {!board ? (
              <div className="card" style={{ textAlign: 'center', padding: 48 }}>
                <div className="muted">{t('boards.empty')}</div>
                <button className="btn btn-accent" style={{ marginTop: 12 }} onClick={addBoard}><Plus size={13}/> {t('boards.new')}</button>
              </div>
            ) : (
              <>
                <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
                  <button className="btn" onClick={() => setAdding(a => !a)}><Plus size={13}/> {t('boards.add_widget')}</button>
                </div>
                {adding && <AddWidget onAdd={addWidget} onCancel={() => setAdding(false)}/>}
                {board.widgets.length === 0 ? (
                  <div className="card" style={{ textAlign: 'center', padding: 40 }}><span className="muted">{t('boards.no_widgets')}</span></div>
                ) : (
                  <div className="grid">
                    {board.widgets.map(w => <Widget key={w.id} w={w} onRemove={() => removeWidget(w.id)}/>)}
                  </div>
                )}
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function AddWidget({ onAdd, onCancel }) {
  const [type, setType] = useState('monitors');
  const [title, setTitle] = useState('');
  const [metric, setMetric] = useState('');
  const [text, setText] = useState('');
  const seriesState = useApi(() => api.metrics.series(), []);
  const series = seriesState.data || [];

  const submit = () => {
    const w = { id: uid(), type, title: title.trim() };
    if (type === 'metric') w.metric = metric || (series[0]?.name ?? '');
    if (type === 'note') w.text = text;
    onAdd(w);
  };

  return (
    <div className="card" style={{ marginBottom: 14, display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'flex-end' }}>
      <div>
        <div className="muted" style={{ marginBottom: 4 }}>{t('boards.widget_type')}</div>
        <select className="select" value={type} onChange={e => setType(e.target.value)}>
          {WIDGET_TYPES.map(w => <option key={w.type} value={w.type}>{w.label}</option>)}
        </select>
      </div>
      <div>
        <div className="muted" style={{ marginBottom: 4 }}>{t('boards.widget_title')}</div>
        <input className="input" value={title} onChange={e => setTitle(e.target.value)} placeholder={t('boards.widget_title_ph')}/>
      </div>
      {type === 'metric' && (
        <div>
          <div className="muted" style={{ marginBottom: 4 }}>{t('boards.metric_name')}</div>
          <input className="input" list="board-metric-names" value={metric} onChange={e => setMetric(e.target.value)} placeholder="demo_requests_per_sec"/>
          <datalist id="board-metric-names">{series.map(s => <option key={s.name} value={s.name}/>)}</datalist>
        </div>
      )}
      {type === 'note' && (
        <div style={{ flex: 1 }}>
          <div className="muted" style={{ marginBottom: 4 }}>{t('boards.note_text')}</div>
          <input className="input" style={{ width: '100%' }} value={text} onChange={e => setText(e.target.value)}/>
        </div>
      )}
      <button className="btn btn-accent" onClick={submit}>{t('boards.add')}</button>
      <button className="btn btn-ghost" onClick={onCancel}>{t('common.cancel')}</button>
    </div>
  );
}

function Widget({ w, onRemove }) {
  return (
    <div className="card">
      <div className="wtitle">
        <span>{w.title || defaultTitle(w)}</span>
        <X size={13} className="x" onClick={onRemove}/>
      </div>
      {w.type === 'monitors' && <MonitorsWidget/>}
      {w.type === 'metric' && <MetricWidget name={w.metric}/>}
      {w.type === 'rum' && <RumWidget/>}
      {w.type === 'note' && <div style={{ fontSize: 14 }}>{w.text || ''}</div>}
    </div>
  );
}

function defaultTitle(w) {
  return WIDGET_TYPES.find(t2 => t2.type === w.type)?.label || w.type;
}

function MonitorsWidget() {
  const st = useApi(() => api.monitors.list(), []);
  const ms = st.data || [];
  const by = ms.reduce((a, m) => { const k = m.current_status || 'pending'; a[k] = (a[k] || 0) + 1; return a; }, {});
  return (
    <div>
      <div className="big">{ms.length}</div>
      <div className="muted">{t('boards.monitors_total')}</div>
      <div style={{ display: 'flex', gap: 14, marginTop: 10, fontSize: 13 }}>
        <span style={{ color: 'var(--up)' }}>● {by.up || 0} up</span>
        <span style={{ color: 'var(--down)' }}>● {by.down || 0} down</span>
        <span style={{ color: 'var(--warn)' }}>● {by.warn || 0} warn</span>
      </div>
    </div>
  );
}

function MetricWidget({ name }) {
  const st = useApi(() => (name ? api.metrics.query(name) : Promise.resolve([])), [name]);
  const pts = Array.isArray(st.data) ? st.data : [];
  const last = pts.length ? pts[pts.length - 1].avg : null;
  const vals = pts.map(p => p.avg).filter(v => typeof v === 'number');
  return (
    <div>
      <div className="big">{last == null ? '—' : round(last)}</div>
      <div className="muted">{name || t('boards.no_metric')}</div>
      <Spark vals={vals}/>
    </div>
  );
}

function RumWidget() {
  const st = useApi(() => api.rum.summary(undefined, 24), []);
  const s = st.data || {};
  return (
    <div style={{ display: 'flex', gap: 22 }}>
      <div><div className="big">{fmtMs(s.lcp_p75)}</div><div className="muted">LCP p75</div></div>
      <div><div className="big">{s.cls_p75 == null ? '—' : round(s.cls_p75, 2)}</div><div className="muted">CLS p75</div></div>
    </div>
  );
}

function Spark({ vals }) {
  if (!vals || vals.length < 2) return null;
  const min = Math.min(...vals), max = Math.max(...vals), span = max - min || 1;
  const w = 220, h = 40;
  const pts = vals.map((v, i) => `${(i / (vals.length - 1)) * w},${h - ((v - min) / span) * h}`).join(' ');
  return <svg width={w} height={h} style={{ marginTop: 10, display: 'block', maxWidth: '100%' }}><polyline points={pts} fill="none" stroke="var(--accent)" strokeWidth="2"/></svg>;
}

const round = (v, d = 1) => (typeof v === 'number' ? Number(v.toFixed(d)) : v);
const fmtMs = (v) => (v == null ? '—' : `${Math.round(v)}ms`);
