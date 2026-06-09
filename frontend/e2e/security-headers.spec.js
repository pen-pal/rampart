// E2E: confirm production-grade security response headers ship on
// every served path — the dashboard shell, an API endpoint, the
// embedded asset bundle, AND the public status-page surface.
//
// Asserting on the headers up here (rather than just trusting the
// unit tests) catches a class of regression where someone adds a
// route-level handler that builds its own Response and forgets to
// run through the middleware stack — `set_header::if_not_present`
// would silently skip the headers in that case.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

// The headers the security middleware layers into every response.
// `permissions-policy` value isn't asserted character-for-character —
// just that it exists and locks out one of the listed APIs — because
// the exact comma-spacing is browser/proxy-stripping-sensitive.
const REQUIRED_HEADERS = [
  ['strict-transport-security',  /max-age=\d+/],
  ['x-content-type-options',     /nosniff/i],
  ['x-frame-options',            /sameorigin/i],
  ['referrer-policy',            /strict-origin-when-cross-origin/],
  ['permissions-policy',         /camera=\(\)/],
  ['content-security-policy',    /default-src 'self'/],
];

function assertSecurityHeaders(headers, ctx) {
  for (const [name, pattern] of REQUIRED_HEADERS) {
    const value = headers[name];
    expect(value, `${ctx}: ${name} present`).toBeTruthy();
    expect(value, `${ctx}: ${name} matches ${pattern}`).toMatch(pattern);
  }
}

test('security headers ship on /healthz', async ({ request }) => {
  const resp = await request.get('/healthz');
  expect(resp.status()).toBe(200);
  assertSecurityHeaders(resp.headers(), 'healthz');
});

test('security headers ship on the dashboard shell (index.html)', async ({ request }) => {
  const resp = await request.get('/');
  expect(resp.status()).toBe(200);
  assertSecurityHeaders(resp.headers(), 'index.html');
});

test('security headers ship on an authenticated API endpoint', async ({ page }) => {
  await ensureLoggedIn(page);
  const resp = await page.request.get('/v1/monitors');
  expect(resp.status()).toBe(200);
  assertSecurityHeaders(resp.headers(), '/v1/monitors');
});

test('security headers ship on the public status-page surface', async ({ page, browserName }) => {
  await ensureLoggedIn(page);
  const slug = uniq('e2e-sec', browserName);
  const created = await api(page, 'POST', '/v1/status-pages', {
    slug,
    title: `Security headers smoke · ${browserName}`,
    public: true,
  });
  try {
    const resp = await page.request.get(`/v1/public/status-pages/${slug}`);
    expect(resp.status()).toBe(200);
    assertSecurityHeaders(resp.headers(), `public status page ${slug}`);
  } finally {
    await api(page, 'DELETE', `/v1/status-pages/${created.id}`).catch(() => {});
  }
});

test('413 (or connection-reset) on bodies past the 2 MiB request-body cap', async ({ request }) => {
  // ~3 MiB of `a`-bytes — past the RequestBodyLimitLayer cap of 2 MiB.
  // The exact endpoint doesn't matter — the layer fires before any
  // handler sees the body. /v1/auth/login is convenient because it
  // doesn't need auth and naturally expects JSON.
  //
  // Two valid server-side reactions to oversize content-length:
  //   - 413 Payload Too Large + close — clean rejection
  //   - reset the TCP stream mid-write so the client gets EPIPE —
  //     also RFC-compliant; the client doesn't get to send the full
  //     body, so the server stops reading. hyper takes this path by
  //     default for `RequestBodyLimitLayer` over a content-length-
  //     advertised request, because it knows the limit will be hit
  //     before it finishes parsing the body.
  //
  // Pass on either — the contract being asserted is "the server
  // refused to materialise a 3 MiB body", not the specific RFC
  // wording.
  const oversized = 'a'.repeat(3 * 1024 * 1024);
  let status = null;
  let writeError = null;
  try {
    const resp = await request.post('/v1/auth/login', {
      headers: {
        'content-type': 'application/json',
        // Unique IP so the rate-limiter bucket doesn't collide with
        // the other rate-limit-burst test in this file.
        'x-forwarded-for': '10.123.123.123',
      },
      data: oversized,
    });
    status = resp.status();
  } catch (e) {
    writeError = String(e.message || e);
  }
  const rejected =
    status === 413 ||
    /EPIPE|ECONNRESET|aborted|socket hang up/i.test(writeError || '');
  expect(rejected, `oversize body should be 413 or stream-reset; got status=${status} err=${writeError}`).toBe(true);
});

test('429 after auth rate-limit burst from one IP', async ({ request }) => {
  // 12 rapid logins from a unique X-Forwarded-For. The first 10 hit
  // the bad-credentials path (401). The 11th + 12th hit the
  // rate-limiter and return 429 with a Retry-After header.
  const headers = {
    'content-type': 'application/json',
    'x-forwarded-for': '10.222.222.222',
  };
  const statuses = [];
  for (let i = 0; i < 12; i++) {
    const resp = await request.post('/v1/auth/login', {
      headers,
      data: JSON.stringify({ email: 'nope@example.com', password: 'wrong' }),
    });
    statuses.push(resp.status());
    if (resp.status() === 429) {
      expect(resp.headers()['retry-after']).toBeTruthy();
    }
  }
  // First 10 should be 401, last 2 should be 429.
  const first10 = statuses.slice(0, 10);
  const last2 = statuses.slice(10);
  for (const s of first10) {
    expect(s, `early attempt should be 401, was ${s}`).toBe(401);
  }
  for (const s of last2) {
    expect(s, `late attempt should be 429, was ${s}`).toBe(429);
  }
});
