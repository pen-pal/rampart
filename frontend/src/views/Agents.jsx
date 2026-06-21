import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, Radio, Copy, Check, AlertCircle, Loader2, X, Eye, EyeOff,
} from 'lucide-react';
import { api, useApi, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { confirmDialog } from '../lib/notify.js';

const css = `
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
  .input {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .row {
    display: grid; grid-template-columns: 1fr auto auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .pill {
    display: inline-flex; align-items: center; gap: 5px;
    font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500;
  }
  .pill-online  { background: var(--up-soft);     color: #047857; }
  .pill-offline { background: var(--surface-2);   color: var(--text-2); }
  .dot { width: 6px; height: 6px; border-radius: 50%; }
  .modal-backdrop {
    position: fixed; inset: 0; background: rgba(0,0,0,.4);
    display: flex; align-items: center; justify-content: center;
    z-index: 100;
  }
  .modal {
    background: var(--surface); border-radius: 12px;
    max-width: 560px; width: 90%; padding: 24px;
    box-shadow: 0 20px 40px rgba(0,0,0,.2);
  }
`;

const tsToDate = (ts) => (Array.isArray(ts) ? offsetDateTimeArrayToDate(ts) : new Date(ts));

export default function Agents() {
  const [reloadKey, setReloadKey] = useState(0);
  const agentsState = useApi(() => api.agents.list(), [reloadKey], { pollMs: 30_000 });
  const [creating, setCreating] = useState(false);
  const [issued,   setIssued]   = useState(null);  // shown once after create
  const [err,      setErr]      = useState(null);
  const [busy,     setBusy]     = useState(null);

  const reload = () => setReloadKey((k) => k + 1);

  const revoke = async (id) => {
    if (!(await confirmDialog({ message: t('agents.revoke_confirm') }))) return;
    setBusy(id); setErr(null);
    try {
      await api.agents.remove(id);
      reload();
    } catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const agents = agentsState.data || [];

  return (
    <div className="rampart">
      <style>{css}</style>

      <div style={{ maxWidth: 880, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>
              {t('agents.title')}
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              {t('agents.subtitle')}
            </p>
          </div>
          <button className="btn btn-accent" onClick={() => setCreating(true)}>
            <Plus size={14}/> {t('agents.new')}
          </button>
        </div>

        {err && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}
          </div>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {agentsState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16}/>
            </div>
          ) : agents.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Radio size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>
                {t('agents.empty.title')}
              </div>
              <div style={{ fontSize: 12.5 }}>
                {t('agents.empty.cta')}
              </div>
            </div>
          ) : agents.map(a => (
            <div className="row" key={a.id}>
              <div>
                <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 3, display: 'flex', alignItems: 'center', gap: 8 }}>
                  {a.name}
                  <span className={`pill pill-${a.online ? 'online' : 'offline'}`}>
                    <span className="dot" style={{ background: a.online ? 'var(--up)' : 'var(--text-3)' }}/>
                    {a.online ? t('agents.online') : t('agents.offline')}
                  </span>
                </div>
                <div className="mono" style={{ fontSize: 11.5, color: 'var(--text-3)', marginBottom: 4 }}>
                  {a.location && `${a.location} · `}
                  {a.version && `v${a.version} · `}
                  {t('agents.monitor_count', { n: a.monitor_count })}
                </div>
                <div style={{ fontSize: 11, color: 'var(--text-3)' }}>
                  {a.last_seen_at
                    ? t('agents.last_seen', { when: formatRelative(tsToDate(a.last_seen_at)) })
                    : t('agents.never_seen')}
                </div>
              </div>
              <button className="btn btn-ghost btn-danger" onClick={() => revoke(a.id)} disabled={busy === a.id}>
                <Trash2 size={13}/> {t('agents.revoke')}
              </button>
              <span/>
            </div>
          ))}
        </div>
      </div>

      {creating && (
        <CreateModal
          onCancel={() => setCreating(false)}
          onCreated={(r) => { setCreating(false); setIssued(r); }}
        />
      )}

      {issued && (
        <TokenModal issued={issued} onClose={() => { setIssued(null); reload(); }}/>
      )}
    </div>
  );
}

function CreateModal({ onCancel, onCreated }) {
  const [name,     setName]     = useState('');
  const [location, setLocation] = useState('');
  const [busy,     setBusy]     = useState(false);
  const [err,      setErr]      = useState(null);

  const submit = async () => {
    setErr(null);
    if (!name.trim()) { setErr(t('agents.err_name')); return; }
    setBusy(true);
    try {
      const issued = await api.agents.create({
        name: name.trim(),
        ...(location.trim() ? { location: location.trim() } : {}),
      });
      onCreated(issued);
    } catch (e) {
      setErr(e.message || t('agents.err_create'));
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
          <h3 style={{ fontSize: 16, fontWeight: 600, margin: 0 }}>{t('agents.new')}</h3>
          <button className="btn btn-ghost" onClick={onCancel} disabled={busy} aria-label={t('common.cancel')}><X size={14}/></button>
        </div>

        {err && <div className="banner-err">{err}</div>}

        <div style={{ marginBottom: 14 }}>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('agents.name')}</label>
          <input className="input" autoFocus value={name} onChange={e => setName(e.target.value)} placeholder="eu-west probe"/>
        </div>

        <div style={{ marginBottom: 18 }}>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('agents.location_optional')}</label>
          <input className="input" value={location} onChange={e => setLocation(e.target.value)} placeholder="Frankfurt, DE"/>
        </div>

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
          <button className="btn btn-accent" onClick={submit} disabled={busy}>
            {busy ? <><Loader2 size={13}/> {t('agents.creating')}</> : <>{t('agents.create')}</>}
          </button>
        </div>
      </div>
    </div>
  );
}

function TokenModal({ issued, onClose }) {
  const [revealed, setRevealed] = useState(true);
  const [copied,   setCopied]   = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(issued.token);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch { /* user denied */ }
  };

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <h3 style={{ fontSize: 16, fontWeight: 600, margin: '0 0 6px' }}>{t('agents.created_title')}</h3>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 16px' }}>
          {t('agents.copy_intro')} <strong>{t('agents.not_shown_again')}</strong>
        </p>

        <div style={{
          display: 'flex', gap: 6, alignItems: 'stretch', marginBottom: 18,
        }}>
          <code className="mono" style={{
            flex: 1, padding: '10px 12px', borderRadius: 8,
            background: 'var(--surface-2)', border: '1px solid var(--border)',
            fontSize: 12, overflow: 'auto', whiteSpace: 'nowrap',
          }}>
            {revealed ? issued.token : '••••••••••••••••••••••••••••••••••••'}
          </code>
          <button className="btn btn-ghost" onClick={() => setRevealed(r => !r)}>
            {revealed ? <EyeOff size={13}/> : <Eye size={13}/>}
          </button>
          <button className="btn btn-ghost" onClick={copy}>
            {copied ? <><Check size={13}/> {t('apikeys.copied')}</> : <><Copy size={13}/> {t('apikeys.copy')}</>}
          </button>
        </div>

        <div style={{ fontSize: 12, color: 'var(--text-3)', marginBottom: 18 }}>
          {t('agents.run_hint')}
          <pre className="mono" style={{
            margin: '6px 0 0', padding: '10px 12px', borderRadius: 8,
            background: 'var(--surface-2)', border: '1px solid var(--border)',
            fontSize: 11.5, color: 'var(--text-2)', overflow: 'auto', whiteSpace: 'pre',
          }}>
{`RAMPART_URL=${window.location.origin} RAMPART_AGENT_TOKEN=${issued.token} rampart-agent`}
          </pre>
        </div>

        <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
          <button className="btn btn-accent" onClick={onClose}>{t('agents.saved_token')}</button>
        </div>
      </div>
    </div>
  );
}
