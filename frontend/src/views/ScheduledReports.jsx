import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, Mail, AlertCircle, Loader2, X, Pencil, Check, Send,
} from 'lucide-react';
import { api, useApi, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { confirmDialog } from '../lib/notify.js';
import { isAdmin } from '../lib/roles.js';

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
  .btn:disabled { opacity: .55; cursor: not-allowed; }
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-danger { color: var(--down); }
  .btn-danger:hover { background: var(--down-soft); border-color: var(--down); }
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .input, .select, textarea.input {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .row {
    display: grid; grid-template-columns: 1fr auto auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .pill {
    display: inline-flex; align-items: center;
    font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500;
    background: var(--accent-soft); color: var(--accent-2);
  }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); display: block; margin-bottom: 6px; }
  .field-hint { font-size: 11.5px; color: var(--text-3); margin-top: 6px; }
`;

const tsToDate = (ts) => (Array.isArray(ts) ? offsetDateTimeArrayToDate(ts) : new Date(ts));

// Recipients are stored as a string[]; the form edits them as a textarea
// (one address per line). These two helpers convert between the shapes.
const recipientsToText = (arr) => (arr || []).join('\n');
const textToRecipients = (text) =>
  text.split('\n').map(s => s.trim()).filter(Boolean);

export default function ScheduledReports({ user }) {
  const admin = isAdmin(user);
  const reportsState = useApi(() => api.scheduledReports.list(), [], { pollMs: 30_000 });
  const [creating, setCreating] = useState(false);
  const [editing,  setEditing]  = useState(null); // report being edited, or null
  const [busy,     setBusy]      = useState(null);
  const [err,      setErr]       = useState(null);
  // Transient per-row send-now feedback: { [id]: 'sending' | 'sent' | 'err' }.
  const [sendState, setSendState] = useState({});

  const reload = () => window.location.reload();

  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('reports.delete_confirm') }))) return;
    setBusy(id); setErr(null);
    try { await api.scheduledReports.remove(id); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const sendNow = async (id) => {
    setSendState(s => ({ ...s, [id]: 'sending' }));
    try {
      await api.scheduledReports.send(id);
      setSendState(s => ({ ...s, [id]: 'sent' }));
      // Refetch so last_sent_at refreshes; clear the transient flag after.
      // Refetch (window reload) so last_sent_at refreshes after a short
      // beat showing the "sent" confirmation.
      setTimeout(() => { reload(); }, 1200);
    } catch (e) {
      setSendState(s => ({ ...s, [id]: 'err' }));
      setErr(e.message || t('reports.send_failed'));
    }
  };

  const reports = reportsState.data || [];

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 920, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('reports.back')}
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('reports.title')}</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              {t('reports.subtitle')}
            </p>
          </div>
          <button className="btn btn-accent" onClick={() => { setEditing(null); setCreating(true); }}>
            <Plus size={14}/> {t('reports.new')}
          </button>
        </div>

        {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        {creating && (
          <ReportForm onCancel={() => setCreating(false)} onSaved={reload}/>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {reportsState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16}/> {t('reports.loading')}
            </div>
          ) : reports.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Mail size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('reports.empty.title')}</div>
              <div style={{ fontSize: 12.5 }}>{t('reports.empty.cta')}</div>
            </div>
          ) : reports.map(r => (
            editing === r.id ? (
              <div key={r.id} style={{ padding: 18, borderTop: '1px solid var(--border)' }}>
                <ReportForm report={r} onCancel={() => setEditing(null)} onSaved={reload}/>
              </div>
            ) : (
              <div className="row" key={r.id}>
                <div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
                    <span style={{ fontSize: 14, fontWeight: 600 }}>{r.name}</span>
                    <span className="pill">{t(`reports.cadence.${r.cadence}`) || r.cadence}</span>
                  </div>
                  <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                    {t('reports.recipients_count', { n: (r.recipients || []).length })}
                    {' · '}
                    {r.last_sent_at
                      ? t('reports.field.last_sent') + ': ' + formatRelative(tsToDate(r.last_sent_at))
                      : t('reports.never_sent')}
                  </div>
                  {(r.recipients || []).length > 0 && (
                    <div className="mono" style={{ fontSize: 11, color: 'var(--text-2)', marginTop: 4, wordBreak: 'break-all' }}>
                      {(r.recipients || []).join(', ')}
                    </div>
                  )}
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  {admin && (
                    <button className="btn btn-ghost" onClick={() => sendNow(r.id)}
                      disabled={busy === r.id || sendState[r.id] === 'sending'}
                      title={t('reports.send_now')}>
                      {sendState[r.id] === 'sending'
                        ? <><Loader2 size={13}/> {t('reports.sending')}</>
                        : sendState[r.id] === 'sent'
                        ? <><Check size={13}/> {t('reports.sent')}</>
                        : <><Send size={13}/> {t('reports.send_now')}</>}
                    </button>
                  )}
                  <button className="btn btn-ghost" onClick={() => { setCreating(false); setEditing(r.id); }} disabled={busy === r.id}>
                    <Pencil size={13}/> {t('reports.edit')}
                  </button>
                  <button className="btn btn-ghost btn-danger" onClick={() => remove(r.id)} disabled={busy === r.id}>
                    <Trash2 size={13}/>
                  </button>
                </div>
              </div>
            )
          ))}
        </div>
      </div>
    </div>
  );
}

// Shared create / edit form. When `report` is passed it edits in place via
// PATCH; otherwise it creates a new report via POST.
function ReportForm({ report, onCancel, onSaved }) {
  const editing = !!report;
  const [name,       setName]       = useState(report?.name || '');
  const [recipients, setRecipients] = useState(recipientsToText(report?.recipients));
  const [cadence,    setCadence]    = useState(report?.cadence || 'weekly');
  const [busy,       setBusy]       = useState(false);
  const [err,        setErr]        = useState(null);

  const submit = async () => {
    setErr(null);
    if (!name.trim()) { setErr(t('reports.err_name')); return; }
    const recips = textToRecipients(recipients);
    if (recips.length === 0) { setErr(t('reports.err_recipients')); return; }
    setBusy(true);
    try {
      if (editing) {
        await api.scheduledReports.update(report.id, { name: name.trim(), recipients: recips, cadence });
      } else {
        await api.scheduledReports.create({ name: name.trim(), recipients: recips, cadence });
      }
      onSaved();
    } catch (e) {
      setErr(e.message || t(editing ? 'reports.err_save' : 'reports.err_create'));
      setBusy(false);
    }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: editing ? 0 : 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>{editing ? t('reports.edit') : t('reports.new')}</h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy} aria-label={t('common.cancel')}><X size={14}/></button>
      </div>
      {err && <div className="banner-err">{err}</div>}

      <div style={{ marginBottom: 14 }}>
        <label className="field-label">{t('reports.field.name')}</label>
        <input className="input" value={name} onChange={e => setName(e.target.value)} placeholder={t('reports.field.name_placeholder')}/>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 160px', gap: 12, marginBottom: 16 }}>
        <div>
          <label className="field-label">{t('reports.field.recipients')}</label>
          <textarea className="input mono" rows={3} value={recipients} onChange={e => setRecipients(e.target.value)}
            placeholder={t('reports.field.recipients_placeholder')}/>
          <div className="field-hint">{t('reports.field.recipients_hint')}</div>
        </div>
        <div>
          <label className="field-label">{t('reports.field.cadence')}</label>
          <select className="select" value={cadence} onChange={e => setCadence(e.target.value)}>
            <option value="daily">{t('reports.cadence.daily')}</option>
            <option value="weekly">{t('reports.cadence.weekly')}</option>
            <option value="monthly">{t('reports.cadence.monthly')}</option>
          </select>
        </div>
      </div>

      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>
          {busy
            ? <><Loader2 size={13}/> {editing ? t('reports.saving') : t('reports.creating')}</>
            : <><Check size={13}/> {editing ? t('reports.save') : t('reports.create')}</>}
        </button>
      </div>
    </div>
  );
}
