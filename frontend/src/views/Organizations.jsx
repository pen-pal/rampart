import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, Loader2, AlertCircle, X, Check,
  Building2, ArrowRightLeft, Pencil,
} from 'lucide-react';
import { api, useApi, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { confirmDialog, toast } from '../lib/notify.js';

const css = `
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
    display: grid; grid-template-columns: 1fr auto auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .mrow {
    display: grid; grid-template-columns: 1fr auto auto;
    align-items: center; gap: 12px;
    padding: 12px 18px; border-top: 1px solid var(--border);
  }
  .mrow:first-child { border-top: none; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .pill { display: inline-flex; align-items: center; gap: 4px; font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500; }
  .pill-admin { background: var(--accent-soft); color: var(--accent-2); }
  .pill-editor { background: #e0e7ff; color: #4338ca; }
  .pill-readonly { background: var(--surface-2); color: var(--text-2); }
  .pill-active { background: var(--accent-soft); color: var(--accent-2); }
  .select {
    padding: 6px 10px; border-radius: 8px; font-size: 12.5px; cursor: pointer;
    background: var(--surface); border: 1px solid var(--border); color: var(--text);
    font-family: inherit; outline: none;
  }
  .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .select:disabled { opacity: .55; cursor: not-allowed; }
`;

const tsToDate = (ts) => (Array.isArray(ts) ? offsetDateTimeArrayToDate(ts) : new Date(ts));
const roleLabel = (role) => t(`orgs.role.${role}`) || role;

export default function Organizations() {
  const meState   = useApi(() => api.auth.me(), []);
  const [reloadKey, setReloadKey] = useState(0);
  const orgsState = useApi(() => api.orgs.list(), [reloadKey]);
  const [creating, setCreating] = useState(false);
  const [selected, setSelected] = useState(null); // org id whose members are shown
  const [busy,     setBusy]     = useState(null);
  const [err,      setErr]      = useState(null);

  const activeOrgId = meState.data?.active_org_id || null;
  const orgs = orgsState.data || [];
  const reload = () => setReloadKey((k) => k + 1);

  const doSwitch = async (id) => {
    if (id === activeOrgId) return;
    setBusy(id); setErr(null);
    try {
      await api.orgs.switch(id);
      toast(t('orgs.switched'), 'success');
      window.location.reload();
    } catch (e) { setErr(e.message); setBusy(null); }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 920, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('orgs.title')}</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('orgs.subtitle')}</p>
          </div>
          <button className="btn btn-accent" onClick={() => setCreating(true)}>
            <Plus size={14}/> {t('orgs.new')}
          </button>
        </div>

        {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        {creating && <CreateForm onCancel={() => setCreating(false)} onCreated={() => { setCreating(false); reload(); }}/>}

        <div className="card" style={{ overflow: 'hidden' }}>
          {orgsState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
          ) : orgs.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Building2 size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 13 }}>{t('orgs.empty')}</div>
            </div>
          ) : orgs.map(o => {
            const isActive = o.id === activeOrgId;
            const isOpen   = selected === o.id;
            return (
              <div key={o.id}>
                <div className="row">
                  <button
                    onClick={() => setSelected(isOpen ? null : o.id)}
                    className="btn btn-ghost"
                    style={{ justifyContent: 'flex-start', padding: '4px 6px', width: '100%' }}
                  >
                    <Building2 size={15} style={{ flexShrink: 0, color: 'var(--text-3)' }}/>
                    <span style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 2, textAlign: 'left' }}>
                      <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <span style={{ fontSize: 13.5, fontWeight: 600, color: 'var(--text)' }}>{o.name}</span>
                        {isActive && <span className="pill pill-active"><Check size={10}/> {t('orgs.active')}</span>}
                      </span>
                      <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>{o.slug}</span>
                    </span>
                  </button>
                  <button className="btn btn-ghost" onClick={() => setSelected(isOpen ? null : o.id)}>
                    {isOpen ? t('orgs.hide_members') : t('orgs.manage')}
                  </button>
                  {isActive ? (
                    <span className="pill pill-active" style={{ justifySelf: 'end' }}>{t('orgs.current')}</span>
                  ) : (
                    <button className="btn" onClick={() => doSwitch(o.id)} disabled={busy === o.id}>
                      {busy === o.id ? <Loader2 size={13}/> : <ArrowRightLeft size={13}/>} {t('orgs.switch')}
                    </button>
                  )}
                </div>
                {isOpen && <OrgDetail org={o} onChanged={reload}/>}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ─── create-org form ─────────────────────────────────────────────────────────
function CreateForm({ onCancel, onCreated }) {
  const [slug, setSlug] = useState('');
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [err,  setErr]  = useState(null);

  const submit = async () => {
    setErr(null);
    const s = slug.trim();
    if (!/^[a-z0-9-]{2,40}$/.test(s)) { setErr(t('orgs.err_slug')); return; }
    if (!name.trim()) { setErr(t('orgs.err_name')); return; }
    setBusy(true);
    try {
      await api.orgs.create({ slug: s, name: name.trim() });
      toast(t('orgs.created'), 'success');
      onCreated();
    } catch (e) {
      setErr(e.status === 409 ? t('orgs.err_slug_taken') : (e.message || t('orgs.err_create')));
      setBusy(false);
    }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>{t('orgs.new')}</h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy} aria-label={t('common.cancel')}><X size={14}/></button>
      </div>
      {err && <div className="banner-err">{err}</div>}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10, marginBottom: 12 }}>
        <div>
          <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>{t('orgs.slug')}</label>
          <input className="input" value={slug} onChange={e => setSlug(e.target.value)} placeholder="acme"/>
          <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 4 }}>{t('orgs.slug_hint')}</div>
        </div>
        <div>
          <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>{t('common.name')}</label>
          <input className="input" value={name} onChange={e => setName(e.target.value)} placeholder="Acme Inc"/>
        </div>
      </div>
      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>
          {busy ? <><Loader2 size={13}/> {t('orgs.creating')}</> : <><Plus size={13}/> {t('orgs.create')}</>}
        </button>
      </div>
    </div>
  );
}

// ─── per-org member management ───────────────────────────────────────────────
// Loads the org's members; the caller's own row carries their per-org role, so
// the Admin-only controls (rename, add/change/remove member) gate on it.
function OrgDetail({ org, onChanged }) {
  const meState  = useApi(() => api.auth.me(), []);
  const [reloadKey, setReloadKey] = useState(0);
  const memState = useApi(() => api.orgs.members(org.id), [org.id, reloadKey]);
  const [busy, setBusy] = useState(null);
  const [err,  setErr]  = useState(null);
  const [renaming, setRenaming] = useState(false);
  const [newName,  setNewName]  = useState(org.name);

  const myId = meState.data?.user?.id;
  const members = memState.data || [];
  const myRole = members.find(m => m.user_id === myId)?.role;
  const isOrgAdmin = myRole === 'admin';
  const reload = () => setReloadKey((k) => k + 1);

  const rename = async () => {
    const n = newName.trim();
    if (!n || n === org.name) { setRenaming(false); return; }
    setBusy('rename'); setErr(null);
    try {
      await api.orgs.rename(org.id, n);
      toast(t('orgs.renamed'), 'success');
      setRenaming(false);
      onChanged();
    } catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const changeRole = async (m, role) => {
    if (role === m.role) return;
    setBusy(m.user_id); setErr(null);
    try { await api.orgs.setMemberRole(org.id, m.user_id, role); reload(); }
    catch (e) { setErr(e.status === 409 ? t('orgs.err_last_admin') : e.message); }
    finally { setBusy(null); }
  };

  const removeMember = async (m) => {
    if (!(await confirmDialog({ message: t('orgs.remove_confirm', { who: m.email }) }))) return;
    setBusy(m.user_id); setErr(null);
    try { await api.orgs.removeMember(org.id, m.user_id); reload(); }
    catch (e) { setErr(e.status === 409 ? t('orgs.err_last_admin') : e.message); }
    finally { setBusy(null); }
  };

  return (
    <div style={{ background: 'var(--surface-2)', borderTop: '1px solid var(--border)', padding: '6px 0 4px' }}>
      {err && <div className="banner-err" style={{ margin: '12px 18px' }}>{err}</div>}

      {isOrgAdmin && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '10px 18px' }}>
          {renaming ? (
            <>
              <input className="input" style={{ maxWidth: 280 }} value={newName} onChange={e => setNewName(e.target.value)} autoFocus/>
              <button className="btn btn-accent" onClick={rename} disabled={busy === 'rename'}>
                {busy === 'rename' ? <Loader2 size={13}/> : <Check size={13}/>} {t('common.save')}
              </button>
              <button className="btn btn-ghost" onClick={() => { setRenaming(false); setNewName(org.name); }}>{t('common.cancel')}</button>
            </>
          ) : (
            <button className="btn btn-ghost" onClick={() => { setNewName(org.name); setRenaming(true); }}>
              <Pencil size={13}/> {t('orgs.rename')}
            </button>
          )}
        </div>
      )}

      {isOrgAdmin && <AddMemberForm orgId={org.id} onAdded={reload} setErr={setErr}/>}

      <div style={{ padding: '4px 0' }}>
        {memState.loading ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={14}/></div>
        ) : members.map(m => (
          <div className="mrow" key={m.user_id}>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span style={{ fontSize: 13, fontWeight: 600 }}>{m.name || m.email}</span>
                {m.user_id === myId && <span className="pill pill-readonly">{t('users.you')}</span>}
              </div>
              <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                {m.email}{m.created_at ? ` · ${t('orgs.member_since', { when: formatRelative(tsToDate(m.created_at)) })}` : ''}
              </div>
            </div>
            {isOrgAdmin ? (
              <select
                className="select"
                value={m.role}
                disabled={busy === m.user_id}
                onChange={e => changeRole(m, e.target.value)}
                aria-label={t('orgs.role_label')}
              >
                <option value="admin">{t('orgs.role.admin')}</option>
                <option value="editor">{t('orgs.role.editor')}</option>
                <option value="readonly">{t('orgs.role.readonly')}</option>
              </select>
            ) : (
              <span className={`pill pill-${m.role}`}>{roleLabel(m.role)}</span>
            )}
            {isOrgAdmin ? (
              <button className="btn btn-ghost btn-danger" onClick={() => removeMember(m)} disabled={busy === m.user_id}>
                <Trash2 size={13}/>
              </button>
            ) : <span/>}
          </div>
        ))}
      </div>
    </div>
  );
}

// ─── add-member-by-email form (org Admin only) ───────────────────────────────
function AddMemberForm({ orgId, onAdded, setErr }) {
  const [email, setEmail] = useState('');
  const [role,  setRole]  = useState('editor');
  const [busy,  setBusy]  = useState(false);

  const submit = async () => {
    const e = email.trim();
    if (!e.includes('@')) { setErr(t('orgs.err_email')); return; }
    setBusy(true); setErr(null);
    try {
      await api.orgs.addMember(orgId, { email: e, role });
      toast(t('orgs.member_added'), 'success');
      setEmail('');
      onAdded();
    } catch (err) {
      setErr(err.status === 404 ? t('orgs.err_no_user') : (err.message || t('orgs.err_add')));
    } finally { setBusy(false); }
  };

  return (
    <div style={{ display: 'flex', alignItems: 'flex-end', gap: 8, padding: '4px 18px 12px', flexWrap: 'wrap' }}>
      <div style={{ flex: '1 1 220px' }}>
        <label style={{ fontSize: 12, color: 'var(--text-2)', display: 'block', marginBottom: 4 }}>{t('orgs.add_email')}</label>
        <input className="input" value={email} onChange={e => setEmail(e.target.value)} placeholder="alice@example.com"
          onKeyDown={e => { if (e.key === 'Enter') submit(); }}/>
      </div>
      <select className="select" value={role} onChange={e => setRole(e.target.value)} style={{ height: 38 }}>
        <option value="admin">{t('orgs.role.admin')}</option>
        <option value="editor">{t('orgs.role.editor')}</option>
        <option value="readonly">{t('orgs.role.readonly')}</option>
      </select>
      <button className="btn btn-accent" onClick={submit} disabled={busy} style={{ height: 38 }}>
        {busy ? <Loader2 size={13}/> : <Plus size={13}/>} {t('orgs.add')}
      </button>
    </div>
  );
}
