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

import { test, expect } from './fixtures.js';
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

// ══════════════════════════════════════════════════════════════════════
// Observability tiers (12–21). Each step seeds realistic data through the
// tier's own public ingest endpoint (OTLP / Sentry / RUM beacon) or admin
// API, then screenshots the rendered view. Seeding is best-effort: a view
// still captures (empty) if an ingest shape drifts.
// ══════════════════════════════════════════════════════════════════════

// OTLP timestamps are unix-nanos as strings. Anchor everything a few
// seconds ago so the spans sit inside any recent-window read.
const NS = (offsetMs) => String((BigInt(Date.now()) - 5000n + BigInt(offsetMs)) * 1_000_000n);
const hex = (n, len) => n.toString(16).padStart(len, '0');
const svc = (name) => ({ attributes: [{ key: 'service.name', value: { stringValue: name } }] });

// A realistic checkout→payments trace: root + 4 children, one error span,
// one cross-service edge (so the service map + error-rate populate).
function buildTrace(seed) {
  const traceId = hex(seed, 8).repeat(4);          // 32 hex chars
  const sid = (n) => hex(seed * 16 + n, 16);       // 16 hex chars
  const span = (n, parent, service, name, t0, t1, errCode = 1) => ({
    traceId, spanId: sid(n), parentSpanId: parent ? sid(parent) : undefined,
    name, kind: 2, startTimeUnixNano: NS(t0), endTimeUnixNano: NS(t1),
    status: { code: errCode },
  });
  return {
    traceId,
    body: {
      resourceSpans: [
        { resource: svc('checkout'), scopeSpans: [{ spans: [
          span(1, 0, 'checkout', 'POST /checkout', 0, 240),
          span(2, 1, 'checkout', 'validate cart', 10, 40),
          span(3, 1, 'checkout', 'SELECT orders', 45, 95),
        ] }] },
        { resource: svc('payments'), scopeSpans: [{ spans: [
          span(4, 1, 'payments', 'charge card', 100, 220),
          span(5, 4, 'payments', 'POST api.stripe.com', 110, 205, seed % 3 === 0 ? 2 : 1),
        ] }] },
      ],
    },
  };
}

// 12 — Errors: a Sentry-keyed project with a few grouped issues.
test('12 errors — issues list', async ({ page }) => {
  await ensureLoggedIn(page);
  try {
    const proj = await api(page, 'POST', '/v1/error-projects', { name: 'web-frontend' });
    const events = [
      { type: 'TypeError', value: "Cannot read properties of undefined (reading 'id')", tx: '/checkout' },
      { type: 'TypeError', value: "Cannot read properties of undefined (reading 'id')", tx: '/checkout' },
      { type: 'NetworkError', value: 'Failed to fetch /api/cart', tx: '/cart' },
      { type: 'RangeError', value: 'Maximum call stack size exceeded', tx: '/dashboard' },
    ];
    for (let i = 0; i < events.length; i++) {
      const e = events[i];
      await api(page, 'POST', `/api/${proj.id}/store/?sentry_key=${proj.public_key}`, {
        event_id: hex(0x1000 + i, 8).repeat(4),
        level: 'error', platform: 'javascript', transaction: e.tx,
        exception: { values: [{ type: e.type, value: e.value, stacktrace: { frames: [
          { filename: 'app://bundle.js', function: 'render', lineno: 142 + i },
          { filename: 'app://bundle.js', function: 'onClick', lineno: 88 },
        ] } }] },
      }).catch(() => {});
    }
  } catch { /* project may already exist on re-run */ }
  await gotoView(page, '#/errors', 'h1, h2, [class*="page-title"]');
  await page.waitForTimeout(700);
  // Best-effort: open the first project to reveal its issue list.
  const proj = page.getByText(/web-frontend/i).first();
  if (await proj.isVisible().catch(() => false)) { await proj.click().catch(() => {}); await page.waitForTimeout(600); }
  await page.screenshot({ path: shot('12-errors.png'), fullPage: false });
});

// 13–15 — Traces: list, waterfall, service map.
test('13 traces — recent traces + 14 waterfall + 15 service map', async ({ page }) => {
  await ensureLoggedIn(page);
  let firstTrace = null;
  for (let s = 1; s <= 6; s++) {
    const t = buildTrace(s);
    if (!firstTrace) firstTrace = t.traceId;
    await api(page, 'POST', '/otlp/v1/traces', t.body).catch(() => {});
  }
  await page.waitForTimeout(400);
  await gotoView(page, '#/traces', 'h1, h2, [class*="page-title"]');
  await page.waitForTimeout(700);
  await page.screenshot({ path: shot('13-traces.png'), fullPage: false });

  // 14 — waterfall: deep-link to a known trace. The detail view has no
  // generic page-title heading, so navigate + settle rather than wait on a
  // selector (a miss here must not abort the rest of the serial sweep).
  if (firstTrace) {
    await gotoView(page, `#/traces/${firstTrace}`);
    await page.waitForTimeout(1200);
    await page.screenshot({ path: shot('14-trace-waterfall.png'), fullPage: false });
  }

  // 15 — service map tab (best-effort click).
  await gotoView(page, '#/traces', 'h1, h2, [class*="page-title"]');
  const mapTab = page.getByRole('button', { name: /service map/i })
    .or(page.getByText(/service map/i)).first();
  if (await mapTab.isVisible().catch(() => false)) {
    await mapTab.click().catch(() => {});
    await page.waitForTimeout(800);
    await page.screenshot({ path: shot('15-service-map.png'), fullPage: false });
  }
});

// 16 — Logs: severity-mixed records, one correlated to a trace.
test('16 logs — filtered stream', async ({ page }) => {
  await ensureLoggedIn(page);
  const rec = (sevNum, sevText, body, traceId) => ({
    timeUnixNano: NS(0), severityNumber: sevNum, severityText: sevText,
    body: { stringValue: body }, ...(traceId ? { traceId } : {}),
  });
  const payload = {
    resourceLogs: [
      { resource: svc('checkout'), scopeLogs: [{ logRecords: [
        rec(9, 'INFO', 'order 4821 placed'),
        rec(9, 'INFO', 'cart validated for user 91'),
        rec(13, 'WARN', 'retrying payment gateway (attempt 2)'),
        rec(17, 'ERROR', 'payment gateway timeout after 30s', buildTrace(3).traceId),
      ] }] },
      { resource: svc('payments'), scopeLogs: [{ logRecords: [
        rec(9, 'INFO', 'charge authorized: $42.00'),
        rec(17, 'ERROR', 'stripe 502 Bad Gateway'),
      ] }] },
    ],
  };
  await api(page, 'POST', '/otlp/v1/logs', payload).catch(() => {});
  await page.waitForTimeout(400);
  await gotoView(page, '#/logs');
  await page.waitForTimeout(700);
  await page.screenshot({ path: shot('16-logs.png'), fullPage: false });
});

// 17 — RUM: web-vitals beacons across a couple of pages.
test('17 rum — web vitals', async ({ page }) => {
  await ensureLoggedIn(page);
  const beacon = (url, m) => api(page, 'POST', '/rum/v1/events', {
    app: 'storefront', url, session: Math.random().toString(36).slice(2),
    ua: 'Mozilla/5.0', metrics: m,
  }).catch(() => {});
  for (let i = 0; i < 5; i++) {
    await beacon('/', { lcp: 1800 + i * 120, fcp: 900 + i * 40, cls: 0.04 + i * 0.01, inp: 120 + i * 15, ttfb: 210, load: 2400 });
    await beacon('/product/42', { lcp: 2600 + i * 90, fcp: 1300, cls: 0.11, inp: 240, ttfb: 320, load: 3200 });
  }
  await page.waitForTimeout(400);
  await gotoView(page, '#/rum');
  await page.waitForTimeout(700);
  await page.screenshot({ path: shot('17-rum.png'), fullPage: false });
});

// 18 — On-call: a rotation over two channels.
test('18 on-call — rotation schedule', async ({ page, browserName }) => {
  await ensureLoggedIn(page);
  try {
    const c1 = await api(page, 'POST', '/v1/notifications', { kind: 'webhook', name: uniq('primary', browserName), config: { url: 'https://hooks.example.com/primary' }, active: true });
    const c2 = await api(page, 'POST', '/v1/notifications', { kind: 'webhook', name: uniq('secondary', browserName), config: { url: 'https://hooks.example.com/secondary' }, active: true });
    await api(page, 'POST', '/v1/on-call-schedules', {
      name: 'Platform on-call', rotation_seconds: 604800,
      anchor: new Date().toISOString(), participant_ids: [c1.id, c2.id],
    }).catch(() => {});
  } catch { /* ignore */ }
  await gotoView(page, '#/on-call');
  await page.waitForTimeout(600);
  await page.screenshot({ path: shot('18-on-call.png'), fullPage: false });
});

// 19 — Alert rules: a few telemetry threshold rules across tiers.
test('19 alert-rules — telemetry thresholds', async ({ page }) => {
  await ensureLoggedIn(page);
  const rules = [
    { name: 'Checkout error spike', kind: 'error_rate', target: 'web-frontend', op: 'gt', threshold: 10, window_seconds: 300 },
    { name: 'API p95 latency', kind: 'trace_latency', target: 'checkout', op: 'gt', threshold: 500, window_seconds: 600 },
    { name: 'Payments error rate', kind: 'trace_error_rate', target: 'payments', op: 'gt', threshold: 5, window_seconds: 600 },
    { name: 'Error-log flood', kind: 'log_volume', target: '', min_level: 17, op: 'gt', threshold: 50, window_seconds: 300 },
  ];
  for (const r of rules) await api(page, 'POST', '/v1/telemetry-rules', r).catch(() => {});
  await gotoView(page, '#/alert-rules');
  await page.waitForTimeout(600);
  await page.screenshot({ path: shot('19-alert-rules.png'), fullPage: false });
});

// 20 — Ingest token settings (populated field).
test('20 ingest-settings — telemetry token', async ({ page }) => {
  await ensureLoggedIn(page);
  await api(page, 'PUT', '/v1/settings/telemetry-token', { token: 'rmp_demo_ingest_token_5f3c9a' }).catch(() => {});
  await gotoView(page, '#/settings/ingest');
  await page.waitForTimeout(500);
  await page.screenshot({ path: shot('20-ingest-token.png'), fullPage: false });
  // Clear it again so it doesn't gate the other shots / reruns.
  await api(page, 'PUT', '/v1/settings/telemetry-token', { token: '' }).catch(() => {});
});

// 21 — Synthetics: the multi-step builder in the monitor wizard. Every
// interaction uses a short timeout + catch so a selector miss can't hang the
// 30s test budget; we screenshot whatever state we reach.
test('21 synthetics — step builder', async ({ page }) => {
  await ensureLoggedIn(page);
  await gotoView(page, '#/new-monitor');
  await expect(page.getByText(/Pick a check type/i)).toBeVisible();
  try {
    await page.getByText('Synthetic transaction', { exact: false })
      .first().click({ timeout: 4000 });
    await page.getByRole('button', { name: /continue/i })
      .click({ timeout: 4000 });
    // Step 2 of the synthetic flow is the ordered HTTP-step builder.
    await page.waitForTimeout(900);
  } catch { /* fall back to whatever rendered */ }
  await page.screenshot({ path: shot('21-synthetics.png'), fullPage: false });
});
