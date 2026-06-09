// Unauthenticated public status page.
// Rendered at #/s/:slug. No login chrome, no auth gating in App.jsx.
//
// Layout (top → bottom):
//   - Brand row (page title + auto-refresh indicator)
//   - Hero status banner with overall state + the "all systems operational"
//     line OR an active-incident summary
//   - KPI row: total components, components down, 90d uptime, last incident
//   - Active incidents (if any), inline cards
//   - Components list — one row per monitor, each carrying a 90-day daily
//     status bar (one cell per day, color-coded) + uptime% + avg-latency
//   - Subscribe widget (email-only, single opt-in)
//   - Footer: brand attribution + last-updated relative time
//
// The page polls the API every 30 seconds; the auto-refresh dot in the
// brand row pulses on each refresh so visitors can see the page is alive.

import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  CheckCircle2, AlertCircle, AlertTriangle, Calendar, Loader2,
  Activity, Bell, Wrench, Shield, Mail, Rss, Link2, Check, X, History,
} from 'lucide-react';
import { api, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t, getLocale } from '../lib/i18n.js';

// Timestamps from the API are `time::OffsetDateTime` → serde renders them
// as a numeric array, which `new Date()` can't parse ("Invalid Date").
// Coerce arrays via the helper; fall back to Date for ISO strings.
function toDate(v) {
  return Array.isArray(v) ? offsetDateTimeArrayToDate(v) : new Date(v);
}

// Friendly relative time like "2 minutes ago" / "yesterday" without
// pulling in a date library.
function relative(date) {
  const now = Date.now();
  const ms = now - date.getTime();
  const s = Math.round(ms / 1000);
  if (s < 5)        return t('statuspage.public.rel.just_now');
  if (s < 60)       return t('statuspage.public.rel.seconds', { n: s });
  const m = Math.round(s / 60);
  if (m < 60)       return t(m === 1 ? 'statuspage.public.rel.minute' : 'statuspage.public.rel.minutes', { n: m });
  const h = Math.round(m / 60);
  if (h < 24)       return t(h === 1 ? 'statuspage.public.rel.hour' : 'statuspage.public.rel.hours', { n: h });
  const d = Math.round(h / 24);
  if (d === 1)      return t('statuspage.public.rel.yesterday');
  if (d < 30)       return t('statuspage.public.rel.days', { n: d });
  return date.toLocaleDateString(getLocale());
}

// Two-digit UTC HH:MM for a maintenance window edge.
function utcHm(date) {
  const hh = String(date.getUTCHours()).padStart(2, '0');
  const mm = String(date.getUTCMinutes()).padStart(2, '0');
  return `${hh}:${mm}`;
}

// Human window string for a maintenance entry. "Today 02:00–04:00 UTC"
// when it starts today and has a concrete end; "02:00–04:00 UTC" / "From
// 02:00 UTC" otherwise (recurring windows carry no single end).
function maintWindow(m) {
  const start = toDate(m.starts_at);
  const end = m.ends_at != null ? toDate(m.ends_at) : null;
  const s = utcHm(start);
  const startsToday = start.getTime() - Date.now() < 86_400_000
    && start.toISOString().slice(0, 10) === new Date().toISOString().slice(0, 10);
  if (end) {
    const e = utcHm(end);
    return startsToday
      ? t('statuspage.public.maint.win.today', { start: s, end: e })
      : t('statuspage.public.maint.win.range', { start: s, end: e });
  }
  return t('statuspage.public.maint.win.from', { start: s });
}

// Coarse relative duration ("2 hours" / "40 minutes" / "now") between a
// future timestamp and now, used for both the "starts in" and "ends in"
// maintenance countdowns. Reads Date.now() at call time so each render
// recomputes against the current clock.
function maintCountdown(targetMs) {
  const ms = targetMs - Date.now();
  if (ms <= 0) return t('statuspage.public.maint.starts.now');
  const mins = Math.round(ms / 60000);
  if (mins < 60) return t(mins === 1 ? 'statuspage.public.maint.starts.minute' : 'statuspage.public.maint.starts.minutes', { n: mins });
  const hours = Math.round(mins / 60);
  if (hours < 24) return t(hours === 1 ? 'statuspage.public.maint.starts.hour' : 'statuspage.public.maint.starts.hours', { n: hours });
  const days = Math.round(hours / 24);
  return t(days === 1 ? 'statuspage.public.maint.starts.day' : 'statuspage.public.maint.starts.days', { n: days });
}

// Coarse "starts in {x}" countdown for an upcoming maintenance window.
function maintStartsIn(m) {
  return maintCountdown(toDate(m.starts_at).getTime());
}

// Coarse "ends in {x}" countdown for an active maintenance window that
// carries a concrete end. Recurring windows (no `ends_at`) return null so
// the banner falls back to the plain "Active now" pill.
function maintEndsIn(m) {
  if (m.ends_at == null) return null;
  return maintCountdown(toDate(m.ends_at).getTime());
}

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap');

  .public {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    --warn:#f59e0b; --warn-soft:#fef3c7;
    --maint:#6366f1; --maint-soft:#e0e7ff;
    --none:#e7e5e4;

    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
    font-feature-settings: 'cv11','ss01';
  }
  .public.dark {
    --bg:#0c0a09; --surface:#1c1917; --surface-2:#292524;
    --border:#292524; --border-2:#44403c;
    --text:#fafaf9; --text-2:#d6d3d1; --text-3:#78716c;
    --accent:#2dd4bf; --accent-2:#14b8a6; --accent-soft:#134e4a;
    --up:#34d399; --up-soft:#064e3b;
    --down:#f87171; --down-soft:#7f1d1d;
    --warn:#fbbf24; --warn-soft:#78350f;
    --maint:#818cf8; --maint-soft:#3730a3;
    --none:#292524;
  }
  .public * { box-sizing: border-box; }

  .card {
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 14px;
  }

  .row {
    display: grid; grid-template-columns: 1fr auto;
    column-gap: 18px;
    padding: 18px 22px; border-top: 1px solid var(--border);
    align-items: center;
  }
  .row:first-child { border-top: none; }

  /* ── Daily uptime strip ───────────────────────────────────────── */
  .strip {
    display: flex; gap: 2px; height: 34px;
    margin: 10px 0 4px;
  }
  .strip > div {
    flex: 1; min-width: 2px; border-radius: 3px;
    transition: transform .12s, opacity .12s;
    cursor: default;
    position: relative;
  }
  .strip > div:hover { transform: scaleY(1.15); }
  .strip > div { cursor: pointer; outline: none; }
  .strip > div:focus-visible { box-shadow: 0 0 0 2px var(--accent); }

  /* Day-drilldown popover anchored under the clicked cell. */
  .day-pop {
    position: absolute; z-index: 50;
    width: 300px;
    background: var(--surface); color: var(--text);
    border: 1px solid var(--border); border-radius: 10px;
    box-shadow: 0 16px 40px rgba(0,0,0,.15);
    padding: 14px 16px;
    font-size: 13px;
  }
  .day-pop .day-pop-x {
    position: absolute; top: 8px; right: 8px;
    width: 24px; height: 24px; border-radius: 6px;
    background: transparent; border: none;
    color: var(--text-3); cursor: pointer; font: inherit;
    display: flex; align-items: center; justify-content: center;
  }
  .day-pop .day-pop-x:hover { background: var(--surface-2); color: var(--text); }
  .day-pop .day-pop-date { font-weight: 600; margin-right: 32px; margin-bottom: 8px; }
  .day-pop .day-pop-pill {
    display: inline-block; padding: 3px 9px; border-radius: 999px;
    font-size: 11px; font-weight: 600; margin-bottom: 10px;
  }
  .day-pop .day-pop-pill.u { background: var(--up-soft);    color: #047857; }
  .day-pop .day-pop-pill.d { background: var(--down-soft);  color: #b91c1c; }
  .day-pop .day-pop-pill.w { background: var(--warn-soft);  color: #92400e; }
  .day-pop .day-pop-pill.m { background: var(--maint-soft); color: #4338ca; }
  .day-pop .day-pop-pill.n { background: var(--surface-2);  color: var(--text-3); }
  .public.dark .day-pop-pill.u { color: #6ee7b7; }
  .public.dark .day-pop-pill.d { color: #fca5a5; }
  .public.dark .day-pop-pill.w { color: #fde68a; }
  .public.dark .day-pop-pill.m { color: #c7d2fe; }
  /* ── Day-popover hourly latency mini-chart ───────────────────
     24 thin bars, one per UTC hour of the clicked day. Height ∝
     avg_latency_ms / max_latency_ms in the day's window; no-data
     hours render as a muted grey full-height bar so the axis stays
     visually anchored. CSS-only — no recharts dependency. */
  .day-pop .day-pop-lat {
    border-top: 1px solid var(--border);
    padding-top: 10px; margin-top: 10px;
  }
  .day-pop .day-pop-lat-title {
    font-size: 11px; color: var(--text-3); font-weight: 600;
    text-transform: uppercase; letter-spacing: .06em;
    margin-bottom: 6px;
    display: flex; align-items: center; justify-content: space-between;
  }
  .day-pop .day-pop-lat-max {
    font-weight: 500; color: var(--text-2);
    text-transform: none; letter-spacing: 0; font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }
  .day-pop .day-pop-lat-chart {
    display: grid; grid-template-columns: repeat(24, 1fr);
    gap: 1px; height: 42px; align-items: end;
  }
  .day-pop .day-pop-lat-chart > div {
    background: var(--accent);
    border-radius: 2px 2px 0 0;
    min-height: 2px;
    cursor: default;
    transition: opacity .12s;
  }
  .day-pop .day-pop-lat-chart > div:hover { opacity: .75; }
  .day-pop .day-pop-lat-chart > div.empty {
    background: var(--surface-2);
    height: 100% !important;
    opacity: .6;
  }
  .day-pop .day-pop-lat-axis {
    display: flex; justify-content: space-between;
    font-size: 9.5px; color: var(--text-3);
    margin-top: 4px; letter-spacing: .04em;
    font-variant-numeric: tabular-nums;
  }
  .day-pop .day-pop-lat-state {
    font-size: 12px; color: var(--text-3);
    padding: 12px 0; text-align: center;
  }
  .day-pop .day-pop-lat-state.err { color: var(--down); }

  .day-pop .day-pop-inc {
    border-top: 1px solid var(--border); padding-top: 10px; margin-top: 6px;
  }
  .day-pop .day-pop-inc-title {
    font-size: 11px; color: var(--text-3); font-weight: 600;
    text-transform: uppercase; letter-spacing: .06em; margin-bottom: 6px;
  }
  .day-pop ul { list-style: none; padding: 0; margin: 0; }
  .day-pop ul li {
    font-size: 12.5px; line-height: 1.4;
    padding: 4px 0;
  }
  .day-pop ul li strong { font-weight: 500; }
  .s-u { background: var(--up); opacity: .9; }
  .s-d { background: var(--down); }
  .s-w { background: var(--warn); }
  .s-m { background: var(--maint); opacity: .7; }
  .s-n { background: var(--none); }

  .strip-axis {
    display: flex; justify-content: space-between;
    font-size: 10.5px; color: var(--text-3); margin-top: 2px;
    letter-spacing: .04em;
  }

  /* ── Monthly uptime chip row ──────────────────────────────────
     12 chips under each component's daily strip — the
     "Jun 99.97% · Jul 100% · …" summary every modern status page
     ships. Chip colour reflects an SLA threshold:
        ≥99.9% green, ≥99%   amber, <99% red, no-data grey. */
  .month-row {
    display: grid; grid-template-columns: repeat(12, 1fr);
    gap: 4px; margin-top: 12px;
  }
  .month-chip {
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    padding: 6px 4px; border-radius: 6px;
    border: 1px solid var(--border);
    color: var(--text-3);
    cursor: default;
    transition: transform .12s;
  }
  .month-chip:hover { transform: translateY(-1px); }
  .month-chip-mo  { font-size: 9px; opacity: .8; margin-bottom: 2px; text-transform: uppercase; letter-spacing: .04em; font-weight: 500; }
  .month-chip-pct { font-size: 10.5px; font-weight: 600; font-variant-numeric: tabular-nums; line-height: 1; }
  .month-chip-good  { background: var(--up-soft);   color: #047857; border-color: var(--up); }
  .month-chip-okay  { background: var(--warn-soft); color: #92400e; border-color: var(--warn); }
  .month-chip-bad   { background: var(--down-soft); color: #b91c1c; border-color: var(--down); }
  .month-chip-none  { background: var(--surface-2); color: var(--text-3); }
  .public.dark .month-chip-good { color: #6ee7b7; }
  .public.dark .month-chip-okay { color: #fde68a; }
  .public.dark .month-chip-bad  { color: #fca5a5; }

  /* ── Badges ───────────────────────────────────────────────────── */
  .badge {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 5px 10px; border-radius: 999px;
    font-size: 12px; font-weight: 600;
    white-space: nowrap;
  }
  .badge-up      { background: var(--up-soft);    color: #047857; }
  .badge-down    { background: var(--down-soft);  color: #b91c1c; }
  .badge-warn    { background: var(--warn-soft);  color: #92400e; }
  .badge-paused  { background: var(--surface-2);  color: var(--text-2); }
  .badge-pending { background: var(--surface-2);  color: var(--text-3); }
  .badge-maint   { background: var(--maint-soft); color: #4338ca; }

  .public.dark .badge-up   { color: #6ee7b7; }
  .public.dark .badge-down { color: #fca5a5; }
  .public.dark .badge-warn { color: #fde68a; }
  .public.dark .badge-maint{ color: #c7d2fe; }

  /* ── Hero status banner ───────────────────────────────────────── */
  .hero {
    padding: 32px 32px; border-radius: 18px;
    display: flex; align-items: center; gap: 18px;
    font-size: 22px; font-weight: 600; letter-spacing: -.01em;
    border: 1px solid transparent;
    box-shadow: 0 1px 2px rgba(0,0,0,.03);
  }
  .hero-up   { background: var(--up-soft);   color: #047857; border-color: var(--up); }
  .hero-down { background: var(--down-soft); color: #b91c1c; border-color: var(--down); }
  .hero-warn { background: var(--warn-soft); color: #92400e; border-color: var(--warn); }
  .hero-maint{ background: var(--maint-soft); color: #4338ca; border-color: var(--maint); }
  .hero-icon {
    width: 52px; height: 52px; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    background: rgba(255,255,255,.55); flex-shrink: 0;
  }
  .public.dark .hero { box-shadow: none; }
  .public.dark .hero-up   { color: #6ee7b7; }
  .public.dark .hero-down { color: #fca5a5; }
  .public.dark .hero-warn { color: #fde68a; }
  .public.dark .hero-maint{ color: #c7d2fe; }
  .public.dark .hero-icon { background: rgba(0,0,0,.3); }

  /* ── KPI row ──────────────────────────────────────────────────── */
  .kpis {
    display: grid; grid-template-columns: repeat(4, 1fr); gap: 14px;
  }
  .kpi {
    padding: 16px 18px;
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 12px;
  }
  .kpi-label {
    font-size: 11px; color: var(--text-3); font-weight: 600;
    text-transform: uppercase; letter-spacing: .06em;
    display: flex; align-items: center; gap: 6px;
  }
  .kpi-value {
    font-size: 24px; font-weight: 600; letter-spacing: -.02em;
    margin-top: 6px; line-height: 1;
  }
  .kpi-sub {
    font-size: 11px; color: var(--text-3); margin-top: 4px;
  }

  /* ── Subscribe ────────────────────────────────────────────────── */
  .subscribe-card {
    padding: 22px 26px;
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 14px;
    display: flex; align-items: center; gap: 18px; flex-wrap: wrap;
  }
  .subscribe-card h3 {
    margin: 0 0 4px; font-size: 15px; font-weight: 600;
  }
  .subscribe-card p {
    margin: 0; font-size: 12.5px; color: var(--text-3);
  }
  .sub-input {
    flex: 1; min-width: 240px;
    padding: 10px 14px; border-radius: 9px;
    background: var(--surface); border: 1px solid var(--border);
    color: var(--text); font-size: 13.5px; font-family: inherit; outline: none;
    transition: border-color .12s, box-shadow .12s;
  }
  .sub-input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .sub-input::placeholder { color: var(--text-3); }
  .sub-btn {
    padding: 10px 16px; border-radius: 9px;
    background: var(--accent); color: white; border: 1px solid var(--accent);
    font-size: 13.5px; font-weight: 500; cursor: pointer; font-family: inherit;
    transition: background .12s;
  }
  .sub-btn:hover:not([disabled]) { background: var(--accent-2); }
  .sub-btn[disabled] { opacity: .6; cursor: not-allowed; }

  /* ── Scheduled-maintenance banner ─────────────────────────────────
     Distinct from incident cards: keyed off the --maint colour token so
     planned work reads as "informational, not an outage". Active windows
     get the filled treatment; upcoming ones a softer outlined card. */
  .maint-block { margin-top: 18px; display: flex; flex-direction: column; gap: 10px; }
  .maint-card {
    border-radius: 12px; padding: 16px 20px;
    border: 1px solid var(--maint);
    background: var(--maint-soft); color: #4338ca;
    display: grid; grid-template-columns: auto 1fr auto; gap: 14px;
    align-items: start;
  }
  .maint-card.upcoming {
    background: var(--surface);
    border-style: dashed;
    color: var(--text);
  }
  .public.dark .maint-card { color: #c7d2fe; }
  .public.dark .maint-card.upcoming { color: var(--text); }
  .maint-icon {
    width: 34px; height: 34px; border-radius: 9px;
    background: rgba(255,255,255,.5);
    display: flex; align-items: center; justify-content: center;
    color: var(--maint); flex-shrink: 0;
  }
  .public.dark .maint-icon { background: rgba(0,0,0,.25); }
  .maint-card.upcoming .maint-icon { background: var(--maint-soft); }
  .maint-head {
    font-size: 10.5px; font-weight: 600; text-transform: uppercase;
    letter-spacing: .06em; opacity: .85; margin-bottom: 3px;
  }
  .maint-title { font-size: 14.5px; font-weight: 600; letter-spacing: -.01em; }
  .maint-desc { font-size: 12.5px; opacity: .85; margin-top: 4px; line-height: 1.45; white-space: pre-wrap; }
  .maint-when {
    font-size: 11.5px; font-weight: 600; white-space: nowrap;
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 10px; border-radius: 999px;
    background: rgba(255,255,255,.45);
    align-self: center;
  }
  .public.dark .maint-when { background: rgba(0,0,0,.25); }
  .maint-card.upcoming .maint-when { background: var(--maint-soft); color: #4338ca; }
  .public.dark .maint-card.upcoming .maint-when { color: #c7d2fe; }

  /* ── Section header ───────────────────────────────────────────── */
  .section-h {
    font-size: 12px; font-weight: 600; color: var(--text-2);
    text-transform: uppercase; letter-spacing: .06em;
    margin: 30px 0 12px;
    display: flex; align-items: center; gap: 8px;
  }

  /* ── Brand row ────────────────────────────────────────────────── */
  .brand-row {
    display: flex; align-items: center; justify-content: space-between;
    margin-bottom: 24px;
  }
  .brand-row h1 {
    font-size: 26px; font-weight: 600; margin: 0; letter-spacing: -.02em;
  }
  .live-dot {
    display: inline-flex; align-items: center; gap: 7px;
    font-size: 11.5px; color: var(--text-3); font-weight: 500;
  }
  .live-dot::before {
    content: ''; width: 7px; height: 7px; border-radius: 50%;
    background: var(--up); box-shadow: 0 0 0 3px var(--up-soft);
    animation: pulse 2s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1;   transform: scale(1);   }
    50%      { opacity: .55; transform: scale(.85); }
  }

  /* ── Footer ───────────────────────────────────────────────────── */
  .footer {
    margin-top: 40px; padding-top: 24px;
    border-top: 1px solid var(--border);
    display: flex; justify-content: space-between; align-items: center;
    font-size: 11.5px; color: var(--text-3);
  }
  .footer a { color: var(--text-2); text-decoration: none; }
  .footer a:hover { color: var(--text); }

  /* ── Legend pills under the strip ─────────────────────────────── */
  .legend {
    display: flex; gap: 14px; flex-wrap: wrap;
    font-size: 11px; color: var(--text-3);
    margin-top: 18px;
  }
  .legend-dot {
    display: inline-block; width: 10px; height: 10px;
    border-radius: 2px; margin-right: 6px; vertical-align: -1px;
  }

  @media (max-width: 640px) {
    .kpis { grid-template-columns: repeat(2, 1fr); }
    .hero { padding: 22px 20px; font-size: 18px; }
    .month-row { grid-template-columns: repeat(6, 1fr); }
    .month-row > :nth-child(-n+6) { display: none; }
  }
`;

// Status → localized badge label. A function (not a const map) so the
// lookup happens at render time against the active locale.
function statusLabel(status) {
  const key = {
    up:          'statuspage.public.status.up',
    down:        'statuspage.public.status.down',
    warn:        'statuspage.public.status.degraded',
    paused:      'statuspage.public.status.paused',
    pending:     'statuspage.public.status.pending',
    maintenance: 'statuspage.public.status.maintenance',
  }[status];
  return key ? t(key) : status;
}

function statusIcon(status, size = 14) {
  if (status === 'down')        return <AlertCircle  size={size}/>;
  if (status === 'warn')        return <AlertTriangle size={size}/>;
  if (status === 'maintenance') return <Wrench size={size}/>;
  return <CheckCircle2 size={size}/>;
}

// Rolled-up overall: worst-wins. Maintenance is treated as informational
// (overrides Up, but Down wins over Maintenance — a real outage still surfaces).
function overall(monitors) {
  if (monitors.some(m => m.current_status === 'down')) return 'down';
  if (monitors.some(m => m.current_status === 'warn')) return 'warn';
  if (monitors.some(m => m.current_status === 'maintenance')) return 'maintenance';
  return 'up';
}

// Avg of `uptime_90d` across all monitors, weighted equally.
function avgUptime(monitors) {
  const have = monitors.filter(m => m.uptime_90d != null);
  if (have.length === 0) return null;
  return have.reduce((s, m) => s + m.uptime_90d, 0) / have.length;
}

// Renders the public status page. Two entry points:
//   - `slug`         the #/s/:slug hash route (loads by slug)
//   - `byDomainHost` host-header routing (loads by the page's custom domain)
// When `byDomainHost` is set, the initial fetch + 30s poll go through the
// by-domain endpoint. The resolved slug from that payload still flows down to
// the day-latency / subscribe children so those keep working on a custom
// domain just like on the hash route.
export default function StatusPageView({ slug, byDomainHost }) {
  const [data,    setData]    = useState(null);
  const [error,   setError]   = useState(null);
  const [loading, setLoading] = useState(true);

  // Client-side tick (every 30s) that re-renders the maintenance banner so
  // its relative countdowns ("Starts in 2h", "Ends in 40m") stay honest
  // between the 30s data polls. Purely a re-render trigger — it does NOT
  // re-fetch; the countdown strings recompute from the start/end
  // timestamps already in `data`.
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick(n => n + 1), 30_000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const r = byDomainHost
          ? await api.statusPages.byDomain(byDomainHost)
          : await api.statusPages.publicView(slug);
        if (!cancelled) { setData(r); setError(null); setLoading(false); }
      } catch (e) {
        if (!cancelled) { setError(e); setLoading(false); }
      }
    };
    load();
    // Light polling so the page updates without a manual refresh.
    const t = setInterval(load, 30_000);
    return () => { cancelled = true; clearInterval(t); };
  }, [slug, byDomainHost]);

  // The slug threaded to child components (day-latency, subscribe, feeds).
  // On the by-domain path the prop slug is undefined, so fall back to the
  // slug the API resolved from the domain.
  const effectiveSlug = slug || data?.slug;

  if (loading) {
    return (
      <div className="public">
        <style>{css}</style>
        <div style={{ maxWidth: 720, margin: '0 auto', padding: '120px 24px', textAlign: 'center', color: 'var(--text-3)' }}>
          <Loader2 size={28} style={{ animation: 'spin 1s linear infinite' }}/>
        </div>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="public">
        <style>{css}</style>
        <div style={{ maxWidth: 560, margin: '0 auto', padding: '120px 24px', textAlign: 'center' }}>
          <AlertCircle size={36} color="var(--text-3)" style={{ marginBottom: 14 }}/>
          <h1 style={{ fontSize: 24, fontWeight: 600, margin: '0 0 8px' }}>{t('statuspage.public.not_found.title')}</h1>
          <p style={{ fontSize: 14, color: 'var(--text-2)', margin: 0 }}>
            {t('statuspage.public.not_found.body_pre')} <code style={{ background: 'var(--surface-2)', padding: '2px 6px', borderRadius: 4 }}>{byDomainHost ? byDomainHost : `/${slug}`}</code>{t('statuspage.public.not_found.body_post')}
          </p>
        </div>
      </div>
    );
  }

  const themeClass = data.theme === 'dark' ? 'public dark' : 'public';
  const monitors = data.monitors || [];
  const incidents = data.incidents || [];
  const hasIncidents = incidents.length > 0;

  let status = overall(monitors);
  if (status === 'up' && hasIncidents) status = 'warn';

  const hero = {
    up:          { title: t('statuspage.public.hero.up.title'),   sub: t('statuspage.public.hero.up.sub'),   cls: 'hero-up' },
    warn:        { title: hasIncidents ? t('statuspage.public.hero.incident.title') : t('statuspage.public.hero.degraded.title'), sub: hasIncidents ? t('statuspage.public.hero.incident.sub') : t('statuspage.public.hero.degraded.sub'), cls: 'hero-warn' },
    down:        { title: t('statuspage.public.hero.down.title'), sub: t('statuspage.public.hero.down.sub'), cls: 'hero-down' },
    maintenance: { title: t('statuspage.public.hero.maint.title'), sub: t('statuspage.public.hero.maint.sub'), cls: 'hero-maint' },
  }[status];

  const downCount = monitors.filter(m => m.current_status === 'down').length;
  const warnCount = monitors.filter(m => m.current_status === 'warn').length;
  const upCount   = monitors.filter(m => m.current_status === 'up').length;
  const overallUptime = avgUptime(monitors);

  return (
    <div className={themeClass}>
      <style>{css}</style>

      <div style={{ maxWidth: 880, margin: '0 auto', padding: '40px 24px 24px' }}>

        {/* ── Brand row ──────────────────────────────────────────── */}
        <div className="brand-row">
          <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
            {data.logo_url && (
              <img
                src={data.logo_url}
                alt=""
                className="brand-logo"
                style={{ maxHeight: 40, maxWidth: 160, objectFit: 'contain', flexShrink: 0 }}
              />
            )}
            <div>
              <h1>{data.title}</h1>
              {data.description && (
                <p style={{ fontSize: 13.5, color: 'var(--text-3)', margin: '4px 0 0' }}>{data.description}</p>
              )}
            </div>
          </div>
          <div className="live-dot">{t('statuspage.public.live_refresh')}</div>
        </div>

        {/* ── Hero status banner ─────────────────────────────────── */}
        <div className={`hero ${hero.cls}`}>
          <div className="hero-icon">{statusIcon(status, 26)}</div>
          <div>
            <div>{hero.title}</div>
            <div style={{ fontSize: 13, fontWeight: 400, opacity: .85, marginTop: 4 }}>{hero.sub}</div>
          </div>
        </div>

        {/* ── KPIs ───────────────────────────────────────────────── */}
        <div className="kpis" style={{ marginTop: 18 }}>
          <Kpi icon={<Activity size={12}/>} label={t('statuspage.public.kpi.components')} value={String(monitors.length)} sub={t('statuspage.public.kpi.components_sub', { up: upCount, degraded: warnCount, down: downCount })}/>
          <Kpi icon={<Shield size={12}/>}   label={t('statuspage.public.kpi.uptime_90d')}  value={overallUptime == null ? '—' : `${overallUptime.toFixed(2)}%`} sub={t('statuspage.public.kpi.uptime_90d_sub')}/>
          <Kpi icon={<Bell size={12}/>}     label={t('statuspage.public.kpi.active_incidents')} value={String(incidents.length)} sub={incidents.length ? t('statuspage.public.kpi.incidents_see_below') : t('statuspage.public.kpi.incidents_none')}/>
          <Kpi icon={<Calendar size={12}/>} label={t('statuspage.public.kpi.last_update')}   value={data.generated_at ? relative(toDate(data.generated_at)) : '—'} sub={t('statuspage.public.kpi.last_update_sub')}/>
        </div>

        {/* ── Scheduled-maintenance banner (above components — planned
             work is context the visitor should see before scanning the
             component list; distinct --maint styling keeps it from
             reading as an outage). ── */}
        {(data.maintenance || []).length > 0 && (
          <div className="maint-block">
            {(data.maintenance || []).map((m, i) => <MaintenanceBanner key={i} entry={m}/>)}
          </div>
        )}

        {/* ── Components (always above incidents — components are the
             primary signal; incident threads sit below as context) ── */}
        <div className="section-h"><Activity size={13}/> {t('statuspage.public.section.components')}</div>
        <div className="card" style={{ overflow: 'hidden' }}>
          {monitors.length === 0 ? (
            <div style={{ padding: 36, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
              {t('statuspage.public.empty.components')}
            </div>
          ) : monitors.map((m, i) => (
            <Component
              key={i}
              monitor={m}
              monitorIdx={i}
              slug={effectiveSlug}
              activeIncidents={incidents}
              incidentHistory={data.incident_history || []}
            />
          ))}
        </div>

        {/* ── Legend ───────────────────────────────────────────── */}
        <div className="legend">
          <span><span className="legend-dot" style={{ background: 'var(--up)' }}/> {t('statuspage.public.legend.operational')}</span>
          <span><span className="legend-dot" style={{ background: 'var(--warn)' }}/> {t('statuspage.public.legend.degraded')}</span>
          <span><span className="legend-dot" style={{ background: 'var(--down)' }}/> {t('statuspage.public.legend.down')}</span>
          <span><span className="legend-dot" style={{ background: 'var(--maint)' }}/> {t('statuspage.public.legend.maintenance')}</span>
          <span><span className="legend-dot" style={{ background: 'var(--none)' }}/> {t('statuspage.public.legend.no_data')}</span>
        </div>

        {/* ── Active incidents (below components — components are the
             primary signal; incident threads sit below as context.
             Backend's `list_active` orders pinned-first, so the pin
             ranking takes effect inside this section only — pinning
             never floats incidents above the components list). ── */}
        {hasIncidents && (
          <>
            <div className="section-h"><Bell size={13}/> {t('statuspage.public.section.active_incidents')}</div>
            <div>
              {incidents.map((inc, i) => <Incident key={i} incident={inc}/>)}
            </div>
          </>
        )}

        {/* ── Incident history ────────────────────────────────── */}
        {(data.incident_history || []).length > 0 && (
          <>
            <div className="section-h"><History size={13}/> {t('statuspage.public.section.incident_history')}</div>
            <div>
              {(data.incident_history || []).map((inc, i) => <ResolvedIncident key={i} incident={inc}/>)}
            </div>
          </>
        )}

        {/* ── Subscribe ────────────────────────────────────────── */}
        <div className="section-h"><Bell size={13}/> {t('statuspage.public.section.subscribe')}</div>
        <SubscribeBox slug={effectiveSlug}/>

        {/* ── Footer ───────────────────────────────────────────── */}
        <div className="footer">
          <span>{t('statuspage.public.footer.powered_by')} <a href="https://github.com/pen-pal/rampart" target="_blank" rel="noreferrer">Rampart</a></span>
          <span>{t('statuspage.public.footer.last_updated', { when: data.generated_at ? relative(toDate(data.generated_at)) : '—' })}</span>
        </div>
      </div>
    </div>
  );
}

function Kpi({ icon, label, value, sub }) {
  return (
    <div className="kpi">
      <div className="kpi-label">{icon} {label}</div>
      <div className="kpi-value">{value}</div>
      {sub && <div className="kpi-sub">{sub}</div>}
    </div>
  );
}

// One scheduled-maintenance banner card. Active windows render filled
// with an "Active now" pill; upcoming ones use a softer dashed card with
// a "Starts in {x}" pill. Title/description are operator-authored data —
// only the chrome (heading + pills + window string) is localized.
function MaintenanceBanner({ entry }) {
  return (
    <div className={`maint-card ${entry.active ? 'active' : 'upcoming'}`}>
      <div className="maint-icon"><Wrench size={17}/></div>
      <div>
        <div className="maint-head">{t('statuspage.public.maint.heading')}</div>
        <div className="maint-title">{entry.title}</div>
        {entry.description && <div className="maint-desc">{entry.description}</div>}
        <div className="maint-desc" style={{ opacity: .95, fontWeight: 500, marginTop: 6 }}>{maintWindow(entry)}</div>
      </div>
      <span className="maint-when">
        {entry.active
          ? (() => {
              // Active windows with a concrete end show a live "Ends in
              // {x}" countdown; recurring windows (no end) keep the plain
              // "Active now" pill.
              const endsIn = maintEndsIn(entry);
              return endsIn
                ? <>{statusIcon('maintenance', 12)} {t('statuspage.public.maint.ends_in')} {endsIn}</>
                : <>{statusIcon('maintenance', 12)} {t('statuspage.public.maint.active')}</>;
            })()
          : <>{t('statuspage.public.maint.starts_in')} {maintStartsIn(entry)}</>}
      </span>
    </div>
  );
}

function Component({ monitor, monitorIdx, slug, activeIncidents = [], incidentHistory = [] }) {
  const status = monitor.current_status;
  const statusKey = status === 'maintenance' ? 'maint' : status;
  // Daily-status strip carries 90 chars. If the backend hasn't backfilled
  // a row yet it might be shorter — pad with 'n' so the strip always has
  // 90 cells (otherwise it'd render at variable widths between monitors).
  const strip = (monitor.daily_status_90d || '').padStart(90, 'n').slice(-90);

  // Click-day drilldown popover. `openCell` is the cell-index from
  // strip-left; `null` means closed.
  const [openCell, setOpenCell] = useState(null);
  const stripRef = useRef(null);
  useEffect(() => {
    if (openCell == null) return undefined;
    const onKey = (e) => { if (e.key === 'Escape') setOpenCell(null); };
    const onClick = (e) => {
      if (!stripRef.current) return;
      if (!stripRef.current.parentElement.contains(e.target)) setOpenCell(null);
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('click', onClick, true);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('click', onClick, true);
    };
  }, [openCell]);

  return (
    <div className="row">
      <div style={{ position: 'relative' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 2 }}>
          <span style={{ fontSize: 15, fontWeight: 500 }}>{monitor.name}</span>
        </div>
        <div className="strip" ref={stripRef} title={t('statuspage.public.strip.click_hint')}>
          {strip.split('').map((c, i) => {
            const daysAgo = 90 - 1 - i;
            return (
              <div
                key={i}
                className={`s-${c}`}
                tabIndex={0}
                role="button"
                aria-label={cellTitle(c, daysAgo)}
                title={cellTitle(c, daysAgo)}
                onClick={(e) => { e.stopPropagation(); setOpenCell(openCell === i ? null : i); }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    setOpenCell(openCell === i ? null : i);
                  }
                }}
              />
            );
          })}
        </div>
        {openCell != null && (
          <DayPopover
            cellIndex={openCell}
            statusChar={strip[openCell]}
            slug={slug}
            monitorIdx={monitorIdx}
            activeIncidents={activeIncidents}
            incidentHistory={incidentHistory}
            onClose={() => setOpenCell(null)}
          />
        )}
        <div className="strip-axis">
          <span>{t('statuspage.public.strip.days_ago_90')}</span>
          <span>
            {monitor.uptime_90d == null
              ? t('statuspage.public.strip.no_data')
              : t('statuspage.public.strip.uptime', { pct: monitor.uptime_90d.toFixed(2) })}
            {monitor.avg_latency_ms_24h != null && (
              <> {t('statuspage.public.strip.avg_latency', { ms: Math.round(monitor.avg_latency_ms_24h) })}</>
            )}
          </span>
          <span>{t('statuspage.public.strip.today')}</span>
        </div>
        {(monitor.monthly_uptime_12mo || []).length > 0 && (
          <MonthRow months={monitor.monthly_uptime_12mo}/>
        )}
      </div>
      <span className={`badge badge-${statusKey}`}>
        {statusIcon(status)} {statusLabel(status)}
      </span>
    </div>
  );
}

function MonthRow({ months }) {
  return (
    <div className="month-row">
      {months.map((m, i) => <MonthChip key={i} point={m}/>)}
    </div>
  );
}

const MONTH_SHORT = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];

function MonthChip({ point }) {
  // year_month from the backend is a `time::Date` which serde renders as
  // [yyyy, ordinal-day]. Handle both that array shape and a plain ISO
  // date string defensively.
  const ym = point.year_month;
  let label = '?';
  if (Array.isArray(ym) && ym.length >= 2) {
    // [year, day_of_year] — convert ordinal → month.
    const d = new Date(Date.UTC(ym[0], 0, ym[1]));
    label = MONTH_SHORT[d.getUTCMonth()];
  } else if (typeof ym === 'string') {
    const d = new Date(ym);
    if (!isNaN(d.getTime())) label = MONTH_SHORT[d.getUTCMonth()];
  }

  const pct = point.uptime_pct;
  let cls = 'month-chip month-chip-none';
  let pctStr = '—';
  if (pct != null) {
    pctStr = pct >= 99.995 ? '100%' : `${pct.toFixed(2)}%`;
    if (pct >= 99.9)      cls = 'month-chip month-chip-good';
    else if (pct >= 99.0) cls = 'month-chip month-chip-okay';
    else                  cls = 'month-chip month-chip-bad';
  }
  const title = pct == null
    ? t('statuspage.public.month.no_data', { month: label })
    : t('statuspage.public.month.uptime', { month: label, pct: pct.toFixed(3) });
  return (
    <div className={cls} title={title}>
      <span className="month-chip-mo">{label}</span>
      <span className="month-chip-pct">{pctStr}</span>
    </div>
  );
}

/** UTC midnight Date for `daysAgo` days back (0 = today). */
function dayStartUtc(daysAgo) {
  const d = new Date();
  d.setUTCHours(0, 0, 0, 0);
  d.setUTCDate(d.getUTCDate() - daysAgo);
  return d;
}

/** True when `inc`'s active window touches the `[dayStart, dayEnd)` range. */
function incidentTouchesDay(inc, dayStart, dayEnd) {
  const created = toDate(inc.created_at).getTime();
  const resolved = inc.resolved_at != null
    ? toDate(inc.resolved_at).getTime()
    : Date.now();
  return created < dayEnd.getTime() && resolved >= dayStart.getTime();
}

// Day-status char → { localized label, css class }. A function (not a
// const map) so labels resolve against the active locale at render time.
const DAY_STATUS_CLS = { u: 'u', d: 'd', w: 'w', m: 'm', n: 'n' };
function dayStatus(c) {
  const cls = DAY_STATUS_CLS[c] || 'n';
  const label = {
    u: t('statuspage.public.day.operational'),
    d: t('statuspage.public.day.down'),
    w: t('statuspage.public.day.degraded'),
    m: t('statuspage.public.day.maintenance'),
    n: t('statuspage.public.day.no_data'),
  }[cls];
  return { label, cls };
}

function DayPopover({ cellIndex, statusChar, slug, monitorIdx, activeIncidents, incidentHistory, onClose }) {
  const closeRef = useRef(null);
  useEffect(() => { closeRef.current?.focus(); }, []);
  const daysAgo = 90 - 1 - cellIndex;
  const dayStart = dayStartUtc(daysAgo);
  const dayEnd = new Date(dayStart.getTime() + 86_400_000);
  const meta = dayStatus(statusChar);
  const all = [...activeIncidents, ...incidentHistory];
  const touching = all.filter(inc => incidentTouchesDay(inc, dayStart, dayEnd));

  // Hourly-latency chart is only meaningful when at least some heartbeats
  // are expected to be `up` — suppress for hard-down, maintenance, and
  // no-data days where the bars would be empty or misleading.
  const showLatency = statusChar === 'u' || statusChar === 'w';
  const isoDate = dayStart.toISOString().slice(0, 10);
  const [latency, setLatency] = useState({ state: 'idle', hours: [], error: null });

  useEffect(() => {
    if (!showLatency || slug == null || monitorIdx == null) return undefined;
    let cancelled = false;
    setLatency({ state: 'loading', hours: [], error: null });
    api.statusPages.dayLatency(slug, monitorIdx, isoDate)
      .then(r => { if (!cancelled) setLatency({ state: 'ok', hours: r.hours || [], error: null }); })
      .catch(e => { if (!cancelled) setLatency({ state: 'err', hours: [], error: e?.message || 'failed to load' }); });
    return () => { cancelled = true; };
  }, [showLatency, slug, monitorIdx, isoDate]);

  // Position the popover so its left edge sits over the clicked cell.
  // 90 cells span the .strip's full width — cellIndex/89 maps to the
  // strip's horizontal range. Clamp at the edges so the popover
  // doesn't escape the row.
  const pct = Math.max(2, Math.min(98, (cellIndex / 89) * 100));
  const transform =
    cellIndex < 12 ? 'translateX(0)' :
    cellIndex > 77 ? 'translateX(-100%)' :
    'translateX(-50%)';

  const dateLabel = dayStart.toLocaleDateString(getLocale(), {
    weekday: 'short', month: 'long', day: 'numeric', year: 'numeric',
    timeZone: 'UTC',
  });

  return (
    <div className="day-pop" style={{ top: 'calc(100% + 4px)', left: `${pct}%`, transform }}
         onClick={e => e.stopPropagation()}>
      <button ref={closeRef} className="day-pop-x" aria-label={t('statuspage.public.close')} onClick={onClose}>×</button>
      <div className="day-pop-date">{dateLabel}</div>
      <span className={`day-pop-pill ${meta.cls}`}>{meta.label}</span>
      {showLatency && <HourlyLatencyChart latency={latency}/>}
      {!showLatency && statusChar === 'u' && (
        <div style={{ fontSize: 12.5, color: 'var(--text-3)' }}>
          {t('statuspage.public.day.all_passed')}
        </div>
      )}
      {touching.length > 0 && (
        <div className="day-pop-inc">
          <div className="day-pop-inc-title">{t('statuspage.public.day.incidents_on_date')}</div>
          <ul>
            {touching.map((inc, i) => (
              <li key={i}>
                <strong>{inc.title}</strong>
                {inc.resolved_at != null && (
                  <span style={{ color: 'var(--text-3)' }}> {t('statuspage.public.day.resolved_suffix')}</span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

/** 24-bar hourly latency mini-chart for the day-drilldown popover.
 *  `latency` is `{ state: 'idle'|'loading'|'ok'|'err', hours, error }`
 *  shaped by `DayPopover`. Pure CSS bars — no chart library. */
function HourlyLatencyChart({ latency }) {
  if (latency.state === 'loading' || latency.state === 'idle') {
    return (
      <div className="day-pop-lat">
        <div className="day-pop-lat-title">{t('statuspage.public.latency.title')}</div>
        <div className="day-pop-lat-state">
          <Loader2 size={14} style={{ animation: 'spin 1s linear infinite', verticalAlign: 'middle' }}/>
          <span style={{ marginLeft: 6 }}>{t('statuspage.public.latency.loading')}</span>
        </div>
      </div>
    );
  }
  if (latency.state === 'err') {
    return (
      <div className="day-pop-lat">
        <div className="day-pop-lat-title">{t('statuspage.public.latency.title')}</div>
        <div className="day-pop-lat-state err">{t('statuspage.public.latency.error')}</div>
      </div>
    );
  }

  // Build a dense 24-entry view. Backend already returns 24 rows, but be
  // defensive against partial responses.
  const byHour = new Map(latency.hours.map(h => [h.hour, h]));
  const dense = Array.from({ length: 24 }, (_, h) => byHour.get(h) || { hour: h, avg_latency_ms: null, samples: 0 });
  const valid = dense.filter(h => h.avg_latency_ms != null);
  if (valid.length === 0) {
    return (
      <div className="day-pop-lat">
        <div className="day-pop-lat-title">{t('statuspage.public.latency.title')}</div>
        <div className="day-pop-lat-state">{t('statuspage.public.latency.no_samples')}</div>
      </div>
    );
  }
  const maxMs = Math.max(...valid.map(h => h.avg_latency_ms));

  return (
    <div className="day-pop-lat">
      <div className="day-pop-lat-title">
        <span>{t('statuspage.public.latency.title')}</span>
        <span className="day-pop-lat-max">{t('statuspage.public.latency.peak', { ms: Math.round(maxMs) })}</span>
      </div>
      <div className="day-pop-lat-chart">
        {dense.map((h) => {
          const hh = String(h.hour).padStart(2, '0');
          if (h.avg_latency_ms == null) {
            return (
              <div
                key={h.hour}
                className="empty"
                title={t('statuspage.public.latency.hour_no_data', { hh })}
                aria-label={t('statuspage.public.latency.hour_no_data', { hh })}
              />
            );
          }
          // Normalize to the day's peak; clamp the floor so tiny values
          // still render as a visible 2px nub.
          const pct = maxMs > 0 ? (h.avg_latency_ms / maxMs) * 100 : 0;
          const ms = Math.round(h.avg_latency_ms);
          return (
            <div
              key={h.hour}
              style={{ height: `${Math.max(pct, 5)}%` }}
              title={t(h.samples === 1 ? 'statuspage.public.latency.hour_ms_one' : 'statuspage.public.latency.hour_ms', { hh, ms, n: h.samples })}
              aria-label={t('statuspage.public.latency.hour_ms_aria', { hh, ms, n: h.samples })}
            />
          );
        })}
      </div>
      <div className="day-pop-lat-axis">
        <span>00</span><span>06</span><span>12</span><span>18</span><span>23</span>
      </div>
    </div>
  );
}

function cellTitle(c, daysAgo) {
  const label = {
    u: t('statuspage.public.day.operational'),
    d: t('statuspage.public.day.down'),
    w: t('statuspage.public.day.degraded'),
    m: t('statuspage.public.day.maintenance'),
    n: t('statuspage.public.day.no_data'),
  }[c] || t('statuspage.public.day.unknown');
  if (daysAgo === 0)         return t('statuspage.public.cell.today', { label });
  if (daysAgo === 1)         return t('statuspage.public.cell.yesterday', { label });
  return t('statuspage.public.cell.days_ago', { label, n: daysAgo });
}

function SubscribeBox({ slug }) {
  // Tabs: 'email' | 'rss' | 'atom' | 'webhook'
  const [tab, setTab] = useState('email');

  const tabs = [
    { id: 'email',   label: t('statuspage.public.subscribe.email_tab'),   icon: Mail },
    { id: 'rss',     label: t('statuspage.public.subscribe.rss_tab'),     icon: Rss },
    { id: 'atom',    label: t('statuspage.public.subscribe.atom_tab'),    icon: Rss },
    { id: 'webhook', label: t('statuspage.public.subscribe.webhook_tab'), icon: Link2 },
  ];

  return (
    <div className="subscribe-card" style={{ display: 'block', padding: '6px 6px 22px' }}>
      <div style={{
        display: 'flex', gap: 4, padding: 6,
        background: 'var(--surface-2)', borderRadius: 10,
        marginBottom: 18,
      }}>
        {tabs.map(t => {
          const Icon = t.icon;
          const active = tab === t.id;
          return (
            <button key={t.id} type="button" onClick={() => setTab(t.id)}
              style={{
                flex: 1, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 6,
                padding: '9px 12px', borderRadius: 7,
                background: active ? 'var(--surface)' : 'transparent',
                color: active ? 'var(--text)' : 'var(--text-3)',
                border: 'none', cursor: 'pointer', font: 'inherit',
                fontSize: 12.5, fontWeight: 500,
                boxShadow: active ? '0 1px 2px rgba(0,0,0,.05)' : 'none',
                transition: 'all .12s',
              }}>
              <Icon size={13}/> {t.label}
            </button>
          );
        })}
      </div>
      <div style={{ padding: '0 18px' }}>
        {tab === 'email'   && <EmailTab slug={slug}/>}
        {tab === 'rss'     && <FeedTab slug={slug} kind="rss"/>}
        {tab === 'atom'    && <FeedTab slug={slug} kind="atom"/>}
        {tab === 'webhook' && <WebhookTab/>}
      </div>
    </div>
  );
}

function EmailTab({ slug }) {
  const [email, setEmail] = useState('');
  const [state, setState] = useState('idle');
  const [err,   setErr]   = useState(null);

  const submit = async (e) => {
    e.preventDefault();
    if (!email.includes('@')) { setErr(t('statuspage.public.subscribe.err_email')); setState('err'); return; }
    setState('sending'); setErr(null);
    try {
      await api.subscribers.subscribe(slug, email.trim());
      setState('ok');
    } catch (e2) {
      setErr(e2.message || t('statuspage.public.subscribe.err_failed'));
      setState('err');
    }
  };

  if (state === 'ok') {
    return (
      <div style={{
        background: 'var(--up-soft)', color: '#047857',
        padding: '14px 18px', borderRadius: 9, fontSize: 13.5, fontWeight: 500,
        display: 'flex', alignItems: 'center', gap: 10,
      }}>
        <Check size={16}/> {t('statuspage.public.subscribe.success')}
      </div>
    );
  }

  return (
    <>
      <p style={{ margin: '0 0 12px', fontSize: 13, color: 'var(--text-2)', lineHeight: 1.5 }}>
        {t('statuspage.public.subscribe.email_blurb')}
      </p>
      <form onSubmit={submit} style={{ display: 'flex', gap: 8 }}>
        <input className="sub-input" type="email" value={email}
          onChange={e => setEmail(e.target.value)}
          placeholder={t('statuspage.public.subscribe.email_placeholder')}
          required/>
        <button className="sub-btn" type="submit" disabled={state === 'sending'}>
          {state === 'sending' ? t('statuspage.public.subscribe.subscribing') : t('statuspage.public.subscribe.subscribe')}
        </button>
      </form>
      {state === 'err' && err && (
        <div style={{ fontSize: 12, color: 'var(--down)', marginTop: 8, fontWeight: 500 }}>{err}</div>
      )}
    </>
  );
}

function CopyButton({ value }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      // Older browsers / non-secure contexts — fall back to a temp
      // textarea selectall+copy. Quiet fallback; no toast.
      const el = document.createElement('textarea');
      el.value = value;
      document.body.appendChild(el);
      el.select();
      try { document.execCommand('copy'); setCopied(true); setTimeout(() => setCopied(false), 1800); } catch {}
      document.body.removeChild(el);
    }
  };
  return (
    <button onClick={copy} className="sub-btn" type="button" style={{ padding: '8px 12px' }}>
      {copied ? <><Check size={13}/> {t('statuspage.public.copy.copied')}</> : <><Link2 size={13}/> {t('statuspage.public.copy.copy')}</>}
    </button>
  );
}

function FeedTab({ slug, kind }) {
  // URL is relative to the current origin so it works whether the page
  // is served from `rampart.example.com` or behind a path-prefix proxy.
  const url = `${window.location.origin}/v1/public/status-pages/${slug}/feed.${kind}`;
  const label = kind === 'atom' ? 'Atom 1.0' : 'RSS 2.0';
  return (
    <>
      <p style={{ margin: '0 0 12px', fontSize: 13, color: 'var(--text-2)', lineHeight: 1.5 }}>
        {t('statuspage.public.feed.blurb_pre', { format: label })}<code style={{ background: 'var(--surface-2)', padding: '1px 5px', borderRadius: 4 }}>/feed</code>{t('statuspage.public.feed.blurb_post')}
      </p>
      <div style={{ display: 'flex', gap: 8 }}>
        <input className="sub-input" readOnly value={url}
          onFocus={e => e.target.select()}
          style={{ fontFamily: 'JetBrains Mono, monospace', fontSize: 12.5 }}/>
        <CopyButton value={url}/>
        <a className="sub-btn" href={url} target="_blank" rel="noreferrer" style={{ textDecoration: 'none', display: 'inline-flex', alignItems: 'center', gap: 6 }}>
          <Rss size={13}/> {t('statuspage.public.feed.open')}
        </a>
      </div>
    </>
  );
}

function WebhookTab() {
  return (
    <>
      <p style={{ margin: '0 0 10px', fontSize: 13, color: 'var(--text-2)', lineHeight: 1.5 }}>
        {t('statuspage.public.webhook.blurb_pre')}<strong>{t('statuspage.public.webhook.path')}</strong>{t('statuspage.public.webhook.blurb_post')}
      </p>
      <p style={{ margin: 0, fontSize: 12, color: 'var(--text-3)' }}>
        {t('statuspage.public.webhook.note')}
      </p>
    </>
  );
}

const STYLE_TO_BG = {
  info:    ['#dbeafe', '#1e40af'],
  warning: ['#fef3c7', '#92400e'],
  danger:  ['#fee2e2', '#b91c1c'],
  primary: ['#e0e7ff', '#4338ca'],
  success: ['#d1fae5', '#047857'],
};

/** A short "47 minutes" / "2 days" style duration. */
function duration(start, end) {
  const ms = end.getTime() - start.getTime();
  const m = Math.round(ms / 60000);
  if (m < 1)    return t('statuspage.public.duration.under_minute');
  if (m < 60)   return t(m === 1 ? 'statuspage.public.duration.minute' : 'statuspage.public.duration.minutes', { n: m });
  const h = Math.floor(m / 60);
  const rm = m % 60;
  if (h < 24)   return rm === 0 ? t(h === 1 ? 'statuspage.public.duration.hour' : 'statuspage.public.duration.hours', { n: h }) : t('statuspage.public.duration.hm', { h, m: rm });
  const d = Math.floor(h / 24);
  const rh = h % 24;
  return rh === 0 ? t(d === 1 ? 'statuspage.public.duration.day' : 'statuspage.public.duration.days', { n: d }) : t('statuspage.public.duration.dh', { d, h: rh });
}

function ResolvedIncident({ incident }) {
  const created = toDate(incident.created_at);
  const resolved = toDate(incident.resolved_at);
  const [bg, fg] = STYLE_TO_BG[incident.style] || STYLE_TO_BG.success;
  return (
    <div style={{
      padding: '14px 18px', marginBottom: 10,
      borderRadius: 12, background: 'var(--surface)',
      border: '1px solid var(--border)',
      display: 'grid', gridTemplateColumns: '6px 1fr', gap: 14, alignItems: 'flex-start',
    }}>
      <div style={{ width: 6, alignSelf: 'stretch', borderRadius: 3, background: bg }}/>
      <div>
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 10, flexWrap: 'wrap', marginBottom: 4 }}>
          <strong style={{ fontSize: 14 }}>{incident.title}</strong>
          <span style={{ fontSize: 11, color: fg, fontWeight: 500, background: bg, padding: '2px 8px', borderRadius: 999 }}>
            {t('statuspage.public.incident.resolved')}
          </span>
        </div>
        <div style={{ fontSize: 12, color: 'var(--text-3)', marginBottom: 6 }}>
          {t('statuspage.public.incident.lasted', { date: created.toLocaleDateString(getLocale()), duration: duration(created, resolved) })}
        </div>
        <div style={{ fontSize: 13, color: 'var(--text-2)', whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{incident.content}</div>
      </div>
    </div>
  );
}

function Incident({ incident }) {
  const [bg, fg] = STYLE_TO_BG[incident.style] || STYLE_TO_BG.warning;
  return (
    <div style={{
      padding: '18px 22px', marginBottom: 12,
      borderRadius: 12, background: bg, color: fg,
      border: `1px solid ${fg}33`,
    }}>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 10, marginBottom: 6, flexWrap: 'wrap' }}>
        <strong style={{ fontSize: 15 }}>{incident.title}</strong>
        <span style={{ fontSize: 11, opacity: .75 }}>
          {t('statuspage.public.incident.posted', { when: relative(toDate(incident.created_at)) })}
        </span>
      </div>
      <div style={{ fontSize: 13.5, whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{incident.content}</div>
      {(incident.updates || []).length > 0 && (
        <div style={{ marginTop: 14, paddingTop: 12, borderTop: `1px solid ${fg}33` }}>
          {(incident.updates || []).map((u, i) => (
            <div key={i} style={{
              marginTop: i === 0 ? 0 : 10,
              paddingLeft: 12, borderLeft: `2px solid ${fg}80`,
              fontSize: 13, opacity: .9,
            }}>
              <div style={{ fontSize: 11, opacity: .8, marginBottom: 2 }}>
                {relative(toDate(u.posted_at))}
              </div>
              <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>{u.message}</div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
