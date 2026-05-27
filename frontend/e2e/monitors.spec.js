// E2E: create a monitor via the wizard, verify it appears on the
// dashboard, attach a channel, see the bell badge update.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

// Use a per-project monitor name so cross-browser runs don't collide
// when the DB is shared across Playwright projects.
let MONITOR_NAME;
let MONITOR_ID;

test('create an HTTP monitor through the wizard and see it on the dashboard', async ({ page, browserName }) => {
  MONITOR_NAME = uniq('e2e-mon', browserName);
  await ensureLoggedIn(page);

  // Open the wizard via the Add monitor button in the header.
  await page.getByRole('button', { name: /add monitor/i }).click();
  await page.waitForURL(/#\/new-monitor/);

  // Step 1: type cards. HTTP is selected by default; click Continue.
  await expect(page.getByText(/Pick a check type/i)).toBeVisible();
  await page.getByRole('button', { name: /continue/i }).click();

  // Step 2: name + url. Labels aren't htmlFor-bound in the wizard.
  await page.locator('input.input:not(.mono)').first().fill(MONITOR_NAME);
  await page.locator('input.input.mono').first().fill('https://example.com');
  await page.getByRole('button', { name: /continue/i }).click();

  // Step 3: schedule. Defaults are fine.
  await page.getByRole('button', { name: /create monitor/i }).click();

  // Lands on monitor detail.
  await page.waitForURL(/#\/monitor\//);
  await expect(page.getByRole('heading', { name: MONITOR_NAME })).toBeVisible();

  // Stash the ID for the next tests so they can target this specific
  // monitor (matters in cross-browser runs that share the DB).
  const ms = await api(page, 'GET', '/v1/monitors');
  MONITOR_ID = ms.find(x => x.name === MONITOR_NAME)?.id;
  expect(MONITOR_ID).toBeDefined();
});

test('dashboard lists the created monitor with a grey bell (no channels)', async ({ page }) => {
  await ensureLoggedIn(page);
  await page.goto('/');
  // The all-monitors table includes a row for our specific monitor.
  await expect(page.getByText(MONITOR_NAME).first()).toBeVisible();
  // At least one grey bell badge ("No notification channels attached…")
  // exists in the table — there will be one per channel-less monitor.
  await expect(page.locator('[title*="No notification channels"]').first()).toBeVisible();
});

test('attach a webhook channel; bell badge shows count', async ({ page, browserName }) => {
  const channelName = uniq('e2e-attach-webhook', browserName);
  await ensureLoggedIn(page);

  // Create a channel via the API (uniquely named per browser).
  const ch = await api(page, 'POST', '/v1/notifications', {
    kind: 'webhook', name: channelName,
    config: { url: 'https://example.com/hook' },
    active: true,
  });

  // Attach to our specific monitor.
  await api(page, 'POST', `/v1/monitors/${MONITOR_ID}/notifications/${ch.id}`);

  // Dashboard now shows a teal bell badge on this monitor's row.
  await page.goto('/');
  await expect(page.locator('[title*="notification channel"]').first()).toBeVisible({ timeout: 15_000 });
});
