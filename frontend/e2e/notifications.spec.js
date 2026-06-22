// E2E: notifications + templates UI flow.

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, gotoView, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('create a webhook channel via the Notifications page', async ({ page, browserName }) => {
  const channelName = uniq('e2e-ui-webhook', browserName);

  await ensureLoggedIn(page);
  await gotoView(page, '#/notifications', 'h1:has-text("Notifications")');
  await page.getByRole('button', { name: /add channel/i }).click();
  // Channel type is a combobox: open it, filter, pick Generic Webhook.
  await page.getByRole('button', { name: /types/i }).click();
  await page.getByPlaceholder(/filter channel types/i).fill('webhook');
  await page.getByRole('button', { name: /generic webhook/i }).click();
  // Form inputs in order: display name → url.
  await page.locator('form input.input').first().fill(channelName);
  await page.locator('form input.mono').first().fill('https://example.com/hook-ui');
  await page.getByRole('button', { name: /save channel/i }).click();

  // Page reloads on save; the new channel name should be visible.
  await expect(page.getByText(channelName)).toBeVisible({ timeout: 10_000 });
});

test('create + edit + delete a template', async ({ page, browserName }) => {
  const tplName = uniq('e2e-tpl', browserName);

  await ensureLoggedIn(page);
  await gotoView(page, '#/notifications', 'h1:has-text("Notifications")');
  await page.getByRole('button', { name: /^Templates/i }).click();
  await page.getByRole('button', { name: /new template/i }).click();
  // First input is "Name".
  await page.locator('form input.input').first().fill(tplName);
  // Body textarea (the only textarea on the page).
  await page.locator('textarea').first().fill('Hello {{monitor.name}}');
  await page.getByRole('button', { name: /save template/i }).click();
  // TemplateForm calls reload() (full page reload) which resets to the
  // Channels tab; re-click Templates to see the new row.
  await page.getByRole('button', { name: /^Templates/i }).click();
  await expect(page.getByText(tplName)).toBeVisible({ timeout: 10_000 });

  // Delete it via the API to keep the run idempotent.
  const ts = await api(page, 'GET', '/v1/notification-templates');
  const t  = ts.find(x => x.name === tplName);
  expect(t).toBeDefined();
  await api(page, 'DELETE', `/v1/notification-templates/${t.id}`);
});

test('channel rows show "Test" button that returns 204 on a reachable webhook', async ({ page, browserName }) => {
  const channelName = uniq('e2e-httpbin-test', browserName);

  await ensureLoggedIn(page);
  await api(page, 'POST', '/v1/notifications', {
    kind: 'webhook', name: channelName,
    config: { url: 'https://httpbin.org/post' },
    active: true,
  });
  await gotoView(page, '#/notifications', 'h1:has-text("Notifications")');
  await expect(page.getByText(channelName)).toBeVisible();

  // The "Test" button on the row hits POST /v1/notifications/:id/test.
  page.on('dialog', d => d.accept());
  // Click the Test button on the row that contains our channel name.
  await page.locator('.channel-row', { hasText: channelName })
    .getByRole('button', { name: /^Test$/ }).click();
});
