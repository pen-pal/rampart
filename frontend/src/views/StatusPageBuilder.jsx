import React, { useEffect, useMemo, useState } from 'react';
import {
  ChevronLeft, ChevronDown, ChevronRight, ChevronUp, Plus, Trash2, ExternalLink, Save, AlertCircle, Loader2, X, Globe,
  Pencil, Check, Copy, Upload, Image as ImageIcon, Lock,
} from 'lucide-react';
import { api, useApi, offsetDateTimeArrayToDate, formatRelative } from '../lib/api.js';
import { canWrite } from '../lib/roles.js';
import { t } from '../lib/i18n.js';
import { confirmDialog, toast } from '../lib/notify.js';

const css = `
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
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
  .field { margin-bottom: 14px; }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); margin-bottom: 6px; display: block; }
  .input, .textarea {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input.mono { font-family: 'JetBrains Mono', monospace; }
  .input:focus, .textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .row {
    display: grid; grid-template-columns: 1fr auto auto auto auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
`;

export default function StatusPageBuilder({ user } = {}) {
  const writable = canWrite(user);
  const [reloadKey, setReloadKey] = useState(0);
  const pagesState    = useApi(() => api.statusPages.list(), [reloadKey], { pollMs: 30_000 });
  const monitorsState = useApi(() => api.monitors.list(), []);

  const [editing, setEditing] = useState(null); // page object or 'new' or null
  const [err,     setErr]     = useState(null);
  const [busy,    setBusy]    = useState(null);

  const reload = () => setReloadKey((k) => k + 1);

  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('statuspage.delete_confirm') }))) return;
    setBusy(id); setErr(null);
    try {
      await api.statusPages.remove(id);
      reload();
    } catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const pages = pagesState.data || [];

  if (editing) {
    return (
      <Editor
        page={editing === 'new' ? null : editing}
        monitors={monitorsState.data || []}
        writable={writable}
        onCancel={() => setEditing(null)}
        onSaved={reload}
      />
    );
  }

  return (
    <div className="rampart">
      <style>{css}</style>

      <div style={{ maxWidth: 980, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>
              {t('statuspage.title')}
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              {t('statuspage.subtitle')}
            </p>
          </div>
          {writable && (
            <button className="btn btn-accent" onClick={() => setEditing('new')}>
              <Plus size={14}/> {t('statuspage.new')}
            </button>
          )}
        </div>

        {err && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}
          </div>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {pagesState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16}/> {t('common.loading')}
            </div>
          ) : pages.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Globe size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>
                {t('statuspage.empty.title')}
              </div>
              <div style={{ fontSize: 12.5 }}>
                {t('statuspage.empty.cta')}
              </div>
            </div>
          ) : pages.map(p => (
            <PageRow
              key={p.id}
              page={p}
              busy={busy === p.id}
              writable={writable}
              onEdit={() => setEditing(p)}
              onClone={() => setEditing({
                // Strip id so Editor treats this as a new POST; blank slug
                // (it must be unique) and suffix the title so the duplicate
                // is visually distinct in the list before the user tweaks.
                // A custom domain is also unique, so leave it blank on clone;
                // the logo carries over since it's just branding.
                slug: '',
                title: `${p.title} ${t('statuspage.copy_suffix')}`,
                description: p.description,
                theme: p.theme,
                custom_domain: '',
                logo_url: p.logo_url,
                custom_css: p.custom_css,
                // A clone starts public — the password hash is page-specific
                // and never exposed to the client to copy anyway.
                monitor_ids: [...p.monitor_ids],
              })}
              onDelete={() => remove(p.id)}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

function PageRow({ page, busy, onEdit, onClone, onDelete, writable }) {
  const url = `${window.location.origin}/#/s/${page.slug}`;
  return (
    <div className="row">
      <div>
        <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 3, display: 'flex', alignItems: 'center', gap: 6 }}>
          {page.title}
          {page.private && (
            <span title={t('statuspage.private_lock_indicator')} style={{
              display: 'inline-flex', alignItems: 'center', gap: 3,
              fontSize: 10.5, fontWeight: 500, color: 'var(--text-2)',
              background: 'var(--surface-2)', border: '1px solid var(--border)',
              padding: '1px 6px', borderRadius: 999,
            }}>
              <Lock size={10}/> {t('statuspage.private_lock_indicator')}
            </span>
          )}
        </div>
        <a href={url} target="_blank" rel="noreferrer" style={{
          fontSize: 11.5, color: 'var(--accent-2)', textDecoration: 'none',
          display: 'inline-flex', alignItems: 'center', gap: 4,
        }}>
          {url} <ExternalLink size={11}/>
        </a>
        <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 4 }}>
          {page.monitor_ids.length === 1 ? t('statuspage.monitors_attached_one', { n: page.monitor_ids.length }) : t('statuspage.monitors_attached', { n: page.monitor_ids.length })}
          {page.description && ` · ${page.description}`}
        </div>
      </div>
      {writable ? (
        <>
          <button className="btn btn-ghost" onClick={onEdit} disabled={busy}>{t('common.edit')}</button>
          <button className="btn btn-ghost" onClick={onClone} disabled={busy} title={t('statuspage.clone_title')}><Copy size={13}/></button>
          <button className="btn btn-ghost btn-danger" onClick={onDelete} disabled={busy}><Trash2 size={13}/></button>
        </>
      ) : (<><span/><span/><span/></>)}
      <span/>
    </div>
  );
}

function Editor({ page, monitors, writable, onCancel, onSaved }) {
  // Clones come in with everything pre-filled except `id`, so detect new
  // mode via absence-of-id rather than absence-of-page.
  const isNew = !page?.id;
  const [slug,         setSlug]         = useState(page?.slug          || '');
  const [title,        setTitle]        = useState(page?.title         || '');
  const [description,  setDescription]  = useState(page?.description   || '');
  const [customDomain, setCustomDomain] = useState(page?.custom_domain || '');
  const [logoUrl,      setLogoUrl]      = useState(page?.logo_url      || '');
  const [customCss,    setCustomCss]    = useState(page?.custom_css    || '');
  // The page's existing privacy state (read-only `private` flag from the
  // API; the hash itself is never sent to the client). `password` holds the
  // operator's new/changed plaintext to submit; empty means "no change".
  const [wasPrivate]   = useState(!!page?.private);
  const [password,     setPassword]     = useState('');
  // True once the operator clicks "Make public" on a currently-private page,
  // so we submit an explicit clear (password: null) even with a blank field.
  const [clearPassword, setClearPassword] = useState(false);
  const [picked,       setPicked]       = useState(new Set(page?.monitor_ids || []));
  const [err,          setErr]          = useState(null);
  const [saving,       setSaving]       = useState(false);

  // Cap an uploaded logo at 512 KB — matches the backend's data: URI guard.
  const MAX_LOGO_BYTES = 512 * 1024;

  const onLogoPick = (e) => {
    const file = e.target.files && e.target.files[0];
    e.target.value = ''; // allow re-picking the same file later
    if (!file) return;
    if (file.size > MAX_LOGO_BYTES) {
      setErr(t('statuspage.err_logo_size'));
      return;
    }
    const reader = new FileReader();
    reader.onload = () => { setLogoUrl(reader.result); setErr(null); };
    reader.onerror = () => setErr(t('statuspage.err_logo_read'));
    reader.readAsDataURL(file);
  };

  const toggle = (id) => {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id); else next.add(id);
    setPicked(next);
  };

  // Drag-free ordering: we render picked in the order they were toggled
  // (insertion order of the Set). Good enough for v1; richer ordering UI later.
  const orderedPicked = useMemo(() => Array.from(picked), [picked]);

  const save = async () => {
    setErr(null);
    if (!title.trim())             { setErr(t('statuspage.err_title')); return; }
    if (isNew && !/^[a-z0-9-]{2,40}$/.test(slug)) {
      setErr(t('statuspage.err_slug'));
      return;
    }
    setSaving(true);
    try {
      const domain = customDomain.trim().toLowerCase() || null;
      const logo   = logoUrl || null;
      const css    = customCss.trim() || null;
      const pw     = password.trim();
      if (isNew) {
        await api.statusPages.create({
          slug:          slug.trim(),
          title:         title.trim(),
          description:   description.trim() || null,
          custom_domain: domain,
          logo_url:      logo,
          custom_css:    css,
          // Only send a password when one was typed; otherwise the page is
          // public. Omitting the key keeps the page public on the backend.
          ...(pw ? { password: pw } : {}),
          monitor_ids:   orderedPicked,
        });
      } else {
        // Password patch is triple-state: a typed value sets/changes it, an
        // explicit "Make public" sends null to clear, and otherwise we omit
        // the key entirely so the existing hash is left untouched.
        let passwordPatch = {};
        if (pw)                  passwordPatch = { password: pw };
        else if (clearPassword)  passwordPatch = { password: null };
        await api.statusPages.update(page.id, {
          title:         title.trim(),
          description:   description.trim() || null,
          // Backend treats a present value as "set", `null` as "clear".
          custom_domain: domain,
          logo_url:      logo,
          custom_css:    css,
          ...passwordPatch,
          monitor_ids:   orderedPicked,
        });
      }
      onSaved();
    } catch (e) {
      setErr(e.message || t('statuspage.err_save'));
      setSaving(false);
    }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 720, margin: '0 auto', padding: '32px 32px 64px' }}>
        <button className="btn btn-ghost" onClick={onCancel} style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('statuspage.back')}
        </button>

        <h1 style={{ fontSize: 24, fontWeight: 600, margin: '0 0 22px', letterSpacing: '-.02em' }}>
          {isNew ? t('statuspage.new') : t('statuspage.edit_title', { name: page.title })}
        </h1>

        {err && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}
          </div>
        )}

        <div className="card" style={{ padding: 22, marginBottom: 18 }}>
          <div className="field">
            <label className="field-label">{t('statuspage.slug')}{!isNew && ` ${t('statuspage.slug_immutable')}`}</label>
            <input
              className="input mono"
              value={slug}
              onChange={e => setSlug(e.target.value.toLowerCase())}
              placeholder="status-public"
              disabled={!isNew}
            />
            <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 4 }}>
              {t('statuspage.slug_hint')}
              {slug && ` ${t('statuspage.slug_served_at', { url: `${window.location.origin}/#/s/${slug}` })}`}
            </div>
          </div>

          <div className="field">
            <label className="field-label">{t('statuspage.field_title')}</label>
            <input className="input" value={title} onChange={e => setTitle(e.target.value)} placeholder="Acme Corp Status"/>
          </div>

          <div className="field">
            <label className="field-label">{t('statuspage.description')}</label>
            <textarea className="textarea" rows={2} value={description} onChange={e => setDescription(e.target.value)} placeholder="Live status of acme.com services"/>
          </div>

          <div className="field">
            <label className="field-label">{t('statuspage.custom_domain')}</label>
            <input
              className="input mono"
              value={customDomain}
              onChange={e => setCustomDomain(e.target.value.toLowerCase())}
              placeholder="status.example.com"
            />
            <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 4 }}>
              {t('statuspage.custom_domain_hint')}
            </div>
          </div>

          <div className="field">
            <label className="field-label">{t('statuspage.logo')}</label>
            <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
              {logoUrl ? (
                <img
                  src={logoUrl}
                  alt={t('statuspage.logo_preview_alt')}
                  style={{ height: 40, maxWidth: 140, objectFit: 'contain', borderRadius: 6, border: '1px solid var(--border)', background: 'var(--surface-2)', padding: 4 }}
                />
              ) : (
                <div style={{
                  height: 40, width: 40, borderRadius: 6, border: '1px dashed var(--border-2)',
                  display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-3)',
                }}>
                  <ImageIcon size={16}/>
                </div>
              )}
              <label className="btn" style={{ cursor: 'pointer' }}>
                <Upload size={13}/> {logoUrl ? t('statuspage.logo_replace') : t('statuspage.logo_upload')}
                <input type="file" accept="image/*" onChange={onLogoPick} style={{ display: 'none' }}/>
              </label>
              {logoUrl && (
                <button type="button" className="btn btn-ghost btn-danger" onClick={() => setLogoUrl('')}>
                  <X size={13}/> {t('statuspage.logo_remove')}
                </button>
              )}
            </div>
            <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 6 }}>
              {t('statuspage.logo_hint')}
            </div>
          </div>

          <div className="field">
            <label className="field-label">
              {t('statuspage.private')}
              {(wasPrivate && !clearPassword) && (
                <span style={{ marginLeft: 6, color: 'var(--accent-2)' }}>
                  <Lock size={11} style={{ verticalAlign: '-1px' }}/>
                </span>
              )}
            </label>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <input
                className="input"
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={e => { setPassword(e.target.value); if (e.target.value) setClearPassword(false); }}
                placeholder={t('statuspage.private_password_placeholder')}
              />
              {wasPrivate && !clearPassword && (
                <button
                  type="button"
                  className="btn btn-ghost btn-danger"
                  onClick={() => { setPassword(''); setClearPassword(true); }}
                  style={{ whiteSpace: 'nowrap' }}
                >
                  <X size={13}/> {t('statuspage.private_clear')}
                </button>
              )}
            </div>
            <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 4 }}>
              {clearPassword
                ? t('statuspage.private_set')
                : (wasPrivate ? t('statuspage.private_change') : t('statuspage.private_set'))}
            </div>
          </div>

          <div className="field" style={{ marginBottom: 0 }}>
            <label className="field-label">{t('statuspage.custom_css')}</label>
            <textarea
              className="textarea mono"
              rows={6}
              value={customCss}
              onChange={e => setCustomCss(e.target.value)}
              placeholder={t('statuspage.custom_css_placeholder')}
              style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 12 }}
            />
            <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 4 }}>
              {t('statuspage.custom_css_hint')}
            </div>
          </div>
        </div>

        <div className="card" style={{ padding: 22 }}>
          <div className="field-label" style={{ marginBottom: 10 }}>
            {t('statuspage.monitors_selected', { n: picked.size })}
          </div>
          <div style={{ maxHeight: 320, overflow: 'auto', border: '1px solid var(--border)', borderRadius: 8, padding: 4 }}>
            {monitors.length === 0 ? (
              <div style={{ padding: 12, fontSize: 12, color: 'var(--text-3)' }}>
                {t('statuspage.no_monitors')}
              </div>
            ) : monitors.map(m => (
              <label key={m.id} style={{
                display: 'flex', alignItems: 'center', gap: 8, padding: '6px 10px',
                borderRadius: 6, cursor: 'pointer',
                background: picked.has(m.id) ? 'var(--accent-soft)' : 'transparent',
              }}>
                <input type="checkbox" checked={picked.has(m.id)} onChange={() => toggle(m.id)}/>
                <span style={{ fontSize: 12.5 }}>{m.name}</span>
                <span style={{ fontSize: 10.5, color: 'var(--text-3)', textTransform: 'uppercase', marginLeft: 'auto' }}>{m.kind}</span>
              </label>
            ))}
          </div>
        </div>

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 18 }}>
          <button className="btn btn-ghost" onClick={onCancel} disabled={saving}>{t('common.cancel')}</button>
          <button className="btn btn-accent" onClick={save} disabled={saving}>
            {saving ? <><Loader2 size={13}/> {t('common.saving')}</> : <><Save size={13}/> {t('common.save')}</>}
          </button>
        </div>

        {!isNew && (
          <>
            <div style={{ height: 22 }}/>
            <SectionsManager pageId={page.id} monitors={monitors} attached={orderedPicked} writable={writable}/>
            <div style={{ height: 22 }}/>
            <IncidentsPanel pageId={page.id}/>
            <div style={{ height: 22 }}/>
            <SubscribersPanel pageId={page.id}/>
            {writable && (
              <>
                <div style={{ height: 22 }}/>
                <IngestTokensPanel pageId={page.id}/>
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}

// Manage component sections (sub-section grouping) for a saved page.
//   - Add / rename / delete sections (each is a labelled header on the
//     public page; deleting one leaves its monitors ungrouped).
//   - Assign each attached monitor to a section via a per-monitor <select>.
// Section CRUD + assignment are immediate API calls against the saved page
// (independent of the page's main Save button), so this panel only renders
// in edit mode. `attached` is the editor's current ordered monitor-id set;
// `monitors` is the full monitor list for name lookup.
function SectionsManager({ pageId, monitors, attached, writable }) {
  // `useApi` re-fires when its deps change, so bump these counters to force a
  // re-fetch after a mutation (there's no imperative reload on the hook).
  const [secBump, setSecBump]       = useState(0);
  const [detailBump, setDetailBump] = useState(0);
  const sectionsState = useApi(() => api.statusPages.listSections(pageId), [pageId, secBump], { pollMs: 0 });
  // Full page detail carries `monitor_sections` (per-monitor assignment),
  // which the list endpoint the Editor was seeded from does not include.
  const detailState   = useApi(() => api.statusPages.get(pageId), [pageId, detailBump], { pollMs: 0 });

  const [newName, setNewName] = useState('');
  const [busy,    setBusy]    = useState(null);
  const [err,     setErr]     = useState(null);
  // Local override of monitor→section so a <select> change reflects
  // immediately without re-fetching the whole page detail.
  const [assignOverride, setAssignOverride] = useState({});
  const [editing, setEditing] = useState(null);   // section id being renamed
  const [editName, setEditName] = useState('');
  // HTML5 drag-and-drop reorder: index of the row currently being dragged,
  // and the index it's hovering over (for a drop-target highlight). Null when
  // no drag is in progress. This is additive to the up/down arrow buttons,
  // which remain for keyboard/accessibility.
  const [dragIndex, setDragIndex] = useState(null);
  const [overIndex, setOverIndex] = useState(null);
  // Optimistic reorder buffer: when the operator moves a section we render
  // this immediately, then persist + refetch. `null` means "use the fetched
  // order". We key it to the fetched ids so a refetch that changes the set
  // (add/delete) discards a stale optimistic order.
  const [orderOverride, setOrderOverride] = useState(null);

  const fetched = sectionsState.data || [];
  // Once a refetch lands (new `data` reference from useApi), trust the server
  // order and drop our optimistic buffer.
  useEffect(() => { setOrderOverride(null); }, [sectionsState.data]);
  const sections = useMemo(() => {
    if (!orderOverride) return fetched;
    const sameSet = orderOverride.length === fetched.length
      && orderOverride.every(s => fetched.some(f => f.id === s.id));
    return sameSet ? orderOverride : fetched;
  }, [fetched, orderOverride]);
  const assignments = useMemo(() => {
    const map = {};
    for (const a of (detailState.data?.monitor_sections || [])) map[a.monitor_id] = a.section_id || '';
    return { ...map, ...assignOverride };
  }, [detailState.data, assignOverride]);

  // Only show monitors actually attached to this page; resolve names.
  const attachedMonitors = (attached || [])
    .map(id => monitors.find(m => m.id === id))
    .filter(Boolean);

  const addSection = async () => {
    const name = newName.trim();
    if (!name) { setErr(t('statuspage.sections.err_name')); return; }
    setBusy('new'); setErr(null);
    try {
      await api.statusPages.createSection(pageId, name);
      setNewName('');
      setSecBump(n => n + 1);
    } catch (e) { setErr(e.message || t('statuspage.sections.err_save')); }
    finally { setBusy(null); }
  };

  const rename = async (id) => {
    const name = editName.trim();
    if (!name) { setEditing(null); return; }
    setBusy(id); setErr(null);
    try {
      await api.statusPages.updateSection(pageId, id, { name });
      setEditing(null);
      setSecBump(n => n + 1);
    } catch (e) { setErr(e.message || t('statuspage.sections.err_save')); }
    finally { setBusy(null); }
  };

  const removeSection = async (id) => {
    if (!(await confirmDialog({ message: t('statuspage.sections.delete_confirm') }))) return;
    setBusy(id); setErr(null);
    try {
      await api.statusPages.deleteSection(pageId, id);
      setSecBump(n => n + 1);
      setDetailBump(n => n + 1);
    } catch (e) { setErr(e.message || t('statuspage.sections.err_save')); }
    finally { setBusy(null); }
  };

  // Move a section one slot up (dir = -1) or down (dir = +1). Swaps the two
  // affected sections optimistically, then PATCHes both with their new
  // `position` values and refetches so the canonical order wins.
  const move = async (index, dir) => {
    const target = index + dir;
    if (target < 0 || target >= sections.length) return;
    const reordered = sections.slice();
    [reordered[index], reordered[target]] = [reordered[target], reordered[index]];
    setOrderOverride(reordered);
    setErr(null);
    const a = sections[index];
    const b = sections[target];
    setBusy(a.id);
    try {
      // Swap the two stored positions so the new visual order persists. Fall
      // back to array indices when a section has no position yet.
      const posA = a.position ?? index;
      const posB = b.position ?? target;
      await Promise.all([
        api.statusPages.updateSection(pageId, a.id, { position: posB }),
        api.statusPages.updateSection(pageId, b.id, { position: posA }),
      ]);
      setSecBump(n => n + 1);
    } catch (e) {
      setOrderOverride(null); // drop the optimistic order on failure
      setErr(e.message || t('statuspage.sections.err_reorder'));
    } finally { setBusy(null); }
  };

  // Drop-based reorder: pull the dragged row out of `from` and splice it in
  // at `to`, then persist the full new order (one PATCH per section whose
  // position changed) and refetch. Mirrors `move`'s optimistic-then-persist
  // approach but handles an arbitrary distance, not just an adjacent swap.
  const reorderTo = async (from, to) => {
    if (from == null || to == null || from === to) return;
    if (from < 0 || from >= sections.length || to < 0 || to >= sections.length) return;
    const reordered = sections.slice();
    const [moved] = reordered.splice(from, 1);
    reordered.splice(to, 0, moved);
    setOrderOverride(reordered);
    setErr(null);
    setBusy('reorder');
    try {
      // Renumber positions densely so the new visual order persists. Only
      // PATCH the sections whose stored position actually changed.
      await Promise.all(reordered.map((sec, idx) =>
        (sec.position === idx
          ? null
          : api.statusPages.updateSection(pageId, sec.id, { position: idx }))
      ).filter(Boolean));
      setSecBump(n => n + 1);
    } catch (e) {
      setOrderOverride(null);
      setErr(e.message || t('statuspage.sections.err_reorder'));
    } finally { setBusy(null); }
  };

  const assign = async (monitorId, sectionId) => {
    setAssignOverride(prev => ({ ...prev, [monitorId]: sectionId }));
    setErr(null);
    try {
      await api.statusPages.assignSection(pageId, monitorId, sectionId || null);
    } catch (e) { setErr(e.message || t('statuspage.sections.err_save')); }
  };

  return (
    <div className="card" style={{ padding: 22 }}>
      <h3 style={{ fontSize: 15, fontWeight: 600, margin: '0 0 4px' }}>{t('statuspage.sections.title')}</h3>
      <p style={{ fontSize: 12, color: 'var(--text-3)', margin: '0 0 14px' }}>{t('statuspage.sections.hint')}</p>

      {err && <div style={{ background: 'var(--down-soft)', color: '#b91c1c', padding: '8px 12px', borderRadius: 6, fontSize: 12, marginBottom: 10 }}>{err}</div>}

      {writable && (
        <div style={{ display: 'flex', gap: 6, marginBottom: 14 }}>
          <input
            className="input"
            placeholder={t('statuspage.sections.add_placeholder')}
            value={newName}
            onChange={e => setNewName(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') addSection(); }}
            style={{ fontSize: 12 }}
          />
          <button className="btn btn-accent" onClick={addSection} disabled={busy === 'new'} style={{ padding: '5px 10px', fontSize: 12, whiteSpace: 'nowrap' }}>
            <Plus size={12}/> {busy === 'new' ? t('statuspage.sections.adding') : t('statuspage.sections.add')}
          </button>
        </div>
      )}

      {sections.length === 0 ? (
        <div style={{ padding: 12, fontSize: 12.5, color: 'var(--text-3)', textAlign: 'center', marginBottom: 14 }}>
          {t('statuspage.sections.empty')}
        </div>
      ) : sections.map((sec, i) => (
        <div key={sec.id}
          draggable={writable && editing == null && busy == null}
          onDragStart={writable ? (e) => { setDragIndex(i); e.dataTransfer.effectAllowed = 'move'; } : undefined}
          onDragOver={writable ? (e) => { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; if (overIndex !== i) setOverIndex(i); } : undefined}
          onDrop={writable ? (e) => { e.preventDefault(); reorderTo(dragIndex, i); setDragIndex(null); setOverIndex(null); } : undefined}
          onDragEnd={writable ? () => { setDragIndex(null); setOverIndex(null); } : undefined}
          title={writable ? t('statuspage.sections.drag_hint') : undefined}
          style={{
            display: 'flex', alignItems: 'center', gap: 8, padding: '6px 0',
            borderBottom: '1px solid var(--border)',
            cursor: (writable && editing == null) ? 'grab' : 'default',
            opacity: dragIndex === i ? 0.4 : 1,
            borderTop: (overIndex === i && dragIndex != null && dragIndex !== i) ? '2px solid var(--accent)' : '2px solid transparent',
          }}>
          {editing === sec.id ? (
            <>
              <input className="input" autoFocus value={editName} onChange={e => setEditName(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter') rename(sec.id); if (e.key === 'Escape') setEditing(null); }}
                style={{ fontSize: 12.5, padding: '4px 8px', flex: 1 }}/>
              <button className="btn btn-accent" onClick={() => rename(sec.id)} disabled={busy === sec.id} style={{ padding: '3px 8px', fontSize: 11 }}><Check size={11}/></button>
              <button className="btn btn-ghost" onClick={() => setEditing(null)} disabled={busy === sec.id} aria-label={t('common.cancel')} style={{ padding: '3px 8px', fontSize: 11 }}><X size={11}/></button>
            </>
          ) : (
            <>
              {writable && (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
                  <button
                    className="btn btn-ghost"
                    onClick={() => move(i, -1)}
                    disabled={busy != null || i === 0}
                    aria-label={t('statuspage.sections.move_up')}
                    title={t('statuspage.sections.move_up')}
                    style={{ padding: '0 4px', minHeight: 0, lineHeight: 1 }}
                  ><ChevronUp size={13}/></button>
                  <button
                    className="btn btn-ghost"
                    onClick={() => move(i, 1)}
                    disabled={busy != null || i === sections.length - 1}
                    aria-label={t('statuspage.sections.move_down')}
                    title={t('statuspage.sections.move_down')}
                    style={{ padding: '0 4px', minHeight: 0, lineHeight: 1 }}
                  ><ChevronDown size={13}/></button>
                </div>
              )}
              <span style={{ fontSize: 13, fontWeight: 600, flex: 1 }}>{sec.name}</span>
              {writable && <button className="btn btn-ghost" onClick={() => { setEditing(sec.id); setEditName(sec.name); }} disabled={busy === sec.id} style={{ padding: '3px 8px', fontSize: 11 }} title={t('statuspage.sections.rename')}><Pencil size={11}/></button>}
              {writable && <button className="btn btn-ghost btn-danger" onClick={() => removeSection(sec.id)} disabled={busy === sec.id} style={{ padding: '3px 8px', fontSize: 11 }} title={t('statuspage.sections.delete')}><Trash2 size={11}/></button>}
            </>
          )}
        </div>
      ))}

      {attachedMonitors.length > 0 && (
        <div style={{ marginTop: 16 }}>
          {attachedMonitors.map(m => (
            <div key={m.id} style={{ display: 'grid', gridTemplateColumns: '1fr auto', alignItems: 'center', gap: 12, padding: '7px 0', borderTop: '1px solid var(--border)' }}>
              <span style={{ fontSize: 12.5 }}>{m.name}</span>
              <select
                className="input"
                value={assignments[m.id] || ''}
                onChange={e => assign(m.id, e.target.value)}
                disabled={!writable}
                style={{ width: 180, fontSize: 12, padding: '5px 8px' }}
              >
                <option value="">{t('statuspage.sections.ungrouped')}</option>
                {sections.map(sec => (
                  <option key={sec.id} value={sec.id}>{sec.name}</option>
                ))}
              </select>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function SubscribersPanel({ pageId }) {
  const [reloadKey, setReloadKey] = useState(0);
  const list = useApi(() => api.subscribers.listForPage(pageId), [pageId, reloadKey], { pollMs: 30_000 });
  const [busy, setBusy] = useState(null);
  const reload = () => setReloadKey((k) => k + 1);
  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('statuspage.subscriber_remove_confirm') }))) return;
    setBusy(id);
    try { await api.subscribers.remove(id); reload(); }
    finally { setBusy(null); }
  };
  const subs = list.data || [];
  return (
    <div className="card" style={{ padding: 22 }}>
      <h3 style={{ fontSize: 15, fontWeight: 600, margin: '0 0 4px' }}>{t('statuspage.subscribers')}</h3>
      <p style={{ fontSize: 12, color: 'var(--text-3)', margin: '0 0 14px' }}>
        {t('statuspage.subscribers_hint_pre')} <a href="#/settings/smtp" style={{ color: 'var(--accent)' }}>{t('statuspage.subscribers_smtp_link')}</a> {t('statuspage.subscribers_hint_post')}
      </p>
      {subs.length === 0 ? (
        <div style={{ padding: 14, fontSize: 12.5, color: 'var(--text-3)', textAlign: 'center' }}>
          {t('statuspage.no_subscribers')}
        </div>
      ) : subs.map(s => (
        <div key={s.id} style={{ display: 'grid', gridTemplateColumns: '1fr auto auto', alignItems: 'center', gap: 12, padding: '8px 0', borderTop: '1px solid var(--border)' }}>
          <div>
            <div style={{ fontSize: 12.5 }}>{s.destination}</div>
            <div style={{ fontSize: 10.5, color: 'var(--text-3)' }}>
              {s.channel} · {s.confirmed ? t('statuspage.confirmed') : t('statuspage.pending')}
            </div>
          </div>
          <button className="btn btn-ghost btn-danger" onClick={() => remove(s.id)} disabled={busy === s.id} style={{ padding: '3px 8px', fontSize: 11 }}>
            <Trash2 size={11}/>
          </button>
          <span/>
        </div>
      ))}
    </div>
  );
}

// A single ingest token works for every receiver vendor — the URL only
// differs by the `/{vendor}/` path segment. List them all so operators can
// grab whichever their alerting stack speaks, rather than guessing.
const INGEST_VENDORS = [
  { id: 'alertmanager', label: 'Alertmanager' },
  { id: 'grafana',      label: 'Grafana' },
  { id: 'datadog',      label: 'Datadog' },
  { id: 'pagerduty',    label: 'PagerDuty' },
  { id: 'opsgenie',     label: 'Opsgenie' },
];

function IngestTokensPanel({ pageId }) {
  const list = useApi(() => api.ingestTokens.list(pageId), [pageId], { pollMs: 30_000 });
  const [tokens, setTokens] = useState(null);
  const [label,  setLabel]  = useState('');
  const [busy,   setBusy]   = useState(null);
  const [err,    setErr]    = useState(null);
  // Per-token disclosure: which tokens have their webhook-URL list expanded.
  // Collapsed by default so 5 URLs × N tokens doesn't flood the panel.
  const [openUrls, setOpenUrls] = useState(() => new Set());
  const toggleUrls = (id) => setOpenUrls(prev => {
    const next = new Set(prev);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });

  // Mirror the polled list into local state so a freshly generated token can
  // be prepended without waiting for the next poll. Fall back to the polled
  // data until the first local edit.
  const rows = tokens ?? (list.data || []);

  const generate = async () => {
    const l = label.trim();
    if (!l) { setErr(t('statuspage.ingest.err_label')); return; }
    setBusy('new'); setErr(null);
    try {
      const created = await api.ingestTokens.create(pageId, l);
      setTokens([created, ...rows]);
      setLabel('');
    } catch (e) { setErr(e.message || t('statuspage.ingest.err_generate')); }
    finally { setBusy(null); }
  };

  const revoke = async (id) => {
    if (!(await confirmDialog({ message: t('statuspage.ingest.revoke_confirm') }))) return;
    setBusy(id); setErr(null);
    try {
      await api.ingestTokens.remove(id);
      setTokens(rows.filter(tok => tok.id !== id));
    } catch (e) { setErr(e.message || t('statuspage.ingest.err_revoke')); }
    finally { setBusy(null); }
  };

  const copy = (text) => { try { navigator.clipboard.writeText(text); } catch { /* clipboard unavailable */ } };

  return (
    <div className="card" style={{ padding: 22 }}>
      <h3 style={{ fontSize: 15, fontWeight: 600, margin: '0 0 4px' }}>{t('statuspage.ingest.title')}</h3>
      <p style={{ fontSize: 12, color: 'var(--text-3)', margin: '0 0 14px' }}>
        {t('statuspage.ingest.subtitle')}
      </p>

      {err && <div style={{ background: 'var(--down-soft)', color: '#b91c1c', padding: '8px 12px', borderRadius: 6, fontSize: 12, marginBottom: 10 }}>{err}</div>}

      <div style={{ display: 'flex', gap: 6, marginBottom: 14 }}>
        <input
          className="input"
          placeholder={t('statuspage.ingest.label_placeholder')}
          value={label}
          onChange={e => setLabel(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') generate(); }}
          style={{ fontSize: 12 }}
        />
        <button className="btn btn-accent" onClick={generate} disabled={busy === 'new'} style={{ padding: '5px 10px', fontSize: 12, whiteSpace: 'nowrap' }}>
          <Plus size={12}/> {busy === 'new' ? t('statuspage.ingest.generating') : t('statuspage.ingest.generate')}
        </button>
      </div>

      {rows.length === 0 ? (
        <div style={{ padding: 16, fontSize: 12.5, color: 'var(--text-3)', textAlign: 'center' }}>
          {t('statuspage.ingest.empty')}
        </div>
      ) : rows.map(tok => {
        const urlsOpen = openUrls.has(tok.id);
        return (
          <div key={tok.id} style={{ padding: 12, marginBottom: 10, border: '1px solid var(--border)', borderRadius: 8 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
              <span style={{ fontSize: 13, fontWeight: 600 }}>{tok.label}</span>
              <span style={{ fontSize: 10.5, color: 'var(--text-3)' }}>
                {t('statuspage.ingest.created', { when: new Date(Array.isArray(tok.created_at) ? offsetDateTimeArrayToDate(tok.created_at) : tok.created_at).toLocaleDateString() })}
                {' · '}
                {tok.last_used_at ? t('statuspage.ingest.last_used', { when: formatRelative(tok.last_used_at) }) : t('statuspage.ingest.never_used')}
              </span>
              <button className="btn btn-ghost btn-danger" onClick={() => revoke(tok.id)} disabled={busy === tok.id} style={{ marginLeft: 'auto', padding: '3px 8px', fontSize: 11 }}>
                <Trash2 size={11}/> {t('statuspage.ingest.revoke')}
              </button>
            </div>

            <div className="field" style={{ marginBottom: 8 }}>
              <label className="field-label" style={{ marginBottom: 4 }}>{t('statuspage.ingest.token')}</label>
              <div style={{ display: 'flex', gap: 6 }}>
                <input className="input mono" readOnly value={tok.token} style={{ fontSize: 11.5 }} onFocus={e => e.target.select()}/>
                <button className="btn btn-ghost" onClick={() => copy(tok.token)} title={t('statuspage.ingest.copy_token')} style={{ padding: '5px 10px' }}>
                  <Copy size={13}/>
                </button>
              </div>
            </div>

            <div className="field" style={{ marginBottom: 0 }}>
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => toggleUrls(tok.id)}
                style={{ padding: '3px 0', fontSize: 12, color: 'var(--text-2)' }}
              >
                {urlsOpen ? <ChevronDown size={13}/> : <ChevronRight size={13}/>}
                {t('statuspage.ingest.webhook_urls', { n: INGEST_VENDORS.length })}
              </button>
              {urlsOpen && (
                <div style={{ marginTop: 6, display: 'flex', flexDirection: 'column', gap: 6 }}>
                  {INGEST_VENDORS.map(v => {
                    const url = `${window.location.origin}/v1/public/ingest/${v.id}/${tok.token}`;
                    return (
                      <div key={v.id} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                        <span style={{ fontSize: 11, fontWeight: 500, color: 'var(--text-2)', minWidth: 88 }}>{v.label}</span>
                        <input className="input mono" readOnly value={url} style={{ fontSize: 11 }} onFocus={e => e.target.select()}/>
                        <button className="btn btn-ghost" onClick={() => copy(url)} title={t('statuspage.ingest.copy_webhook')} style={{ padding: '5px 10px' }}>
                          <Copy size={13}/>
                        </button>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function IncidentsPanel({ pageId }) {
  const [reloadKey, setReloadKey] = useState(0);
  const list = useApi(() => api.incidents.listForPage(pageId), [pageId, reloadKey], { pollMs: 20_000 });
  const [tplBump, setTplBump] = useState(0);
  const tpls = useApi(() => api.incidentTemplates.list(), [tplBump]);
  const [showNew, setShowNew] = useState(false);
  const [busy,    setBusy]    = useState(null);
  const [err,     setErr]     = useState(null);

  const reload = () => setReloadKey((k) => k + 1);
  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('statuspage.incident.delete_confirm') }))) return;
    setBusy(id);
    try { await api.incidents.remove(id); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };
  const resolve = async (id) => {
    setBusy(id);
    try { await api.incidents.resolve(id); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const incidents = list.data || [];

  return (
    <div className="card" style={{ padding: 22 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>{t('statuspage.incident.title')}</h3>
        <button className="btn btn-accent" onClick={() => setShowNew(true)} style={{ padding: '5px 10px', fontSize: 12 }}>
          <Plus size={12}/> {t('statuspage.incident.post')}
        </button>
      </div>
      {err && <div style={{ background: 'var(--down-soft)', color: '#b91c1c', padding: '8px 12px', borderRadius: 6, fontSize: 12, marginBottom: 10 }}>{err}</div>}

      {showNew && <NewIncidentForm pageId={pageId} templates={tpls.data || []} onCancel={() => setShowNew(false)} onCreated={reload}/>}

      {incidents.length === 0 ? (
        <div style={{ padding: 16, fontSize: 12.5, color: 'var(--text-3)', textAlign: 'center' }}>
          {t('statuspage.incident.empty')}
        </div>
      ) : incidents.map(i => (
        <IncidentRow key={i.id} incident={i} busy={busy === i.id} onResolve={() => resolve(i.id)} onDelete={() => remove(i.id)} onPostUpdate={reload}/>
      ))}

      <TemplatesPanel templates={tpls.data || []} onChanged={() => setTplBump(n => n + 1)}/>
    </div>
  );
}

// Manage the global incident-update template library. Operators repeat the
// same phases ("Investigating", "Identified", "Monitoring", "Resolved")
// on every incident; this small list/add/delete affordance lets them save
// the canned text once and apply it from the "Use template" dropdown in
// NewIncidentForm. Global, not per-page — the queries carry no page id.
function TemplatesPanel({ templates, onChanged }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [body, setBody] = useState('');
  const [style, setStyle] = useState('warning');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(null);

  const add = async () => {
    setErr(null);
    if (!name.trim() || !body.trim()) { setErr(t('statuspage.tpl.err_required')); return; }
    setBusy(true);
    try {
      await api.incidentTemplates.create({ name: name.trim(), body: body.trim(), style });
      setName(''); setBody(''); setStyle('warning');
      onChanged();
    } catch (e) { setErr(e.message || t('statuspage.tpl.err_save')); }
    finally { setBusy(false); }
  };

  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('statuspage.tpl.delete_confirm') }))) return;
    setBusy(true);
    try { await api.incidentTemplates.remove(id); onChanged(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(false); }
  };

  return (
    <div style={{ marginTop: 14, paddingTop: 14, borderTop: '1px solid var(--border)' }}>
      <button
        className="btn btn-ghost"
        onClick={() => setOpen(o => !o)}
        style={{ padding: '3px 0', fontSize: 12, color: 'var(--text-2)' }}
      >
        {open ? <ChevronDown size={12}/> : <ChevronRight size={12}/>} {t('statuspage.tpl.manage')} ({templates.length})
      </button>
      {open && (
        <div style={{ marginTop: 10 }}>
          {err && <div style={{ background: 'var(--down-soft)', color: '#b91c1c', padding: '6px 10px', borderRadius: 6, fontSize: 12, marginBottom: 8 }}>{err}</div>}
          {templates.length === 0 ? (
            <div style={{ fontSize: 12, color: 'var(--text-3)', marginBottom: 10 }}>{t('statuspage.tpl.empty')}</div>
          ) : templates.map(tpl => (
            <div key={tpl.id} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '6px 0', borderBottom: '1px solid var(--border)' }}>
              <span style={{ fontSize: 12.5, fontWeight: 600 }}>{tpl.name}</span>
              <span className="mono" style={{ fontSize: 10, color: 'var(--text-3)' }}>{tpl.style}</span>
              <span style={{ flex: 1, fontSize: 11.5, color: 'var(--text-3)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{tpl.body}</span>
              <button className="btn btn-ghost" onClick={() => remove(tpl.id)} disabled={busy} style={{ padding: '2px 6px' }} title={t('statuspage.tpl.delete')}><Trash2 size={12}/></button>
            </div>
          ))}
          <div style={{ marginTop: 10 }}>
            <input className="input" placeholder={t('statuspage.tpl.name_placeholder')} value={name} onChange={e => setName(e.target.value)} style={{ marginBottom: 8 }}/>
            <textarea className="textarea" rows={2} placeholder={t('statuspage.tpl.body_placeholder')} value={body} onChange={e => setBody(e.target.value)} style={{ marginBottom: 8 }}/>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <select className="input" value={style} onChange={e => setStyle(e.target.value)} style={{ width: 140 }}>
                <option value="info">{t('statuspage.incident.style.info')}</option>
                <option value="warning">{t('statuspage.incident.style.warning')}</option>
                <option value="danger">{t('statuspage.incident.style.danger')}</option>
                <option value="primary">{t('statuspage.incident.style.primary')}</option>
                <option value="success">{t('statuspage.incident.style.success')}</option>
              </select>
              <button className="btn btn-accent" onClick={add} disabled={busy} style={{ padding: '5px 10px', fontSize: 12 }}>
                <Plus size={12}/> {t('statuspage.tpl.add')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function NewIncidentForm({ pageId, templates, onCancel, onCreated }) {
  const [title,   setTitle]   = useState('');
  const [content, setContent] = useState('');
  const [style,   setStyle]   = useState('warning');
  const [pinned,  setPinned]  = useState(true);
  const [busy,    setBusy]    = useState(false);
  const [err,     setErr]     = useState(null);

  // "Use template" — fill the body + style from a saved template. The
  // value carried in the <select> is the template id; on pick we copy its
  // body into the content field and its style into the style field,
  // leaving the title for the operator to fill in per-incident.
  const applyTemplate = (id) => {
    const tpl = (templates || []).find(x => x.id === id);
    if (!tpl) return;
    setContent(tpl.body);
    setStyle(tpl.style);
  };

  const submit = async () => {
    setErr(null);
    if (!title.trim() || !content.trim()) { setErr(t('statuspage.incident.err_required')); return; }
    setBusy(true);
    try {
      await api.incidents.create(pageId, { title: title.trim(), content: content.trim(), style, pinned });
      onCreated();
    } catch (e) { setErr(e.message || t('statuspage.incident.err_post')); setBusy(false); }
  };

  return (
    <div style={{ padding: 12, marginBottom: 12, border: '1px solid var(--border)', borderRadius: 8 }}>
      {err && <div style={{ background: 'var(--down-soft)', color: '#b91c1c', padding: '6px 10px', borderRadius: 6, fontSize: 12, marginBottom: 8 }}>{err}</div>}
      {(templates || []).length > 0 && (
        <select
          className="input"
          value=""
          onChange={e => { applyTemplate(e.target.value); e.target.value = ''; }}
          style={{ marginBottom: 8 }}
          title={t('statuspage.tpl.use')}
        >
          <option value="">{t('statuspage.tpl.use')}</option>
          {(templates || []).map(tpl => (
            <option key={tpl.id} value={tpl.id}>{tpl.name}</option>
          ))}
        </select>
      )}
      <input className="input" placeholder={t('statuspage.incident.title_placeholder')} value={title} onChange={e => setTitle(e.target.value)} style={{ marginBottom: 8 }}/>
      <textarea className="textarea" rows={2} placeholder={t('statuspage.incident.content_placeholder')} value={content} onChange={e => setContent(e.target.value)} style={{ marginBottom: 8 }}/>
      <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 10 }}>
        <select className="input" value={style} onChange={e => setStyle(e.target.value)} style={{ width: 140 }}>
          <option value="info">{t('statuspage.incident.style.info')}</option>
          <option value="warning">{t('statuspage.incident.style.warning')}</option>
          <option value="danger">{t('statuspage.incident.style.danger')}</option>
          <option value="primary">{t('statuspage.incident.style.primary')}</option>
          <option value="success">{t('statuspage.incident.style.success')}</option>
        </select>
        <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12 }}>
          <input type="checkbox" checked={pinned} onChange={e => setPinned(e.target.checked)}/> {t('statuspage.incident.pin_top')}
        </label>
      </div>
      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 6 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>{busy ? t('statuspage.incident.posting') : t('statuspage.incident.post_incident')}</button>
      </div>
    </div>
  );
}

function IncidentRow({ incident, busy, onResolve, onDelete, onPostUpdate }) {
  const updates = useApi(() => api.incidents.listUpdates(incident.id), [incident.id]);
  const [msg,  setMsg]  = useState('');
  const [posting, setPosting] = useState(false);
  const post = async () => {
    if (!msg.trim()) return;
    setPosting(true);
    try { await api.incidents.postUpdate(incident.id, msg.trim()); setMsg(''); onPostUpdate(); }
    finally { setPosting(false); }
  };

  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(incident.title);
  const [content, setContent] = useState(incident.content);
  const [pinned, setPinned] = useState(!!incident.pinned);
  const [saving, setSaving] = useState(false);
  const saveEdit = async () => {
    const newTitle = title.trim(), c = content.trim();
    if (!newTitle) { setEditing(false); setTitle(incident.title); return; }
    const unchanged = newTitle === incident.title
                   && c === incident.content
                   && pinned === !!incident.pinned;
    if (unchanged) { setEditing(false); return; }
    setSaving(true);
    try {
      await api.incidents.update(incident.id, { title: newTitle, content: c, pinned });
      onPostUpdate();
      setEditing(false);
    }
    catch (e) { toast(e.message, 'error'); }
    finally { setSaving(false); }
  };
  const cancelEdit = () => {
    setEditing(false);
    setTitle(incident.title);
    setContent(incident.content);
    setPinned(!!incident.pinned);
  };

  return (
    <div style={{ padding: 12, marginBottom: 10, border: '1px solid var(--border)', borderRadius: 8 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
        {editing ? (
          <>
            <input className="input" autoFocus value={title} onChange={e => setTitle(e.target.value)}
              style={{ fontSize: 13.5, fontWeight: 600, padding: '4px 8px', flex: 1 }}/>
            <button className="btn btn-accent" onClick={saveEdit} disabled={saving} style={{ padding: '3px 8px', fontSize: 11 }}><Check size={11}/></button>
            <button className="btn btn-ghost" onClick={cancelEdit} disabled={saving} aria-label={t('common.cancel')} style={{ padding: '3px 8px', fontSize: 11 }}><X size={11}/></button>
          </>
        ) : (
          <>
            <span style={{ fontSize: 13.5, fontWeight: 600 }}>{incident.title}</span>
            <span className="mono" style={{ fontSize: 10, color: 'var(--text-3)' }}>{incident.style}</span>
            {!incident.active && <span style={{ fontSize: 10.5, color: 'var(--accent-2)', background: 'var(--accent-soft)', padding: '1px 6px', borderRadius: 999 }}>{t('statuspage.incident.resolved')}</span>}
            {incident.pinned && incident.active && <span style={{ fontSize: 10.5, color: 'var(--text-3)' }}>{t('statuspage.incident.pinned')}</span>}
            <div style={{ marginLeft: 'auto', display: 'flex', gap: 4 }}>
              {incident.active && <button className="btn btn-ghost" onClick={() => setEditing(true)} disabled={busy} style={{ padding: '3px 8px', fontSize: 11 }} title={t('statuspage.incident.edit_title')}><Pencil size={11}/></button>}
              {incident.active && <button className="btn btn-ghost" onClick={onResolve} disabled={busy} style={{ padding: '3px 8px', fontSize: 11 }}>{t('statuspage.incident.resolve')}</button>}
              <button className="btn btn-ghost btn-danger" onClick={onDelete} disabled={busy} style={{ padding: '3px 8px', fontSize: 11 }}><Trash2 size={11}/></button>
            </div>
          </>
        )}
      </div>
      {editing ? (
        <>
          <textarea className="input" value={content} onChange={e => setContent(e.target.value)}
            rows={3} style={{ fontSize: 12.5, marginBottom: 8, width: '100%' }}/>
          <label style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12, marginBottom: 8, color: 'var(--text-2)' }}>
            <input type="checkbox" checked={pinned} onChange={e => setPinned(e.target.checked)}/>
            {t('statuspage.incident.pin_active')}
          </label>
        </>
      ) : (
        <div style={{ fontSize: 12.5, color: 'var(--text-2)', marginBottom: 8, whiteSpace: 'pre-wrap' }}>{incident.content}</div>
      )}

      {(updates.data || []).map(u => (
        <div key={u.id} style={{ fontSize: 12, color: 'var(--text-2)', paddingLeft: 12, borderLeft: '2px solid var(--border)', marginBottom: 6 }}>
          <span style={{ color: 'var(--text-3)', fontSize: 11 }}>{new Date(Array.isArray(u.posted_at) ? offsetDateTimeArrayToDate(u.posted_at) : u.posted_at).toLocaleString()}</span>
          <div style={{ whiteSpace: 'pre-wrap' }}>{u.message}</div>
        </div>
      ))}

      {incident.active && (
        <div style={{ display: 'flex', gap: 6, marginTop: 6 }}>
          <input className="input" placeholder={t('statuspage.incident.update_placeholder')} value={msg} onChange={e => setMsg(e.target.value)} style={{ fontSize: 12 }}/>
          <button className="btn btn-accent" onClick={post} disabled={posting || !msg.trim()} style={{ padding: '5px 10px', fontSize: 12 }}>{t('statuspage.incident.post')}</button>
        </div>
      )}
    </div>
  );
}
