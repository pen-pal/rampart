import React, { useState } from 'react';
import {
  ChevronLeft, Upload, FileText, Loader2, AlertCircle, CheckCircle2,
} from 'lucide-react';
import { api } from '../lib/api.js';
import { canWrite } from '../lib/roles.js';
import { t } from '../lib/i18n.js';

// Self-contained styling, mirroring StatusPageBuilder so this lightweight
// view doesn't depend on the dashboard chrome being mounted.
const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
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
  .btn-ghost { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); margin-bottom: 6px; display: block; }
  .textarea {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 12px; color: var(--text); outline: none;
    font-family: 'JetBrains Mono', monospace;
  }
  .textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
`;

export default function ImportMonitors({ user } = {}) {
  const writable = canWrite(user);
  const [csv,     setCsv]     = useState('');
  const [busy,    setBusy]    = useState(false);
  const [err,     setErr]     = useState(null);
  const [result,  setResult]  = useState(null);

  const onFilePick = (e) => {
    const file = e.target.files && e.target.files[0];
    e.target.value = ''; // allow re-picking the same file
    if (!file) return;
    const reader = new FileReader();
    reader.onload  = () => { setCsv(String(reader.result || '')); setErr(null); };
    reader.onerror = () => setErr(t('import.err_file_read'));
    reader.readAsText(file);
  };

  const doImport = async () => {
    const text = csv.trim();
    if (!text) { setErr(t('import.err_empty')); return; }
    setBusy(true); setErr(null); setResult(null);
    try {
      const res = await api.monitors.importCsv(text);
      setResult(res);
    } catch (e) {
      setErr(e.message || t('import.err_failed'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 720, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>

        <h1 style={{ fontSize: 24, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>
          {t('import.title')}
        </h1>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 22px' }}>
          {t('import.subtitle')}
        </p>

        {!writable ? (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>
            {t('import.err_failed')}
          </div>
        ) : (
          <>
            {err && (
              <div className="banner-err">
                <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}
              </div>
            )}

            <div className="card" style={{ padding: 22, marginBottom: 18 }}>
              <div style={{ marginBottom: 16 }}>
                <div className="field-label" style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <FileText size={13}/> {t('import.columns_title')}
                </div>
                <div style={{ fontSize: 12, color: 'var(--text-3)', lineHeight: 1.5 }}>
                  {t('import.columns_hint')}
                </div>
              </div>

              <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 16 }}>
                <label className="btn" style={{ cursor: 'pointer' }}>
                  <Upload size={13}/> {t('import.file_label')}
                  <input type="file" accept=".csv,text/csv,text/plain" onChange={onFilePick} style={{ display: 'none' }}/>
                </label>
                <span style={{ fontSize: 12, color: 'var(--text-3)' }}>{t('import.or')}</span>
              </div>

              <label className="field-label">{t('import.paste_label')}</label>
              <textarea
                className="textarea"
                rows={8}
                value={csv}
                onChange={e => setCsv(e.target.value)}
                placeholder={t('import.paste_placeholder')}
              />

              <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 16 }}>
                <button className="btn btn-accent" onClick={doImport} disabled={busy}>
                  {busy
                    ? <><Loader2 size={13}/> {t('import.importing')}</>
                    : <><Upload size={13}/> {t('import.import')}</>}
                </button>
              </div>
            </div>

            {result && (
              <div className="card" style={{ padding: 22 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 14, fontWeight: 600, marginBottom: 12 }}>
                  <CheckCircle2 size={16} color="var(--up)"/>
                  {t('import.result_created', { n: result.created })}
                </div>
                {(result.skipped && result.skipped.length > 0) ? (
                  <>
                    <div style={{ fontSize: 12.5, color: 'var(--text-2)', marginBottom: 8 }}>
                      {t('import.result_skipped', { n: result.skipped.length })}
                    </div>
                    <div style={{ border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden' }}>
                      {result.skipped.map((s, i) => (
                        <div key={i} style={{
                          display: 'grid', gridTemplateColumns: '1fr 2fr', gap: 12,
                          padding: '8px 12px', fontSize: 12,
                          borderTop: i === 0 ? 'none' : '1px solid var(--border)',
                        }}>
                          <span className="mono" style={{ color: 'var(--text)' }}>{s.row}</span>
                          <span style={{ color: 'var(--down)' }}>{s.reason}</span>
                        </div>
                      ))}
                    </div>
                  </>
                ) : (
                  <div style={{ fontSize: 12.5, color: 'var(--text-3)' }}>{t('import.result_none')}</div>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
