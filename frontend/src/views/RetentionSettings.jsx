import React, { useEffect, useState } from 'react';
import {
  ChevronLeft, Save, Loader2, AlertCircle, Database,
} from 'lucide-react';
import { api } from '../lib/api.js';

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
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .input {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .field { margin-bottom: 14px; }
  .field-label { font-size: 11px; font-weight: 500; color: var(--text-3); text-transform: uppercase; letter-spacing: .04em; display: block; margin-bottom: 5px; }
  .field-hint { font-size: 12px; color: var(--text-3); margin-top: 4px; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 14px; }
  .banner-ok  { background: var(--up-soft);   color: #047857; border: 1px solid #a7f3d0; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 14px; }
  .mono { font-family: 'JetBrains Mono', ui-monospace, monospace; }
`;

export default function RetentionSettings() {
  const [heartbeats, setHb] = useState(90);
  const [auditLog,   setAl] = useState(365);
  const [busy, setBusy] = useState(false);
  const [load, setLoad] = useState(true);
  const [err,  setErr]  = useState(null);
  const [ok,   setOk]   = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const r = await api.retention.get();
        if (r && typeof r === 'object') {
          setHb(Number(r.heartbeats) || 90);
          setAl(Number(r.audit_log)  || 365);
        }
      } catch (e) { setErr(e.message); }
      finally { setLoad(false); }
    })();
  }, []);

  const save = async () => {
    setErr(null); setOk(false);
    const hb = parseInt(heartbeats, 10);
    const al = parseInt(auditLog, 10);
    if (!hb || !al || hb < 1 || al < 1) {
      setErr('Both windows must be a positive number of days.');
      return;
    }
    setBusy(true);
    try {
      await api.retention.put(hb, al);
      setOk(true);
    } catch (e) { setErr(e.message || 'Save failed.'); }
    finally { setBusy(false); }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 640, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> Dashboard
        </a>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <Database size={20}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>Retention</h1>
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 22px' }}>
          A background prune loop runs every hour and deletes rows older than
          these windows. Lowering a window deletes data on the next tick — there
          is no undo.
        </p>

        {load ? (
          <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
        ) : (
          <div className="card" style={{ padding: 22 }}>
            {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}
            {ok  && <div className="banner-ok">Retention saved. Next prune tick applies the new window.</div>}

            <div className="field">
              <label className="field-label">Heartbeats · days</label>
              <input className="input mono" type="number" min="1" step="1"
                value={heartbeats} onChange={e => setHb(e.target.value)}/>
              <div className="field-hint">
                Per-check probe results. The dashboard's uptime strip rolls up
                90 days, so anything lower trims that view. Default <code>90</code>.
              </div>
            </div>

            <div className="field">
              <label className="field-label">Audit log · days</label>
              <input className="input mono" type="number" min="1" step="1"
                value={auditLog} onChange={e => setAl(e.target.value)}/>
              <div className="field-hint">
                Admin actions, logins, config changes. Compliance reviews
                typically want 1 year — default <code>365</code>.
              </div>
            </div>

            <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
              <button className="btn btn-accent" onClick={save} disabled={busy}>
                {busy ? <><Loader2 size={13}/> Saving…</> : <><Save size={13}/> Save</>}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
