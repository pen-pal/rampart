import React, { useEffect, useState } from 'react';
import {
  ChevronLeft, Loader2, AlertCircle, ScrollText, ChevronDown,
} from 'lucide-react';
import { api, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
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
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .select {
    padding: 6px 10px; border-radius: 7px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 12.5px; color: var(--text); outline: none; font-family: inherit;
  }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 14px; }
`;

const tsToDate = (t) => (Array.isArray(t) ? offsetDateTimeArrayToDate(t) : new Date(t));

const KIND_OPTIONS = [
  '', 'monitor', 'user', 'api_key', 'status_page',
];

export default function AuditLog() {
  const [entries, setEntries] = useState([]);
  const [loading, setLoading] = useState(true);
  const [err,     setErr]     = useState(null);
  const [kind,    setKind]    = useState('');
  const [action,  setAction]  = useState('');
  const [done,    setDone]    = useState(false);

  const load = async (before) => {
    setLoading(true); setErr(null);
    try {
      const rows = await api.audit.list(100, before, kind || null, action.trim() || null);
      if (before == null) setEntries(rows);
      else                setEntries(prev => [...prev, ...rows]);
      if (rows.length < 100) setDone(true);
    } catch (e) { setErr(e.message || 'Failed to load audit log.'); }
    finally { setLoading(false); }
  };

  // initial + on filter change. Debounce the free-text action filter so
  // we don't fire a request per keystroke.
  useEffect(() => {
    const t = setTimeout(() => { setDone(false); load(null); }, 250);
    return () => clearTimeout(t);
    /* eslint-disable-next-line */
  }, [kind, action]);

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 1000, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> Dashboard
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <ScrollText size={20}/>
              <h1 style={{ fontSize: 28, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>Audit log</h1>
            </div>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '4px 0 0' }}>
              Append-only record of mutating actions. Admin-only.
            </p>
          </div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <input className="input" style={{ width: 200 }} value={action}
              onChange={e => setAction(e.target.value)}
              placeholder="action prefix e.g. monitor."/>
            <select className="select" value={kind} onChange={e => setKind(e.target.value)}>
              <option value="">All kinds</option>
              {KIND_OPTIONS.filter(Boolean).map(k => <option key={k} value={k}>{k}</option>)}
            </select>
          </div>
        </div>

        {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        <div className="card" style={{ overflow: 'hidden' }}>
          <div style={{
            display: 'grid', gridTemplateColumns: '170px 1fr 1fr 1fr',
            gap: 16, padding: '10px 18px',
            fontSize: 11, fontWeight: 600, color: 'var(--text-3)',
            textTransform: 'uppercase', letterSpacing: '.04em',
            background: 'var(--surface-2)', borderBottom: '1px solid var(--border)'
          }}>
            <span>When</span><span>Action</span><span>Resource</span><span>Payload</span>
          </div>
          {entries.length === 0 && !loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
              No audit entries yet.
            </div>
          ) : entries.map(e => (
            <Row key={e.id} entry={e}/>
          ))}
          {loading && <div style={{ padding: 16, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={14}/></div>}
        </div>

        {!done && entries.length > 0 && (
          <div style={{ display: 'flex', justifyContent: 'center', marginTop: 16 }}>
            <button className="btn" onClick={() => load(entries[entries.length - 1].id)} disabled={loading}>
              <ChevronDown size={13}/> Load more
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function Row({ entry }) {
  return (
    <div style={{
      display: 'grid', gridTemplateColumns: '170px 1fr 1fr 1fr',
      gap: 16, padding: '12px 18px',
      borderBottom: '1px solid var(--border)', alignItems: 'baseline',
      fontSize: 12.5,
    }}>
      <span style={{ color: 'var(--text-3)' }} title={tsToDate(entry.ts).toLocaleString()}>
        {formatRelative(tsToDate(entry.ts))}
      </span>
      <span className="mono">{entry.action}</span>
      <span>
        <span className="mono" style={{ color: 'var(--text-2)' }}>{entry.resource_kind}</span>
        {entry.resource_id && <span className="mono" style={{ color: 'var(--text-3)', fontSize: 11 }}>{' '}{entry.resource_id.slice(0, 8)}</span>}
      </span>
      <code className="mono" style={{ fontSize: 11, color: 'var(--text-3)', overflow: 'hidden', textOverflow: 'ellipsis' }}>
        {entry.payload ? JSON.stringify(entry.payload) : '—'}
      </code>
    </div>
  );
}
