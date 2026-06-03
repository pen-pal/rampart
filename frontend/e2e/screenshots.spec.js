// Screenshot generator — drives the app through the canonical first-run
// journey and saves a labelled PNG for each surface into
// `docs/assets/screenshots/`. Re-run any time the UI changes:
//
//   cd frontend
//   npm run screenshots                  # full sweep
//   npm run screenshots -- --grep 03     # one step only
//
// The spec lives alongside the regular e2e specs but is excluded from
// the CI matrix in `playwright.config.js` because it mutates files on
// disk under `docs/`. Run it locally and commit the result.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, fixtures, gotoView, uniq } from './helpers.js';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import fs from 'node:fs/promises';

const HERE       = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT  = path.resolve(HERE, '..', '..');
const SHOTS_DIR  = path.join(REPO_ROOT, 'docs', 'assets', 'screenshots');

const shot = (name) => path.join(SHOTS_DIR, name);

// Deterministic 16:10 viewport — same aspect as a 1440×900 MacBook and
// most modern wide monitors, so dropped-into-the-README crops cleanly.
test.use({
  viewport: { width: 1440, height: 900 },
  // Hide the mouse cursor in the screenshot output. It otherwise lands
  // wherever the last `.click()` left it.
  deviceScaleFactor: 2,
});

// Serial — each step depends on state the previous step left behind
// (admin created, monitor created, channel attached, etc.).
test.describe.configure({ mode: 'serial' });

// ──────────────────────────────────────────────────────────────────────
// Setup: ensure the shots dir exists. Playwright runs the body once per
// browser project, but the directory creation is idempotent so it's
// fine to re-enter.
// ──────────────────────────────────────────────────────────────────────
test.beforeAll(async () => {
  await fs.mkdir(SHOTS_DIR, { recursive: true });
});

// ──────────────────────────────────────────────────────────────────────
// 01 — First-run admin setup
// ──────────────────────────────────────────────────────────────────────
test('01 setup — first-run admin creation', async ({ page }) => {
  await page.goto('/');
  await page.waitForURL(/#\/login/);

  // The setup screen renders the "Create admin account" submit button.
  // If we hit a returning-user login screen instead, the DB wasn't
  // freshly migrated — bail loudly rather than overwrite the next shot.
  const createBtn = page.getByRole('button', { name: /create admin account/i });
  await expect(createBtn).toBeVisible({ timeout: 5_000 });

  await page.getByLabel(/email/i).fill(fixtures.ADMIN_EMAIL);
  await page.getByLabel(/name/i).fill(fixtures.ADMIN_NAME);
  await page.getByLabel(/password/i).fill(fixtures.ADMIN_PASSWORD);

  // Capture *before* clicking — we want the populated form, not the
  // dashboard that follows.
  await page.screenshot({ path: shot('01-setup.png'), fullPage: false });

  await createBtn.click();
  await page.waitForURL((url) => !url.toString().includes('#/login'));
});

// ──────────────────────────────────────────────────────────────────────
// 02 — Returning-user login screen
// ──────────────────────────────────────────────────────────────────────
// Force a logout so the next visit lands on the post-setup login form
// (not the first-run setup form). The screenshot is of the *login*
// surface, captured with the email already typed so it doesn't look
// blank.
test('02 login — sign-in screen', async ({ page }) => {
  await page.goto('/#/login');
  // Logout link in the header if we're still authed.
  await page.evaluate(async () => {
    await fetch('/v1/auth/logout', { method: 'POST', credentials: 'include' });
  });

  await page.goto('/#/login');
  const signinBtn = page.getByRole('button', { name: /sign in/i });
  await expect(signinBtn).toBeVisible({ timeout: 5_000 });

  await page.getByLabel(/email/i).fill(fixtures.ADMIN_EMAIL);
  await page.getByLabel(/password/i).fill(fixtures.ADMIN_PASSWORD);
  await page.screenshot({ path: shot('02-login.png'), fullPage: false });

  await signinBtn.click();
  await page.waitForURL((url) => !url.toString().includes('#/login'));
});

// ──────────────────────────────────────────────────────────────────────
// 03 — Empty dashboard (fresh install, no monitors yet)
// ──────────────────────────────────────────────────────────────────────
test('03 dashboard — empty state', async ({ page }) => {
  await ensureLoggedIn(page);
  await page.goto('/');
  await expect(page.getByRole('button', { name: /add monitor/i })).toBeVisible();
  // Let the no-monitors empty-state panel settle.
  await page.waitForTimeout(500);
  await page.screenshot({ path: shot('03-dashboard-empty.png'), fullPage: false });
});

// ──────────────────────────────────────────────────────────────────────
// 04–06 — Monitor wizard, three steps
// ──────────────────────────────────────────────────────────────────────
const DEMO_MONITOR_NAME = 'Acme API';
const DEMO_MONITOR_URL  = 'https://api.example.com/health';

test('04 wizard — step 1, pick a probe kind', async ({ page }) => {
  await ensureLoggedIn(page);
  await page.goto('/');
  await page.getByRole('button', { name: /add monitor/i }).click();
  await page.waitForURL(/#\/new-monitor/);
  await expect(page.getByText(/Pick a check type/i)).toBeVisible();
  await page.screenshot({ path: shot('04-wizard-kind.png'), fullPage: false });
});

test('05 wizard — step 2, target URL + name', async ({ page }) => {
  await ensureLoggedIn(page);
  await gotoView(page, '#/new-monitor');
  await expect(page.getByText(/Pick a check type/i)).toBeVisible();
  await page.getByRole('button', { name: /continue/i }).click();

  await page.locator('input.input:not(.mono)').first().fill(DEMO_MONITOR_NAME);
  await page.locator('input.input.mono').first().fill(DEMO_MONITOR_URL);
  await page.screenshot({ path: shot('05-wizard-target.png'), fullPage: false });
});

test('06 wizard — step 3, schedule defaults', async ({ page }) => {
  await ensureLoggedIn(page);
  await gotoView(page, '#/new-monitor');
  await page.getByRole('button', { name: /continue/i }).click();
  await page.locator('input.input:not(.mono)').first().fill(DEMO_MONITOR_NAME);
  await page.locator('input.input.mono').first().fill(DEMO_MONITOR_URL);
  await page.getByRole('button', { name: /continue/i }).click();

  await expect(page.getByRole('button', { name: /create monitor/i })).toBeVisible();
  await page.screenshot({ path: shot('06-wizard-schedule.png'), fullPage: false });
  await page.getByRole('button', { name: /create monitor/i }).click();
  await page.waitForURL(/#\/monitor\//);
});

// ──────────────────────────────────────────────────────────────────────
// 07 — Monitor detail after the first few heartbeats
// ──────────────────────────────────────────────────────────────────────
test('07 monitor-detail — first heartbeats', async ({ page }) => {
  await ensureLoggedIn(page);
  // Trigger a couple of out-of-cycle probes so the chart has something
  // to render. test-now is exposed on the API.
  const monitors = await api(page, 'GET', '/v1/monitors');
  const target = monitors.find(m => m.name === DEMO_MONITOR_NAME);
  expect(target, 'wizard step 06 should have created the demo monitor').toBeDefined();

  // Fire 3 probes ~600ms apart so the chart has multiple data points.
  for (let i = 0; i < 3; i++) {
    await api(page, 'POST', `/v1/monitors/${target.id}/test-now`).catch(() => {});
    await page.waitForTimeout(600);
  }

  await gotoView(page, `#/monitor/${target.id}`, 'h1, h2, [class*="page-title"]');
  await page.waitForTimeout(1500); // chart settle
  await page.screenshot({ path: shot('07-monitor-detail.png'), fullPage: false });
});

// ──────────────────────────────────────────────────────────────────────
// 08 — Dashboard with one monitor in the list
// ──────────────────────────────────────────────────────────────────────
test('08 dashboard — first monitor visible', async ({ page }) => {
  await ensureLoggedIn(page);
  await page.goto('/');
  await expect(page.getByText(DEMO_MONITOR_NAME).first()).toBeVisible();
  await page.waitForTimeout(800);
  await page.screenshot({ path: shot('08-dashboard-populated.png'), fullPage: false });
  // Also overwrite the README hero image so it picks up the new brand
  // mark in the header without a separate manual step.
  await page.screenshot({ path: path.join(REPO_ROOT, 'docs/assets/dashboard.png'), fullPage: false });
});

// ──────────────────────────────────────────────────────────────────────
// 09 — Notification channels page (create a webhook channel for the demo)
// ──────────────────────────────────────────────────────────────────────
test('09 notifications — channels list with one webhook', async ({ page, browserName }) => {
  await ensureLoggedIn(page);
  const name = uniq('demo-webhook', browserName);
  await api(page, 'POST', '/v1/notifications', {
    kind: 'webhook', name,
    config: { url: 'https://hooks.example.com/incoming/demo' },
    active: true,
  });
  await gotoView(page, '#/notifications', 'h1, h2, [class*="page-title"]');
  await page.waitForTimeout(500);
  await page.screenshot({ path: shot('09-notifications.png'), fullPage: false });
});

// ──────────────────────────────────────────────────────────────────────
// 10 — Status-page builder
// ──────────────────────────────────────────────────────────────────────
test('10 status-page — builder view', async ({ page }) => {
  await ensureLoggedIn(page);
  await gotoView(page, '#/status-page', 'h1, h2, [class*="page-title"]');
  await page.waitForTimeout(500);
  await page.screenshot({ path: shot('10-status-pages.png'), fullPage: false });
});

// ──────────────────────────────────────────────────────────────────────
// 11 — Dashboard in dark theme
// ──────────────────────────────────────────────────────────────────────
// Flip the persisted theme directly via localStorage so the toggle
// state matches what a user with dark mode preference would see, then
// reload to repaint everything from the new tokens.
test('11 dashboard-dark — dark theme', async ({ page }) => {
  await ensureLoggedIn(page);
  await page.goto('/');
  await page.evaluate(() => localStorage.setItem('rampart_theme', 'dark'));
  await page.reload();
  await expect(page.getByText(DEMO_MONITOR_NAME).first()).toBeVisible();
  await page.waitForTimeout(800);
  await page.screenshot({ path: shot('11-dashboard-dark.png'), fullPage: false });
  await page.screenshot({ path: path.join(REPO_ROOT, 'docs/assets/dashboard-dark.png'), fullPage: false });
});
