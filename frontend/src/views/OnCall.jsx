import React, { useState } from 'react';
import {
  Plus, Trash2, AlertCircle, Loader2, X, Pencil, Check,
  CalendarClock, ArrowUp, ArrowDown,
} from 'lucide-react';
import { api, useApi, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import SubViewHeader from '../components/SubViewHeader.jsx';
import { confirmDialog } from '../lib/notify.js';
import { canWrite } from '../lib/roles.js';

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
  .input, .select {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input.mono { font-family: 'JetBrains Mono', monospace; font-size: 12.5px; }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); display: block; margin-bottom: 6px; }
  .field-hint { font-size: 11.5px; color: var(--text-3); margin-top: 6px; }
  .field-err { font-size: 11.5px; color: var(--down); margin-top: 6px; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .row {
    display: grid; grid-template-columns: 1fr auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .pill {
    display: inline-flex; align-items: center;
    font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500;
    background: var(--accent-soft); color: var(--accent-2);
  }
  .ring-row {
    display: flex; align-items: center; gap: 10px;
    border: 1px solid var(--border); border-radius: 10px;
    padding: 8px 12px; background: var(--surface-2);
  }
  .ring-pos {
    display: inline-flex; align-items: center; justify-content: center;
    width: 22px; height: 22px; border-radius: 999px; flex-shrink: 0;
    font-size: 11px; font-weight: 600;
    background: var(--accent-soft); color: var(--accent-2);
  }
`;

const MIN_ROTATION = 300; // 5 minutes — matches the backend floor
const MAX_ROTATION = 31_536_000; // 365 days
const MAX_PARTICIPANTS = 50;

// Cadence units, smallest first. The form edits a number + unit; the API
// speaks seconds, so we convert at the boundary.
const UNITS = [
  { key: 'minutes', secs: 60 },
  { key: 'hours', secs: 3600 },
  { key: 'days', secs: 86400 },
  { key: 'weeks', secs: 604800 },
];
const UNIT_SECS = Object.fromEntries(UNITS.map(u => [u.key, u.secs]));

// Pick the largest unit that divides the cadence evenly, so a weekly
// rotation reads as "1 week" rather than "10080 minutes".
function decomposeCadence(secs) {
  for (let i = UNITS.length - 1; i >= 0; i--) {
    if (secs % UNITS[i].secs === 0) {
      return { value: String(secs / UNITS[i].secs), unit: UNITS[i].key };
    }
  }
  return { value: String(Math.round(secs / 60)), unit: 'minutes' };
}

// `<input type="datetime-local">` has no timezone; convert both ways so the
// value the user sees is local while the wire format stays UTC RFC 3339.
function toLocalInput(d) {
  const tzOffset = d.getTimezoneOffset() * 60000;
  return new Date(d.getTime() - tzOffset).toISOString().slice(0, 16);
}
function fromLocalInput(s) {
  return new Date(s).toISOString();
}
function anchorToDate(anchor) {
  return anchor instanceof Array ? offsetDateTimeArrayToDate(anchor) : new Date(anchor);
}

// Mirror of the backend rotation math (rampart_core::on_call::on_call_index)
// so list rows can show who's on call now without an extra request per row.
function onCallChannelId(schedule, nowMs) {
  const ring = schedule.participant_ids || [];
  const rot = schedule.rotation_seconds || 0;
  if (ring.length === 0 || rot <= 0) return null;
  const elapsed = Math.floor((nowMs - anchorToDate(schedule.anchor).getTime()) / 1000);
  const period = Math.floor(elapsed / rot);
  const idx = ((period % ring.length) + ring.length) % ring.length;
  return ring[idx];
}

function cadenceLabel(secs) {
  const { value, unit } = decomposeCadence(secs);
  return t('oncall.cadence_summary', { value, unit: t(`oncall.unit.${unit}`) });
}

export default function OnCall({ user }) {
  const writable = canWrite(user);
  const [reloadKey, setReloadKey] = useState(0);
  const schedulesState = useApi(() => api.onCallSchedules.list(), [reloadKey]);
  const channelsState = useApi(() => api.notifications.list(), []);
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState(null); // schedule id being edited
  const [busy, setBusy] = useState(null);
  const [err, setErr] = useState(null);

  const reload = () => { setCreating(false); setEditing(null); setReloadKey(k => k + 1); };
  const schedules = schedulesState.data || [];
  const channels = channelsState.data || [];
  const channelName = (id) => (channels.find(c => c.id === id)?.name) || id;
  const now = Date.now();

  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('oncall.delete_confirm') }))) return;
    setBusy(id); setErr(null);
    try { await api.onCallSchedules.remove(id); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 920, margin: '0 auto', padding: '32px 32px 64px' }}>
        <SubViewHeader title={t('oncall.title')} icon={CalendarClock} />

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('oncall.title')}</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('oncall.subtitle')}</p>
          </div>
          {writable && (
            <button className="btn btn-accent" onClick={() => { setEditing(null); setCreating(true); }}>
              <Plus size={14}/> {t('oncall.new')}
            </button>
          )}
        </div>

        {(err || schedulesState.error) && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>
            {err || t('oncall.error')}
            {schedulesState.error && (
              <button className="btn btn-ghost" style={{ marginLeft: 8 }} onClick={reload}>{t('oncall.retry')}</button>
            )}
          </div>
        )}

        {creating && (
          <ScheduleForm channels={channels} onCancel={() => setCreating(false)} onSaved={reload}/>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {schedulesState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16}/> {t('oncall.loading')}
            </div>
          ) : schedules.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <CalendarClock size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('oncall.empty.title')}</div>
              <div style={{ fontSize: 12.5 }}>{t('oncall.empty.cta')}</div>
            </div>
          ) : schedules.map(sc => (
            editing === sc.id ? (
              <div key={sc.id} style={{ padding: 18, borderTop: '1px solid var(--border)' }}>
                <ScheduleForm schedule={sc} channels={channels}
                  onCancel={() => setEditing(null)} onSaved={reload}/>
              </div>
            ) : (
              <div className="row" key={sc.id}>
                <div style={{ minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
                    <span style={{ fontSize: 14, fontWeight: 600 }}>{sc.name}</span>
                    <span className="pill">{t('oncall.participants_count', { n: (sc.participant_ids || []).length })}</span>
                  </div>
                  <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                    {cadenceLabel(sc.rotation_seconds)}
                    {onCallChannelId(sc, now) && (
                      <> · {t('oncall.oncall_now', { name: channelName(onCallChannelId(sc, now)) })}</>
                    )}
                  </div>
                </div>
                {writable && (
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <button className="btn btn-ghost" onClick={() => { setCreating(false); setEditing(sc.id); }} disabled={busy === sc.id}>
                      <Pencil size={13}/> {t('common.edit')}
                    </button>
                    <button className="btn btn-ghost btn-danger" onClick={() => remove(sc.id)} disabled={busy === sc.id}>
                      <Trash2 size={13}/>
                    </button>
                  </div>
                )}
              </div>
            )
          ))}
        </div>
      </div>
    </div>
  );
}

// Shared create / edit form. With `schedule` it PATCHes in place; without,
// it POSTs a new one. The participant ring is ordered (rotation order
// matters), so it's a reorderable list rather than a checkbox grid.
function ScheduleForm({ schedule, channels, onCancel, onSaved }) {
  const editing = !!schedule;
  const initCadence = editing ? decomposeCadence(schedule.rotation_seconds) : { value: '1', unit: 'weeks' };
  const [name, setName] = useState(schedule?.name || '');
  const [cadenceValue, setCadenceValue] = useState(initCadence.value);
  const [cadenceUnit, setCadenceUnit] = useState(initCadence.unit);
  const [anchor, setAnchor] = useState(() =>
    toLocalInput(editing ? anchorToDate(schedule.anchor) : new Date()),
  );
  const [participantIds, setParticipantIds] = useState(() =>
    editing ? [...(schedule.participant_ids || [])] : [],
  );
  const [participantUserIds, setParticipantUserIds] = useState(() =>
    editing ? [...(schedule.participant_user_ids || [])] : [],
  );
  const usersState = useApi(() => api.errorIssues.assignableUsers(), []);
  const users = usersState.data || [];
  const userName = (id) => { const u = users.find(x => x.id === id); return u ? (u.name || u.email) : id; };
  const availableUsers = users.filter(u => !participantUserIds.includes(u.id));
  const addUser = (id) => { if (id) setParticipantUserIds(p => [...p, id]); };
  const removeUser = (id) => setParticipantUserIds(p => p.filter(x => x !== id));
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(null);

  const channelName = (id) => (channels.find(c => c.id === id)?.name) || id;
  const available = channels.filter(c => !participantIds.includes(c.id));

  const addParticipant = (id) => { if (id) setParticipantIds(p => [...p, id]); };
  const removeParticipant = (i) => setParticipantIds(p => p.filter((_, j) => j !== i));
  const move = (i, dir) => setParticipantIds(p => {
    const j = i + dir;
    if (j < 0 || j >= p.length) return p;
    const next = [...p];
    [next[i], next[j]] = [next[j], next[i]];
    return next;
  });

  const rotationSeconds = () => Number(cadenceValue) * (UNIT_SECS[cadenceUnit] || 60);
  const cadenceInvalid = () => {
    const n = Number(cadenceValue);
    if (cadenceValue.trim() === '' || !Number.isInteger(n) || n < 1) return true;
    const secs = rotationSeconds();
    return secs < MIN_ROTATION || secs > MAX_ROTATION;
  };

  const submit = async () => {
    setErr(null);
    if (!name.trim()) { setErr(t('oncall.err_name')); return; }
    if (cadenceInvalid()) { setErr(t('oncall.err_cadence')); return; }
    const total = participantIds.length + participantUserIds.length;
    if (total < 1 || total > MAX_PARTICIPANTS) {
      setErr(t('oncall.err_participants', { max: MAX_PARTICIPANTS })); return;
    }
    if (!anchor || Number.isNaN(new Date(anchor).getTime())) { setErr(t('oncall.err_anchor')); return; }
    const body = {
      name: name.trim(),
      rotation_seconds: rotationSeconds(),
      anchor: fromLocalInput(anchor),
      participant_ids: participantIds,
      participant_user_ids: participantUserIds,
    };
    setBusy(true);
    try {
      if (editing) await api.onCallSchedules.update(schedule.id, body);
      else await api.onCallSchedules.create(body);
      onSaved();
    } catch (e) {
      setErr(e.message || t('oncall.err_save'));
      setBusy(false);
    }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: editing ? 0 : 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>
          {editing ? t('oncall.edit_title') : t('oncall.new')}
        </h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy} aria-label={t('common.cancel')}><X size={14}/></button>
      </div>
      {err && <div className="banner-err">{err}</div>}

      <div style={{ marginBottom: 16 }}>
        <label className="field-label">{t('oncall.field.name')}</label>
        <input className="input" value={name} onChange={e => setName(e.target.value)}
          placeholder={t('oncall.field.name_placeholder')}/>
      </div>

      <div style={{ display: 'flex', gap: 16, marginBottom: 16, flexWrap: 'wrap' }}>
        <div style={{ flex: '1 1 200px' }}>
          <label className="field-label">{t('oncall.field.cadence')}</label>
          <div style={{ display: 'flex', gap: 8 }}>
            <input className="input mono" type="number" min="1" step="1" style={{ width: 90 }}
              value={cadenceValue} onChange={e => setCadenceValue(e.target.value)}/>
            <select className="select" value={cadenceUnit} onChange={e => setCadenceUnit(e.target.value)}>
              {UNITS.map(u => <option key={u.key} value={u.key}>{t(`oncall.unit.${u.key}`)}</option>)}
            </select>
          </div>
          {cadenceInvalid() && <div className="field-err">{t('oncall.err_cadence')}</div>}
          <div className="field-hint">{t('oncall.field.cadence_hint')}</div>
        </div>
        <div style={{ flex: '1 1 200px' }}>
          <label className="field-label">{t('oncall.field.anchor')}</label>
          <input type="datetime-local" className="input" value={anchor}
            onChange={e => setAnchor(e.target.value)}/>
          <div className="field-hint">{t('oncall.field.anchor_hint')}</div>
        </div>
      </div>

      <label className="field-label">{t('oncall.field.participants')}</label>
      {channels.length === 0 ? (
        <div className="field-hint" style={{ marginTop: 0 }}>{t('oncall.participant.empty')}</div>
      ) : (
        <>
          {participantIds.length === 0 ? (
            <div className="field-hint" style={{ marginTop: 0, marginBottom: 8 }}>{t('oncall.participant.none_added')}</div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 10 }}>
              {participantIds.map((id, i) => (
                <div className="ring-row" key={`${id}-${i}`}>
                  <span className="ring-pos">{i + 1}</span>
                  <span style={{ flex: 1, fontSize: 13, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {channelName(id)}
                  </span>
                  <button className="btn btn-ghost" style={{ padding: '4px 6px' }} disabled={busy || i === 0}
                    onClick={() => move(i, -1)} title={t('oncall.move_up')}>
                    <ArrowUp size={13}/>
                  </button>
                  <button className="btn btn-ghost" style={{ padding: '4px 6px' }} disabled={busy || i === participantIds.length - 1}
                    onClick={() => move(i, 1)} title={t('oncall.move_down')}>
                    <ArrowDown size={13}/>
                  </button>
                  <button className="btn btn-ghost btn-danger" style={{ padding: '4px 6px' }} disabled={busy}
                    onClick={() => removeParticipant(i)} title={t('oncall.remove')}>
                    <Trash2 size={13}/>
                  </button>
                </div>
              ))}
            </div>
          )}
          {available.length > 0 && (
            <select className="select" value="" disabled={busy}
              onChange={e => { addParticipant(e.target.value); e.target.value = ''; }}>
              <option value="">{t('oncall.participant.add')}</option>
              {available.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
            </select>
          )}
          <div className="field-hint">{t('oncall.field.participants_hint')}</div>
        </>
      )}

      <div style={{ marginTop: 16 }}>
        <label className="field-label">{t('oncall.field.users')}</label>
        {participantUserIds.length > 0 && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, margin: '6px 0 8px' }}>
            {participantUserIds.map(id => (
              <span key={id} className="pill" style={{ gap: 6 }}>
                {userName(id)}
                <X size={11} style={{ cursor: 'pointer' }} onClick={() => removeUser(id)}/>
              </span>
            ))}
          </div>
        )}
        {availableUsers.length > 0 && (
          <select className="select" value="" disabled={busy}
            onChange={e => { addUser(e.target.value); e.target.value = ''; }}>
            <option value="">{t('oncall.field.add_user')}</option>
            {availableUsers.map(u => <option key={u.id} value={u.id}>{u.name || u.email}</option>)}
          </select>
        )}
        <div className="field-hint">{t('oncall.field.users_hint')}</div>
      </div>

      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 16 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>
          {busy
            ? <><Loader2 size={13}/> {t('common.saving')}</>
            : <><Check size={13}/> {editing ? t('oncall.save') : t('oncall.create')}</>}
        </button>
      </div>
    </div>
  );
}
