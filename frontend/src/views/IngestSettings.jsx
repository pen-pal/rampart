import React, { useEffect, useState } from 'react';
import {
  Save, Loader2, AlertCircle, KeyRound, Eye, EyeOff, Share2, Filter,
} from 'lucide-react';
import { api } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import SubViewHeader from '../components/SubViewHeader.jsx';

const css = `
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
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
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .input {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .field { margin-bottom: 14px; }
  .field-label { font-size: 11px; font-weight: 500; color: var(--text-3); text-transform: uppercase; letter-spacing: .04em; display: block; margin-bottom: 5px; }
  .field-hint { font-size: 12px; color: var(--text-3); margin-top: 4px; line-height: 1.5; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 14px; }
  .banner-ok  { background: var(--up-soft);   color: #047857; border: 1px solid #a7f3d0; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 14px; }
  .mono { font-family: 'JetBrains Mono', ui-monospace, monospace; }
`;

export default function IngestSettings() {
  const [token, setToken] = useState('');
  const [reveal, setReveal] = useState(false);
  const [busy, setBusy] = useState(false);
  const [load, setLoad] = useState(true);
  const [err,  setErr]  = useState(null);
  const [ok,   setOk]   = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const r = await api.telemetryToken.get();
        if (r && typeof r === 'object') setToken(r.token || '');
      } catch (e) { setErr(e.message); }
      finally { setLoad(false); }
    })();
  }, []);

  const save = async () => {
    setErr(null); setOk(false); setBusy(true);
    try {
      await api.telemetryToken.put(token.trim());
      setOk(true);
    } catch (e) { setErr(e.message || t('settings.ingest.err_save')); }
    finally { setBusy(false); }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 640, margin: '0 auto', padding: '32px 32px 64px' }}>
        <SubViewHeader title={t('settings.ingest.title')} icon={KeyRound} />
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <KeyRound size={20}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{t('settings.ingest.title')}</h1>
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 22px' }}>
          {t('settings.ingest.subtitle')}
        </p>

        {load ? (
          <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
        ) : (
          <div className="card" style={{ padding: 22 }}>
            {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}
            {ok  && <div className="banner-ok">{t('settings.ingest.saved')}</div>}

            <div className="field">
              <label className="field-label">{t('settings.ingest.token_label')}</label>
              <div style={{ display: 'flex', gap: 8 }}>
                <input
                  className="input mono"
                  type={reveal ? 'text' : 'password'}
                  autoComplete="off"
                  placeholder={t('settings.ingest.token_placeholder')}
                  value={token}
                  onChange={e => setToken(e.target.value)}
                />
                <button className="btn" type="button" onClick={() => setReveal(r => !r)} title={reveal ? t('settings.ingest.hide') : t('settings.ingest.show')}>
                  {reveal ? <EyeOff size={14}/> : <Eye size={14}/>}
                </button>
              </div>
              <div className="field-hint">
                {t('settings.ingest.token_hint')}
              </div>
            </div>

            <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
              <button className="btn btn-accent" onClick={save} disabled={busy}>
                {busy ? <><Loader2 size={13}/> {t('common.saving')}</> : <><Save size={13}/> {t('common.save')}</>}
              </button>
            </div>
          </div>
        )}

        <SiemExport />
        <Sampling />
      </div>
    </div>
  );
}

// Ingest head-sampling — keep only a percentage of high-volume traces/logs at
// the door, to cap storage. 100 = keep everything (off).
function Sampling() {
  const [cfg, setCfg] = useState({ traces_pct: 100, logs_pct: 100 });
  const [busy, setBusy] = useState(false);
  const [ok, setOk] = useState(false);
  const [err, setErr] = useState(null);

  useEffect(() => {
    (async () => {
      try {
        const r = await api.ingestSampling.get();
        if (r) setCfg({ traces_pct: Number(r.traces_pct ?? 100), logs_pct: Number(r.logs_pct ?? 100) });
      } catch { /* default off */ }
    })();
  }, []);

  const save = async () => {
    setErr(null); setOk(false);
    const tp = parseInt(cfg.traces_pct, 10); const lp = parseInt(cfg.logs_pct, 10);
    if ([tp, lp].some(p => Number.isNaN(p) || p < 0 || p > 100)) {
      setErr(t('settings.sampling.err_range')); return;
    }
    setBusy(true);
    try { await api.ingestSampling.put({ traces_pct: tp, logs_pct: lp }); setOk(true); }
    catch (e) { setErr(e.message || t('settings.ingest.err_save')); }
    finally { setBusy(false); }
  };

  return (
    <div className="card" style={{ padding: 22, marginTop: 18 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
        <Filter size={16}/>
        <h2 style={{ fontSize: 16, fontWeight: 600, margin: 0 }}>{t('settings.sampling.title')}</h2>
      </div>
      <p style={{ fontSize: 12.5, color: 'var(--text-2)', margin: '0 0 14px' }}>{t('settings.sampling.subtitle')}</p>
      {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}
      {ok && <div className="banner-ok">{t('settings.ingest.saved')}</div>}

      <div className="field">
        <label className="field-label">{t('settings.sampling.traces')}</label>
        <input className="input mono" type="number" min="0" max="100"
          value={cfg.traces_pct} onChange={e => setCfg({ ...cfg, traces_pct: e.target.value })}/>
      </div>
      <div className="field">
        <label className="field-label">{t('settings.sampling.logs')}</label>
        <input className="input mono" type="number" min="0" max="100"
          value={cfg.logs_pct} onChange={e => setCfg({ ...cfg, logs_pct: e.target.value })}/>
        <div className="field-hint">{t('settings.sampling.hint')}</div>
      </div>
      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
        <button className="btn btn-accent" onClick={save} disabled={busy}>
          {busy ? <><Loader2 size={13}/> {t('common.saving')}</> : <><Save size={13}/> {t('common.save')}</>}
        </button>
      </div>
    </div>
  );
}

// SIEM / syslog export config — stream the audit log (security events) to an
// external sink. Off by default.
function SiemExport() {
  const [cfg, setCfg] = useState({ enabled: false, kind: 'webhook', target: '', format: 'json' });
  const [busy, setBusy] = useState(false);
  const [ok, setOk] = useState(false);
  const [err, setErr] = useState(null);

  useEffect(() => {
    (async () => {
      try {
        const r = await api.siemExport.get();
        if (r) setCfg({ enabled: !!r.enabled, kind: r.kind || 'webhook', target: r.target || '', format: r.format || 'json' });
      } catch { /* default off */ }
    })();
  }, []);

  const save = async () => {
    setErr(null); setOk(false); setBusy(true);
    try { await api.siemExport.put(cfg); setOk(true); }
    catch (e) { setErr(e.message || t('settings.ingest.err_save')); }
    finally { setBusy(false); }
  };

  return (
    <div className="card" style={{ padding: 22, marginTop: 18 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
        <Share2 size={16}/>
        <h2 style={{ fontSize: 16, fontWeight: 600, margin: 0 }}>{t('settings.siem.title')}</h2>
      </div>
      <p style={{ fontSize: 12.5, color: 'var(--text-2)', margin: '0 0 14px' }}>{t('settings.siem.subtitle')}</p>
      {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}
      {ok && <div className="banner-ok">{t('settings.ingest.saved')}</div>}

      <label style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12, fontSize: 13 }}>
        <input type="checkbox" checked={cfg.enabled} onChange={e => setCfg({ ...cfg, enabled: e.target.checked })}/>
        {t('settings.siem.enable')}
      </label>
      <div className="field">
        <label className="field-label">{t('settings.siem.kind')}</label>
        <select className="input" value={cfg.kind} onChange={e => setCfg({ ...cfg, kind: e.target.value })}>
          <option value="webhook">Webhook (HTTP POST)</option>
          <option value="syslog">Syslog (UDP)</option>
          <option value="syslog_tcp">Syslog (TCP)</option>
        </select>
      </div>
      <div className="field">
        <label className="field-label">{t('settings.siem.format')}</label>
        <select className="input" value={cfg.format} onChange={e => setCfg({ ...cfg, format: e.target.value })}>
          <option value="json">JSON (raw Rampart events)</option>
          <option value="cef">CEF (ArcSight / Splunk)</option>
          <option value="leef">LEEF (IBM QRadar)</option>
        </select>
        <div className="field-hint">{t('settings.siem.format_hint')}</div>
      </div>
      <div className="field">
        <label className="field-label">{t('settings.siem.target')}</label>
        <input className="input mono" placeholder={cfg.kind === 'syslog' ? 'host:514' : 'https://siem.example.com/ingest'}
          value={cfg.target} onChange={e => setCfg({ ...cfg, target: e.target.value })}/>
        <div className="field-hint">{t('settings.siem.target_hint')}</div>
      </div>
      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
        <button className="btn btn-accent" onClick={save} disabled={busy}>
          {busy ? <><Loader2 size={13}/> {t('common.saving')}</> : <><Save size={13}/> {t('common.save')}</>}
        </button>
      </div>
    </div>
  );
}
