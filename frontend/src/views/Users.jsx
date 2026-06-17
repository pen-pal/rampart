import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, ShieldCheck, Shield, Loader2, AlertCircle, X,
  Users as UsersIcon,
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
    display: grid; grid-template-columns: 1fr auto auto auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .pill { display: inline-flex; align-items: center; gap: 4px; font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500; }
  .pill-admin { background: var(--accent-soft); color: var(--accent-2); }
  .pill-editor { background: #e0e7ff; color: #4338ca; }
  .pill-readonly { background: var(--surface-2); color: var(--text-2); }
  .pill-user  { background: var(--surface-2);   color: var(--text-2); }
  .select {
    padding: 6px 10px; border-radius: 8px; font-size: 12.5px; cursor: pointer;
    background: var(--surface); border: 1px solid var(--border); color: var(--text);
    font-family: inherit; outline: none;
  }
  .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .select:disabled { opacity: .55; cursor: not-allowed; }
`;

const roleLabel = (role) => t(`users.role.${role}`);
function roleOf(u) {
  // `role` is authoritative; fall back to the legacy is_admin shim for any
  // row that predates the migration backfill.
  return u.role || (u.is_admin ? 'admin' : 'editor');
}

const tsToDate = (ts) => (Array.isArray(ts) ? offsetDateTimeArrayToDate(ts) : new Date(ts));

export default function Users() {
  const meState    = useApi(() => api.auth.me(), []);
  const [reloadKey, setReloadKey] = useState(0);
  const usersState = useApi(() => api.users.list(), [reloadKey], { pollMs: 30_000 });
  const [creating, setCreating] = useState(false);
  const [busy,     setBusy]     = useState(null);
  const [err,      setErr]      = useState(null);

  const me = meState.data?.user;

  // Reset password modal lives on the Security page for the caller; admin
  // password-set for other users isn't exposed via API yet.
  const reload = () => setReloadKey((k) => k + 1);

  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('users.delete_confirm') }))) return;
    setBusy(id); setErr(null);
    try { await api.users.remove(id); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };
  const changeRole = async (u, role) => {
    if (role === roleOf(u)) return;
    setBusy(u.id); setErr(null);
    try { await api.users.setRole(u.id, role); reload(); }
    catch (e) { setErr(e.message); setBusy(null); }
  };

  // Non-admins shouldn't even land here; if /v1/users returns 403 we show
  // a small explanation rather than blank.
  const forbidden = usersState.error && usersState.error.status === 403;
  const users = usersState.data || [];

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 920, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('users.title')}</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              {t('users.subtitle')}
            </p>
          </div>
          {me?.is_admin && (
            <button className="btn btn-accent" onClick={() => setCreating(true)}>
              <Plus size={14}/> {t('users.new')}
            </button>
          )}
        </div>

        {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        {creating && <CreateForm onCancel={() => setCreating(false)} onCreated={reload}/>}

        <div className="card" style={{ overflow: 'hidden' }}>
          {forbidden ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Shield size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>
                {t('users.admin_only')}
              </div>
              <div style={{ fontSize: 12.5 }}>{t('users.admin_only_hint')}</div>
            </div>
          ) : usersState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
          ) : users.map(u => (
            <div className="row" key={u.id}>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
                  <span style={{ fontSize: 13.5, fontWeight: 600 }}>{u.name || u.email}</span>
                  <span className={`pill pill-${roleOf(u)}`}>
                    {roleOf(u) === 'admin' ? <><ShieldCheck size={10}/> {t('users.role.admin')}</> : roleLabel(roleOf(u))}
                  </span>
                  {u.totp_enabled && <span className="pill pill-admin" title={t('users.totp_enabled')}><Shield size={10}/> 2FA</span>}
                  {me?.id === u.id && <span className="pill pill-user">{t('users.you')}</span>}
                </div>
                <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                  {u.email}{' · '}{t('users.joined', { when: formatRelative(tsToDate(u.created_at)) })}
                  {u.last_login_at && ` · ${t('users.last_login', { when: formatRelative(tsToDate(u.last_login_at)) })}`}
                </div>
              </div>
              {me?.is_admin && me?.id !== u.id ? (
                <>
                  <select
                    className="select"
                    value={roleOf(u)}
                    disabled={busy === u.id}
                    onChange={e => changeRole(u, e.target.value)}
                    aria-label={t('users.role_label')}
                  >
                    <option value="admin">{t('users.role.admin')}</option>
                    <option value="editor">{t('users.role.editor')}</option>
                    <option value="readonly">{t('users.role.readonly')}</option>
                  </select>
                  <button className="btn btn-ghost btn-danger" onClick={() => remove(u.id)} disabled={busy === u.id}>
                    <Trash2 size={13}/>
                  </button>
                </>
              ) : <><span/><span/></>}
              <span/>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function CreateForm({ onCancel, onCreated }) {
  const [email,    setEmail]    = useState('');
  const [name,     setName]     = useState('');
  const [password, setPassword] = useState('');
  const [role,     setRole]     = useState('editor');
  const [busy,     setBusy]     = useState(false);
  const [err,      setErr]      = useState(null);

  const submit = async () => {
    setErr(null);
    if (!email.includes('@')) { setErr(t('users.err_email')); return; }
    if (password.length < 10)  { setErr(t('users.err_password')); return; }
    setBusy(true);
    try {
      await api.users.create(email.trim(), name.trim() || null, password, role);
      onCreated();
    } catch (e) { setErr(e.message || t('users.err_create')); setBusy(false); }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>{t('users.new')}</h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy} aria-label={t('common.cancel')}><X size={14}/></button>
      </div>
      {err && <div className="banner-err">{err}</div>}

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10, marginBottom: 10 }}>
        <div>
          <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>{t('users.email')}</label>
          <input className="input" value={email} onChange={e => setEmail(e.target.value)} placeholder="alice@example.com"/>
        </div>
        <div>
          <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>{t('users.name_optional')}</label>
          <input className="input" value={name} onChange={e => setName(e.target.value)} placeholder="Alice"/>
        </div>
      </div>
      <div style={{ marginBottom: 10 }}>
        <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>{t('users.initial_password')}</label>
        <input className="input" type="password" value={password} onChange={e => setPassword(e.target.value)} placeholder={t('users.password_placeholder')}/>
        <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 4 }}>{t('users.password_hint')}</div>
      </div>
      <div style={{ marginBottom: 14 }}>
        <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>{t('users.role_label')}</label>
        <select className="select" value={role} onChange={e => setRole(e.target.value)} style={{ width: '100%' }}>
          <option value="admin">{t('users.role_opt.admin')}</option>
          <option value="editor">{t('users.role_opt.editor')}</option>
          <option value="readonly">{t('users.role_opt.readonly')}</option>
        </select>
      </div>

      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>
          {busy ? <><Loader2 size={13}/> {t('users.creating')}</> : <><Plus size={13}/> {t('users.create')}</>}
        </button>
      </div>
    </div>
  );
}
