// Tags admin — list / create / rename / recolor / delete, with usage
// counts so operators can see where a tag is referenced before deleting.
//
// Tags were previously only manageable inline on the monitor detail page,
// which made bulk audits (renames, colour normalisation, finding stale
// tags) impossible.

import React, { useEffect, useMemo, useState } from 'react';
import {
  Tag as TagIcon, Plus, Trash2, Loader2, AlertCircle, X,
  Pencil, Check, Activity, Bell, Folder,
} from 'lucide-react';
import { api, useApi } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import SubViewHeader from '../components/SubViewHeader.jsx';
import { confirmDialog } from '../lib/notify.js';

const css = `
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --down:#ef4444; --down-soft:#fee2e2;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif; min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn { display:inline-flex; align-items:center; gap:6px; padding:7px 12px; border-radius:8px; cursor:pointer;
    font-size:13px; font-weight:500; line-height:1; background:var(--surface); border:1px solid var(--border); color:var(--text-2); font-family:inherit; }
  .btn:hover { background:var(--surface-2); color:var(--text); border-color:var(--border-2); }
  .btn:disabled { opacity:.55; cursor:not-allowed; }
  .btn-accent { background:var(--accent); color:#fff; border-color:var(--accent); }
  .btn-accent:hover { background:var(--accent-2); }
  .btn-ghost { background:transparent; border-color:transparent; }
  .btn-danger { color:var(--down); }
  .input { padding:7px 10px; border-radius:8px; background:var(--surface);
    border:1px solid var(--border); font-size:13px; color:var(--text); outline:none; font-family:inherit; }
  .input:focus { border-color:var(--accent); box-shadow:0 0 0 3px var(--accent-soft); }
  .banner-err { background:var(--down-soft); color:#b91c1c; border:1px solid #fecaca; padding:10px 14px; border-radius:8px; font-size:13px; }
  .chip { display:inline-flex; align-items:center; gap:5px; padding:3px 10px; border-radius:999px;
    font-size:12px; font-weight:500; color:#fff; }
  .tag-row { display:grid; grid-template-columns: 1fr auto auto;
    align-items:center; gap:14px; padding:12px 16px; border-top:1px solid var(--border); }
  .tag-row:first-child { border-top: none; }
  .meta { font-size:11.5px; color:var(--text-3); display:inline-flex; align-items:center; gap:14px; }
  .meta-item { display:inline-flex; align-items:center; gap:4px; }
`;

// Default palette for the New-tag form picker. Operators can still type
// any #rrggbb by hand.
const PALETTE = [
  '#dc2626', '#f59e0b', '#10b981', '#14b8a6',
  '#3b82f6', '#6366f1', '#a855f7', '#ec4899',
  '#78716c', '#1c1917',
];

export default function Tags() {
  const tagsState  = useApi(() => api.tags.list(),  []);
  const usageState = useApi(() => api.tags.usage(), []);

  const [tags, setTags] = useState([]);
  useEffect(() => { if (tagsState.data) setTags(tagsState.data); }, [tagsState.data]);

  const usageById = useMemo(() => {
    const m = new Map();
    for (const u of (usageState.data || [])) m.set(u.tag_id, u);
    return m;
  }, [usageState.data]);

  const [newName,  setNewName]  = useState('');
  const [newColor, setNewColor] = useState(PALETTE[3]);
  const [busy,     setBusy]     = useState(false);
  const [err,      setErr]      = useState(null);

  const create = async () => {
    if (!newName.trim()) return;
    setBusy(true); setErr(null);
    try {
      const created = await api.tags.create(newName.trim(), newColor);
      setTags(ts => [...ts, created].sort((a, b) => a.name.localeCompare(b.name)));
      setNewName('');
    } catch (e) { setErr(e.message); } finally { setBusy(false); }
  };

  const onPatch = async (id, patch) => {
    setErr(null);
    try {
      const updated = await api.tags.update(id, patch);
      setTags(ts => ts.map(x => x.id === id ? updated : x).sort((a, b) => a.name.localeCompare(b.name)));
    } catch (e) { setErr(e.message); throw e; }
  };
  const onDelete = async (id, name) => {
    const u = usageById.get(id);
    const refs = u ? (u.monitors + u.channels + u.groups) : 0;
    const confirmMsg = refs > 0
      ? t('tags.delete_confirm_refs', { name, monitors: u.monitors, channels: u.channels, groups: u.groups })
      : t('tags.delete_confirm', { name });
    if (!(await confirmDialog({ message: confirmMsg }))) return;
    setErr(null);
    try {
      await api.tags.remove(id);
      setTags(ts => ts.filter(x => x.id !== id));
    } catch (e) { setErr(e.message); }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 760, margin: '0 auto', padding: '32px 32px 64px' }}>
        <SubViewHeader title={t('tags.title')} icon={TagIcon} />
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <TagIcon size={20}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{t('tags.title')}</h1>
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '4px 0 22px' }}>
          {t('tags.subtitle')}
        </p>

        {err && <div className="banner-err" style={{ marginBottom: 16 }}><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        {/* Create */}
        <div className="card" style={{ padding: 16, marginBottom: 20, display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
          <input className="input" value={newName} onChange={e => setNewName(e.target.value)} style={{ flex: '1 1 200px' }}
            placeholder={t('tags.new_placeholder')} onKeyDown={e => e.key === 'Enter' && create()}/>
          <ColorSwatch color={newColor} onChange={setNewColor}/>
          <button className="btn btn-accent" onClick={create} disabled={busy} style={{ flexShrink: 0 }}>
            {busy ? <Loader2 size={13} className="spin"/> : <Plus size={13}/>} {t('tags.create')}
          </button>
        </div>

        {tagsState.loading && <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>}
        {!tagsState.loading && tags.length === 0 && (
          <div className="card" style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
            <TagIcon size={20} style={{ opacity: .4, marginBottom: 8 }}/>
            <div>{t('tags.empty')}</div>
          </div>
        )}

        {tags.length > 0 && (
          <div className="card">
            {tags.map(tag => (
              <TagRow key={tag.id} tag={tag} usage={usageById.get(tag.id)} onPatch={onPatch} onDelete={onDelete}/>
            ))}
          </div>
        )}
      </div>
      <style>{`@keyframes spin{to{transform:rotate(360deg)}}.spin{animation:spin 1s linear infinite}`}</style>
    </div>
  );
}

function TagRow({ tag, usage, onPatch, onDelete }) {
  const [editing, setEditing] = useState(false);
  const [name,    setName]    = useState(tag.name);
  const [color,   setColor]   = useState(tag.color);
  const [busy,    setBusy]    = useState(false);

  useEffect(() => { setName(tag.name); setColor(tag.color); }, [tag.name, tag.color]);

  const save = async () => {
    const trimmed = name.trim();
    if (!trimmed) { setEditing(false); setName(tag.name); return; }
    if (trimmed === tag.name && color === tag.color) { setEditing(false); return; }
    setBusy(true);
    try { await onPatch(tag.id, { name: trimmed, color }); setEditing(false); }
    catch { /* err surfaced by parent */ } finally { setBusy(false); }
  };

  const u = usage || { monitors: 0, channels: 0, groups: 0 };

  return (
    <div className="tag-row">
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        {editing ? (
          <>
            <ColorSwatch color={color} onChange={setColor}/>
            <input className="input" autoFocus value={name} onChange={e => setName(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') save(); if (e.key === 'Escape') { setEditing(false); setName(tag.name); setColor(tag.color); } }}
              style={{ width: 200 }}/>
          </>
        ) : (
          <>
            <span className="chip" style={{ background: tag.color }}>{tag.name}</span>
            <span className="meta">
              <span className="meta-item" title={t('tags.usage.monitors')}><Activity size={11}/>{u.monitors}</span>
              <span className="meta-item" title={t('tags.usage.channels')}><Bell size={11}/>{u.channels}</span>
              <span className="meta-item" title={t('tags.usage.folders')}><Folder size={11}/>{u.groups}</span>
            </span>
          </>
        )}
      </div>
      <div>
        {editing ? (
          <span style={{ display: 'inline-flex', gap: 6 }}>
            <button className="btn btn-accent" disabled={busy} onClick={save} style={{ padding: '5px 8px' }}><Check size={13}/></button>
            <button className="btn btn-ghost" onClick={() => { setEditing(false); setName(tag.name); setColor(tag.color); }} aria-label={t('common.cancel')} style={{ padding: '5px 8px' }}><X size={13}/></button>
          </span>
        ) : (
          <button className="btn btn-ghost" style={{ padding: '5px 8px' }} title={t('tags.rename_recolour')} onClick={() => setEditing(true)}>
            <Pencil size={12}/>
          </button>
        )}
      </div>
      <button className="btn btn-ghost btn-danger" style={{ padding: '5px 8px' }} onClick={() => onDelete(tag.id, tag.name)}>
        <Trash2 size={13}/>
      </button>
    </div>
  );
}

function ColorSwatch({ color, onChange }) {
  const [open, setOpen] = useState(false);
  return (
    <span style={{ position: 'relative', display: 'inline-flex' }}>
      <button type="button" aria-label={t('tags.pick_colour')} className="btn" style={{
        background: color, borderColor: color, width: 28, height: 28, padding: 0, borderRadius: 8,
      }} onClick={() => setOpen(o => !o)}/>
      {open && (
        <span style={{
          position: 'absolute', top: '110%', left: 0, zIndex: 5,
          background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8,
          padding: 8, display: 'grid', gridTemplateColumns: 'repeat(5, 1fr)', gap: 6,
        }}>
          {PALETTE.map(c => (
            <button key={c} type="button" onClick={() => { onChange(c); setOpen(false); }}
              style={{ width: 24, height: 24, padding: 0, borderRadius: 6, border: color === c ? '2px solid var(--text)' : '1px solid var(--border)', background: c, cursor: 'pointer' }}/>
          ))}
        </span>
      )}
    </span>
  );
}
