// E2E: SLO error-budget burn-down endpoint + window param.
//
// Drives GET /v1/monitors/{id}/slo/burndown (monitors.rs::slo_burndown):
//   - Returns `{window_days, target_pct, allowed_downtime_secs, points:[…]}`
//     for a monitor with both `slo_target_pct` + `slo_window_days` set.
//   - `?window_days=` is whitelisted to 7/30/90; anything else -> 400.
//     When omitted it falls back to the monitor's configured window.
//   - A monitor with NO SLO configured -> 404 (same contract as the
//     error-budget endpoint; the frontend reads the row first to decide
//     whether to render the gauge at all).
//
// Two monitors created (one with SLO, one without), both uniq()-named and
// removed in a finally so the shared cross-browser DB stays clean.

import { test, expect } from '@playwright/test';
import { api, rawApi, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('slo burndown: window param whitelist + 404 for unconfigured monitor', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  // Monitor WITH an SLO configured. slo_target_pct + slo_window_days must
  // both be set (the backend 404s the endpoint when either is null).
  const withSlo = await api(page, 'POST', '/v1/monitors', {
    name: uniq('e2e-slo', browserName),
    kind: 'http',
    url: 'https://example.com',
    interval_seconds: 60,
    slo_target_pct: 99.9,
    slo_window_days: 30,
  });
  expect(withSlo?.id).toBeTruthy();

  // Monitor with NO SLO — burndown should 404.
  const noSlo = await api(page, 'POST', '/v1/monitors', {
    name: uniq('e2e-noslo', browserName),
    kind: 'http',
    url: 'https://example.com',
    interval_seconds: 60,
  });
  expect(noSlo?.id).toBeTruthy();

  try {
    // Default window -> 200, falls back to the configured 30.
    const base = await api(page, 'GET', `/v1/monitors/${withSlo.id}/slo/burndown`);
    expect(Array.isArray(base.points), 'burndown returns points[]').toBe(true);
    expect(base.window_days, 'default window = configured 30').toBe(30);

    // Explicit whitelisted windows echo back.
    const w7 = await api(page, 'GET', `/v1/monitors/${withSlo.id}/slo/burndown?window_days=7`);
    expect(w7.window_days).toBe(7);
    expect(Array.isArray(w7.points)).toBe(true);

    const w30 = await api(page, 'GET', `/v1/monitors/${withSlo.id}/slo/burndown?window_days=30`);
    expect(w30.window_days).toBe(30);

    // Non-whitelisted window -> 400.
    const bad = await rawApi(page, 'GET', `/v1/monitors/${withSlo.id}/slo/burndown?window_days=99`);
    expect(bad.status(), 'window_days=99 should 400').toBe(400);

    // Monitor with no SLO -> 404.
    const unconfigured = await rawApi(page, 'GET', `/v1/monitors/${noSlo.id}/slo/burndown`);
    expect(unconfigured.status(), 'unconfigured monitor burndown should 404').toBe(404);
  } finally {
    await api(page, 'DELETE', `/v1/monitors/${withSlo.id}`).catch(() => {});
    await api(page, 'DELETE', `/v1/monitors/${noSlo.id}`).catch(() => {});
  }
});
