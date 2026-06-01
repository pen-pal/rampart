import React, { useMemo, useState } from 'react';
import {
  Calendar, Plus, Trash2, ChevronLeft, Loader2, AlertCircle,
  Pause, Play, X, Pencil, Check,
} from 'lucide-react';
import { api, useApi, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';

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
  .input, .select, .textarea {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input:focus, .select:focus, .textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .row {
    display: grid; grid-template-columns: 1fr auto auto auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .pill {
    display: inline-flex; align-items: center; gap: 4px;
    font-size: 10.5px; padding: 2px 7px; border-radius: 999px; font-weight: 500;
  }
  .pill-active   { background: var(--accent-soft); color: var(--accent-2); }
  .pill-paused   { background: var(--surface-2);   color: var(--text-2); }
  .pill-upcoming { background: var(--warn-soft);   color: #92400e; }
  .pill-past     { background: var(--surface-2);   color: var(--text-3); }
`;

// State machine: when is *right now* vs start/end?
function windowState(w) {
  if (!w.active) return 'paused';
  const now = new Date();
  const start = offsetDateTimeArrayToDate(w.start_at);
  const end   = offsetDateTimeArrayToDate(w.end_at);
  if (now < start) return 'upcoming';
  if (now > end)   return 'past';
  return 'active';
}

// `<input type="datetime-local">` doesn't speak ISO with timezone; we
// convert both directions ourselves to keep the wire format honest.
function toLocalInput(d) {
  const tzOffset = d.getTimezoneOffset() * 60000;
  return new Date(d.getTime() - tzOffset).toISOString().slice(0, 16);
}
function fromLocalInput(s) {
  // Treats the value as local time and produces an ISO string with the
  // user's offset baked in. The backend stores TIMESTAMPTZ so this round
  // trips correctly.
  return new Date(s).toISOString();
}

function buildRecurrence(kind, weekdays, untilStr) {
  if (kind === 'none') return { kind: 'none' };
  const until = untilStr ? fromLocalInput(untilStr) : undefined;
  if (kind === 'daily')  return until ? { kind: 'daily',  until } : { kind: 'daily' };
  if (kind === 'weekly') {
    const ws = Array.from(weekdays).sort((a,b) => a - b);
    return until ? { kind: 'weekly', weekdays: ws, until }
                 : { kind: 'weekly', weekdays: ws };
  }
  return { kind: 'none' };
}

function describeRecurrence(r) {
  if (!r || r.kind === 'none') return 'Once';
  const names = ['Sun','Mon','Tue','Wed','Thu','Fri','Sat'];
  // `until` comes back as a `time::OffsetDateTime` — serde renders that
  // as a 9-tuple [year, ordinal-day, hour, minute, …]. Use the same
  // helper the rest of the page uses for start/end_at.
  const untilDate = r.until ? (offsetDateTimeArrayToDate(r.until) || new Date(r.until)) : null;
  const tail = untilDate ? ` · until ${untilDate.toLocaleDateString()}` : '';
  if (r.kind === 'daily') return `Daily${tail}`;
  if (r.kind === 'weekly') {
    const ws = (r.weekdays || []).map(i => names[i] ?? '?').join(' ');
    return `Weekly · ${ws || '—'}${tail}`;
  }
  return 'Once';
}

export default function Maintenance() {
  const windowsState = useApi(() => api.maintenance.list(), [], { pollMs: 30_000 });
  const monitorsState = useApi(() => api.monitors.list(), []);

  const [showForm, setShowForm] = useState(false);
  const [err,      setErr]      = useState(null);
  const [busy,     setBusy]     = useState(null); // id being mutated

  const monitorsById = useMemo(() => {
    const m = {};
    (monitorsState.data || []).forEach(x => { m[x.id] = x; });
    return m;
  }, [monitorsState.data]);

  // Same trick the Notifications view uses: full-page reload after a
  // mutation. The view is light and React Query / SWR would be overkill
  // for tens of windows.
  const reload = () => window.location.reload();

  const remove = async (id) => {
    if (!confirm('Delete this maintenance window?')) return;
    setBusy(id); setErr(null);
    try {
      await api.maintenance.remove(id);
      reload();
    } catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const togglePause = async (w) => {
    setBusy(w.id); setErr(null);
    try {
      await api.maintenance.setActive(w.id, !w.active);
      reload();
    } catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const rename = async (w, name) => {
    setBusy(w.id); setErr(null);
    try {
      await api.maintenance.update(w.id, { name });
      reload();
    } catch (e) { setErr(e.message); throw e; }
    finally { setBusy(null); }
  };

  const onCreate = () => {
    setShowForm(false);
    reload();
  };

  const windows = windowsState.data || [];

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
              Maintenance windows
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              Suppress checks + alerts during planned downtime. Heartbeats inside a window are recorded as "in maintenance".
            </p>
          </div>
          <button className="btn btn-accent" onClick={() => setShowForm(true)}>
            <Plus size={14}/> New window
          </button>
        </div>

        {err && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>
            {err}
          </div>
        )}

        {showForm && (
          <CreateForm
            monitors={monitorsState.data || []}
            onCancel={() => setShowForm(false)}
            onCreated={onCreate}
          />
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {windowsState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16} className="spin"/> Loading…
            </div>
          ) : windows.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Calendar size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>No maintenance windows yet</div>
              <div style={{ fontSize: 12.5 }}>Create one to suppress alerts during planned downtime.</div>
            </div>
          ) : windows.map(w => (
            <WindowRow
              key={w.id}
              w={w}
              monitorsById={monitorsById}
              busy={busy === w.id}
              onTogglePause={() => togglePause(w)}
              onRename={name => rename(w, name)}
              onDelete={() => remove(w.id)}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

function WindowRow({ w, monitorsById, busy, onTogglePause, onRename, onDelete }) {
  const state = windowState(w);
  const startDate = offsetDateTimeArrayToDate(w.start_at);
  const endDate   = offsetDateTimeArrayToDate(w.end_at);
  const fmt = d => d.toLocaleString(undefined, {
    month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
  });

  const [editing, setEditing] = useState(false);
  const [draft,   setDraft]   = useState(w.name);
  React.useEffect(() => { setDraft(w.name); }, [w.name]);
  const saveName = async () => {
    const n = draft.trim();
    if (!n || n === w.name) { setEditing(false); setDraft(w.name); return; }
    try { await onRename(n); setEditing(false); }
    catch { /* err surfaced by parent */ }
  };

  return (
    <div className="row">
      <div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
          {editing ? (
            <>
              <input className="input" autoFocus value={draft} onChange={e => setDraft(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter') saveName(); if (e.key === 'Escape') { setEditing(false); setDraft(w.name); } }}
                style={{ width: 240, fontSize: 13, padding: '4px 8px' }}/>
              <button className="btn btn-accent" disabled={busy} onClick={saveName} style={{ padding: '4px 8px' }}><Check size={12}/></button>
              <button className="btn btn-ghost" onClick={() => { setEditing(false); setDraft(w.name); }} style={{ padding: '4px 8px' }}><X size={12}/></button>
            </>
          ) : (
            <>
              <span style={{ fontSize: 13.5, fontWeight: 600 }}>{w.name}</span>
              <button className="btn btn-ghost" style={{ padding: '2px 5px' }} title="Rename" onClick={() => setEditing(true)}>
                <Pencil size={11}/>
              </button>
            </>
          )}
          <span className={`pill pill-${state}`}>{state}</span>
        </div>
        <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginBottom: 4 }}>
          {fmt(startDate)} → {fmt(endDate)}
          {state === 'upcoming' && ` · starts ${formatRelative(startDate)}`}
          {state === 'active'   && ` · ends ${formatRelative(endDate)}`}
          {' · '}<span style={{ color: 'var(--accent-2)' }}>{describeRecurrence(w.recurrence)}</span>
        </div>
        <div style={{ fontSize: 11.5, color: 'var(--text-2)' }}>
          {w.monitor_ids.length === 0
            ? <em style={{ color: 'var(--text-3)' }}>no monitors attached</em>
            : w.monitor_ids
                .map(id => monitorsById[id]?.name || id.slice(0, 8))
                .join(' · ')}
        </div>
      </div>
      <button className="btn btn-ghost" onClick={onTogglePause} disabled={busy} title={w.active ? 'Pause' : 'Resume'}>
        {w.active ? <Pause size={13}/> : <Play size={13}/>}
        {w.active ? 'Pause' : 'Resume'}
      </button>
      <button className="btn btn-ghost btn-danger" onClick={onDelete} disabled={busy}>
        <Trash2 size={13}/>
      </button>
      <span/>
    </div>
  );
}

function CreateForm({ monitors, onCancel, onCreated }) {
  // Defaults: now + 1h start, 2h duration. Most maintenance is short.
  const initial = useMemo(() => {
    const now = new Date();
    const start = new Date(now.getTime() + 60 * 60_000);
    const end   = new Date(start.getTime() + 2 * 60 * 60_000);
    return {
      name:   '',
      desc:   '',
      start:  toLocalInput(start),
      end:    toLocalInput(end),
      picked: new Set(),
    };
  }, []);

  const [name,  setName]  = useState(initial.name);
  const [desc,  setDesc]  = useState(initial.desc);
  const [start, setStart] = useState(initial.start);
  const [end,   setEnd]   = useState(initial.end);
  const [picked, setPicked] = useState(initial.picked);
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState(null);
  // Recurrence: 'none' | 'daily' | 'weekly'. Weekdays only meaningful
  // for weekly; until is optional in both repeat modes.
  const [recurKind, setRecurKind] = useState('none');
  const [weekdays,  setWeekdays]  = useState(new Set([1,2,3,4,5])); // Mon-Fri
  const [until,     setUntil]     = useState(''); // datetime-local string

  const toggle = (id) => {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id); else next.add(id);
    setPicked(next);
  };

  const submit = async () => {
    setErr(null);
    if (!name.trim()) { setErr('Name is required.'); return; }
    if (new Date(start) >= new Date(end)) { setErr('End time must be after the start time.'); return; }

    setSubmitting(true);
    try {
      const recurrence = buildRecurrence(recurKind, weekdays, until);
      await api.maintenance.create({
        name:        name.trim(),
        description: desc.trim() || null,
        start_at:    fromLocalInput(start),
        end_at:      fromLocalInput(end),
        monitor_ids: Array.from(picked),
        recurrence,
      });
      onCreated();
    } catch (e) {
      setErr(e.message || 'Failed to create window.');
      setSubmitting(false);
    }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>New maintenance window</h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={submitting}><X size={14}/></button>
      </div>

      {err && (
        <div className="banner-err" style={{ marginBottom: 14 }}>
          <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}
        </div>
      )}

      <div className="field">
        <label className="field-label">Name</label>
        <input className="input" value={name} onChange={e => setName(e.target.value)} placeholder="Friday DB upgrade"/>
      </div>

      <div className="field">
        <label className="field-label">Description (optional)</label>
        <textarea className="textarea" value={desc} onChange={e => setDesc(e.target.value)} rows={2} placeholder="Postgres 17 in-place upgrade — expect ~15min downtime"/>
      </div>

      <div className="form-2col" style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14, marginBottom: 14 }}>
        <div className="field" style={{ marginBottom: 0 }}>
          <label className="field-label">Start</label>
          <input type="datetime-local" className="input" value={start} onChange={e => setStart(e.target.value)}/>
        </div>
        <div className="field" style={{ marginBottom: 0 }}>
          <label className="field-label">End</label>
          <input type="datetime-local" className="input" value={end} onChange={e => setEnd(e.target.value)}/>
        </div>
      </div>

      <div className="field">
        <label className="field-label">Repeats</label>
        <select className="input" value={recurKind} onChange={e => setRecurKind(e.target.value)}>
          <option value="none">Once · single-shot</option>
          <option value="daily">Daily · same time every day</option>
          <option value="weekly">Weekly · pick weekdays</option>
        </select>
        <div className="field-hint" style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 6 }}>
          The Start / End times above define the time-of-day window when repeating.
          Date portion only matters for the first occurrence.
        </div>
      </div>

      {recurKind === 'weekly' && (
        <div className="field">
          <label className="field-label">Weekdays</label>
          <div style={{ display: 'flex', gap: 6 }}>
            {['Sun','Mon','Tue','Wed','Thu','Fri','Sat'].map((d, i) => (
              <button key={d} type="button"
                className="btn"
                onClick={() => {
                  const next = new Set(weekdays);
                  if (next.has(i)) next.delete(i); else next.add(i);
                  setWeekdays(next);
                }}
                style={{
                  background: weekdays.has(i) ? 'var(--accent)' : 'var(--surface)',
                  color: weekdays.has(i) ? 'white' : 'var(--text-2)',
                  borderColor: weekdays.has(i) ? 'var(--accent)' : 'var(--border)',
                  padding: '6px 10px',
                }}>
                {d}
              </button>
            ))}
          </div>
        </div>
      )}

      {recurKind !== 'none' && (
        <div className="field">
          <label className="field-label">Repeat until (optional)</label>
          <input type="datetime-local" className="input" value={until} onChange={e => setUntil(e.target.value)}/>
          <div className="field-hint" style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 6 }}>
            Leave blank to repeat indefinitely.
          </div>
        </div>
      )}

      <div className="field">
        <label className="field-label">Monitors ({picked.size} selected)</label>
        <div style={{ maxHeight: 200, overflow: 'auto', border: '1px solid var(--border)', borderRadius: 8, padding: 4 }}>
          {monitors.length === 0 ? (
            <div style={{ padding: 12, fontSize: 12, color: 'var(--text-3)' }}>No monitors yet — create one first.</div>
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

      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 8 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={submitting}>Cancel</button>
        <button className="btn btn-accent" onClick={submit} disabled={submitting}>
          {submitting ? <><Loader2 size={13} className="spin"/> Creating…</> : <><Plus size={13}/> Create window</>}
        </button>
      </div>
    </div>
  );
}
