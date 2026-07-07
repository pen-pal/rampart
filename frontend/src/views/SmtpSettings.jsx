import React, { useEffect, useState } from 'react';
import {
  Save, Loader2, AlertCircle, Mail,
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
  .input, .select {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .field { margin-bottom: 12px; }
  .field-label { font-size: 11px; font-weight: 500; color: var(--text-3); text-transform: uppercase; letter-spacing: .04em; display: block; margin-bottom: 5px; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 14px; }
  .banner-ok  { background: var(--up-soft);   color: #047857; border: 1px solid #a7f3d0; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 14px; }
`;

export default function SmtpSettings() {
  const [cfg,  setCfg]  = useState({ host: '', port: 587, encryption: 'starttls', username: '', password: '', from: '' });
  const [busy, setBusy] = useState(false);
  const [load, setLoad] = useState(true);
  const [err,  setErr]  = useState(null);
  const [ok,   setOk]   = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const r = await api.smtp.get();
        if (r && typeof r === 'object') {
          setCfg({
            host:       r.host       || '',
            port:       r.port       || 587,
            encryption: r.encryption || 'starttls',
            username:   r.username   || '',
            password:   r.password   || '',
            from:       r.from       || '',
          });
        }
      } catch (e) { setErr(e.message); }
      finally { setLoad(false); }
    })();
  }, []);

  const save = async () => {
    setErr(null); setOk(false);
    if (!cfg.host || !cfg.from) { setErr(t('settings.smtp.err_required')); return; }
    setBusy(true);
    try {
      await api.smtp.put({
        ...cfg,
        port:     parseInt(cfg.port, 10) || 587,
        username: cfg.username || null,
        password: cfg.password || null,
      });
      setOk(true);
    } catch (e) { setErr(e.message || t('settings.smtp.err_save')); }
    finally { setBusy(false); }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 640, margin: '0 auto', padding: '32px 32px 64px' }}>
        <SubViewHeader title={t('settings.smtp.title')} icon={Mail} />
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <Mail size={20}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: 0, letterSpacing: '-.02em' }}>{t('settings.smtp.title')}</h1>
        </div>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 22px' }}>
          {t('settings.smtp.subtitle')}
        </p>

        {load ? (
          <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
        ) : (
          <div className="card" style={{ padding: 22 }}>
            {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}
            {ok  && <div className="banner-ok">{t('settings.smtp.saved')}</div>}

            <div style={{ display: 'grid', gridTemplateColumns: '1fr 110px', gap: 10 }}>
              <div className="field">
                <label className="field-label">{t('settings.smtp.host')}</label>
                <input className="input" value={cfg.host} onChange={e => setCfg({ ...cfg, host: e.target.value })} placeholder="smtp.example.com"/>
              </div>
              <div className="field">
                <label className="field-label">{t('settings.smtp.port')}</label>
                <input className="input mono" value={cfg.port} onChange={e => setCfg({ ...cfg, port: e.target.value })}/>
              </div>
            </div>
            <div className="field">
              <label className="field-label">{t('settings.smtp.encryption')}</label>
              <select className="select" value={cfg.encryption} onChange={e => setCfg({ ...cfg, encryption: e.target.value })}>
                <option value="starttls">{t('settings.smtp.enc_starttls')}</option>
                <option value="tls">{t('settings.smtp.enc_tls')}</option>
                <option value="plain">{t('settings.smtp.enc_plain')}</option>
              </select>
            </div>
            <div className="form-2col" style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
              <div className="field">
                <label className="field-label">{t('settings.smtp.username')}</label>
                <input className="input" value={cfg.username} onChange={e => setCfg({ ...cfg, username: e.target.value })}/>
              </div>
              <div className="field">
                <label className="field-label">{t('settings.smtp.password')}</label>
                <input className="input" type="password" value={cfg.password} onChange={e => setCfg({ ...cfg, password: e.target.value })} placeholder={t('settings.smtp.password_placeholder')}/>
              </div>
            </div>
            <div className="field">
              <label className="field-label">{t('settings.smtp.from')}</label>
              <input className="input" value={cfg.from} onChange={e => setCfg({ ...cfg, from: e.target.value })} placeholder='"Rampart" &lt;status@example.com&gt;'/>
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
              <button className="btn btn-accent" onClick={save} disabled={busy}>
                {busy ? <><Loader2 size={13}/> {t('common.saving')}</> : <><Save size={13}/> {t('common.save')}</>}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
