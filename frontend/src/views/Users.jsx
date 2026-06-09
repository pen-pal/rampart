import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, ShieldCheck, Shield, Loader2, AlertCircle, X,
  Users as UsersIcon,
} from 'lucide-react';
import { api, useApi, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';

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

const ROLE_LABEL = { admin: 'admin', editor: 'editor', readonly: 'readonly' };
function roleOf(u) {
  // `role` is authoritative; fall back to the legacy is_admin shim for any
  // row that predates the migration backfill.
  return u.role || (u.is_admin ? 'admin' : 'editor');
}

const tsToDate = (t) => (Array.isArray(t) ? offsetDateTimeArrayToDate(t) : new Date(t));

export default function Users() {
  const meState    = useApi(() => api.auth.me(), []);
  const usersState = useApi(() => api.users.list(), [], { pollMs: 30_000 });
  const [creating, setCreating] = useState(false);
  const [busy,     setBusy]     = useState(null);
  const [err,      setErr]      = useState(null);

  const me = meState.data?.user;

  // Reset password modal lives on the Security page for the caller; admin
  // password-set for other users isn't exposed via API yet.
  const reload = () => window.location.reload();

  const remove = async (id) => {
    if (!confirm('Delete this user? Their sessions and API keys will be revoked.')) return;
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
          <ChevronLeft size={14}/> Dashboard
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>Users</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              Add team members, promote / demote admins, revoke access. Each user gets their own session + 2FA + API keys.
            </p>
          </div>
          {me?.is_admin && (
            <button className="btn btn-accent" onClick={() => setCreating(true)}>
              <Plus size={14}/> New user
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
                Admin only
              </div>
              <div style={{ fontSize: 12.5 }}>Only admins can view or modify users.</div>
            </div>
          ) : usersState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
          ) : users.map(u => (
            <div className="row" key={u.id}>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
                  <span style={{ fontSize: 13.5, fontWeight: 600 }}>{u.name || u.email}</span>
                  <span className={`pill pill-${roleOf(u)}`}>
                    {roleOf(u) === 'admin' ? <><ShieldCheck size={10}/> admin</> : ROLE_LABEL[roleOf(u)]}
                  </span>
                  {u.totp_enabled && <span className="pill pill-admin" title="Two-factor enabled"><Shield size={10}/> 2FA</span>}
                  {me?.id === u.id && <span className="pill pill-user">you</span>}
                </div>
                <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                  {u.email}{' · joined '}{formatRelative(tsToDate(u.created_at))}
                  {u.last_login_at && ` · last login ${formatRelative(tsToDate(u.last_login_at))}`}
                </div>
              </div>
              {me?.is_admin && me?.id !== u.id ? (
                <>
                  <select
                    className="select"
                    value={roleOf(u)}
                    disabled={busy === u.id}
                    onChange={e => changeRole(u, e.target.value)}
                    aria-label="Role"
                  >
                    <option value="admin">admin</option>
                    <option value="editor">editor</option>
                    <option value="readonly">readonly</option>
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
    if (!email.includes('@')) { setErr('Email looks invalid.'); return; }
    if (password.length < 10)  { setErr('Password must be at least 10 characters.'); return; }
    setBusy(true);
    try {
      await api.users.create(email.trim(), name.trim() || null, password, role);
      onCreated();
    } catch (e) { setErr(e.message || 'Failed to create user.'); setBusy(false); }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>New user</h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}><X size={14}/></button>
      </div>
      {err && <div className="banner-err">{err}</div>}

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10, marginBottom: 10 }}>
        <div>
          <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>Email</label>
          <input className="input" value={email} onChange={e => setEmail(e.target.value)} placeholder="alice@example.com"/>
        </div>
        <div>
          <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>Name (optional)</label>
          <input className="input" value={name} onChange={e => setName(e.target.value)} placeholder="Alice"/>
        </div>
      </div>
      <div style={{ marginBottom: 10 }}>
        <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>Initial password</label>
        <input className="input" type="password" value={password} onChange={e => setPassword(e.target.value)} placeholder="At least 10 characters"/>
        <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 4 }}>Share this securely; the user can change it after first login.</div>
      </div>
      <div style={{ marginBottom: 14 }}>
        <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>Role</label>
        <select className="select" value={role} onChange={e => setRole(e.target.value)} style={{ width: '100%' }}>
          <option value="admin">admin — full access incl. user management & settings</option>
          <option value="editor">editor — monitors, incidents, status pages, notifications</option>
          <option value="readonly">readonly — view only, no changes</option>
        </select>
      </div>

      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>Cancel</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>
          {busy ? <><Loader2 size={13}/> Creating…</> : <><Plus size={13}/> Create user</>}
        </button>
      </div>
    </div>
  );
}
