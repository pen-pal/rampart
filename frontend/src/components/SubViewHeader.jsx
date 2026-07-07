import React from 'react';
import { ChevronRight } from 'lucide-react';

// Shared sticky breadcrumb header for every non-overview view. Replaces the
// bespoke `‹ Dashboard` ghost link each view used to roll on its own — that was
// inconsistent and visually bare. This frames the page like the dashboard
// topbar, gives a one-click path home (brand mark + "Overview"), and stays
// pinned while you scroll long views. The global ☰ launcher (App.jsx, fixed
// top-right) still handles jump-anywhere between siblings, so the right edge is
// padded to clear it. Rendered inside each view's `.rampart` wrapper, so it
// inherits the theme CSS vars (no hardcoded colours).
export default function SubViewHeader({ title, icon: Icon }) {
  return (
    <header
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        position: 'sticky',
        top: 0,
        zIndex: 8,
        // right padding clears the fixed top-right nav launcher
        padding: '12px 112px 12px 2px',
        marginBottom: 18,
        background: 'var(--surface)',
        borderBottom: '1px solid var(--border)',
      }}
    >
      <a
        href="#/"
        title="Back to overview"
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 8,
          textDecoration: 'none',
          color: 'var(--text-3)',
          fontSize: 13,
          fontWeight: 600,
        }}
      >
        <svg width="22" height="22" viewBox="0 0 24 24" role="img" aria-label="Rampart">
          <path fill="#3b414c" d="M12 2 L20 4 V12 C20 17 17 21 12 22 C7 21 4 17 4 12 V4 Z" />
          <path
            fill="none"
            stroke="#d27a3c"
            strokeWidth="1.6"
            strokeLinecap="round"
            strokeLinejoin="round"
            d="M6 13 H9 L11 9 L13 16 L15 7 L17 13 H18"
          />
        </svg>
        Overview
      </a>
      <ChevronRight size={14} style={{ color: 'var(--text-3)', opacity: 0.6, flexShrink: 0 }} />
      <span
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 7,
          fontSize: 14,
          fontWeight: 700,
          color: 'var(--text)',
        }}
      >
        {Icon ? <Icon size={15} /> : null} {title}
      </span>
    </header>
  );
}
