import React, { useState } from 'react';
import {
  ChevronLeft, Trash2, AlertCircle, Loader2, X, Check, Wand2, FileStack, Plus,
} from 'lucide-react';
import { api, useApi } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { confirmDialog } from '../lib/notify.js';
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
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .row {
    display: grid; grid-template-columns: 1fr auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .pill {
    display: inline-flex; align-items: center;
    font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500;
    background: var(--surface-2); color: var(--text-2); text-transform: uppercase;
  }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); display: block; margin-bottom: 6px; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .spin { animation: spin 1s linear infinite; }
`;

export default function MonitorTemplates({ user }) {
  const writable = canWrite(user);
  const [reloadKey, setReloadKey] = useState(0);
  const templatesState = useApi(() => api.monitorTemplates.list(), [reloadKey], { pollMs: 60_000 });
  const [busy, setBusy] = useState(null);
  const [err,  setErr]  = useState(null);
  // Instantiate dialog: null when closed, else { template, name, busy }.
  const [instState, setInstState] = useState(null);
  // From-scratch create form: null when closed.
  const [createState, setCreateState] = useState(null);

  const reload = () => setReloadKey((k) => k + 1);

  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('templates.delete_confirm') }))) return;
    setBusy(id); setErr(null);
    try { await api.monitorTemplates.remove(id); reload(); }
    catch (e) { setErr(e.message || t('templates.err_delete')); setBusy(null); }
  };

  const submitInstantiate = async () => {
    if (!instState || instState.busy) return;
    setInstState(s => ({ ...s, busy: true }));
    setErr(null);
    try {
      const created = await api.monitorTemplates.instantiate(
        instState.template.id,
        instState.name.trim() || undefined,
      );
      // Land on the new monitor when the backend returns one; otherwise fall
      // back to the dashboard.
      if (created && created.id) window.location.hash = `#/monitor/${created.id}`;
      else window.location.hash = '#/';
    } catch (e) {
      setErr(e.message || t('templates.err_instantiate'));
      setInstState(s => s && ({ ...s, busy: false }));
    }
  };

  const openCreate = () => {
    setErr(null);
    setCreateState({
      name: '', description: '', busy: false, specErr: null,
      spec: JSON.stringify({
        name: 'Example',
        kind: 'http',
        url: 'https://example.com',
        interval_seconds: 60,
        timeout_seconds: 16,
      }, null, 2),
    });
  };

  const submitCreate = async () => {
    if (!createState || createState.busy) return;
    if (!createState.name.trim()) { setCreateState(s => ({ ...s, specErr: t('templates.err_name') })); return; }
    let spec;
    try {
      spec = JSON.parse(createState.spec);
      if (!spec || typeof spec !== 'object' || Array.isArray(spec) || !spec.kind) throw new Error();
    } catch {
      setCreateState(s => ({ ...s, specErr: t('templates.err_spec') }));
      return;
    }
    setCreateState(s => ({ ...s, busy: true, specErr: null }));
    try {
      await api.monitorTemplates.create({
        name: createState.name.trim(),
        description: createState.description.trim() || undefined,
        spec,
      });
      reload();
    } catch (e) {
      setCreateState(s => s && ({ ...s, busy: false, specErr: e.message || t('templates.err_create') }));
    }
  };

  const templates = templatesState.data || [];

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 920, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('templates.back')}
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('templates.title')}</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('templates.subtitle')}</p>
          </div>
          {writable && (
            <button className="btn btn-accent" onClick={openCreate}>
              <Plus size={13}/> {t('templates.new')}
            </button>
          )}
        </div>

        {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        {createState && (
          <div className="card" style={{ padding: 20, marginBottom: 18 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
              <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>{t('templates.create_title')}</h3>
              <button className="btn btn-ghost" onClick={() => setCreateState(null)} disabled={createState.busy} aria-label={t('common.close')}><X size={14}/></button>
            </div>
            {createState.specErr && <div className="banner-err">{createState.specErr}</div>}
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14, marginBottom: 14 }}>
              <div>
                <label className="field-label">{t('common.name')}</label>
                <input className="input" value={createState.name} autoFocus
                  onChange={e => setCreateState(s => ({ ...s, name: e.target.value }))}/>
              </div>
              <div>
                <label className="field-label">{t('templates.field_description')}</label>
                <input className="input" value={createState.description}
                  onChange={e => setCreateState(s => ({ ...s, description: e.target.value }))}/>
              </div>
            </div>
            <div style={{ marginBottom: 16 }}>
              <label className="field-label">{t('templates.spec_label')}</label>
              <textarea className="input mono" rows={10} value={createState.spec} spellCheck={false}
                style={{ resize: 'vertical', fontSize: 12 }}
                onChange={e => setCreateState(s => ({ ...s, spec: e.target.value }))}/>
              <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 6 }}>{t('templates.spec_hint')}</div>
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
              <button className="btn btn-ghost" onClick={() => setCreateState(null)} disabled={createState.busy}>{t('common.cancel')}</button>
              <button className="btn btn-accent" onClick={submitCreate} disabled={createState.busy}>
                {createState.busy
                  ? <><Loader2 size={13} className="spin"/> {t('templates.creating')}</>
                  : <><Plus size={13}/> {t('templates.create')}</>}
              </button>
            </div>
          </div>
        )}

        {instState && (
          <div className="card" style={{ padding: 20, marginBottom: 18 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
              <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>
                {t('templates.instantiate_title', { name: instState.template.name })}
              </h3>
              <button className="btn btn-ghost" onClick={() => setInstState(null)} disabled={instState.busy} aria-label={t('common.close')}><X size={14}/></button>
            </div>
            <div style={{ marginBottom: 16 }}>
              <label className="field-label">{t('templates.name_override')}</label>
              <input className="input" value={instState.name} autoFocus
                placeholder={instState.template.name}
                onChange={e => setInstState(s => ({ ...s, name: e.target.value }))}
                onKeyDown={e => { if (e.key === 'Enter') submitInstantiate(); }}/>
              <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 6 }}>{t('templates.name_override_hint')}</div>
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
              <button className="btn btn-ghost" onClick={() => setInstState(null)} disabled={instState.busy}>{t('common.cancel')}</button>
              <button className="btn btn-accent" onClick={submitInstantiate} disabled={instState.busy}>
                {instState.busy
                  ? <><Loader2 size={13} className="spin"/> {t('templates.instantiating')}</>
                  : <><Wand2 size={13}/> {t('templates.create_monitor')}</>}
              </button>
            </div>
          </div>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {templatesState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16}/> {t('templates.loading')}
            </div>
          ) : templates.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <FileStack size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('templates.empty.title')}</div>
              <div style={{ fontSize: 12.5 }}>{t('templates.empty.cta')}</div>
              {writable && (
                <button className="btn btn-accent" style={{ marginTop: 14 }} onClick={openCreate}>
                  <Plus size={13}/> {t('templates.new')}
                </button>
              )}
            </div>
          ) : templates.map(tpl => (
            <div className="row" key={tpl.id}>
              <div style={{ minWidth: 0 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
                  <span style={{ fontSize: 14, fontWeight: 600 }}>{tpl.name}</span>
                  {tpl.spec?.kind && <span className="pill">{tpl.spec.kind}</span>}
                </div>
                {tpl.description && (
                  <div style={{ fontSize: 12, color: 'var(--text-2)', marginBottom: 4 }}>{tpl.description}</div>
                )}
                {tpl.spec?.url && (
                  <div className="mono" style={{ fontSize: 11, color: 'var(--text-3)', wordBreak: 'break-all' }}>{tpl.spec.url}</div>
                )}
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                {writable && (
                  <button className="btn btn-accent" disabled={busy === tpl.id}
                    onClick={() => { setErr(null); setInstState({ template: tpl, name: '', busy: false }); }}>
                    <Plus size={13}/> {t('templates.new_from')}
                  </button>
                )}
                {writable && (
                  <button className="btn btn-ghost btn-danger" onClick={() => remove(tpl.id)} disabled={busy === tpl.id}
                    title={t('templates.delete')}>
                    {busy === tpl.id ? <Loader2 size={13} className="spin"/> : <Trash2 size={13}/>}
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
