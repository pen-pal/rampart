import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, AlertCircle, Loader2, X, Pencil, Check, BellRing, ArrowDown,
} from 'lucide-react';
import { api, useApi } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { canWrite } from '../lib/roles.js';

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
  .step-card {
    border: 1px solid var(--border); border-radius: 10px;
    padding: 12px 14px; background: var(--surface-2);
  }
`;

const MAX_STEPS = 10;
const MAX_WAIT_MIN = 1440; // 86400s — the backend cap on inter-step waits

// Form-side step shape: { waitMin: string, channelIds: [uuid] }. The API
// speaks seconds; the UI edits minutes, so we convert at the boundary.
const stepsToForm = (steps) =>
  (steps || []).map(s => ({
    waitMin: String(Math.round((s.wait_seconds || 0) / 60)),
    channelIds: [...(s.channel_ids || [])],
  }));

// "3 steps · pages 4 channels" — channel count is the distinct set across
// all steps (a channel paged twice is still one channel).
const uniqueChannelCount = (steps) =>
  new Set((steps || []).flatMap(s => s.channel_ids || [])).size;

export default function Escalations({ user }) {
  const writable = canWrite(user);
  const [reloadKey, setReloadKey] = useState(0);
  const policiesState = useApi(() => api.escalation.list(), [reloadKey]);
  const channelsState = useApi(() => api.notifications.list(), []);
  const [creating, setCreating] = useState(false);
  const [editing,  setEditing]  = useState(null); // policy id being edited
  const [busy,     setBusy]     = useState(null);
  const [err,      setErr]      = useState(null);

  const reload = () => { setCreating(false); setEditing(null); setReloadKey(k => k + 1); };
  const policies = policiesState.data || [];
  const channels = channelsState.data || [];

  const remove = async (id) => {
    if (!confirm(t('esc.delete_confirm'))) return;
    setBusy(id); setErr(null);
    try { await api.escalation.remove(id); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 920, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('esc.back')}
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('esc.title')}</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('esc.subtitle')}</p>
          </div>
          {writable && (
            <button className="btn btn-accent" onClick={() => { setEditing(null); setCreating(true); }}>
              <Plus size={14}/> {t('esc.new')}
            </button>
          )}
        </div>

        {(err || policiesState.error) && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>
            {err || t('esc.error')}
            {policiesState.error && (
              <button className="btn btn-ghost" style={{ marginLeft: 8 }} onClick={reload}>{t('esc.retry')}</button>
            )}
          </div>
        )}

        {creating && (
          <PolicyForm channels={channels} onCancel={() => setCreating(false)} onSaved={reload}/>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {policiesState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16}/> {t('esc.loading')}
            </div>
          ) : policies.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <BellRing size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('esc.empty.title')}</div>
              <div style={{ fontSize: 12.5 }}>{t('esc.empty.cta')}</div>
            </div>
          ) : policies.map(p => (
            editing === p.id ? (
              <div key={p.id} style={{ padding: 18, borderTop: '1px solid var(--border)' }}>
                <PolicyForm policy={p} channels={channels}
                  onCancel={() => setEditing(null)} onSaved={reload}/>
              </div>
            ) : (
              <div className="row" key={p.id}>
                <div style={{ minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
                    <span style={{ fontSize: 14, fontWeight: 600 }}>{p.name}</span>
                    <span className="pill">{t('esc.monitors_count', { n: p.monitor_count ?? 0 })}</span>
                  </div>
                  <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                    {t('esc.steps_summary', { n: (p.steps || []).length, c: uniqueChannelCount(p.steps) })}
                  </div>
                </div>
                {writable && (
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <button className="btn btn-ghost" onClick={() => { setCreating(false); setEditing(p.id); }} disabled={busy === p.id}>
                      <Pencil size={13}/> {t('common.edit')}
                    </button>
                    <button className="btn btn-ghost btn-danger" onClick={() => remove(p.id)} disabled={busy === p.id}>
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

// Shared create / edit form. With `policy` it PATCHes in place; without,
// it POSTs a new one. Steps are edited as ordered rows — step 1 always
// fires immediately (wait locked to 0), later steps wait N minutes after
// the previous step.
function PolicyForm({ policy, channels, onCancel, onSaved }) {
  const editing = !!policy;
  const [name,  setName]  = useState(policy?.name || '');
  const [steps, setSteps] = useState(() =>
    editing ? stepsToForm(policy.steps) : [{ waitMin: '0', channelIds: [] }],
  );
  const [busy, setBusy] = useState(false);
  const [err,  setErr]  = useState(null);

  const patchStep = (i, patch) =>
    setSteps(ss => ss.map((s, j) => (j === i ? { ...s, ...patch } : s)));
  const toggleChannel = (i, id) =>
    patchStep(i, {
      channelIds: steps[i].channelIds.includes(id)
        ? steps[i].channelIds.filter(x => x !== id)
        : [...steps[i].channelIds, id],
    });
  const addStep    = () => setSteps(ss => [...ss, { waitMin: '5', channelIds: [] }]);
  const removeStep = (i) => setSteps(ss => ss.filter((_, j) => j !== i));

  // Per-step wait validity (step 1 is locked to 0 so always valid).
  const waitInvalid = (s, i) => {
    if (i === 0) return false;
    const n = Number(s.waitMin);
    return s.waitMin.trim() === '' || !Number.isInteger(n) || n < 0 || n > MAX_WAIT_MIN;
  };

  const submit = async () => {
    setErr(null);
    if (!name.trim()) { setErr(t('esc.err_name')); return; }
    if (steps.length < 1 || steps.length > MAX_STEPS) { setErr(t('esc.err_steps')); return; }
    for (let i = 0; i < steps.length; i++) {
      if (waitInvalid(steps[i], i)) { setErr(t('esc.err_step_wait', { n: i + 1, max: MAX_WAIT_MIN })); return; }
      if (steps[i].channelIds.length === 0) { setErr(t('esc.err_step_channels', { n: i + 1 })); return; }
    }
    const body = {
      name: name.trim(),
      steps: steps.map((s, i) => ({
        wait_seconds: i === 0 ? 0 : Number(s.waitMin) * 60,
        channel_ids: s.channelIds,
      })),
    };
    setBusy(true);
    try {
      if (editing) await api.escalation.update(policy.id, body);
      else         await api.escalation.create(body);
      onSaved();
    } catch (e) {
      setErr(e.message || t('esc.err_save'));
      setBusy(false);
    }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: editing ? 0 : 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>
          {editing ? t('esc.edit_title') : t('esc.new')}
        </h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}><X size={14}/></button>
      </div>
      {err && <div className="banner-err">{err}</div>}

      <div style={{ marginBottom: 16 }}>
        <label className="field-label">{t('esc.field.name')}</label>
        <input className="input" value={name} onChange={e => setName(e.target.value)}
          placeholder={t('esc.field.name_placeholder')}/>
      </div>

      <label className="field-label">{t('esc.field.steps')}</label>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginBottom: 12 }}>
        {steps.map((s, i) => (
          <React.Fragment key={i}>
            {i > 0 && (
              <div style={{ display: 'flex', justifyContent: 'center', color: 'var(--text-3)' }}>
                <ArrowDown size={13}/>
              </div>
            )}
            <div className="step-card">
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 10 }}>
                <span style={{ fontSize: 12.5, fontWeight: 600 }}>{t('esc.step.n', { n: i + 1 })}</span>
                {i === 0 ? (
                  <span style={{ fontSize: 12, color: 'var(--text-3)' }}>{t('esc.step.immediately')}</span>
                ) : (
                  <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12, color: 'var(--text-2)' }}>
                    {t('esc.step.wait_prefix')}
                    <input className="input mono" type="number" min="0" max={MAX_WAIT_MIN} step="1"
                      value={s.waitMin} onChange={e => patchStep(i, { waitMin: e.target.value })}
                      style={{ width: 90, padding: '5px 8px' }}/>
                    {t('esc.step.wait_suffix')}
                  </span>
                )}
                {steps.length > 1 && (
                  <button className="btn btn-ghost btn-danger" style={{ marginLeft: 'auto', padding: '5px 8px' }}
                    onClick={() => removeStep(i)} title={t('esc.step.remove')} disabled={busy}>
                    <Trash2 size={13}/>
                  </button>
                )}
              </div>
              {channels.length === 0 ? (
                <div className="field-hint" style={{ marginTop: 0 }}>{t('esc.step.channels_empty')}</div>
              ) : (
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px 16px' }}>
                  {channels.map(c => (
                    <label key={c.id} style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 13, cursor: 'pointer' }}>
                      <input type="checkbox" checked={s.channelIds.includes(c.id)} onChange={() => toggleChannel(i, c.id)}/>
                      {c.name}
                    </label>
                  ))}
                </div>
              )}
              {waitInvalid(s, i) && <div className="field-err">{t('esc.err_step_wait', { n: i + 1, max: MAX_WAIT_MIN })}</div>}
              {s.channelIds.length === 0 && channels.length > 0 && (
                <div className="field-err">{t('esc.err_step_channels', { n: i + 1 })}</div>
              )}
            </div>
          </React.Fragment>
        ))}
      </div>

      <button className="btn" onClick={addStep} disabled={busy || steps.length >= MAX_STEPS}>
        <Plus size={13}/> {t('esc.step.add')}
      </button>
      {steps.length >= MAX_STEPS && <div className="field-hint">{t('esc.step.max_hint', { n: MAX_STEPS })}</div>}

      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 16 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>
          {busy
            ? <><Loader2 size={13}/> {t('common.saving')}</>
            : <><Check size={13}/> {editing ? t('esc.save') : t('esc.create')}</>}
        </button>
      </div>
    </div>
  );
}
