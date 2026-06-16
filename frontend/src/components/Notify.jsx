import React, { useState, useEffect, useRef } from 'react';
import { subscribeToasts, dismissToast, subscribeDialog } from '../lib/notify.js';
import { t } from '../lib/i18n.js';

// Toast stack (top-right) + a single accessible confirm/prompt modal host.
// Both are mounted once in App, alongside the theme/locale togglers.

const KIND_BG = { info: 'var(--surface)', success: 'var(--up-soft, #d1fae5)', error: 'var(--down-soft, #fee2e2)' };
const KIND_FG = { info: 'var(--text)', success: '#047857', error: '#b91c1c' };

export function Toaster() {
  const [items, setItems] = useState([]);
  useEffect(() => subscribeToasts(setItems), []);
  if (items.length === 0) return null;
  return (
    <div
      aria-live="polite"
      style={{
        position: 'fixed', top: 16, right: 16, zIndex: 9999,
        display: 'flex', flexDirection: 'column', gap: 8, maxWidth: 380,
        fontFamily: 'Inter, ui-sans-serif, system-ui, sans-serif',
      }}
    >
      {items.map((it) => (
        <div
          key={it.id}
          role="status"
          onClick={() => dismissToast(it.id)}
          style={{
            background: KIND_BG[it.kind] || KIND_BG.info,
            color: KIND_FG[it.kind] || KIND_FG.info,
            border: '1px solid var(--border, #e7e5e4)',
            borderRadius: 10, padding: '10px 14px', fontSize: 13, cursor: 'pointer',
            boxShadow: '0 6px 24px rgba(0,0,0,.12)', wordBreak: 'break-word',
          }}
          title={t('common.close')}
        >
          {it.message}
        </div>
      ))}
    </div>
  );
}

// Focusable-element selector for the focus trap.
const FOCUSABLE = 'a[href], button:not([disabled]), input, textarea, select, [tabindex]:not([tabindex="-1"])';

export function DialogHost() {
  const [req, setReq] = useState(null); // active dialog request, or null
  const [value, setValue] = useState('');
  const cardRef = useRef(null);
  const restoreRef = useRef(null);

  useEffect(() => subscribeDialog((r) => {
    restoreRef.current = document.activeElement;
    setValue(r.kind === 'prompt' ? (r.defaultValue || '') : '');
    setReq(r);
  }), []);

  const close = (result) => {
    const r = req;
    setReq(null);
    if (r) r.resolve(result);
    // Restore focus to whatever opened the dialog.
    if (restoreRef.current && restoreRef.current.focus) restoreRef.current.focus();
  };
  const cancel = () => close(req.kind === 'prompt' ? null : false);
  const accept = () => close(req.kind === 'prompt' ? value : true);

  // Focus the card on open; trap Tab; Escape cancels.
  useEffect(() => {
    if (!req) return undefined;
    const node = cardRef.current;
    const first = node?.querySelector(FOCUSABLE);
    (first || node)?.focus();
    const onKey = (e) => {
      if (e.key === 'Escape') { e.preventDefault(); cancel(); return; }
      if (e.key === 'Tab' && node) {
        const els = [...node.querySelectorAll(FOCUSABLE)].filter((x) => x.offsetParent !== null);
        if (els.length === 0) return;
        const i = els.indexOf(document.activeElement);
        if (e.shiftKey && (i <= 0)) { e.preventDefault(); els[els.length - 1].focus(); }
        else if (!e.shiftKey && i === els.length - 1) { e.preventDefault(); els[0].focus(); }
      }
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  if (!req) return null;
  const titleId = 'dlg-title';
  return (
    <div
      onMouseDown={(e) => { if (e.target === e.currentTarget) cancel(); }}
      style={{
        position: 'fixed', inset: 0, zIndex: 10000,
        background: 'rgba(0,0,0,.4)', display: 'flex',
        alignItems: 'center', justifyContent: 'center', padding: 20,
        fontFamily: 'Inter, ui-sans-serif, system-ui, sans-serif',
      }}
    >
      <div
        ref={cardRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={req.title ? titleId : undefined}
        aria-label={req.title ? undefined : (req.message || 'Dialog')}
        tabIndex={-1}
        onMouseDown={(e) => e.stopPropagation()}
        style={{
          background: 'var(--surface, #fff)', color: 'var(--text, #1c1917)',
          border: '1px solid var(--border, #e7e5e4)', borderRadius: 14,
          padding: 20, width: 'min(440px, 100%)', boxShadow: '0 20px 60px rgba(0,0,0,.25)',
        }}
      >
        {req.title && (
          <div id={titleId} style={{ fontSize: 15, fontWeight: 600, marginBottom: 8 }}>{req.title}</div>
        )}
        {req.message && (
          <div style={{ fontSize: 13.5, color: 'var(--text-2, #57534e)', marginBottom: 14, whiteSpace: 'pre-wrap' }}>
            {req.message}
          </div>
        )}
        {req.kind === 'prompt' && (
          <input
            className="input"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') accept(); }}
            style={{
              width: '100%', padding: '9px 12px', borderRadius: 8, marginBottom: 14,
              border: '1px solid var(--border, #e7e5e4)', fontSize: 13,
              background: 'var(--surface, #fff)', color: 'var(--text, #1c1917)', outline: 'none',
            }}
          />
        )}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <button onClick={cancel} style={btn(false)}>{req.cancelLabel || t('common.cancel')}</button>
          <button onClick={accept} style={btn(true, req.danger)}>
            {req.confirmLabel || (req.kind === 'prompt' ? t('common.save') : t('common.confirm'))}
          </button>
        </div>
      </div>
    </div>
  );
}

function btn(primary, danger) {
  return {
    padding: '7px 14px', borderRadius: 8, fontSize: 13, fontWeight: 500, cursor: 'pointer',
    fontFamily: 'inherit', border: '1px solid var(--border, #e7e5e4)',
    background: primary ? (danger ? 'var(--down, #ef4444)' : 'var(--accent, #14b8a6)') : 'var(--surface, #fff)',
    color: primary ? '#fff' : 'var(--text, #1c1917)',
    borderColor: primary ? (danger ? 'var(--down, #ef4444)' : 'var(--accent, #14b8a6)') : 'var(--border, #e7e5e4)',
  };
}
