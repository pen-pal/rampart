import { useEffect } from 'react';

// Accessible-modal helper: when `active`, traps Tab focus inside `ref`, focuses
// the first control on open, closes on Escape, and restores focus to the opener
// on close. Pair with `role="dialog" aria-modal="true" tabIndex={-1}` on the
// element `ref` points at. (Same behaviour as the toast/dialog primitive's host,
// extracted so the in-view edit modals get it too.)
const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function useFocusTrap(ref, active, onEscape) {
  useEffect(() => {
    if (!active) return undefined;
    const node = ref.current;
    const restore = document.activeElement;
    const first = node?.querySelector(FOCUSABLE);
    (first || node)?.focus?.();
    const onKey = (e) => {
      if (e.key === 'Escape') { e.preventDefault(); onEscape?.(); return; }
      if (e.key === 'Tab' && node) {
        const els = [...node.querySelectorAll(FOCUSABLE)].filter((x) => x.offsetParent !== null);
        if (els.length === 0) return;
        const i = els.indexOf(document.activeElement);
        if (e.shiftKey && i <= 0) { e.preventDefault(); els[els.length - 1].focus(); }
        else if (!e.shiftKey && i === els.length - 1) { e.preventDefault(); els[0].focus(); }
      }
    };
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      if (restore && restore.focus) restore.focus();
    };
  }, [ref, active, onEscape]);
}
