// E2E: public maintenance banner surface.
//
// A maintenance window surfaces in the public status-page projection
// (PublicStatusPage.maintenance) when it is currently active OR starts
// within the next 7 days AND is attached to at least one monitor shown on
// that page (see the `maintenance` doc on `PublicStatusPage`). Each entry
// is a `PublicMaintenance { title, description, starts_at, ends_at, active }`.
//
// Flow:
//   1. Create a monitor.
//   2. Create a status page with that monitor attached (monitor_ids).
//   3. Create a maintenance window live right now (start 1h ago, end 1h
//      out, active:true) — POST /v1/maintenance-windows (maintenance.rs::
//      create; rejects end_at <= start_at, validates `NewMaintenanceWindow`).
//   4. Attach the window to the monitor:
//      POST /v1/maintenance-windows/{id}/monitors/{monitor_id}.
//   5. GET /v1/public/status-pages/{slug} -> `maintenance` is non-empty and
//      carries the window title.
//
// Everything uniq()-named; window + page + monitor torn down in a finally.

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('maintenance banner: an active window attached to a page monitor surfaces publicly', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  let monitorId = null;
  let pageId = null;
  let windowId = null;

  try {
    // 1. Monitor.
    const monitor = await api(page, 'POST', '/v1/monitors', {
      name: uniq('e2e-maint-mon', browserName),
      kind: 'http',
      url: 'https://example.com',
      interval_seconds: 60,
    });
    monitorId = monitor.id;
    expect(monitorId).toBeTruthy();

    // 2. Status page with the monitor attached.
    const slug = uniq('e2e-maint', browserName);
    const sp = await api(page, 'POST', '/v1/status-pages', {
      slug,
      title: `E2E Maintenance ${browserName}`,
      monitor_ids: [monitorId],
    });
    pageId = sp.id;
    expect(pageId).toBeTruthy();

    // 3. Maintenance window live RIGHT NOW (1h ago -> 1h out).
    const title = uniq('e2e-maint-win', browserName);
    const now = Date.now();
    const startsAt = new Date(now - 60 * 60_000).toISOString();
    const endsAt   = new Date(now + 60 * 60_000).toISOString();
    const win = await api(page, 'POST', '/v1/maintenance-windows', {
      name: title,
      description: 'e2e public maintenance banner',
      recurrence: { kind: 'none' },
      start_at: startsAt,
      end_at: endsAt,
    });
    windowId = win.id;
    expect(windowId).toBeTruthy();
    expect(win.active).toBe(true);

    // 4. Attach the window to the page's monitor.
    await api(page, 'POST', `/v1/maintenance-windows/${windowId}/monitors/${monitorId}`);

    // 5. Public projection carries the window under `maintenance`.
    const view = await api(page, 'GET', `/v1/public/status-pages/${slug}`);
    expect(Array.isArray(view.maintenance)).toBe(true);
    expect(view.maintenance.length, 'maintenance banner non-empty').toBeGreaterThan(0);
    const banner = view.maintenance.find(m => m.title === title);
    expect(banner, 'banner carries the window title').toBeTruthy();
    expect(banner.active, 'window active right now').toBe(true);
  } finally {
    if (windowId) await api(page, 'DELETE', `/v1/maintenance-windows/${windowId}`).catch(() => {});
    if (pageId)   await api(page, 'DELETE', `/v1/status-pages/${pageId}`).catch(() => {});
    if (monitorId) await api(page, 'DELETE', `/v1/monitors/${monitorId}`).catch(() => {});
  }
});
