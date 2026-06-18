import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, KeyRound, Copy, Check, AlertCircle, Loader2, X, Eye, EyeOff,
} from 'lucide-react';
import { api, useApi, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { confirmDialog } from '../lib/notify.js';

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

// Telemetry signal a key is scoped to. `all` accepts every signal.
const KINDS = ['all', 'otlp', 'prometheus', 'rum', 'profiles'];

const KIND_COLORS = {
  all:        { bg: 'var(--surface-2)',   fg: 'var(--text-2)',  bd: 'var(--border-2)' },
  otlp:       { bg: 'var(--accent-soft)',  fg: 'var(--accent-2)', bd: 'var(--accent)' },
  prometheus: { bg: 'var(--warn-soft)',   fg: '#b45309',         bd: 'var(--warn)' },
  rum:        { bg: '#ede9fe',            fg: '#6d28d9',         bd: '#a78bfa' },
  profiles:   { bg: '#dbeafe',            fg: '#1d4ed8',         bd: '#60a5fa' },
};

function KindPill({ kind }) {
  const c = KIND_COLORS[kind] || KIND_COLORS.all;
  return (
    <span style={{
      fontSize: 10.5, fontWeight: 600, padding: '2px 7px', borderRadius: 999,
      background: c.bg, color: c.fg, border: `1px solid ${c.bd}`,
      textTransform: 'uppercase', letterSpacing: '.03em',
    }}>
      {t(`ingestkeys.kind_${kind}`)}
    </span>
  );
}

export default function IngestKeys() {
  const [reloadKey, setReloadKey] = useState(0);
  const keysState = useApi(() => api.ingestKeys.list(), [reloadKey], { pollMs: 30_000 });
  const [creating, setCreating] = useState(false);
  const [issued,   setIssued]   = useState(null);  // shown once after create
  const [err,      setErr]      = useState(null);
  const [busy,     setBusy]     = useState(null);

  const reload = () => setReloadKey((k) => k + 1);

  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('ingestkeys.delete_confirm') }))) return;
    setBusy(id); setErr(null);
    try {
      await api.ingestKeys.remove(id);
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
              {t('ingestkeys.title')}
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              {t('ingestkeys.subtitle')} <span className="mono">Authorization: Bearer ingk_…</span>
            </p>
          </div>
          <button className="btn btn-accent" onClick={() => setCreating(true)}>
            <Plus size={14}/> {t('ingestkeys.new')}
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
              <KeyRound size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>
                {t('ingestkeys.empty.title')}
              </div>
              <div style={{ fontSize: 12.5 }}>
                {t('ingestkeys.empty.cta')}
              </div>
            </div>
          ) : keys.map(k => (
            <div className="row" key={k.id}>
              <div>
                <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 3, display: 'flex', alignItems: 'center', gap: 8 }}>
                  {k.label}
                  <KindPill kind={k.kind}/>
                </div>
                <div className="mono" style={{ fontSize: 11.5, color: 'var(--text-3)', marginBottom: 4 }}>
                  {t('ingestkeys.created', { when: formatRelative(tsToDate(k.created_at)) })}
                  {k.last_used_at
                    ? ` · ${t('ingestkeys.last_used', { when: formatRelative(tsToDate(k.last_used_at)) })}`
                    : ` · ${t('ingestkeys.never_used')}`}
                </div>
                {k.allowed_origins && k.allowed_origins.length > 0 && (
                  <div style={{ fontSize: 11, color: 'var(--text-3)' }}>
                    {t('ingestkeys.origins')}: <span className="mono">{k.allowed_origins.join(', ')}</span>
                  </div>
                )}
              </div>
              <button className="btn btn-ghost btn-danger" onClick={() => remove(k.id)} disabled={busy === k.id}>
                <Trash2 size={13}/> {t('ingestkeys.delete')}
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
  const [label,   setLabel]   = useState('');
  const [kind,    setKind]    = useState('all');
  const [origins, setOrigins] = useState('');
  const [busy,    setBusy]    = useState(false);
  const [err,     setErr]     = useState(null);

  const submit = async () => {
    setErr(null);
    if (!label.trim()) { setErr(t('ingestkeys.err_label')); return; }
    // Split the origins textarea on whitespace / commas / newlines into a
    // clean list; empty for non-RUM keys.
    const allowed_origins = origins
      .split(/[\s,]+/)
      .map(s => s.trim())
      .filter(Boolean);
    setBusy(true);
    try {
      const issued = await api.ingestKeys.create({
        label: label.trim(),
        kind,
        ...(allowed_origins.length ? { allowed_origins } : {}),
      });
      onCreated(issued);
    } catch (e) {
      setErr(e.message || t('ingestkeys.err_create'));
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
          <h3 style={{ fontSize: 16, fontWeight: 600, margin: 0 }}>{t('ingestkeys.new')}</h3>
          <button className="btn btn-ghost" onClick={onCancel} disabled={busy} aria-label={t('common.cancel')}><X size={14}/></button>
        </div>

        {err && <div className="banner-err">{err}</div>}

        <div style={{ marginBottom: 14 }}>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('ingestkeys.label')}</label>
          <input className="input" autoFocus value={label} onChange={e => setLabel(e.target.value)} placeholder="prod OTLP collector"/>
        </div>

        <div style={{ marginBottom: 14 }}>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('ingestkeys.kind')}</label>
          <select className="input" value={kind} onChange={e => setKind(e.target.value)}>
            {KINDS.map(k => (
              <option key={k} value={k}>{t(`ingestkeys.kind_${k}`)}</option>
            ))}
          </select>
          <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 4 }}>{t(`ingestkeys.kind_hint_${kind}`)}</div>
        </div>

        {kind === 'rum' && (
          <div style={{ marginBottom: 18 }}>
            <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('ingestkeys.origins_optional')}</label>
            <textarea
              className="input"
              rows={3}
              value={origins}
              onChange={e => setOrigins(e.target.value)}
              placeholder="https://app.example.com&#10;https://www.example.com"
            />
            <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 4 }}>{t('ingestkeys.origins_hint')}</div>
          </div>
        )}

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
          <button className="btn btn-accent" onClick={submit} disabled={busy}>
            {busy ? <><Loader2 size={13}/> {t('ingestkeys.creating')}</> : <>{t('ingestkeys.generate')}</>}
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
        <h3 style={{ fontSize: 16, fontWeight: 600, margin: '0 0 6px' }}>{t('ingestkeys.created_title')}</h3>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 16px' }}>
          {t('ingestkeys.copy_intro')} <strong>{t('ingestkeys.not_shown_again')}</strong>
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
            {copied ? <><Check size={13}/> {t('ingestkeys.copied')}</> : <><Copy size={13}/> {t('ingestkeys.copy')}</>}
          </button>
        </div>

        <div style={{ fontSize: 12, color: 'var(--text-3)', marginBottom: 18 }}>
          {t('ingestkeys.use_with')} <code className="mono" style={{ color: 'var(--text-2)' }}>curl -H "Authorization: Bearer {issued.token.slice(0, 12)}…" ...</code>
        </div>

        <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
          <button className="btn btn-accent" onClick={onClose}>{t('ingestkeys.saved_key')}</button>
        </div>
      </div>
    </div>
  );
}
