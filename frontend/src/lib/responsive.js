// Mobile + small-tablet responsiveness.
//
// Per-view stylesheets use a mix of inline styles and per-class rules.
// Media queries can't override inline styles, so the strategy is:
//   * each problem layout opts in by adding one of the marker classes
//     below (e.g. `dash-shell`, `kpi-grid`, `detail-grid`)
//   * this file ships a global stylesheet with `@media` overrides that
//     redefine those layouts at narrow viewports
//
// Breakpoint:
//   * `<= 720px` — phones in portrait. Sidebar stacks, multi-col grids
//     collapse to one column, padding tightens.

const CSS = `
@media (max-width: 720px) {
  .rampart .dash-shell {
    display: block !important;          /* drop the 320 / 1fr split */
  }
  .rampart .dash-sidebar {
    border-right: none !important;
    border-bottom: 1px solid var(--border);
    max-height: 38vh;
    overflow: auto;
  }
  .rampart .dash-main {
    padding: 16px 14px 32px !important;
    max-width: 100% !important;
  }
  .rampart .dash-topbar {
    flex-wrap: wrap;
    gap: 6px !important;
    padding: 10px 12px !important;
  }
  .rampart .dash-topbar .search {
    flex: 1 1 100%;
    order: -1;
  }
  .rampart .kpi-grid {
    grid-template-columns: repeat(2, 1fr) !important;
  }
  .rampart .hero-split {
    grid-template-columns: 1fr !important;
  }
  .rampart .activity-row {
    grid-template-columns: 1fr !important;
    row-gap: 4px !important;
  }
  .rampart .activity-row > * {
    text-align: left !important;
  }
  .rampart .detail-grid {
    grid-template-columns: 1fr !important;
  }
  .rampart .modal-card {
    max-width: 100% !important;
    border-radius: 0 !important;
    max-height: 100vh !important;
    min-height: 100vh;
  }
  .rampart .card {
    border-radius: 10px;
  }
  .rampart .form-2col {
    grid-template-columns: 1fr !important;
  }
}

/* Hide the floating dev "Views" switcher on phones — it covers the bottom
   action area on the dashboard and isn't user-facing in any case. */
@media (max-width: 480px) {
  .rampart-view-switcher {
    display: none !important;
  }
}
`;

export function loadResponsive() {
  if (document.getElementById('rampart-responsive-css')) return;
  const style = document.createElement('style');
  style.id = 'rampart-responsive-css';
  style.textContent = CSS;
  document.head.appendChild(style);
}
