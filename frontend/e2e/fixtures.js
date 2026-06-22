// Shared Playwright test harness.
//
// Why this exists: the server's auth surface (login / register / TOTP-verify)
// is protected by a per-client-IP token-bucket rate limiter (10 burst). The
// limiter keys on the *real* client IP — the trusted TCP peer, or the client
// behind a trusted proxy via X-Forwarded-For. Every e2e request originates
// from one loopback peer, so without isolation the whole suite's logins pool
// into a single bucket and exhaust it (429), which is not what any individual
// test is exercising.
//
// Fix (test-env only — production is unchanged): the e2e webServer sets
// RAMPART_TRUSTED_PROXIES to the loopback peer so the server honours
// X-Forwarded-For, and this harness stamps a UNIQUE X-Forwarded-For on every
// browser context + API request context. Each test therefore gets its own
// rate-limit bucket and the suite never starves itself. Tests that explicitly
// assert the limiter (security-headers.spec.js) set their own X-Forwarded-For
// per request, which overrides the per-context default.
//
// Import `{ test, expect }` (and `request` if needed) from this module instead
// of '@playwright/test'.

import { test as base, expect, request } from '@playwright/test';

// Unique, non-loopback, non-trusted client IP per call. 198.18.0.0/15 is the
// RFC 2544 benchmarking range — never a real client and never in the test
// RAMPART_TRUSTED_PROXIES list, so the server treats it as the client identity.
let ipCounter = 0;
export function nextClientIp() {
  ipCounter += 1;
  return `198.18.${(ipCounter >> 8) & 0xff}.${ipCounter & 0xff}`;
}

// Give every browser context its own client IP. We replace the `context`
// fixture with one created via browser.newContext({ extraHTTPHeaders }) — the
// header passed at *creation* reliably rides every request the context makes
// (page navigations + in-page fetch), whereas a post-hoc setExtraHTTPHeaders on
// the built-in context did not propagate to page fetches. The `page` fixture
// depends on `context`, so it inherits the isolated IP automatically.
export const test = base.extend({
  context: async ({ browser }, use) => {
    const context = await browser.newContext({
      extraHTTPHeaders: { 'x-forwarded-for': nextClientIp() },
    });
    await use(context);
    await context.close();
  },
});

// For tests that spin up extra logged-out browser contexts by hand: gives each
// one its own client IP too (a bare browser.newContext() would share the
// loopback bucket).
export async function isolatedContext(browser, options = {}) {
  return browser.newContext({
    ...options,
    extraHTTPHeaders: { 'x-forwarded-for': nextClientIp(), ...(options.extraHTTPHeaders || {}) },
  });
}

export { expect, request };
