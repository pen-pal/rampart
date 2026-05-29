import React, { useState, useEffect } from 'react';
import {
  ChevronLeft, Folder, Plus, Trash2, Loader2, AlertCircle, X, Tag as TagIcon, Bell,
} from 'lucide-react';
import { api, useApi } from '../lib/api.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --down:#ef4444; --down-soft:#fee2e2;
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
  .input, .select { width:100%; padding:9px 12px; border-radius:8px; background:var(--surface);
    border:1px solid var(--border); font-size:13px; color:var(--text); outline:none; font-family:inherit; }
  .input:focus { border-color:var(--accent); box-shadow:0 0 0 3px var(--accent-soft); }
  .field-label { font-size:11px; font-weight:500; color:var(--text-3); text-transform:uppercase; letter-spacing:.04em; }
  .banner-err { background:var(--down-soft); color:#b91c1c; border:1px solid #fecaca; padding:10px 14px; border-radius:8px; font-size:13px; }
  .chip { display:inline-flex; align-items:center; gap:5px; font-size:11px; font-weight:500; padding:3px 4px 3px 9px; border-radius:999px; color:#fff; }
  .chip button { background:rgba(255,255,255,.25); border:none; color:#fff; border-radius:50%; width:15px; height:15px; cursor:pointer; display:inline-flex; align-items:center; justify-content:center; }
`;

export default function Folders() {
  const groups   = useApi(() => api.monitorGroups.list(), []);
  const tags     = useApi(() => api.tags.list(), []);
  const channels = useApi(() => api.notifications.list(), []);
  const monitors = useApi(() => api.monitors.list(), []);

  const [newName, setNewName] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(null);

  const reload = () => window.location.reload();

  const createFolder = async () => {
    if (!newName.trim()) return;
    setBusy(true); setErr(null);
    try { await api.monitorGroups.create(newName.trim()); reload(); }
    catch (e) { setErr(e.message); setBusy(false); }
  };
  const tagsById = new Map((tags.data || []).map(t => [t.id, t]));
  const channelsById = new Map((channels.data || []).map(c => [c.id, c]));
  const monitorCount = (gid) => (monitors.data || []).filter(m => m.group_id === gid).length;

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 900, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}><ChevronLeft size={14}/> Dashboard</a>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <Folder size={20}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>Folders</h1>
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '4px 0 22px' }}>
          Group monitors into folders. Tag a folder and tag a channel with the same tag, and that channel
          auto-routes to every monitor in the folder. Attach a channel directly to a folder to cover all its monitors.
        </p>

        {err && <div className="banner-err" style={{ marginBottom: 16 }}><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        {/* Create */}
        <div className="card" style={{ padding: 16, marginBottom: 20, display: 'flex', gap: 8, alignItems: 'center' }}>
          <input className="input" value={newName} onChange={e => setNewName(e.target.value)}
            placeholder="New folder name…" onKeyDown={e => e.key === 'Enter' && createFolder()}/>
          <button className="btn btn-accent" onClick={createFolder} disabled={busy} style={{ flexShrink: 0 }}>
            {busy ? <Loader2 size={13} className="spin"/> : <Plus size={13}/>} Create folder
          </button>
        </div>

        {groups.loading && <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>}
        {!groups.loading && (groups.data || []).length === 0 && (
          <div className="card" style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
            <Folder size={20} style={{ opacity: .4, marginBottom: 8 }}/>
            <div>No folders yet. Create one above.</div>
          </div>
        )}

        {(groups.data || []).map(g => (
          <FolderCard key={g.id} group={g} monitorCount={monitorCount(g.id)}
            allTags={tags.data || []} tagsById={tagsById}
            allChannels={channels.data || []} channelsById={channelsById}/>
        ))}
      </div>
      <style>{`@keyframes spin{to{transform:rotate(360deg)}}.spin{animation:spin 1s linear infinite}`}</style>
    </div>
  );
}

function FolderCard({ group, monitorCount, allTags, tagsById, allChannels, channelsById }) {
  const [tagIds, setTagIds] = useState(null);     // null = loading
  const [chanIds, setChanIds] = useState(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    Promise.all([api.monitorGroups.tags(group.id), api.monitorGroups.channels(group.id)])
      .then(([t, c]) => { if (live) { setTagIds(t); setChanIds(c); } })
      .catch(() => { if (live) { setTagIds([]); setChanIds([]); } });
    return () => { live = false; };
  }, [group.id]);

  const toggleTag = async (tagId) => {
    setBusy(true);
    try {
      if (tagIds.includes(tagId)) { await api.monitorGroups.delTag(group.id, tagId); setTagIds(ids => ids.filter(x => x !== tagId)); }
      else { await api.monitorGroups.addTag(group.id, tagId); setTagIds(ids => [...ids, tagId]); }
    } catch (e) { alert(e.message); } finally { setBusy(false); }
  };
  const toggleChannel = async (notifId) => {
    setBusy(true);
    try {
      if (chanIds.includes(notifId)) { await api.monitorGroups.delChannel(group.id, notifId); setChanIds(ids => ids.filter(x => x !== notifId)); }
      else { await api.monitorGroups.addChannel(group.id, notifId); setChanIds(ids => [...ids, notifId]); }
    } catch (e) { alert(e.message); } finally { setBusy(false); }
  };
  const removeFolder = async () => {
    if (!confirm(`Delete folder "${group.name}"? Monitors are kept (moved to Ungrouped).`)) return;
    try { await api.monitorGroups.remove(group.id); window.location.reload(); } catch (e) { alert(e.message); }
  };

  const availTags = allTags.filter(t => !(tagIds || []).includes(t.id));
  const availChans = allChannels.filter(c => !(chanIds || []).includes(c.id));

  return (
    <div className="card" style={{ padding: 18, marginBottom: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14 }}>
        <Folder size={16} color="var(--accent-2)"/>
        <span style={{ fontSize: 15, fontWeight: 600 }}>{group.name}</span>
        <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>{monitorCount} monitor{monitorCount === 1 ? '' : 's'}</span>
        <button className="btn btn-ghost btn-danger" style={{ marginLeft: 'auto', padding: '4px 8px' }} onClick={removeFolder}>
          <Trash2 size={13}/>
        </button>
      </div>

      {/* Tags */}
      <div style={{ marginBottom: 14 }}>
        <div className="field-label" style={{ display: 'flex', alignItems: 'center', gap: 5, marginBottom: 7 }}><TagIcon size={11}/> Tags</div>
        {tagIds === null ? <span style={{ fontSize: 12, color: 'var(--text-3)' }}>…</span> : (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
            {tagIds.length === 0 && <span style={{ fontSize: 12, color: 'var(--text-3)' }}>No tags. </span>}
            {tagIds.map(id => {
              const t = tagsById.get(id);
              return <span key={id} className="chip" style={{ background: t?.color || '#888' }}>{t?.name || id.slice(0,8)}
                <button disabled={busy} onClick={() => toggleTag(id)}><X size={9}/></button></span>;
            })}
            {availTags.length > 0 && (
              <select className="select" style={{ width: 'auto', padding: '4px 8px', fontSize: 12 }} disabled={busy}
                value="" onChange={e => e.target.value && toggleTag(e.target.value)}>
                <option value="">+ tag…</option>
                {availTags.map(t => <option key={t.id} value={t.id}>{t.name}</option>)}
              </select>
            )}
          </div>
        )}
      </div>

      {/* Folder-level channels */}
      <div>
        <div className="field-label" style={{ display: 'flex', alignItems: 'center', gap: 5, marginBottom: 7 }}><Bell size={11}/> Channels (apply to all monitors in this folder)</div>
        {chanIds === null ? <span style={{ fontSize: 12, color: 'var(--text-3)' }}>…</span> : (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
            {chanIds.length === 0 && <span style={{ fontSize: 12, color: 'var(--text-3)' }}>None. </span>}
            {chanIds.map(id => {
              const c = channelsById.get(id);
              return <span key={id} className="chip" style={{ background: 'var(--accent-2)' }}>{c?.name || id.slice(0,8)}
                <button disabled={busy} onClick={() => toggleChannel(id)}><X size={9}/></button></span>;
            })}
            {availChans.length > 0 && (
              <select className="select" style={{ width: 'auto', padding: '4px 8px', fontSize: 12 }} disabled={busy}
                value="" onChange={e => e.target.value && toggleChannel(e.target.value)}>
                <option value="">+ channel…</option>
                {availChans.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
