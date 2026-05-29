// E2E: dashboard bulk operations + folder grouping.
// Guards UI shipped without browser verification (selection bar, group
// buckets).

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, gotoView, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

function newMonitor(name, groupId) {
  return {
    name, kind: 'http', url: `https://${name}.example.com`,
    interval_seconds: 60, retry_interval_sec: 30, max_retries: 0, timeout_seconds: 10,
    resend_interval_sec: 0, upside_down: false, http_method: 'GET',
    accepted_statuses: [200], follow_redirect: true, ignore_tls: false,
    ...(groupId ? { group_id: groupId } : {}),
  };
}

test('bulk pause: select two monitors and pause them from the action bar', async ({ page, browserName }) => {
  const a = uniq('bulk-a', browserName);
  const b = uniq('bulk-b', browserName);

  await ensureLoggedIn(page);
  const ma = await api(page, 'POST', '/v1/monitors', newMonitor(a));
  const mb = await api(page, 'POST', '/v1/monitors', newMonitor(b));

  await gotoView(page, '#/', '.activity-row');
  // Tick both rows' checkboxes.
  await page.locator('.activity-row', { hasText: a }).getByRole('checkbox').check();
  await page.locator('.activity-row', { hasText: b }).getByRole('checkbox').check();

  // Selection bar shows the count + Pause.
  await expect(page.getByText(/2 selected/)).toBeVisible();
  await page.getByRole('button', { name: /^Pause$/ }).click();

  // runBulk fires the bulk POST then reloads — poll the API until both
  // monitors report paused (avoids racing the reload + request settle).
  await expect.poll(async () => {
    const after = await api(page, 'GET', '/v1/monitors');
    const a2 = after.find(m => m.id === ma.id);
    const b2 = after.find(m => m.id === mb.id);
    return a2 && b2 && !a2.active && !b2.active;
  }, { timeout: 10_000 }).toBe(true);
});

test('dashboard groups monitors under their folder', async ({ page, browserName }) => {
  const folder = uniq('grp-folder', browserName);
  const mon = uniq('grp-mon', browserName);

  await ensureLoggedIn(page);
  const g = await api(page, 'POST', '/v1/monitor-groups', { name: folder, sort_order: 0 });
  await api(page, 'POST', '/v1/monitors', newMonitor(mon, g.id));

  await gotoView(page, '#/', '.group-head');
  // Folder appears as a group header, and the monitor row is present.
  await expect(page.locator('.group-head', { hasText: folder })).toBeVisible({ timeout: 10_000 });
  await expect(page.locator('.activity-row', { hasText: mon })).toBeVisible();
});
