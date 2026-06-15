import React, { useEffect, useState } from 'react';
import {
  ChevronLeft, Plus, Save, Loader2, AlertCircle, Trash2, Target, X,
} from 'lucide-react';
import { api } from '../lib/api.js';
import { canWrite } from '../lib/roles.js';
import { t } from '../lib/i18n.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2; --warn:#f59e0b; --warn-soft:#fef3c7;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif; min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn { display: inline-flex; align-items: center; gap: 6px; padding: 7px 12px; border-radius: 8px; cursor: pointer; font-size: 13px; font-weight: 500; line-height: 1; background: var(--surface); border: 1px solid var(--border); color: var(--text-2); font-family: inherit; }
  .btn:hover { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .btn:disabled { opacity: .55; cursor: not-allowed; }
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-ghost { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .btn-danger:hover { background: var(--down-soft); color: #b91c1c; border-color: #fecaca; }
  .input, .select { width: 100%; padding: 9px 12px; border-radius: 8px; background: var(--surface); border: 1px solid var(--border); font-size: 13px; color: var(--text); outline: none; font-family: inherit; }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .field { margin-bottom: 12px; }
  .field-label { font-size: 11px; font-weight: 500; color: var(--text-3); text-transform: uppercase; letter-spacing: .04em; display: block; margin-bottom: 5px; }
  .field-hint { font-size: 12px; color: var(--text-3); margin-top: 4px; }
  .row { display: flex; align-items: center; justify-content: space-between; padding: 16px 18px; border-top: 1px solid var(--border); }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 14px; }
  .pill { display:inline-flex; align-items:center; gap:5px; font-size:11px; padding:2px 8px; border-radius:999px; background:var(--surface-2); color:var(--text-2); border:1px solid var(--border); }
  .chk { display:inline-flex; align-items:center; gap:7px; font-size:13px; padding:5px 9px; border-radius:8px; border:1px solid var(--border); cursor:pointer; background:var(--surface); }
  .chk.on { border-color: var(--accent); background: var(--accent-soft); color: var(--accent-2); }
  .mono { font-family: 'JetBrains Mono', ui-monospace, monospace; }
  .budget-track { height: 8px; border-radius: 999px; background: var(--surface-2); overflow: hidden; }
  .budget-fill { height: 100%; border-radius: 999px; }
`;

// Budget remaining → bar color. Green healthy, amber low, red exhausted.
function budgetColor(remaining) {
  if (remaining == null) return 'var(--text-3)';
  if (remaining <= 0) return 'var(--down)';
  if (remaining < 25) return 'var(--warn)';
  return 'var(--up)';
}

const fmtPct = (v, d = 2) => (v == null ? '—' : `${v.toFixed(d)}%`);

const emptyForm = () => ({
  name: '', description: '', sli_kind: 'monitor', monitor_id: '',
  good_metric: '', total_metric: '', labels: '{}',
  objective_pct: 99.9, window_days: 30, enabled: true,
  channel_ids: [], escalation_policy_id: '',
});

export default function Slos({ user }) {
  const writable = canWrite(user);
  const [slos, setSlos] = useState([]);
  const [monitors, setMonitors] = useState([]);
  const [channels, setChannels] = useState([]);
  const [policies, setPolicies] = useState([]);
  const [load, setLoad] = useState(true);
  const [err, setErr] = useState(null);
  const [editing, setEditing] = useState(null); // slo id | 'new' | null

  const reload = async () => {
    setErr(null);
    try {
      const [s, m, c, p] = await Promise.all([
        api.slos.list(),
        api.monitors.list().catch(() => []),
        api.notifications.list().catch(() => []),
        api.escalation.list().catch(() => []),
      ]);
      setSlos(Array.isArray(s) ? s : []);
      setMonitors(Array.isArray(m) ? m : (m?.items || []));
      setChannels(Array.isArray(c) ? c : []);
      setPolicies(Array.isArray(p) ? p : []);
    } catch (e) { setErr(e.message); }
    finally { setLoad(false); }
  };
  useEffect(() => { reload(); }, []);

  const onSaved = () => { setEditing(null); reload(); };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 820, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <Target size={20}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{t('slos.title')}</h1>
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 18px' }}>{t('slos.subtitle')}</p>

        {writable && editing == null && (
          <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 14 }}>
            <button className="btn btn-accent" onClick={() => setEditing('new')}><Plus size={14}/> {t('slos.new')}</button>
          </div>
        )}
        {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        {editing === 'new' && (
          <SloForm monitors={monitors} channels={channels} policies={policies} onSaved={onSaved} onCancel={() => setEditing(null)} setErr={setErr}/>
        )}

        {load ? (
          <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
        ) : slos.length === 0 && editing == null ? (
          <div className="card" style={{ padding: 40, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
            {t('slos.empty')}
          </div>
        ) : (
          <div className="card">
            {slos.map(s => editing === s.id ? (
              <div key={s.id} style={{ padding: 18, borderTop: '1px solid var(--border)' }}>
                <SloForm slo={s} monitors={monitors} channels={channels} policies={policies} onSaved={onSaved} onCancel={() => setEditing(null)} setErr={setErr}/>
              </div>
            ) : (
              <SloRow key={s.id} slo={s} writable={writable} onEdit={() => setEditing(s.id)} onChanged={reload} setErr={setErr}/>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function SloRow({ slo, writable, onEdit, onChanged, setErr }) {
  const [busy, setBusy] = useState(false);
  const remove = async () => {
    if (!window.confirm(t('slos.confirm_delete'))) return;
    setBusy(true); setErr(null);
    try { await api.slos.remove(slo.id); onChanged(); }
    catch (e) { setErr(e.message); setBusy(false); }
  };
  const snap = slo.snapshot || {};
  const remaining = snap.remaining_pct;
  const breaching = snap.breaching;
  return (
    <div className="row" style={{ alignItems: 'flex-start' }}>
      <div style={{ minWidth: 0, flex: 1 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <span style={{ fontSize: 14, fontWeight: 600 }}>{slo.name}</span>
          <span className="pill">{t(`slos.kind.${slo.sli_kind}`)}</span>
          <span className="pill mono">{slo.objective_pct}% / {slo.window_days}d</span>
          {breaching
            ? <span className="pill" style={{ background: 'var(--down-soft)', color: '#b91c1c', borderColor: '#fecaca' }}>{t('slos.breaching')}</span>
            : <span className="pill" style={{ background: 'var(--up-soft)', color: 'var(--accent-2)', borderColor: 'var(--up-soft)' }}>{t('slos.healthy')}</span>}
          {!slo.enabled && <span className="pill" style={{ color: 'var(--warn)' }}>{t('slos.disabled')}</span>}
        </div>
        {slo.description && <div style={{ fontSize: 12.5, color: 'var(--text-2)', marginTop: 4 }}>{slo.description}</div>}

        {/* Error-budget bar */}
        <div style={{ marginTop: 10, maxWidth: 460 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--text-3)', marginBottom: 4 }}>
            <span>{t('slos.budget_remaining')}</span>
            <span className="mono">{fmtPct(remaining, 0)}</span>
          </div>
          <div className="budget-track">
            <div className="budget-fill" style={{
              width: `${remaining == null ? 0 : Math.max(0, Math.min(100, remaining))}%`,
              background: budgetColor(remaining),
            }}/>
          </div>
          <div style={{ display: 'flex', gap: 14, fontSize: 11, color: 'var(--text-3)', marginTop: 6 }} className="mono">
            <span>{t('slos.achieved')}: {fmtPct(snap.achieved_pct, 3)}</span>
            <span>{t('slos.burn_1h')}: {snap.burn_rate_1h == null ? '—' : `${snap.burn_rate_1h.toFixed(1)}x`}</span>
            <span>{(slo.channel_ids || []).length} {t('slos.channels')}</span>
          </div>
        </div>
      </div>
      {writable && (
        <div style={{ display: 'flex', gap: 6 }}>
          <button className="btn btn-ghost" onClick={onEdit} disabled={busy}>{t('common.edit')}</button>
          <button className="btn btn-ghost btn-danger" onClick={remove} disabled={busy}>
            {busy ? <Loader2 size={13}/> : <Trash2 size={13}/>}
          </button>
        </div>
      )}
    </div>
  );
}

function SloForm({ slo, monitors, channels, policies, onSaved, onCancel, setErr }) {
  const [f, setF] = useState(slo ? {
    name: slo.name, description: slo.description || '', sli_kind: slo.sli_kind,
    monitor_id: slo.monitor_id || '', good_metric: slo.good_metric || '', total_metric: slo.total_metric || '',
    labels: JSON.stringify(slo.labels || {}), objective_pct: slo.objective_pct, window_days: slo.window_days,
    enabled: slo.enabled, channel_ids: [...(slo.channel_ids || [])], escalation_policy_id: slo.escalation_policy_id || '',
  } : emptyForm());
  const [busy, setBusy] = useState(false);
  const set = (k, v) => setF(p => ({ ...p, [k]: v }));
  const toggleCh = (id) => set('channel_ids', f.channel_ids.includes(id) ? f.channel_ids.filter(x => x !== id) : [...f.channel_ids, id]);

  const save = async () => {
    setErr(null);
    if (!f.name.trim()) { setErr(t('slos.err_name')); return; }
    let labels;
    try { labels = JSON.parse(f.labels || '{}'); }
    catch { setErr(t('slos.err_labels')); return; }
    if (f.sli_kind === 'monitor' && !f.monitor_id) { setErr(t('slos.err_monitor')); return; }
    if (f.sli_kind === 'metric' && (!f.good_metric.trim() || !f.total_metric.trim())) { setErr(t('slos.err_metric')); return; }
    setBusy(true);
    const body = {
      name: f.name.trim(), description: f.description.trim(), sli_kind: f.sli_kind,
      monitor_id: f.sli_kind === 'monitor' ? f.monitor_id : null,
      good_metric: f.sli_kind === 'metric' ? f.good_metric.trim() : null,
      total_metric: f.sli_kind === 'metric' ? f.total_metric.trim() : null,
      labels, objective_pct: Number(f.objective_pct), window_days: Number(f.window_days),
      enabled: f.enabled, channel_ids: f.channel_ids,
      escalation_policy_id: f.escalation_policy_id || null,
    };
    try {
      if (slo) await api.slos.update(slo.id, body);
      else await api.slos.create(body);
      onSaved();
    } catch (e) { setErr(e.message); setBusy(false); }
  };

  return (
    <div className={slo ? '' : 'card'} style={slo ? {} : { padding: 18, marginBottom: 16 }}>
      <div className="grid2">
        <div className="field">
          <label className="field-label">{t('slos.f.name')}</label>
          <input className="input" value={f.name} onChange={e => set('name', e.target.value)} placeholder={t('slos.f.name_ph')}/>
        </div>
        <div className="field">
          <label className="field-label">{t('slos.f.kind')}</label>
          <select className="select" value={f.sli_kind} onChange={e => set('sli_kind', e.target.value)} disabled={!!slo}>
            <option value="monitor">{t('slos.kind.monitor')}</option>
            <option value="metric">{t('slos.kind.metric')}</option>
          </select>
        </div>
      </div>
      <div className="field">
        <label className="field-label">{t('slos.f.description')} <span style={{ textTransform: 'none', color: 'var(--text-3)' }}>({t('slos.f.optional')})</span></label>
        <input className="input" value={f.description} onChange={e => set('description', e.target.value)} placeholder={t('slos.f.description_ph')}/>
      </div>

      {f.sli_kind === 'monitor' ? (
        <div className="field">
          <label className="field-label">{t('slos.f.monitor')}</label>
          <select className="select" value={f.monitor_id} onChange={e => set('monitor_id', e.target.value)}>
            <option value="">{t('slos.f.pick_monitor')}</option>
            {monitors.map(m => <option key={m.id} value={m.id}>{m.name}</option>)}
          </select>
          <div className="field-hint">{t('slos.f.monitor_hint')}</div>
        </div>
      ) : (
        <>
          <div className="grid2">
            <div className="field">
              <label className="field-label">{t('slos.f.good_metric')}</label>
              <input className="input mono" value={f.good_metric} onChange={e => set('good_metric', e.target.value)} placeholder="requests_success_total"/>
            </div>
            <div className="field">
              <label className="field-label">{t('slos.f.total_metric')}</label>
              <input className="input mono" value={f.total_metric} onChange={e => set('total_metric', e.target.value)} placeholder="requests_total"/>
            </div>
          </div>
          <div className="field">
            <label className="field-label">{t('slos.f.labels')} <span style={{ textTransform: 'none', color: 'var(--text-3)' }}>({t('slos.f.optional')})</span></label>
            <input className="input mono" value={f.labels} onChange={e => set('labels', e.target.value)} placeholder='{"service":"api"}'/>
            <div className="field-hint">{t('slos.f.labels_hint')}</div>
          </div>
        </>
      )}

      <div className="grid2">
        <div className="field">
          <label className="field-label">{t('slos.f.objective')}</label>
          <input className="input mono" type="number" step="0.001" min="0" max="100" value={f.objective_pct} onChange={e => set('objective_pct', e.target.value)}/>
          <div className="field-hint">{t('slos.f.objective_hint')}</div>
        </div>
        <div className="field">
          <label className="field-label">{t('slos.f.window')}</label>
          <input className="input mono" type="number" min="1" max="365" value={f.window_days} onChange={e => set('window_days', e.target.value)}/>
        </div>
      </div>

      <div className="field">
        <label className="field-label">{t('slos.f.channels')}</label>
        {channels.length === 0 ? (
          <div className="field-hint">{t('slos.f.no_channels')}</div>
        ) : (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 7 }}>
            {channels.map(c => (
              <span key={c.id} className={`chk ${f.channel_ids.includes(c.id) ? 'on' : ''}`} onClick={() => toggleCh(c.id)}>
                {f.channel_ids.includes(c.id) ? <X size={12}/> : <Plus size={12}/>} {c.name}
              </span>
            ))}
          </div>
        )}
      </div>
      <div className="field">
        <label className="field-label">{t('slos.f.escalation')}</label>
        <select className="input" value={f.escalation_policy_id} onChange={e => set('escalation_policy_id', e.target.value)}>
          <option value="">{t('slos.f.no_escalation')}</option>
          {policies.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <div className="field-hint">{t('slos.f.escalation_hint')}</div>
      </div>
      <label className="chk" style={{ marginBottom: 14 }}>
        <input type="checkbox" checked={f.enabled} onChange={e => set('enabled', e.target.checked)}/> {t('slos.f.enabled')}
      </label>
      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={save} disabled={busy}>
          {busy ? <><Loader2 size={13}/> {t('common.saving')}</> : <><Save size={13}/> {t('common.save')}</>}
        </button>
      </div>
    </div>
  );
}
