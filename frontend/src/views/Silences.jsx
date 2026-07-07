import React, { useState } from 'react';
import { BellOff, Loader2, AlertCircle, X, Plus } from 'lucide-react';
import { api, useApi, formatRelative } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import SubViewHeader from '../components/SubViewHeader.jsx';

const css = `
  .rampart { --bg:#fafaf9; --surface:#fff; --surface-2:#f5f5f4; --border:#e7e5e4; --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e; --accent:#14b8a6; --accent-2:#0d9488; --down:#ef4444;
    background:var(--bg); color:var(--text); font-family:Inter,system-ui,sans-serif; min-height:100vh; }
  .rampart * { box-sizing:border-box; }
  .mono { font-family:'JetBrains Mono',monospace; }
  .card { background:var(--surface); border:1px solid var(--border); border-radius:12px; }
  .btn { display:inline-flex; align-items:center; gap:6px; padding:8px 12px; border-radius:8px; cursor:pointer; font-size:13px; font-weight:500; background:var(--surface); border:1px solid var(--border); color:var(--text-2); font-family:inherit; }
  .btn:hover { background:var(--surface-2); color:var(--text); }
  .btn-ghost { background:transparent; border-color:transparent; }
  .btn-accent { background:var(--accent); border-color:var(--accent); color:#fff; }
  .input, .select { padding:8px 10px; border-radius:8px; background:var(--surface); border:1px solid var(--border); font-size:13px; color:var(--text); outline:none; font-family:inherit; }
  .banner-err { background:#fee2e2; color:#b91c1c; border:1px solid #fecaca; padding:10px 14px; border-radius:8px; font-size:13px; margin-bottom:14px; }
  .pill { display:inline-flex; align-items:center; padding:2px 8px; border-radius:999px; font-size:11px; font-weight:600; background:var(--surface-2); color:var(--text-2); }
`;

const DURATIONS = [
  { v: 15, k: 'silences.dur.15m' },
  { v: 60, k: 'silences.dur.1h' },
  { v: 240, k: 'silences.dur.4h' },
  { v: 1440, k: 'silences.dur.24h' },
  { v: 0, k: 'silences.dur.forever' },
];

export default function Silences() {
  const [monitorId, setMonitorId] = useState('');
  const [duration, setDuration] = useState(60);
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(null);
  const [reloadKey, setReloadKey] = useState(0);

  const monState = useApi(() => api.monitors.list().catch(() => []), []);
  const monitors = monState.data || [];
  const listState = useApi(() => api.silences.list(), [reloadKey]);
  const silences = listState.data || [];

  const add = async () => {
    setErr(null); setBusy(true);
    try {
      await api.silences.create({
        monitor_id: monitorId || null,
        reason: reason.trim(),
        duration_minutes: duration,
      });
      setReason(''); setMonitorId(''); setReloadKey(k => k + 1);
    } catch (e) { setErr(e.message || t('silences.err')); }
    finally { setBusy(false); }
  };
  const remove = async (id) => { try { await api.silences.remove(id); setReloadKey(k => k + 1); } catch { /* ignore */ } };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 880, margin: '0 auto', padding: '32px 32px 64px' }}>
        <SubViewHeader title={t('silences.title')} icon={BellOff} />
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
          <BellOff size={20}/>
          <h1 style={{ fontSize: 28, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{t('silences.title')}</h1>
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 22px' }}>{t('silences.subtitle')}</p>

        <div className="card" style={{ padding: 16, marginBottom: 18, display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
          <select className="select" value={monitorId} onChange={e => setMonitorId(e.target.value)} style={{ minWidth: 180 }}>
            <option value="">{t('silences.all_monitors')}</option>
            {monitors.map(m => <option key={m.id} value={m.id}>{m.name}</option>)}
          </select>
          <select className="select" value={duration} onChange={e => setDuration(Number(e.target.value))}>
            {DURATIONS.map(d => <option key={d.v} value={d.v}>{t(d.k)}</option>)}
          </select>
          <input className="input" style={{ flex: '1 1 180px' }} placeholder={t('silences.reason_ph')}
            value={reason} onChange={e => setReason(e.target.value)} onKeyDown={e => e.key === 'Enter' && add()}/>
          <button className="btn btn-accent" disabled={busy} onClick={add}>
            {busy ? <Loader2 size={14}/> : <Plus size={14}/>} {t('silences.add')}
          </button>
        </div>

        {(err || listState.error) && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err || t('silences.load_error')}</div>}

        <div className="card" style={{ overflow: 'hidden' }}>
          {listState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
          ) : silences.length === 0 ? (
            <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>{t('silences.none')}</div>
          ) : silences.map(s => (
            <div key={s.id} style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '12px 16px', borderTop: '1px solid var(--border)', fontSize: 13 }}>
              <div style={{ flex: 1, minWidth: 0 }}>
                <span className="pill">{s.monitor_name || t('silences.global')}</span>
                {s.reason && <span style={{ marginLeft: 8, color: 'var(--text-2)' }}>{s.reason}</span>}
                <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 3 }}>
                  {s.expires_at ? t('silences.expires', { d: formatRelative(s.expires_at) }) : t('silences.indefinite')}
                </div>
              </div>
              <button className="btn btn-ghost" onClick={() => remove(s.id)} aria-label={t('silences.remove')} title={t('silences.remove')}><X size={14}/></button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
