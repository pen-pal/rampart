import React, { useMemo, useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, ExternalLink, Save, AlertCircle, Loader2, X, Globe,
} from 'lucide-react';
import { api, useApi } from '../lib/api.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
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
    display: grid; grid-template-columns: 1fr auto auto auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
`;

export default function StatusPageBuilder() {
  const pagesState    = useApi(() => api.statusPages.list(), [], { pollMs: 30_000 });
  const monitorsState = useApi(() => api.monitors.list(), []);

  const [editing, setEditing] = useState(null); // page object or 'new' or null
  const [err,     setErr]     = useState(null);
  const [busy,    setBusy]    = useState(null);

  const reload = () => window.location.reload();

  const remove = async (id) => {
    if (!confirm('Delete this status page? Public URL will stop responding.')) return;
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
          <ChevronLeft size={14}/> Dashboard
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>
              Status pages
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              Public, no-login views of selected monitors. Share the URL with customers or post it in your README.
            </p>
          </div>
          <button className="btn btn-accent" onClick={() => setEditing('new')}>
            <Plus size={14}/> New status page
          </button>
        </div>

        {err && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}
          </div>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {pagesState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16}/> Loading…
            </div>
          ) : pages.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Globe size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>
                No status pages yet
              </div>
              <div style={{ fontSize: 12.5 }}>
                Create one to expose a curated, no-auth view of your monitors.
              </div>
            </div>
          ) : pages.map(p => (
            <PageRow
              key={p.id}
              page={p}
              busy={busy === p.id}
              onEdit={() => setEditing(p)}
              onDelete={() => remove(p.id)}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

function PageRow({ page, busy, onEdit, onDelete }) {
  const url = `${window.location.origin}/#/s/${page.slug}`;
  return (
    <div className="row">
      <div>
        <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 3 }}>{page.title}</div>
        <a href={url} target="_blank" rel="noreferrer" style={{
          fontSize: 11.5, color: 'var(--accent-2)', textDecoration: 'none',
          display: 'inline-flex', alignItems: 'center', gap: 4,
        }}>
          {url} <ExternalLink size={11}/>
        </a>
        <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 4 }}>
          {page.monitor_ids.length} monitor{page.monitor_ids.length === 1 ? '' : 's'} attached
          {page.description && ` · ${page.description}`}
        </div>
      </div>
      <button className="btn btn-ghost" onClick={onEdit} disabled={busy}>Edit</button>
      <button className="btn btn-ghost btn-danger" onClick={onDelete} disabled={busy}><Trash2 size={13}/></button>
      <span/>
    </div>
  );
}

function Editor({ page, monitors, onCancel, onSaved }) {
  const isNew = !page;
  const [slug,        setSlug]        = useState(page?.slug        || '');
  const [title,       setTitle]       = useState(page?.title       || '');
  const [description, setDescription] = useState(page?.description || '');
  const [picked,      setPicked]      = useState(new Set(page?.monitor_ids || []));
  const [err,         setErr]         = useState(null);
  const [saving,      setSaving]      = useState(false);

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
    if (!title.trim())             { setErr('Title is required.'); return; }
    if (isNew && !/^[a-z0-9-]{2,40}$/.test(slug)) {
      setErr('Slug must be 2–40 chars of lowercase letters, digits, or dashes.');
      return;
    }
    setSaving(true);
    try {
      if (isNew) {
        await api.statusPages.create({
          slug:        slug.trim(),
          title:       title.trim(),
          description: description.trim() || null,
          monitor_ids: orderedPicked,
        });
      } else {
        await api.statusPages.update(page.id, {
          title:       title.trim(),
          description: description.trim() || null,
          monitor_ids: orderedPicked,
        });
      }
      onSaved();
    } catch (e) {
      setErr(e.message || 'Failed to save.');
      setSaving(false);
    }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 720, margin: '0 auto', padding: '32px 32px 64px' }}>
        <button className="btn btn-ghost" onClick={onCancel} style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> Back
        </button>

        <h1 style={{ fontSize: 24, fontWeight: 600, margin: '0 0 22px', letterSpacing: '-.02em' }}>
          {isNew ? 'New status page' : `Edit · ${page.title}`}
        </h1>

        {err && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}
          </div>
        )}

        <div className="card" style={{ padding: 22, marginBottom: 18 }}>
          <div className="field">
            <label className="field-label">Slug{!isNew && ' (immutable)'}</label>
            <input
              className="input mono"
              value={slug}
              onChange={e => setSlug(e.target.value.toLowerCase())}
              placeholder="status-public"
              disabled={!isNew}
            />
            <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 4 }}>
              Lowercase, digits, dashes. 2–40 chars.
              {slug && ` Page will be served at ${window.location.origin}/#/s/${slug}`}
            </div>
          </div>

          <div className="field">
            <label className="field-label">Title</label>
            <input className="input" value={title} onChange={e => setTitle(e.target.value)} placeholder="Acme Corp Status"/>
          </div>

          <div className="field">
            <label className="field-label">Description (optional)</label>
            <textarea className="textarea" rows={2} value={description} onChange={e => setDescription(e.target.value)} placeholder="Live status of acme.com services"/>
          </div>
        </div>

        <div className="card" style={{ padding: 22 }}>
          <div className="field-label" style={{ marginBottom: 10 }}>
            Monitors ({picked.size} selected)
          </div>
          <div style={{ maxHeight: 320, overflow: 'auto', border: '1px solid var(--border)', borderRadius: 8, padding: 4 }}>
            {monitors.length === 0 ? (
              <div style={{ padding: 12, fontSize: 12, color: 'var(--text-3)' }}>
                No monitors yet. Create one first.
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
          <button className="btn btn-ghost" onClick={onCancel} disabled={saving}>Cancel</button>
          <button className="btn btn-accent" onClick={save} disabled={saving}>
            {saving ? <><Loader2 size={13}/> Saving…</> : <><Save size={13}/> Save</>}
          </button>
        </div>
      </div>
    </div>
  );
}
