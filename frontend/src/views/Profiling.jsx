import React, { useState, useEffect, useMemo } from 'react';
import { ChevronLeft, Loader2, AlertCircle, Flame, RefreshCw, GitCompare } from 'lucide-react';
import { api, useApi } from '../lib/api.js';
import { t } from '../lib/i18n.js';

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
  .select { padding: 8px 10px; border-radius: 8px; background: var(--surface); border: 1px solid var(--border); font-size: 13px; color: var(--text); outline: none; font-family: inherit; }
  .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .banner-err { background: #fee2e2; color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .input { padding: 8px 10px; border-radius: 8px; background: var(--surface); border: 1px solid var(--border); font-size: 13px; color: var(--text); outline: none; font-family: inherit; }
  .input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .flame-cell { position: absolute; overflow: hidden; white-space: nowrap; font-size: 11px; line-height: 16px; padding: 0 4px; border: 1px solid rgba(255,255,255,.55); border-radius: 2px; cursor: pointer; color: #1c1917; transition: opacity .1s; }
  .flame-cell:hover { filter: brightness(1.06); outline: 1px solid var(--text); }
  .flame-cell.dim { opacity: .25; }
  .flame-cell.match { outline: 1.5px solid var(--text); box-shadow: 0 0 0 2px rgba(20,184,166,.4); }
  .topfn-row { display: grid; grid-template-columns: 1fr 130px 130px; gap: 12px; padding: 6px 12px; border-top: 1px solid var(--border); font-size: 12.5px; align-items: center; }
  .topfn-row:first-child { border-top: none; }
  .topfn-head { font-size: 11px; font-weight: 600; color: var(--text-3); text-transform: uppercase; letter-spacing: .04em; background: var(--surface-2); }
  .topfn-row.click { cursor: pointer; }
  .topfn-row.click:hover { background: var(--surface-2); }
  .topfn-row.on { background: var(--accent-soft); }
  .topfn-bar { position: relative; height: 14px; border-radius: 3px; background: var(--surface-2); overflow: hidden; }
  .topfn-bar > span { position: absolute; left: 0; top: 0; bottom: 0; border-radius: 3px; }
  .crumb { font-size: 12px; color: var(--accent-2); cursor: pointer; }
  .crumb:hover { text-decoration: underline; }
  .crumb-sep { color: var(--text-3); margin: 0 4px; }
  .flame-info { font-size: 12px; color: var(--text-2); margin-bottom: 8px; min-height: 18px; display: flex; gap: 14px; flex-wrap: wrap; }
  .flame-info .mono { color: var(--text); }
`;

const ROW_H = 17; // px per flamegraph depth level

// Warm flamegraph palette: hash the frame name to a stable hue in the
// red→yellow band, the convention since Brendan Gregg's original.
function frameColor(name) {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0;
  const hue = 8 + (h % 48);            // 8–56°: reds, oranges, yellows
  const sat = 62 + (h % 16);           // 62–78%
  return `hsl(${hue}, ${sat}%, 58%)`;
}

// Diff coloring: red = hotter (positive delta), blue = colder, grey = ~unchanged.
// Lightness scales with |delta| relative to the biggest mover in view.
function diffColor(delta, maxAbs) {
  if (!delta || maxAbs === 0) return 'hsl(40, 6%, 80%)';
  const light = 76 - Math.min(1, Math.abs(delta) / maxAbs) * 30; // 76% → 46%
  return delta > 0 ? `hsl(8, 78%, ${light}%)` : `hsl(210, 70%, ${light}%)`;
}

// Flatten a tree into absolutely-positioned cells. x/width are percentages of
// the (zoom) root's value, so the root spans full width and a node's share of
// its parent reads off directly. Children that don't sum to the parent leave a
// gap = the parent's self time, as a flamegraph should.
function layout(root) {
  const total = root.value || 1;
  const cells = [];
  let maxDepth = 0;
  const walk = (node, depth, offset) => {
    maxDepth = Math.max(maxDepth, depth);
    cells.push({
      node,
      depth,
      x: (offset / total) * 100,
      w: (node.value / total) * 100,
    });
    let childOffset = offset;
    for (const c of node.children || []) {
      walk(c, depth + 1, childOffset);
      childOffset += c.value;
    }
  };
  walk(root, 0, 0);
  return { cells, maxDepth };
}

// Self time = a node's own value minus what its children account for.
function selfValue(node) {
  const kids = (node.children || []).reduce((a, c) => a + c.value, 0);
  return Math.max(0, node.value - kids);
}

function Flamegraph({ root, unit, diff, highlight }) {
  // Zoom path as a breadcrumb stack: [root, …, current]. Click a crumb to pop.
  const [stack, setStack] = useState([root]);
  const [hovered, setHovered] = useState(null);
  useEffect(() => { setStack([root]); setHovered(null); }, [root]);
  const zoom = stack[stack.length - 1] || root;
  const { cells, maxDepth } = useMemo(() => layout(zoom), [zoom]);
  const grandTotal = root.value || 1;
  const maxAbs = useMemo(
    () => (diff ? Math.max(1, ...cells.map(c => Math.abs(c.node.delta || 0))) : 0),
    [diff, cells],
  );
  const ql = (highlight || '').trim().toLowerCase();
  const matchCount = ql ? cells.filter(c => c.node.name.toLowerCase().includes(ql)).length : 0;

  const info = hovered || zoom;
  const infoPct = ((info.value / grandTotal) * 100).toFixed(1);

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8, flexWrap: 'wrap', gap: 8 }}>
        <div style={{ fontSize: 12 }}>
          {stack.map((n, i) => (
            <React.Fragment key={i}>
              {i > 0 && <span className="crumb-sep">›</span>}
              {i === stack.length - 1
                ? <span style={{ color: 'var(--text-2)' }}>{i === 0 ? t('profiling.root') : n.name}</span>
                : <span className="crumb" onClick={() => setStack(stack.slice(0, i + 1))}>{i === 0 ? t('profiling.root') : n.name}</span>}
            </React.Fragment>
          ))}
        </div>
        {ql && <span style={{ fontSize: 11, color: 'var(--text-3)' }}>{t('profiling.match_count', { n: matchCount })}</span>}
      </div>

      <div className="flame-info">
        <span className="mono" title={info.name} style={{ maxWidth: 520, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{info.name}</span>
        <span>{info.value.toLocaleString()} {unit} · {infoPct}%</span>
        <span>{t('profiling.self')} {selfValue(info).toLocaleString()}</span>
        {diff && info.delta != null && <span style={{ color: info.delta > 0 ? 'var(--down)' : 'var(--accent-2)' }}>Δ {info.delta > 0 ? '+' : ''}{info.delta.toLocaleString()}</span>}
      </div>

      <div style={{ position: 'relative', height: (maxDepth + 1) * ROW_H, width: '100%' }}
        onMouseLeave={() => setHovered(null)}>
        {cells.map((c, i) => {
          const delta = c.node.delta || 0;
          const isMatch = ql && c.node.name.toLowerCase().includes(ql);
          const isDim = ql && !isMatch;
          return (
            <div
              key={i}
              className={`flame-cell${isDim ? ' dim' : ''}${isMatch ? ' match' : ''}`}
              onMouseEnter={() => setHovered(c.node)}
              onClick={() => (c.node.children && c.node.children.length ? setStack([...stack, c.node]) : null)}
              style={{
                left: `${c.x}%`,
                width: `${c.w}%`,
                top: c.depth * ROW_H,
                height: ROW_H - 1,
                background: diff ? diffColor(delta, maxAbs) : frameColor(c.node.name),
              }}
            >
              {c.w > 4 ? c.node.name : ''}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// Read a query param off the hash (e.g. #/profiling?service=api) — the
// trace→profile pivot deep-links the span's service here.
function hashParam(name) {
  const q = (window.location.hash.split('?')[1] || '');
  return new URLSearchParams(q).get(name) || '';
}

export default function Profiling({ profileId }) {
  const single = profileId != null;
  const [service, setService] = useState(() => hashParam('service'));
  const [type, setType] = useState('');
  const [hours, setHours] = useState(24);
  const [reloadKey, setReloadKey] = useState(0);
  const [diff, setDiff] = useState(false);
  const [highlight, setHighlight] = useState('');
  // Trace→profile pivot: an absolute [from_ms,to_ms] window off the hash scopes
  // the flamegraph to one span's lifetime.
  const [win, setWin] = useState(() => {
    const f = hashParam('from_ms'), tt = hashParam('to_ms');
    return f && tt ? { from_ms: Number(f), to_ms: Number(tt) } : null;
  });

  const svcState = useApi(() => (single ? Promise.resolve([]) : api.profiles.services()), [single]);
  const services = svcState.data || [];
  // Default to the first service once the list loads.
  useEffect(() => {
    if (!single && !service && services.length) setService(services[0]);
  }, [single, service, services]);

  const typeState = useApi(
    () => (single || !service ? Promise.resolve([]) : api.profiles.types(service)),
    [single, service],
  );
  const types = typeState.data || [];
  useEffect(() => {
    if (!single && service && !type && types.length) setType(types[0]);
  }, [single, service, type, types]);

  const flameState = useApi(() => {
    if (single) return api.profiles.flamegraphOne(profileId);
    if (service && type) {
      // A span window overrides the diff/hours path — scope to that interval.
      if (win) return api.profiles.flamegraph(service, type, hours, win);
      return diff
        ? api.profiles.flamegraphDiff(service, type, hours)
        : api.profiles.flamegraph(service, type, hours);
    }
    return Promise.resolve(null);
  }, [single, profileId, service, type, hours, diff, reloadKey, win]);

  const data = flameState.data;
  const unit = 'samples'; // values are summed sample weights regardless of type
  // Sample total spans both response shapes (flat vs diff).
  const samples = data ? (data.sample_count ?? (data.after_samples || 0) + (data.before_samples || 0)) : 0;
  const isDiff = diff && !single;

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 1040, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}><ChevronLeft size={14} /> {t('profiling.back')}</a>
        <div style={{ marginBottom: 16 }}>
          <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em', display: 'flex', alignItems: 'center', gap: 8 }}>
            <Flame size={22} /> {t('profiling.title')}
          </h1>
          <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('profiling.subtitle')}</p>
        </div>

        {!single && (
          <div style={{ display: 'flex', gap: 8, marginBottom: 14, flexWrap: 'wrap', alignItems: 'center' }}>
            <select className="select" value={service} onChange={e => { setService(e.target.value); setType(''); }}>
              {services.length === 0 && <option value="">{t('profiling.no_services')}</option>}
              {services.map(s => <option key={s} value={s}>{s}</option>)}
            </select>
            <select className="select" value={type} onChange={e => setType(e.target.value)}>
              {types.length === 0 && <option value="">{t('profiling.no_types')}</option>}
              {types.map(ty => <option key={ty} value={ty}>{ty}</option>)}
            </select>
            <select className="select" value={hours} onChange={e => setHours(Number(e.target.value))}>
              <option value={1}>{t('profiling.window_1h')}</option>
              <option value={24}>{t('profiling.window_24h')}</option>
              <option value={168}>{t('profiling.window_7d')}</option>
            </select>
            <button className={`btn ${diff ? '' : 'btn-ghost'}`} onClick={() => setDiff(v => !v)} title={t('profiling.diff_hint')}>
              <GitCompare size={14} /> {t('profiling.diff')}
            </button>
            <button className="btn btn-ghost" onClick={() => setReloadKey(k => k + 1)} title={t('profiling.refresh')}><RefreshCw size={14} /></button>
          </div>
        )}
        {single && <a href="#/profiling" className="btn btn-ghost" style={{ marginBottom: 14 }}>{t('profiling.all_profiles')}</a>}

        {flameState.error && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }} />{t('profiling.load_error')}</div>}

        {win && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, background: 'var(--accent-soft)', color: 'var(--accent-2)', border: '1px solid var(--accent-soft)', padding: '8px 14px', borderRadius: 8, fontSize: 12.5, marginBottom: 14 }}>
            <Flame size={14} />
            <span style={{ flex: 1 }}>{t('profiling.scoped_window', { ms: (win.to_ms - win.from_ms).toLocaleString() })}</span>
            <button className="btn btn-ghost" style={{ padding: '4px 8px', color: 'var(--accent-2)' }}
              onClick={() => { setWin(null); history.replaceState(null, '', `#/profiling?service=${encodeURIComponent(service)}`); }}>{t('profiling.clear_window')}</button>
          </div>
        )}

        <div className="card" style={{ padding: 16, marginBottom: 18 }}>
          {flameState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16} /> {t('profiling.loading')}</div>
          ) : !data || !data.tree || samples === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Flame size={28} style={{ marginBottom: 10, opacity: .5 }} />
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('profiling.empty.title')}</div>
              <div style={{ fontSize: 12.5 }}>{t('profiling.empty.cta')}</div>
            </div>
          ) : (
            <>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10, gap: 10, flexWrap: 'wrap' }}>
                <div style={{ fontSize: 12, color: 'var(--text-3)' }}>
                  {isDiff
                    ? `${t('profiling.diff_legend')} · after ${(data.after_samples || 0).toLocaleString()} · before ${(data.before_samples || 0).toLocaleString()}`
                    : `${samples.toLocaleString()} ${t('profiling.samples_merged')}`}
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <input className="input" style={{ width: 200, padding: '6px 9px', fontSize: 12.5 }}
                    placeholder={t('profiling.search_ph')} value={highlight} onChange={e => setHighlight(e.target.value)} />
                  {highlight && <button className="btn btn-ghost" style={{ padding: '6px 8px' }} onClick={() => setHighlight('')}>✕</button>}
                </div>
              </div>
              <Flamegraph root={data.tree} unit={unit} diff={isDiff} highlight={highlight} />
            </>
          )}
        </div>

        {data && data.top && data.top.length > 0 && (() => {
          const maxSelf = Math.max(1, ...data.top.map(f => f.self_value));
          return (
            <div className="card" style={{ overflow: 'hidden' }}>
              <div className="topfn-row topfn-head">
                <span>{t('profiling.fn')}</span>
                <span>{t('profiling.self')}</span>
                <span style={{ textAlign: 'right' }}>{t('profiling.total')}</span>
              </div>
              {data.top.map((f, i) => {
                const on = highlight && f.name.toLowerCase().includes(highlight.trim().toLowerCase());
                return (
                  <div className={`topfn-row click${on ? ' on' : ''}`} key={i}
                    onClick={() => setHighlight(highlight === f.name ? '' : f.name)}
                    title={t('profiling.click_highlight')}>
                    <span className="mono" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={f.name}>{f.name}</span>
                    <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <span className="topfn-bar" style={{ flex: 1 }}>
                        <span style={{ width: `${(f.self_value / maxSelf) * 100}%`, background: frameColor(f.name) }} />
                      </span>
                      <span className="mono" style={{ minWidth: 56, textAlign: 'right' }}>{f.self_value.toLocaleString()}</span>
                    </span>
                    <span className="mono" style={{ textAlign: 'right', color: 'var(--text-2)' }}>{f.total_value.toLocaleString()}</span>
                  </div>
                );
              })}
            </div>
          );
        })()}
      </div>
    </div>
  );
}
