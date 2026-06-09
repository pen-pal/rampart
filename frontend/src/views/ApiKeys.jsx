import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, Key, Copy, Check, AlertCircle, Loader2, X, Eye, EyeOff,
} from 'lucide-react';
import { api, useApi, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --down:#ef4444; --down-soft:#fee2e2;
    --warn:#f59e0b; --warn-soft:#fef3c7;
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

export default function ApiKeys() {
  const keysState = useApi(() => api.apiKeys.list(), [], { pollMs: 30_000 });
  const [creating, setCreating] = useState(false);
  const [issued,   setIssued]   = useState(null);  // shown once after create
  const [err,      setErr]      = useState(null);
  const [busy,     setBusy]     = useState(null);

  const reload = () => window.location.reload();

  const revoke = async (id) => {
    if (!confirm(t('apikeys.revoke_confirm'))) return;
    setBusy(id); setErr(null);
    try {
      await api.apiKeys.revoke(id);
      reload();
    } catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const keys = keysState.data || [];

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
              {t('apikeys.title')}
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              {t('apikeys.subtitle')} <span className="mono">Authorization: Bearer rmp_…</span>
            </p>
          </div>
          <button className="btn btn-accent" onClick={() => setCreating(true)}>
            <Plus size={14}/> {t('apikeys.new')}
          </button>
        </div>

        {err && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}
          </div>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {keysState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16}/>
            </div>
          ) : keys.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Key size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>
                {t('apikeys.empty.title')}
              </div>
              <div style={{ fontSize: 12.5 }}>
                {t('apikeys.empty.cta')}
              </div>
            </div>
          ) : keys.map(k => (
            <div className="row" key={k.id}>
              <div>
                <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 3, display: 'flex', alignItems: 'center', gap: 8 }}>
                  {k.name}
                  <ScopePill scope={k.scope}/>
                </div>
                <div className="mono" style={{ fontSize: 11.5, color: 'var(--text-3)', marginBottom: 4 }}>
                  {k.key_prefix}… · {t('apikeys.created', { when: formatRelative(tsToDate(k.created_at)) })}
                </div>
                <div style={{ fontSize: 11, color: 'var(--text-3)' }}>
                  {k.last_used_at
                    ? t('apikeys.last_used', { when: formatRelative(tsToDate(k.last_used_at)) })
                    : t('apikeys.never_used')}
                  {k.expires_at && ` · ${t('apikeys.expires', { when: formatRelative(tsToDate(k.expires_at)) })}`}
                </div>
              </div>
              <button className="btn btn-ghost btn-danger" onClick={() => revoke(k.id)} disabled={busy === k.id}>
                <Trash2 size={13}/> {t('apikeys.revoke')}
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

const SCOPES = ['read', 'write', 'admin'];

const SCOPE_COLORS = {
  read:  { bg: 'var(--surface-2)',  fg: 'var(--text-2)', bd: 'var(--border-2)' },
  write: { bg: 'var(--accent-soft)', fg: 'var(--accent-2)', bd: 'var(--accent)' },
  admin: { bg: 'var(--warn-soft)',  fg: '#b45309',       bd: 'var(--warn)' },
};

function ScopePill({ scope }) {
  const s = SCOPE_COLORS[scope] || SCOPE_COLORS.read;
  return (
    <span style={{
      fontSize: 10.5, fontWeight: 600, padding: '2px 7px', borderRadius: 999,
      background: s.bg, color: s.fg, border: `1px solid ${s.bd}`,
      textTransform: 'uppercase', letterSpacing: '.03em',
    }}>
      {t(`apikeys.scope_${scope}`)}
    </span>
  );
}

function CreateModal({ onCancel, onCreated }) {
  const [name,  setName]  = useState('');
  const [scope, setScope] = useState('read');
  const [exp,   setExp]   = useState('');
  const [busy,  setBusy]  = useState(false);
  const [err,   setErr]   = useState(null);

  const submit = async () => {
    setErr(null);
    if (!name.trim()) { setErr(t('apikeys.err_name')); return; }
    setBusy(true);
    try {
      const issued = await api.apiKeys.create(
        name.trim(),
        scope,
        exp ? new Date(exp).toISOString() : null,
      );
      onCreated(issued);
    } catch (e) {
      setErr(e.message || t('apikeys.err_create'));
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
          <h3 style={{ fontSize: 16, fontWeight: 600, margin: 0 }}>{t('apikeys.new')}</h3>
          <button className="btn btn-ghost" onClick={onCancel} disabled={busy}><X size={14}/></button>
        </div>

        {err && <div className="banner-err">{err}</div>}

        <div style={{ marginBottom: 14 }}>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('apikeys.name')}</label>
          <input className="input" autoFocus value={name} onChange={e => setName(e.target.value)} placeholder="CI deploy bot"/>
        </div>

        <div style={{ marginBottom: 14 }}>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('apikeys.scope')}</label>
          <select className="input" value={scope} onChange={e => setScope(e.target.value)}>
            {SCOPES.map(s => (
              <option key={s} value={s}>{t(`apikeys.scope_${s}`)}</option>
            ))}
          </select>
          <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 4 }}>{t(`apikeys.scope_hint_${scope}`)}</div>
        </div>

        <div style={{ marginBottom: 18 }}>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('apikeys.expires_optional')}</label>
          <input type="datetime-local" className="input" value={exp} onChange={e => setExp(e.target.value)}/>
          <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 4 }}>{t('apikeys.expires_hint')}</div>
        </div>

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
          <button className="btn btn-accent" onClick={submit} disabled={busy}>
            {busy ? <><Loader2 size={13}/> {t('apikeys.creating')}</> : <>{t('apikeys.generate')}</>}
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
        <h3 style={{ fontSize: 16, fontWeight: 600, margin: '0 0 6px' }}>{t('apikeys.created_title')}</h3>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 16px' }}>
          {t('apikeys.copy_intro')} <strong>{t('apikeys.not_shown_again')}</strong>
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
          {t('apikeys.use_with')} <code className="mono" style={{ color: 'var(--text-2)' }}>curl -H "Authorization: Bearer {issued.token.slice(0, 12)}…" ...</code>
        </div>

        <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
          <button className="btn btn-accent" onClick={onClose}>{t('apikeys.saved_key')}</button>
        </div>
      </div>
    </div>
  );
}
