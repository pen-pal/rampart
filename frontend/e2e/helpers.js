// Tiny e2e helpers — keep the spec files readable.

import { request as pwRequest, nextClientIp } from './fixtures.js';

const ADMIN_EMAIL    = 'e2e-admin@example.com';
const ADMIN_NAME     = 'E2E Admin';
const ADMIN_PASSWORD = 'correct-horse-battery-staple';

/**
 * Walk the first-run signup screen if the DB has no users. If an admin
 * already exists (e.g. another spec ran before us in the same suite),
 * returns false without filling anything in — call `login(page)` instead.
 */
export async function signupAdmin(page) {
  await page.goto('/');
  await page.waitForURL(/#\/login/);
  // Wait for the form to render in either mode.
  await page.getByRole('button').first().waitFor();

  const createBtn = page.getByRole('button', { name: /create admin account/i });
  if (await createBtn.isVisible().catch(() => false) === false) {
    // Already signed up. Caller should login() instead.
    return false;
  }

  await page.getByLabel(/email/i).fill(ADMIN_EMAIL);
  await page.getByLabel(/name/i).fill(ADMIN_NAME);
  await page.getByLabel(/password/i).fill(ADMIN_PASSWORD);
  await createBtn.click();
  await page.waitForURL((url) => !url.toString().includes('#/login'));
  return true;
}

/** Sign up if needed, otherwise log in. Either way, page is on dashboard. */
export async function ensureLoggedIn(page) {
  const didSignup = await signupAdmin(page);
  if (!didSignup) await login(page);
}

/**
 * Navigate to a hash route reliably. We force a fresh document load
 * (cannot rely on hashchange firing across all browsers consistently)
 * and then wait for an element that's unique to the destination view.
 */
export async function gotoView(page, hash, signalSelector) {
  // about:blank + then the target URL forces a full document load.
  await page.goto('about:blank');
  await page.goto('/' + hash, { waitUntil: 'load' });
  if (signalSelector) {
    await page.waitForSelector(signalSelector, { timeout: 10_000 });
  }
}

export async function login(page) {
  await page.goto('/#/login');
  await page.getByRole('button', { name: /sign in/i }).waitFor();
  await page.getByLabel(/email/i).fill(ADMIN_EMAIL);
  await page.getByLabel(/password/i).fill(ADMIN_PASSWORD);
  await page.getByRole('button', { name: /sign in/i }).click();
  await page.waitForURL((url) => !url.toString().includes('#/login'));
}

export const fixtures = {
  ADMIN_EMAIL,
  ADMIN_NAME,
  ADMIN_PASSWORD,
};

/**
 * Short browser-aware suffix for unique resource names so tests don't
 * collide across projects (the DB is shared across Playwright projects).
 */
export function uniq(prefix, browserName) {
  const tag = browserName || 'x';
  return `${prefix}-${tag}-${Math.floor(Date.now() % 1e6).toString(36)}`;
}

/**
 * Hit the JSON API directly. Accepts either a Playwright `page` (uses the
 * page's cookie jar) or a standalone `APIRequestContext` (e.g. one returned
 * by `loginAs`). Throws on a non-2xx response.
 * Useful for setting up state without driving the UI.
 */
export async function api(pageOrCtx, method, path, body) {
  const ctx = pageOrCtx.request ?? pageOrCtx;
  const res = await ctx[method.toLowerCase()](path, body !== undefined ? { data: body } : undefined);
  if (!res.ok()) {
    const text = await res.text();
    throw new Error(`${method} ${path} failed: ${res.status()} ${text}`);
  }
  // 204s won't have a body.
  return res.status() === 204 ? null : await res.json();
}

/**
 * Like `api`, but never throws on HTTP status — returns the raw Playwright
 * `APIResponse` so callers can assert on `.status()` (including 401/403).
 * Accepts a `page` or a standalone `APIRequestContext`.
 */
export async function rawApi(pageOrCtx, method, path, body) {
  const ctx = pageOrCtx.request ?? pageOrCtx;
  return ctx[method.toLowerCase()](path, body !== undefined ? { data: body } : undefined);
}

/**
 * Log in as an arbitrary user and return a standalone `APIRequestContext`
 * whose cookie jar holds that user's session. Hand the returned context to
 * `api()` / `rawApi()` to make calls authenticated as that user, fully
 * isolated from the admin `page`'s session.
 *
 * Caller is responsible for `await ctx.dispose()` when done.
 */
export async function loginAs(email, password, baseURL) {
  // Own client IP so this login doesn't share the loopback rate-limit bucket.
  const ctx = await pwRequest.newContext({
    baseURL,
    extraHTTPHeaders: { 'x-forwarded-for': nextClientIp() },
  });
  const res = await ctx.post('/v1/auth/login', { data: { email, password } });
  if (!res.ok()) {
    const text = await res.text();
    await ctx.dispose();
    throw new Error(`login as ${email} failed: ${res.status()} ${text}`);
  }
  return ctx;
}
