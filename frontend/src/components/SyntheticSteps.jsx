import React from 'react';
import { Plus, X } from 'lucide-react';
import { t } from '../lib/i18n.js';

// Shared synthetic-transaction step editor + shape converters. Used by both the
// new-monitor wizard (create) and the monitor edit modal (post-creation edit)
// so the create and edit paths can never drift in how a step maps to
// `config.steps`.

export const SYNTH_METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];
export const SYNTH_FROM = [['json', 'JSON body'], ['header', 'Header'], ['status', 'Status code']];
export const SYNTH_ASSERT_KINDS = [
  ['status', 'Status code'], ['json', 'JSON body'], ['header', 'Header'], ['body_contains', 'Body contains'],
];
export const SYNTH_OPS = [['eq', '=='], ['ne', '!='], ['lt', '<'], ['lte', '<='], ['gt', '>'], ['gte', '>='], ['contains', 'contains']];
export const SYNTH_MAX_STEPS = 20;

export function blankSynthStep() {
  return { name: '', method: 'GET', url: '', headersText: '', body: '', extracts: [], asserts: [] };
}

// Parse a "Key: Value" textarea (one header per line) into a { name: value } map.
export function parseHeaders(text) {
  const out = {};
  for (const line of (text || '').split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const idx = trimmed.indexOf(':');
    if (idx <= 0) continue;
    const name = trimmed.slice(0, idx).trim();
    const value = trimmed.slice(idx + 1).trim();
    if (name) out[name] = value;
  }
  return out;
}

// Serialize a { name: value } map back into textarea lines.
export function headersToText(map) {
  return Object.entries(map || {}).map(([k, v]) => `${k}: ${v}`).join('\n');
}

// Stored config.steps[] → editor UI shape. Tolerant of partial/legacy steps so
// hand-edited JSON still loads into the form without throwing.
export function configToUiSteps(steps) {
  if (!Array.isArray(steps) || steps.length === 0) return [blankSynthStep()];
  return steps.map(s => ({
    name: s.name || '',
    method: SYNTH_METHODS.includes(s.method) ? s.method : 'GET',
    url: s.url || '',
    headersText: headersToText(s.headers),
    body: s.body || '',
    extracts: Array.isArray(s.extract)
      ? s.extract.map(e => ({ var: e.var || '', from: e.from || 'json', path: e.path || '' }))
      : [],
    asserts: Array.isArray(s.assert)
      ? s.assert.map(a => ({
          kind: a.kind || 'status',
          path: a.path || '',
          op: a.op || 'eq',
          value: a.value == null ? '' : String(a.value),
        }))
      : [],
  }));
}

// Editor UI shape → stored config.steps[]. Drops blank rows and omits a step's
// optional pieces when empty so the stored spec stays tight. This is the single
// source of truth for the create + edit paths.
export function uiToConfigSteps(synthSteps) {
  return synthSteps
    .map(s => {
      const out = { method: s.method, url: s.url.trim() };
      if (s.name.trim()) out.name = s.name.trim();
      const hdrs = parseHeaders(s.headersText);
      if (Object.keys(hdrs).length) out.headers = hdrs;
      if (s.body.trim()) out.body = s.body;
      const extract = s.extracts
        .filter(e => e.var.trim())
        .map(e => ({
          var: e.var.trim(),
          from: e.from,
          ...(e.from !== 'status' && e.path.trim() ? { path: e.path.trim() } : {}),
        }));
      if (extract.length) out.extract = extract;
      const assert = s.asserts
        .filter(a => a.value.trim() !== '')
        .map(a => ({
          kind: a.kind,
          op: a.op,
          value: a.value,
          ...((a.kind === 'json' || a.kind === 'header') && a.path.trim() ? { path: a.path.trim() } : {}),
        }));
      if (assert.length) out.assert = assert;
      return out;
    })
    .filter(s => s.url);
}

// Ordered HTTP-step editor for synthetic monitors. Each step's request,
// extractions (response → {{var}}), and assertions are edited inline; the
// parent converts this shape to config.steps via uiToConfigSteps.
export function SyntheticSteps({ steps, setSteps }) {
  const patchStep = (i, p) => setSteps(ss => ss.map((s, j) => (j === i ? { ...s, ...p } : s)));
  const addStep = () => setSteps(ss => (ss.length >= SYNTH_MAX_STEPS ? ss : [...ss, blankSynthStep()]));
  const removeStep = (i) => setSteps(ss => ss.filter((_, j) => j !== i));
  const updNested = (i, key, idx, p) =>
    setSteps(ss => ss.map((s, j) => (j === i ? { ...s, [key]: s[key].map((r, k) => (k === idx ? { ...r, ...p } : r)) } : s)));
  const addNested = (i, key, blank) =>
    setSteps(ss => ss.map((s, j) => (j === i ? { ...s, [key]: [...s[key], blank] } : s)));
  const delNested = (i, key, idx) =>
    setSteps(ss => ss.map((s, j) => (j === i ? { ...s, [key]: s[key].filter((_, k) => k !== idx) } : s)));

  return (
    <>
      <div style={{ margin: '18px 0 10px', fontSize: 11, fontWeight: 600, color: 'var(--text-3)', textTransform: 'uppercase', letterSpacing: '.05em' }}>
        {t('wizard.synth.steps_label')}
      </div>
      <div className="field-hint" style={{ marginTop: 0, marginBottom: 12 }}>{t('wizard.synth.hint')}</div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        {steps.map((s, i) => (
          <div key={i} style={{ border: '1px solid var(--border)', borderRadius: 10, padding: 14, background: 'var(--surface-2)' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 10 }}>
              <span style={{ fontSize: 12.5, fontWeight: 600, whiteSpace: 'nowrap' }}>{t('wizard.synth.step_n', { n: i + 1 })}</span>
              <input className="input" style={{ flex: 1, padding: '5px 8px' }} placeholder={t('wizard.synth.name_ph')}
                value={s.name} onChange={e => patchStep(i, { name: e.target.value })}/>
              {steps.length > 1 && (
                <button type="button" className="btn" style={{ padding: '5px 8px' }} title={t('wizard.synth.remove_step')}
                  onClick={() => removeStep(i)} aria-label={t('common.remove')}><X size={13}/></button>
              )}
            </div>

            <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
              <select className="select" style={{ width: 110 }} value={s.method} onChange={e => patchStep(i, { method: e.target.value })}>
                {SYNTH_METHODS.map(m => <option key={m} value={m}>{m}</option>)}
              </select>
              <input className="input mono" style={{ flex: 1 }} placeholder={t('wizard.synth.url_ph')}
                value={s.url} onChange={e => patchStep(i, { url: e.target.value })}/>
            </div>

            <textarea className="input mono" rows={2} style={{ marginBottom: 8, resize: 'vertical' }} placeholder={t('wizard.synth.headers_ph')}
              value={s.headersText} onChange={e => patchStep(i, { headersText: e.target.value })}/>
            <textarea className="input mono" rows={2} style={{ marginBottom: 12, resize: 'vertical' }} placeholder={t('wizard.synth.body_ph')}
              value={s.body} onChange={e => patchStep(i, { body: e.target.value })}/>

            <div className="field-label">{t('wizard.synth.extracts_label')}</div>
            {s.extracts.map((e, ei) => (
              <div key={ei} style={{ display: 'flex', gap: 6, marginBottom: 6 }}>
                <input className="input mono" style={{ flex: '0 0 120px' }} placeholder={t('wizard.synth.var_ph')}
                  value={e.var} onChange={ev => updNested(i, 'extracts', ei, { var: ev.target.value })}/>
                <select className="select" style={{ flex: '0 0 130px' }} value={e.from}
                  onChange={ev => updNested(i, 'extracts', ei, { from: ev.target.value })}>
                  {SYNTH_FROM.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                </select>
                <input className="input mono" style={{ flex: 1 }} placeholder={t('wizard.synth.path_ph')}
                  disabled={e.from === 'status'}
                  value={e.path} onChange={ev => updNested(i, 'extracts', ei, { path: ev.target.value })}/>
                <button type="button" className="btn" style={{ padding: '5px 8px' }} onClick={() => delNested(i, 'extracts', ei)} aria-label={t('common.remove')}><X size={13}/></button>
              </div>
            ))}
            <button type="button" className="btn" style={{ marginBottom: 12 }}
              onClick={() => addNested(i, 'extracts', { var: '', from: 'json', path: '' })}>
              <Plus size={12}/> {t('wizard.synth.add_extract')}
            </button>

            <div className="field-label">{t('wizard.synth.asserts_label')}</div>
            {s.asserts.map((a, ai) => (
              <div key={ai} style={{ display: 'flex', gap: 6, marginBottom: 6 }}>
                <select className="select" style={{ flex: '0 0 130px' }} value={a.kind}
                  onChange={ev => updNested(i, 'asserts', ai, { kind: ev.target.value })}>
                  {SYNTH_ASSERT_KINDS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                </select>
                <input className="input mono" style={{ flex: '0 0 120px' }} placeholder={t('wizard.synth.path_ph')}
                  disabled={a.kind === 'status' || a.kind === 'body_contains'}
                  value={a.path} onChange={ev => updNested(i, 'asserts', ai, { path: ev.target.value })}/>
                <select className="select" style={{ flex: '0 0 90px' }} value={a.op}
                  disabled={a.kind === 'body_contains'}
                  onChange={ev => updNested(i, 'asserts', ai, { op: ev.target.value })}>
                  {SYNTH_OPS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                </select>
                <input className="input mono" style={{ flex: 1 }} placeholder={t('wizard.synth.value_ph')}
                  value={a.value} onChange={ev => updNested(i, 'asserts', ai, { value: ev.target.value })}/>
                <button type="button" className="btn" style={{ padding: '5px 8px' }} onClick={() => delNested(i, 'asserts', ai)} aria-label={t('common.remove')}><X size={13}/></button>
              </div>
            ))}
            <button type="button" className="btn"
              onClick={() => addNested(i, 'asserts', { kind: 'status', path: '', op: 'eq', value: '' })}>
              <Plus size={12}/> {t('wizard.synth.add_assert')}
            </button>
          </div>
        ))}
      </div>

      <button type="button" className="btn" style={{ marginTop: 12 }} disabled={steps.length >= SYNTH_MAX_STEPS} onClick={addStep}>
        <Plus size={13}/> {t('wizard.synth.add_step')}
      </button>
    </>
  );
}
