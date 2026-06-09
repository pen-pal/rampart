// Shared theme toggle. Cycles light → dark → system → light.
//
// Rendered in two places:
//   - Dashboard header (inline, alongside the bell + nav)
//   - App-level fixed-position button (visible on every view that the
//     header doesn't already chrome)
//
// One component, one persisted preference, one Pure-CSS dark-mode flip
// via the data-theme="dark" attribute the theme runtime sets on <html>.

import React, { useState } from 'react';
import { Moon, Monitor, Sun } from 'lucide-react';
import { getTheme, setTheme } from '../lib/theme.js';

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
