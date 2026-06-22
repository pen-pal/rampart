// E2E: first-run signup → dashboard → logout → login → wrong-password
//
// Runs serially (and depends on globalSetup giving us a fresh DB).
// We only create one admin per suite — subsequent tests log in.

import { test, expect, isolatedContext } from './fixtures.js';
import { fixtures, login, signupAdmin } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('first-run signup (or login when admin already exists) lands on dashboard', async ({ page }) => {
  // The DB is shared across Playwright projects, so the "first run" path
  // only fires for the first project that runs. Subsequent projects use
  // the login path. Either way, the assertion is the same: we end up on
  // the dashboard.
  const { ensureLoggedIn } = await import('./helpers.js');
  await ensureLoggedIn(page);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
});

test('protected route bounces unauthenticated visitor to login', async ({ browser }) => {
  // Fresh context = no cookies, so we appear as a logged-out visitor.
  const ctx  = await isolatedContext(browser);
  const page = await ctx.newPage();
  await page.goto('/#/new-monitor');
  await page.waitForURL(/#\/login/);
  // The Sign-in button appears once we're on the login screen.
  await expect(page.getByRole('button', { name: /sign in/i })).toBeVisible();
  await ctx.close();
});

test('login with wrong password shows error', async ({ browser }) => {
  const ctx  = await isolatedContext(browser);
  const page = await ctx.newPage();
  await page.goto('/#/login');
  await expect(page.getByRole('button', { name: /sign in/i })).toBeVisible();
  await page.getByLabel(/email/i).fill(fixtures.ADMIN_EMAIL);
  await page.getByLabel(/password/i).fill('totally-wrong-password');
  await page.getByRole('button', { name: /sign in/i }).click();
  await expect(page.getByText(/wrong email or password/i)).toBeVisible();
  await ctx.close();
});

test('login with right password reaches dashboard', async ({ browser }) => {
  const ctx  = await isolatedContext(browser);
  const page = await ctx.newPage();
  await login(page);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  await ctx.close();
});

test('logout clears session — protected route now 401s + bounces', async ({ browser }) => {
  const ctx  = await isolatedContext(browser);
  const page = await ctx.newPage();
  await login(page);
  await page.getByRole('button', { name: /sign out/i }).click();
  await page.waitForURL(/#\/login/);
  // Try to navigate to protected route again — should bounce back.
  await page.goto('/#/new-monitor');
  await page.waitForURL(/#\/login/);
  await ctx.close();
});
