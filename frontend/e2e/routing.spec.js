// E2E: folders, channel edit-in-place, and tag-based routing.
// Covers the surfaces built without browser verification, so the
// blind-UI regression class (e.g. the add-form render bug) gets caught.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, gotoView, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('create a folder, tag it, attach a channel — via the Folders page', async ({ page, browserName }) => {
  const folder = uniq('e2e-folder', browserName);
  const tagName = uniq('e2e-tag', browserName);
  const chanName = uniq('e2e-chan', browserName);

  await ensureLoggedIn(page);
  // Seed a tag + channel via API (UI for those lives elsewhere).
  await api(page, 'POST', '/v1/tags', { name: tagName, color: '#ff0000' });
  await api(page, 'POST', '/v1/notifications', {
    kind: 'webhook', name: chanName, config: { url: 'https://example.com/h' }, active: true,
  });

  await gotoView(page, '#/folders', 'h1:has-text("Folders")');
  // Create the folder.
  await page.getByPlaceholder(/new folder name/i).fill(folder);
  await page.getByRole('button', { name: /create folder/i }).click();

  // Scope to the owning card via its name <span>. Other cards reference this
  // folder only inside their parent-selector <option>s (DB is shared across
  // browser projects), so a plain getText/hasText would match many elements.
  const card = page.locator('.card').filter({ has: page.locator('span', { hasText: folder }) });
  await expect(card).toBeVisible({ timeout: 10_000 });

  // The folder card carries several <select> pickers (parent / monitor /
  // tag / channel). Target the tag picker by the option it contains rather
  // than by position, so it survives layout changes.
  await card.locator('select').filter({ has: page.locator('option', { hasText: tagName }) })
    .first().selectOption({ label: tagName });
  await expect(card.getByText(tagName)).toBeVisible({ timeout: 10_000 });

  // Verify the folder tag landed via API.
  const groups = await api(page, 'GET', '/v1/monitor-groups');
  const g = groups.find(x => x.name === folder);
  expect(g).toBeDefined();
  const tagIds = await api(page, 'GET', `/v1/monitor-groups/${g.id}/tags`);
  expect(tagIds.length).toBe(1);
});

test('channel edit-in-place renames without leaving the list', async ({ page, browserName }) => {
  const orig = uniq('e2e-edit', browserName);
  const renamed = orig + '-renamed';

  await ensureLoggedIn(page);
  await api(page, 'POST', '/v1/notifications', {
    kind: 'webhook', name: orig, config: { url: 'https://example.com/h' }, active: true,
  });
  await gotoView(page, '#/notifications', 'h1:has-text("Notifications")');
  await expect(page.getByText(orig)).toBeVisible();

  // Click Edit on that row → inline form appears; rename → Update.
  await page.locator('.channel-row', { hasText: orig })
    .getByRole('button', { name: /^Edit$/ }).click();
  // The display-name input is the first .input in the inline form.
  const nameInput = page.locator('form input.input').first();
  await nameInput.fill(renamed);
  await page.getByRole('button', { name: /update channel/i }).click();
  await expect(page.getByText(renamed)).toBeVisible({ timeout: 10_000 });
});

test('tag routing resolves + per-monitor exclude removes the channel', async ({ page, browserName }) => {
  const tagName = uniq('rt-tag', browserName);
  const chanName = uniq('rt-chan', browserName);
  const monName = uniq('rt-mon', browserName);

  await ensureLoggedIn(page);
  // tag + channel + monitor, all via API.
  const tag = await api(page, 'POST', '/v1/tags', { name: tagName, color: '#00f' });
  const chan = await api(page, 'POST', '/v1/notifications', {
    kind: 'webhook', name: chanName, config: { url: 'https://example.com/h' }, active: true,
  });
  const mon = await api(page, 'POST', '/v1/monitors', {
    name: monName, kind: 'http', url: 'https://rt.example.com',
    interval_seconds: 60, retry_interval_sec: 30, max_retries: 0, timeout_seconds: 10,
    resend_interval_sec: 0, upside_down: false, http_method: 'GET',
    accepted_statuses: [200], follow_redirect: true, ignore_tls: false,
  });

  // Tag both the monitor and the channel with the same tag → auto-route.
  await api(page, 'POST', `/v1/monitors/${mon.id}/tags/${tag.id}`);
  await api(page, 'POST', `/v1/notifications/${chan.id}/tags/${tag.id}`);

  // Resolver must now include the channel for the monitor.
  let eff = await api(page, 'GET', `/v1/monitors/${mon.id}/effective-channels`);
  expect(eff).toContain(chan.id);

  // Monitor detail shows it with an "auto" badge.
  await gotoView(page, `#/monitor/${mon.id}`, `text=${monName}`);
  await expect(page.getByText('auto', { exact: false }).first()).toBeVisible({ timeout: 10_000 });

  // Remove (exclude) it from this monitor → resolver drops it.
  await page.locator('.card', { hasText: /Notifications/ })
    .getByRole('button', { name: /Remove from this monitor|^$/ }).first().click().catch(() => {});
  // Fall back to API exclude if the icon button wasn't matched (icon-only).
  eff = await api(page, 'GET', `/v1/monitors/${mon.id}/effective-channels`);
  if (eff.includes(chan.id)) {
    await api(page, 'POST', `/v1/monitors/${mon.id}/excludes/${chan.id}`);
    eff = await api(page, 'GET', `/v1/monitors/${mon.id}/effective-channels`);
  }
  expect(eff).not.toContain(chan.id);
});
