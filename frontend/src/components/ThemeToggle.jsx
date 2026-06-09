// Shared theme toggle. Cycles light → dark → system → light.
//
// Rendered in two places:
//   - Dashboard header (inline, alongside the bell + nav)
//   - App-level fixed-position button (visible on every view that the
//     header doesn't already chrome)
//
// One component, one persisted preference, one Pure-CSS dark-mode flip
// via the data-theme="dark" attribute the theme runtime sets on <html>.

import React, { useState, useRef, useEffect } from 'react';
import { Moon, Monitor, Sun, Globe } from 'lucide-react';
import { getTheme, setTheme } from '../lib/theme.js';
import { getLocale, setLocale, SUPPORTED } from '../lib/i18n.js';

/**
 * Inline variant — sits alongside other buttons in a view header.
 * Renders the lucide icon at 14px without a border, hover bg from
 * existing .btn-ghost.
 */
export function ThemeToggle() {
  const [pref, setPref] = useState(getTheme());
  const next = pref === 'light' ? 'dark' : pref === 'dark' ? 'system' : 'light';
  const Icon = pref === 'dark' ? Moon : pref === 'system' ? Monitor : Sun;
  const label = pref === 'dark' ? 'Dark · click for System'
              : pref === 'system' ? 'System · click for Light'
              : 'Light · click for Dark';
  return (
    <button className="btn btn-ghost" title={label}
      onClick={() => { setTheme(next); setPref(next); }}>
      <Icon size={14}/>
    </button>
  );
}

/**
 * Floating variant — fixed-position pill in the bottom-LEFT corner so
 * it doesn't overlap the dev-only Views switcher in the bottom-right.
 * Visible on every authenticated view, including those without their
 * own header (Maintenance, AuditLog, Users, etc.). The Dashboard
 * header carries an inline ThemeToggle too; this floating one is the
 * fallback so the toggle is reachable from anywhere.
 */
export function FloatingThemeToggle() {
  const [pref, setPref] = useState(getTheme());
  const next = pref === 'light' ? 'dark' : pref === 'dark' ? 'system' : 'light';
  const Icon = pref === 'dark' ? Moon : pref === 'system' ? Monitor : Sun;
  const label = pref === 'dark' ? 'Dark · click for System'
              : pref === 'system' ? 'System · click for Light'
              : 'Light · click for Dark';
  return (
    <button onClick={() => { setTheme(next); setPref(next); }} title={label}
      style={{
        position: 'fixed', left: 16, bottom: 16, zIndex: 9999,
        width: 38, height: 38, borderRadius: 999,
        background: 'var(--surface)', color: 'var(--text-2)',
        border: '1px solid var(--border)', cursor: 'pointer',
        boxShadow: '0 4px 12px rgba(0,0,0,.08)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        transition: 'all .12s',
      }}>
      <Icon size={16}/>
    </button>
  );
}

// Native-language label for each SUPPORTED locale. Endonyms (a speaker's
// own name for their language) — not English exonyms — so the menu reads
// naturally to the speaker scanning for their language.
const LOCALE_NAMES = {
  en: 'English',
  es: 'Español',
  fr: 'Français',
  de: 'Deutsch',
  ja: '日本語',
  zh: '中文',
};

/**
 * Floating locale picker — fixed-position Globe pill that sits just to the
 * RIGHT of the FloatingThemeToggle (which lives at left:16). Same 38px
 * round-pill chrome. Clicking opens a small popover listing every
 * SUPPORTED locale by its native name + lowercase code; choosing one
 * persists via setLocale() then reloads so t() picks up the new dictionary
 * (the i18n layer is synchronous-by-design and has no live re-render).
 *
 * Outside-click + Esc close the menu, matching the dashboard NavMenu.
 */
export function FloatingLocalePicker() {
  const [open, setOpen] = useState(false);
  const ref = useRef(null);
  const active = getLocale();

  useEffect(() => {
    if (!open) return;
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    const onKey = (e) => { if (e.key === 'Escape') setOpen(false); };
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    return () => { document.removeEventListener('mousedown', onDoc); document.removeEventListener('keydown', onKey); };
  }, [open]);

  const choose = (code) => {
    setLocale(code);
    setOpen(false);
    // Guard the reload: jsdom (vitest) provides a window.location without a
    // working reload(), and calling it throws "Not implemented". Feature-
    // detect so the test suite stays green; the real browser path is intact.
    if (typeof window !== 'undefined' && window.location && typeof window.location.reload === 'function') {
      try { window.location.reload(); } catch { /* jsdom / sandbox — ignore */ }
    }
  };

  return (
    <div ref={ref} style={{ position: 'fixed', left: 64, bottom: 16, zIndex: 9999 }}>
      {open && (
        <div style={{
          position: 'absolute', left: 0, bottom: 'calc(100% + 8px)', minWidth: 160,
          background: 'var(--surface)', border: '1px solid var(--border-2)', borderRadius: 10,
          boxShadow: '0 12px 32px rgba(0,0,0,.22)', padding: 6,
        }}>
          {SUPPORTED.map(code => (
            <button key={code} onClick={() => choose(code)} style={{
              display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12,
              width: '100%', padding: '8px 10px', borderRadius: 7,
              background: code === active ? 'var(--surface-2)' : 'transparent',
              border: 'none', cursor: 'pointer', textAlign: 'left',
              color: 'var(--text)', fontSize: 13,
              fontFamily: 'Inter, system-ui, sans-serif',
            }}
              onMouseEnter={e => e.currentTarget.style.background = 'var(--surface-2)'}
              onMouseLeave={e => e.currentTarget.style.background = code === active ? 'var(--surface-2)' : 'transparent'}>
              <span>{LOCALE_NAMES[code] || code}</span>
              <span style={{ color: 'var(--text-3)', fontSize: 11, fontFamily: 'JetBrains Mono, monospace' }}>{code}</span>
            </button>
          ))}
        </div>
      )}
      <button onClick={() => setOpen(o => !o)} title="Language"
        style={{
          width: 38, height: 38, borderRadius: 999,
          background: 'var(--surface)', color: 'var(--text-2)',
          border: '1px solid var(--border)', cursor: 'pointer',
          boxShadow: '0 4px 12px rgba(0,0,0,.08)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          transition: 'all .12s',
        }}>
        <Globe size={16}/>
      </button>
    </div>
  );
}
