import React, { useEffect, useState } from 'react';
import {
  ChevronLeft, Plus, Save, Loader2, AlertCircle, Trash2, ShieldAlert, X, Check,
} from 'lucide-react';
import { api } from '../lib/api.js';
import { canWrite } from '../lib/roles.js';
import { t } from '../lib/i18n.js';
import { formatRelative } from '../lib/api.js';

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
  .tab { padding: 7px 14px; border-radius: 8px; cursor: pointer; font-size: 13px; font-weight: 500; border: 1px solid transparent; background: transparent; color: var(--text-2); }
  .tab.on { background: var(--surface); border-color: var(--border); color: var(--text); }
`;

const SEVERITIES = ['low', 'medium', 'high', 'critical'];

// Severity → pill colours, sharing the existing up/warn/down tokens.
function sevStyle(sev) {
  switch (sev) {
    case 'critical': return { background: 'var(--down-soft)', color: '#b91c1c', borderColor: '#fecaca' };
    case 'high':     return { background: '#ffedd5', color: '#c2410c', borderColor: '#fed7aa' };
    case 'medium':   return { background: 'var(--accent-soft)', color: 'var(--accent-2)', borderColor: 'var(--accent-soft)' };
    default:         return { background: 'var(--surface-2)', color: 'var(--text-2)', borderColor: 'var(--border)' };
  }
}

const emptyForm = () => ({
  name: '', description: '', severity: 'medium', service: '', min_level: 0,
  body_regex: '', attr_key: '', attr_val: '', threshold: 1, window_seconds: 300,
  enabled: true, channel_ids: [],
});

export default function Detection({ user }) {
  const writable = canWrite(user);
  const [tab, setTab] = useState('findings'); // findings | rules
  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 820, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <ShieldAlert size={20}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{t('detection.title')}</h1>
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 18px' }}>{t('detection.subtitle')}</p>

        <div style={{ display: 'flex', gap: 6, marginBottom: 18 }}>
          <button className={`tab ${tab === 'findings' ? 'on' : ''}`} onClick={() => setTab('findings')}>{t('detection.tab_findings')}</button>
          <button className={`tab ${tab === 'rules' ? 'on' : ''}`} onClick={() => setTab('rules')}>{t('detection.tab_rules')}</button>
        </div>

        {tab === 'findings' ? <Findings writable={writable}/> : <Rules writable={writable}/>}
      </div>
    </div>
  );
}

function Findings({ writable }) {
  const [items, setItems] = useState([]);
  const [openOnly, setOpenOnly] = useState(true);
  const [load, setLoad] = useState(true);
  const [err, setErr] = useState(null);
  const [busyId, setBusyId] = useState(null);

  const reload = async () => {
    setErr(null);
    try {
      const r = await api.detection.findings({ open: openOnly, limit: 200 });
      setItems(Array.isArray(r) ? r : []);
    } catch (e) { setErr(e.message); }
    finally { setLoad(false); }
  };
  useEffect(() => { setLoad(true); reload(); }, [openOnly]);

  const ack = async (id) => {
    setBusyId(id); setErr(null);
    try { await api.detection.ackFinding(id); await reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusyId(null); }
  };

  return (
    <>
      {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}
      <label className="chk" style={{ marginBottom: 14 }}>
        <input type="checkbox" checked={openOnly} onChange={e => setOpenOnly(e.target.checked)}/> {t('detection.open_only')}
      </label>
      {load ? (
        <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
      ) : items.length === 0 ? (
        <div className="card" style={{ padding: 40, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
          {t('detection.no_findings')}
        </div>
      ) : (
        <div className="card">
          {items.map(f => (
            <div className="row" key={f.id} style={{ alignItems: 'flex-start' }}>
              <div style={{ minWidth: 0 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                  <span className="pill" style={sevStyle(f.severity)}>{t(`detection.sev.${f.severity}`)}</span>
                  <span style={{ fontSize: 14, fontWeight: 600 }}>{f.rule_name}</span>
                  <span className="pill">{f.match_count} {t('detection.matches')}</span>
                  {f.service && <span className="pill mono">{f.service}</span>}
                  {f.acknowledged_at && <span className="pill" style={{ color: 'var(--up)' }}>{t('detection.acked')}</span>}
                </div>
                {f.sample && (
                  <div className="mono" style={{ fontSize: 12, color: 'var(--text-2)', marginTop: 6, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 560 }}>
                    {f.sample}
                  </div>
                )}
                <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 4 }}>{formatRelative(f.created_at)}</div>
              </div>
              {writable && !f.acknowledged_at && (
                <button className="btn btn-ghost" onClick={() => ack(f.id)} disabled={busyId === f.id}>
                  {busyId === f.id ? <Loader2 size={13}/> : <Check size={13}/>} {t('detection.ack')}
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </>
  );
}

function Rules({ writable }) {
  const [rules, setRules] = useState([]);
  const [channels, setChannels] = useState([]);
  const [load, setLoad] = useState(true);
  const [err, setErr] = useState(null);
  const [editing, setEditing] = useState(null); // rule id | 'new' | null

  const reload = async () => {
    setErr(null);
    try {
      const [r, c] = await Promise.all([api.detection.list(), api.notifications.list()]);
      setRules(Array.isArray(r) ? r : []);
      setChannels(Array.isArray(c) ? c : []);
    } catch (e) { setErr(e.message); }
    finally { setLoad(false); }
  };
  useEffect(() => { reload(); }, []);

  const onSaved = () => { setEditing(null); reload(); };

  return (
    <>
      {writable && editing == null && (
        <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 14 }}>
          <button className="btn btn-accent" onClick={() => setEditing('new')}><Plus size={14}/> {t('detection.new')}</button>
        </div>
      )}
      {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

      {editing === 'new' && (
        <RuleForm channels={channels} onSaved={onSaved} onCancel={() => setEditing(null)} setErr={setErr}/>
      )}

      {load ? (
        <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
      ) : rules.length === 0 && editing == null ? (
        <div className="card" style={{ padding: 40, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
          {t('detection.empty')}
        </div>
      ) : (
        <div className="card">
          {rules.map(r => editing === r.id ? (
            <div key={r.id} style={{ padding: 18, borderTop: '1px solid var(--border)' }}>
              <RuleForm rule={r} channels={channels} onSaved={onSaved} onCancel={() => setEditing(null)} setErr={setErr}/>
            </div>
          ) : (
            <RuleRow key={r.id} rule={r} writable={writable} onEdit={() => setEditing(r.id)} onChanged={reload} setErr={setErr}/>
          ))}
        </div>
      )}
    </>
  );
}

function RuleRow({ rule, writable, onEdit, onChanged, setErr }) {
  const [busy, setBusy] = useState(false);
  const remove = async () => {
    if (!window.confirm(t('detection.confirm_delete'))) return;
    setBusy(true); setErr(null);
    try { await api.detection.remove(rule.id); onChanged(); }
    catch (e) { setErr(e.message); setBusy(false); }
  };
  return (
    <div className="row">
      <div style={{ minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <span className="pill" style={sevStyle(rule.severity)}>{t(`detection.sev.${rule.severity}`)}</span>
          <span style={{ fontSize: 14, fontWeight: 600 }}>{rule.name}</span>
          {!rule.enabled && <span className="pill" style={{ color: 'var(--warn)' }}>{t('detection.disabled')}</span>}
        </div>
        <div className="mono" style={{ fontSize: 12, color: 'var(--text-2)', marginTop: 4 }}>
          {(rule.service || t('detection.any_service'))}
          {rule.min_level > 0 ? ` · ≥sev ${rule.min_level}` : ''}
          {rule.body_regex ? ` · /${rule.body_regex}/` : ''}
          {rule.attr_key ? ` · ${rule.attr_key}=${rule.attr_val}` : ''}
          {` · ≥${rule.threshold} / ${rule.window_seconds}s`}
          {` · ${(rule.channel_ids || []).length} ${t('detection.channels')}`}
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

function RuleForm({ rule, channels, onSaved, onCancel, setErr }) {
  const [f, setF] = useState(rule ? {
    name: rule.name, description: rule.description || '', severity: rule.severity,
    service: rule.service || '', min_level: rule.min_level || 0, body_regex: rule.body_regex || '',
    attr_key: rule.attr_key || '', attr_val: rule.attr_val || '',
    threshold: rule.threshold, window_seconds: rule.window_seconds,
    enabled: rule.enabled, channel_ids: [...(rule.channel_ids || [])],
  } : emptyForm());
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState(null);
  const [previewing, setPreviewing] = useState(false);
  const set = (k, v) => setF(p => ({ ...p, [k]: v }));
  const toggleCh = (id) => set('channel_ids', f.channel_ids.includes(id) ? f.channel_ids.filter(x => x !== id) : [...f.channel_ids, id]);

  const runPreview = async () => {
    setPreviewing(true); setPreview(null); setErr(null);
    try {
      const r = await api.detection.preview({
        service: f.service.trim(), min_level: Number(f.min_level) || 0,
        body_regex: f.body_regex.trim(), attr_key: f.attr_key.trim(), attr_val: f.attr_val.trim(),
        window_seconds: Number(f.window_seconds) || 300,
      });
      setPreview(r);
    } catch (e) { setErr(e.message); }
    finally { setPreviewing(false); }
  };

  const save = async () => {
    setErr(null);
    if (!f.name.trim()) { setErr(t('detection.err_name')); return; }
    setBusy(true);
    const body = {
      name: f.name.trim(), description: f.description.trim(), severity: f.severity,
      service: f.service.trim(), min_level: Number(f.min_level) || 0, body_regex: f.body_regex.trim(),
      attr_key: f.attr_key.trim(), attr_val: f.attr_val.trim(),
      threshold: Number(f.threshold), window_seconds: Number(f.window_seconds),
      enabled: f.enabled, channel_ids: f.channel_ids,
    };
    try {
      if (rule) await api.detection.update(rule.id, body);
      else await api.detection.create(body);
      onSaved();
    } catch (e) { setErr(e.message); setBusy(false); }
  };

  return (
    <div className={rule ? '' : 'card'} style={rule ? {} : { padding: 18, marginBottom: 16 }}>
      <div className="grid2">
        <div className="field">
          <label className="field-label">{t('detection.f.name')}</label>
          <input className="input" value={f.name} onChange={e => set('name', e.target.value)} placeholder={t('detection.f.name_ph')}/>
        </div>
        <div className="field">
          <label className="field-label">{t('detection.f.severity')}</label>
          <select className="select" value={f.severity} onChange={e => set('severity', e.target.value)}>
            {SEVERITIES.map(s => <option key={s} value={s}>{t(`detection.sev.${s}`)}</option>)}
          </select>
        </div>
      </div>
      <div className="field">
        <label className="field-label">{t('detection.f.description')} <span style={{ textTransform: 'none', color: 'var(--text-3)' }}>({t('detection.f.optional')})</span></label>
        <input className="input" value={f.description} onChange={e => set('description', e.target.value)} placeholder={t('detection.f.description_ph')}/>
      </div>
      <div className="grid2">
        <div className="field">
          <label className="field-label">{t('detection.f.service')} <span style={{ textTransform: 'none', color: 'var(--text-3)' }}>({t('detection.f.optional')})</span></label>
          <input className="input mono" value={f.service} onChange={e => set('service', e.target.value)} placeholder={t('detection.any_service')}/>
        </div>
        <div className="field">
          <label className="field-label">{t('detection.f.min_level')}</label>
          <input className="input mono" type="number" min="0" max="24" value={f.min_level} onChange={e => set('min_level', e.target.value)}/>
          <div className="field-hint">{t('detection.f.min_level_hint')}</div>
        </div>
      </div>
      <div className="field">
        <label className="field-label">{t('detection.f.body_regex')} <span style={{ textTransform: 'none', color: 'var(--text-3)' }}>({t('detection.f.optional')})</span></label>
        <input className="input mono" value={f.body_regex} onChange={e => set('body_regex', e.target.value)} placeholder={t('detection.f.body_regex_ph')}/>
        <div className="field-hint">{t('detection.f.body_regex_hint')}</div>
      </div>
      <div className="grid2">
        <div className="field">
          <label className="field-label">{t('detection.f.attr_key')} <span style={{ textTransform: 'none', color: 'var(--text-3)' }}>({t('detection.f.optional')})</span></label>
          <input className="input mono" value={f.attr_key} onChange={e => set('attr_key', e.target.value)} placeholder={t('detection.f.attr_key_ph')}/>
        </div>
        <div className="field">
          <label className="field-label">{t('detection.f.attr_val')}</label>
          <input className="input mono" value={f.attr_val} onChange={e => set('attr_val', e.target.value)} placeholder={t('detection.f.attr_val_ph')}/>
        </div>
      </div>
      <div className="grid2">
        <div className="field">
          <label className="field-label">{t('detection.f.threshold')}</label>
          <input className="input mono" type="number" min="1" value={f.threshold} onChange={e => set('threshold', e.target.value)}/>
          <div className="field-hint">{t('detection.f.threshold_hint')}</div>
        </div>
        <div className="field">
          <label className="field-label">{t('detection.f.window')}</label>
          <input className="input mono" type="number" min="1" value={f.window_seconds} onChange={e => set('window_seconds', e.target.value)}/>
        </div>
      </div>
      <div className="field">
        <label className="field-label">{t('detection.f.channels')}</label>
        {channels.length === 0 ? (
          <div className="field-hint">{t('detection.f.no_channels')}</div>
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
        <input type="checkbox" checked={f.enabled} onChange={e => set('enabled', e.target.checked)}/> {t('detection.f.enabled')}
      </label>
      {preview && (
        <div className="card" style={{ padding: 12, marginBottom: 14, background: 'var(--surface-2)' }}>
          <div style={{ fontSize: 13, fontWeight: 600, marginBottom: preview.samples.length ? 8 : 0 }}>
            {t('detection.preview_result', { n: preview.count, w: Number(f.window_seconds) || 300 })}
          </div>
          {preview.samples.map((s, i) => (
            <div key={i} className="mono" style={{ fontSize: 11.5, color: 'var(--text-2)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{s}</div>
          ))}
        </div>
      )}
      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        <button className="btn btn-ghost" onClick={runPreview} disabled={busy || previewing}>
          {previewing ? <Loader2 size={13}/> : null} {t('detection.preview')}
        </button>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={save} disabled={busy}>
          {busy ? <><Loader2 size={13}/> {t('common.saving')}</> : <><Save size={13}/> {t('common.save')}</>}
        </button>
      </div>
    </div>
  );
}
