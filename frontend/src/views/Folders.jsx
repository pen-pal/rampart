import React, { useState, useEffect, useMemo } from 'react';
import {
  ChevronLeft, Folder, FolderTree, Plus, Trash2, Loader2, AlertCircle, X,
  Tag as TagIcon, Bell, Pencil, Check, Activity, CornerDownRight,
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
  .chip-mon { background:var(--surface-2); color:var(--text-2); border:1px solid var(--border); }
  .chip-mon button { background:var(--border); color:var(--text-2); }
`;

export default function Folders() {
  const groupsApi   = useApi(() => api.monitorGroups.list(), []);
  const tags        = useApi(() => api.tags.list(), []);
  const channelsApi = useApi(() => api.notifications.list(), []);
  const monitorsApi = useApi(() => api.monitors.list(), []);

  // Lift groups + monitors into local state so rename / reparent / membership
  // edits reflect instantly without a full page reload.
  const [groups, setGroups]     = useState([]);
  const [monitors, setMonitors] = useState([]);
  useEffect(() => { if (groupsApi.data)   setGroups(groupsApi.data); },     [groupsApi.data]);
  useEffect(() => { if (monitorsApi.data) setMonitors(monitorsApi.data); }, [monitorsApi.data]);

  const [newName, setNewName] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(null);

  const tagsById     = new Map((tags.data || []).map(t => [t.id, t]));
  const channelsById = new Map((channelsApi.data || []).map(c => [c.id, c]));

  // Group folders by parent so we can render a tree. Insertion order follows
  // the API's sort_order,name ordering.
  const childrenByParent = useMemo(() => {
    const m = new Map();
    for (const g of groups) {
      const key = g.parent_id || '__root__';
      if (!m.has(key)) m.set(key, []);
      m.get(key).push(g);
    }
    return m;
  }, [groups]);

  // Depth-first flatten so each folder renders once, indented by depth.
  const flat = useMemo(() => {
    const out = [];
    const walk = (key, depth) => {
      for (const g of (childrenByParent.get(key) || [])) {
        out.push({ g, depth });
        walk(g.id, depth + 1);
      }
    };
    walk('__root__', 0);
    return out;
  }, [childrenByParent]);

  const descendantsOf = (id) => {
    const set = new Set();
    const walk = (key) => {
      for (const g of (childrenByParent.get(key) || [])) { set.add(g.id); walk(g.id); }
    };
    walk(id);
    return set;
  };

  const createFolder = async () => {
    if (!newName.trim()) return;
    setBusy(true); setErr(null);
    try {
      const g = await api.monitorGroups.create(newName.trim());
      setGroups(gs => [...gs, g]); setNewName('');
    } catch (e) { setErr(e.message); } finally { setBusy(false); }
  };

  const onRename = async (id, name) => {
    setErr(null);
    try {
      await api.monitorGroups.update(id, { name });
      setGroups(gs => gs.map(g => g.id === id ? { ...g, name } : g));
    } catch (e) { setErr(e.message); throw e; }
  };

  const onReparent = async (id, parentId) => {
    setErr(null);
    try {
      await api.monitorGroups.update(id, { parent_id: parentId }); // null = move to root
      setGroups(gs => gs.map(g => g.id === id ? { ...g, parent_id: parentId } : g));
    } catch (e) { setErr(e.message); }
  };

  const onRemove = async (id, name) => {
    if (!confirm(`Delete folder "${name}"? Monitors and sub-folders are kept (moved to the root).`)) return;
    setErr(null);
    try {
      await api.monitorGroups.remove(id);
      // Backend ON DELETE SET NULL promotes children + demotes monitors to root.
      setGroups(gs => gs.filter(g => g.id !== id).map(g => g.parent_id === id ? { ...g, parent_id: null } : g));
      setMonitors(ms => ms.map(m => m.group_id === id ? { ...m, group_id: null } : m));
    } catch (e) { setErr(e.message); }
  };

  const onMonitorMove = async (monitorId, groupId) => {
    setErr(null);
    try {
      await api.monitors.update(monitorId, { group_id: groupId }); // null detaches
      setMonitors(ms => ms.map(m => m.id === monitorId ? { ...m, group_id: groupId } : m));
    } catch (e) { setErr(e.message); }
  };

  const loading = groupsApi.loading;

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 900, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}><ChevronLeft size={14}/> Dashboard</a>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <FolderTree size={20}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>Folders</h1>
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '4px 0 22px' }}>
          Group monitors into folders, and nest folders inside folders. Add monitors directly to a folder below.
          Tag a folder and a channel with the same tag and the channel auto-routes to every monitor in the folder;
          attach a channel to a folder to cover all its monitors. (Tag/channel routing applies to the folder's own
          monitors, not nested sub-folders.)
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

        {loading && <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>}
        {!loading && groups.length === 0 && (
          <div className="card" style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
            <Folder size={20} style={{ opacity: .4, marginBottom: 8 }}/>
            <div>No folders yet. Create one above.</div>
          </div>
        )}

        {flat.map(({ g, depth }) => {
          const blocked = descendantsOf(g.id); // can't move a folder into itself or a descendant
          const parentOptions = groups.filter(o => o.id !== g.id && !blocked.has(o.id));
          return (
            <div key={g.id} style={{ marginLeft: depth * 26 }}>
              {depth > 0 && (
                <CornerDownRight size={13} color="var(--text-3)"
                  style={{ position: 'absolute', marginLeft: -20, marginTop: 22 }}/>
              )}
              <FolderCard
                group={g} depth={depth}
                monitors={monitors}
                allTags={tags.data || []} tagsById={tagsById}
                allChannels={channelsApi.data || []} channelsById={channelsById}
                parentOptions={parentOptions}
                onRename={onRename} onReparent={onReparent}
                onRemove={onRemove} onMonitorMove={onMonitorMove}
              />
            </div>
          );
        })}
      </div>
      <style>{`@keyframes spin{to{transform:rotate(360deg)}}.spin{animation:spin 1s linear infinite}`}</style>
    </div>
  );
}

function FolderCard({
  group, depth, monitors, allTags, tagsById, allChannels, channelsById,
  parentOptions, onRename, onReparent, onRemove, onMonitorMove,
}) {
  const [tagIds, setTagIds]   = useState(null); // null = loading
  const [chanIds, setChanIds] = useState(null);
  const [busy, setBusy]       = useState(false);
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState(group.name);

  useEffect(() => { setDraftName(group.name); }, [group.name]);

  useEffect(() => {
    let live = true;
    Promise.all([api.monitorGroups.tags(group.id), api.monitorGroups.channels(group.id)])
      .then(([t, c]) => { if (live) { setTagIds(t); setChanIds(c); } })
      .catch(() => { if (live) { setTagIds([]); setChanIds([]); } });
    return () => { live = false; };
  }, [group.id]);

  const members  = monitors.filter(m => m.group_id === group.id);
  const outsiders = monitors.filter(m => m.group_id !== group.id);

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

  const saveName = async () => {
    const n = draftName.trim();
    if (!n || n === group.name) { setEditing(false); setDraftName(group.name); return; }
    setBusy(true);
    try { await onRename(group.id, n); setEditing(false); }
    catch { /* err surfaced by parent */ } finally { setBusy(false); }
  };

  const availTags  = allTags.filter(t => !(tagIds || []).includes(t.id));
  const availChans = allChannels.filter(c => !(chanIds || []).includes(c.id));

  const dot = (s) => ({ up: 'var(--up)', down: 'var(--down)', warn: '#f59e0b', maintenance: '#6366f1' }[s] || 'var(--text-3)');

  return (
    <div className="card" style={{ padding: 18, marginBottom: 12 }}>
      {/* Header: icon · name (editable) · count · parent select · delete */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14, flexWrap: 'wrap' }}>
        <Folder size={16} color="var(--accent-2)"/>
        {editing ? (
          <span style={{ display: 'inline-flex', gap: 6, alignItems: 'center' }}>
            <input className="input" autoFocus value={draftName} onChange={e => setDraftName(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') saveName(); if (e.key === 'Escape') { setEditing(false); setDraftName(group.name); } }}
              style={{ width: 200, padding: '5px 9px', fontSize: 14 }}/>
            <button className="btn btn-accent" style={{ padding: '5px 8px' }} disabled={busy} onClick={saveName}><Check size={13}/></button>
            <button className="btn btn-ghost" style={{ padding: '5px 8px' }} onClick={() => { setEditing(false); setDraftName(group.name); }}><X size={13}/></button>
          </span>
        ) : (
          <>
            <span style={{ fontSize: 15, fontWeight: 600 }}>{group.name}</span>
            <button className="btn btn-ghost" style={{ padding: '4px 6px' }} title="Rename" onClick={() => setEditing(true)}><Pencil size={12}/></button>
          </>
        )}
        <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>{members.length} monitor{members.length === 1 ? '' : 's'}</span>

        <label style={{ marginLeft: 'auto', display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 11.5, color: 'var(--text-3)' }}>
          In
          <select className="select" style={{ width: 'auto', padding: '4px 8px', fontSize: 12 }}
            value={group.parent_id || ''} disabled={busy}
            onChange={e => onReparent(group.id, e.target.value || null)}>
            <option value="">Root</option>
            {parentOptions.map(o => <option key={o.id} value={o.id}>{o.name}</option>)}
          </select>
        </label>
        <button className="btn btn-ghost btn-danger" style={{ padding: '4px 8px' }} onClick={() => onRemove(group.id, group.name)}>
          <Trash2 size={13}/>
        </button>
      </div>

      {/* Monitors in this folder */}
      <div style={{ marginBottom: 14 }}>
        <div className="field-label" style={{ display: 'flex', alignItems: 'center', gap: 5, marginBottom: 7 }}><Activity size={11}/> Monitors</div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
          {members.length === 0 && <span style={{ fontSize: 12, color: 'var(--text-3)' }}>None yet. </span>}
          {members.map(m => (
            <span key={m.id} className="chip chip-mon">
              <span style={{ width: 7, height: 7, borderRadius: '50%', background: dot(m.current_status), flexShrink: 0 }}/>
              {m.name}
              <button title="Remove from folder" onClick={() => onMonitorMove(m.id, null)}><X size={9}/></button>
            </span>
          ))}
          {outsiders.length > 0 && (
            <select className="select" style={{ width: 'auto', padding: '4px 8px', fontSize: 12 }}
              value="" onChange={e => e.target.value && onMonitorMove(e.target.value, group.id)}>
              <option value="">+ add monitor…</option>
              {outsiders.map(m => <option key={m.id} value={m.id}>{m.name}{m.group_id ? ' (move here)' : ''}</option>)}
            </select>
          )}
        </div>
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
