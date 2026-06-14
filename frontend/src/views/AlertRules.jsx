import React, { useEffect, useState } from 'react';
import {
  ChevronLeft, Plus, Save, Loader2, AlertCircle, Trash2, BellRing, X,
} from 'lucide-react';
import { api } from '../lib/api.js';
import { t } from '../lib/i18n.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2; --warn:#f59e0b;
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
  .row { display: flex; align-items: center; justify-content: space-between; padding: 14px 18px; border-top: 1px solid var(--border); }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .grid3 { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 12px; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 14px; }
  .pill { display:inline-flex; align-items:center; gap:5px; font-size:11px; padding:2px 8px; border-radius:999px; background:var(--surface-2); color:var(--text-2); border:1px solid var(--border); }
  .chk { display:inline-flex; align-items:center; gap:7px; font-size:13px; padding:5px 9px; border-radius:8px; border:1px solid var(--border); cursor:pointer; background:var(--surface); }
  .chk.on { border-color: var(--accent); background: var(--accent-soft); color: var(--accent-2); }
  .mono { font-family: 'JetBrains Mono', ui-monospace, monospace; }
`;

// kind → which extra fields apply + the unit shown next to the threshold.
const KINDS = [
  { v: 'error_rate',       unit: 'events', targetLabel: 'Project name', logFields: false },
  { v: 'trace_latency',    unit: 'ms',     targetLabel: 'Service name', logFields: false },
  { v: 'trace_error_rate', unit: '%',      targetLabel: 'Service name', logFields: false },
  { v: 'log_volume',       unit: 'logs',   targetLabel: 'Service name', logFields: true  },
  { v: 'profile_samples',  unit: 'samples', targetLabel: 'Service name', logFields: false },
  { v: 'rum_lcp_p75',      unit: 'ms',      targetLabel: 'App name',     logFields: false },
];
const kindMeta = (v) => KINDS.find(k => k.v === v) || KINDS[0];

const emptyForm = () => ({
  name: '', kind: 'error_rate', target: '', match_text: '', min_level: 0,
  op: 'gt', threshold: 1, window_seconds: 300, for_seconds: 0,
  enabled: true, channel_ids: [],
});

export default function AlertRules() {
  const [rules, setRules] = useState([]);
  const [channels, setChannels] = useState([]);
  const [load, setLoad] = useState(true);
  const [err, setErr] = useState(null);
  const [editing, setEditing] = useState(null); // rule id | 'new' | null

  const reload = async () => {
    setErr(null);
    try {
      const [r, c] = await Promise.all([api.telemetryRules.list(), api.notifications.list()]);
      setRules(Array.isArray(r) ? r : []);
      setChannels(Array.isArray(c) ? c : []);
    } catch (e) { setErr(e.message); }
    finally { setLoad(false); }
  };
  useEffect(() => { reload(); }, []);

  const onSaved = () => { setEditing(null); reload(); };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 760, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 4 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <BellRing size={20}/>
            <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{t('alertrules.title')}</h1>
          </div>
          {editing == null && (
            <button className="btn btn-accent" onClick={() => setEditing('new')}>
              <Plus size={14}/> {t('alertrules.new')}
            </button>
          )}
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 22px' }}>
          {t('alertrules.subtitle')}
        </p>

        {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        {editing === 'new' && (
          <RuleForm channels={channels} onSaved={onSaved} onCancel={() => setEditing(null)} setErr={setErr}/>
        )}

        {load ? (
          <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
        ) : rules.length === 0 && editing == null ? (
          <div className="card" style={{ padding: 40, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
            {t('alertrules.empty')}
          </div>
        ) : (
          <div className="card">
            {rules.map(r => editing === r.id ? (
              <div key={r.id} style={{ padding: 18, borderTop: '1px solid var(--border)' }}>
                <RuleForm rule={r} channels={channels} onSaved={onSaved} onCancel={() => setEditing(null)} setErr={setErr}/>
              </div>
            ) : (
              <RuleRow key={r.id} rule={r} onEdit={() => setEditing(r.id)} onChanged={reload} setErr={setErr}/>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function RuleRow({ rule, onEdit, onChanged, setErr }) {
  const [busy, setBusy] = useState(false);
  const meta = kindMeta(rule.kind);
  const opSym = { gt: '>', lt: '<', gte: '≥', lte: '≤' }[rule.op] || rule.op;
  const remove = async () => {
    if (!window.confirm(t('alertrules.confirm_delete'))) return;
    setBusy(true); setErr(null);
    try { await api.telemetryRules.remove(rule.id); onChanged(); }
    catch (e) { setErr(e.message); setBusy(false); }
  };
  return (
    <div className="row">
      <div style={{ minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 14, fontWeight: 600 }}>{rule.name}</span>
          <span className="pill">{t(`alertrules.kind.${rule.kind}`)}</span>
          {!rule.enabled && <span className="pill" style={{ color: 'var(--warn)' }}>{t('alertrules.disabled')}</span>}
        </div>
        <div className="mono" style={{ fontSize: 12, color: 'var(--text-2)', marginTop: 4 }}>
          {(rule.target || t('alertrules.all'))} · {opSym} {rule.threshold} {meta.unit} · {t('alertrules.window')} {rule.window_seconds}s
          {rule.for_seconds > 0 ? ` · ${t('alertrules.sustain')} ${rule.for_seconds}s` : ''}
          {` · ${(rule.channel_ids || []).length} ${t('alertrules.channels')}`}
        </div>
      </div>
      <div style={{ display: 'flex', gap: 6 }}>
        <button className="btn btn-ghost" onClick={onEdit} disabled={busy}>{t('common.edit')}</button>
        <button className="btn btn-ghost btn-danger" onClick={remove} disabled={busy}>
          {busy ? <Loader2 size={13}/> : <Trash2 size={13}/>}
        </button>
      </div>
    </div>
  );
}

function RuleForm({ rule, channels, onSaved, onCancel, setErr }) {
  const [f, setF] = useState(rule ? {
    name: rule.name, kind: rule.kind, target: rule.target || '', match_text: rule.match_text || '',
    min_level: rule.min_level || 0, op: rule.op, threshold: rule.threshold,
    window_seconds: rule.window_seconds, for_seconds: rule.for_seconds,
    enabled: rule.enabled, channel_ids: [...(rule.channel_ids || [])],
  } : emptyForm());
  const [busy, setBusy] = useState(false);
  const meta = kindMeta(f.kind);
  const set = (k, v) => setF(p => ({ ...p, [k]: v }));
  const toggleCh = (id) => set('channel_ids', f.channel_ids.includes(id) ? f.channel_ids.filter(x => x !== id) : [...f.channel_ids, id]);

  const save = async () => {
    setErr(null);
    if (!f.name.trim()) { setErr(t('alertrules.err_name')); return; }
    setBusy(true);
    const body = {
      name: f.name.trim(), kind: f.kind, target: f.target.trim(), match_text: f.match_text.trim(),
      min_level: Number(f.min_level) || 0, op: f.op, threshold: Number(f.threshold),
      window_seconds: Number(f.window_seconds), for_seconds: Number(f.for_seconds),
      enabled: f.enabled, channel_ids: f.channel_ids,
    };
    try {
      if (rule) await api.telemetryRules.update(rule.id, body);
      else await api.telemetryRules.create(body);
      onSaved();
    } catch (e) { setErr(e.message); setBusy(false); }
  };

  return (
    <div className={rule ? '' : 'card'} style={rule ? {} : { padding: 18, marginBottom: 16 }}>
      <div className="field">
        <label className="field-label">{t('alertrules.f.name')}</label>
        <input className="input" value={f.name} onChange={e => set('name', e.target.value)} placeholder={t('alertrules.f.name_ph')}/>
      </div>
      <div className="grid2">
        <div className="field">
          <label className="field-label">{t('alertrules.f.kind')}</label>
          <select className="select" value={f.kind} onChange={e => set('kind', e.target.value)}>
            {KINDS.map(k => <option key={k.v} value={k.v}>{t(`alertrules.kind.${k.v}`)}</option>)}
          </select>
        </div>
        <div className="field">
          <label className="field-label">{meta.targetLabel} <span style={{ textTransform: 'none', color: 'var(--text-3)' }}>({t('alertrules.f.target_opt')})</span></label>
          <input className="input mono" value={f.target} onChange={e => set('target', e.target.value)} placeholder={t('alertrules.all')}/>
        </div>
      </div>
      <div className="grid3">
        <div className="field">
          <label className="field-label">{t('alertrules.f.op')}</label>
          <select className="select" value={f.op} onChange={e => set('op', e.target.value)}>
            <option value="gt">&gt;</option><option value="gte">≥</option>
            <option value="lt">&lt;</option><option value="lte">≤</option>
          </select>
        </div>
        <div className="field">
          <label className="field-label">{t('alertrules.f.threshold')} ({meta.unit})</label>
          <input className="input mono" type="number" step="any" value={f.threshold} onChange={e => set('threshold', e.target.value)}/>
        </div>
        <div className="field">
          <label className="field-label">{t('alertrules.f.window')}</label>
          <input className="input mono" type="number" min="1" value={f.window_seconds} onChange={e => set('window_seconds', e.target.value)}/>
        </div>
      </div>
      <div className="grid2">
        <div className="field">
          <label className="field-label">{t('alertrules.f.sustain')}</label>
          <input className="input mono" type="number" min="0" value={f.for_seconds} onChange={e => set('for_seconds', e.target.value)}/>
          <div className="field-hint">{t('alertrules.f.sustain_hint')}</div>
        </div>
        {meta.logFields && (
          <div className="field">
            <label className="field-label">{t('alertrules.f.min_level')}</label>
            <input className="input mono" type="number" min="0" max="24" value={f.min_level} onChange={e => set('min_level', e.target.value)}/>
            <div className="field-hint">{t('alertrules.f.min_level_hint')}</div>
          </div>
        )}
      </div>
      {meta.logFields && (
        <div className="field">
          <label className="field-label">{t('alertrules.f.match')} <span style={{ textTransform: 'none', color: 'var(--text-3)' }}>({t('alertrules.f.target_opt')})</span></label>
          <input className="input mono" value={f.match_text} onChange={e => set('match_text', e.target.value)} placeholder={t('alertrules.f.match_ph')}/>
        </div>
      )}
      <div className="field">
        <label className="field-label">{t('alertrules.f.channels')}</label>
        {channels.length === 0 ? (
          <div className="field-hint">{t('alertrules.f.no_channels')}</div>
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
      <label className="chk" style={{ marginBottom: 14 }}>
        <input type="checkbox" checked={f.enabled} onChange={e => set('enabled', e.target.checked)}/> {t('alertrules.f.enabled')}
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
