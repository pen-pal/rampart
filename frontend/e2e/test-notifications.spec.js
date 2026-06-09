// E2E: per-monitor "test all channels" endpoint.
//
// Drives POST /v1/monitors/{id}/test-notifications
//   backend/crates/rampart-api/src/routes/monitors.rs::test_notifications
// which resolves every channel attached to a monitor (directly + via tag /
// folder routing — the same resolution the alerting path uses) and fires a
// synthetic Test event through each, returning a per-channel ok/error list:
//   { sent: [ { channel_id, ok, error? }, ... ] }
//
// Flow:
//   1. Create a monitor + a generic webhook channel pointing at a host that
//      won't actually accept the POST (https://example.com).
//   2. Attach the channel directly to the monitor via
//      POST /v1/monitors/{mid}/notifications/{nid} (notifications.rs
//      monitor_attach_router → attach, returns 204).
//   3. POST /v1/monitors/{id}/test-notifications -> 200 with a `sent` array
//      carrying at least one entry whose channel_id is our channel. The send
//      itself may "fail" against the fake webhook — that's expected; we only
//      assert the endpoint returns the per-channel result, not that every
//      send succeeded.
//
// Cross-browser projects share one DB, so names are uniq()-suffixed and the
// monitor + channel are torn down in a finally.

import { test, expect } from '@playwright/test';
import { api, rawApi, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('monitor test-notifications fires every attached channel and returns a per-channel result', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  let monitorId = null;
  let channelId = null;
  try {
    // 1. Monitor.
    const monitor = await api(page, 'POST', '/v1/monitors', {
      name: uniq('e2e-testnotif-mon', browserName),
      kind: 'http',
      url: 'https://example.com',
      interval_seconds: 60,
    });
    monitorId = monitor.id;
    expect(monitorId).toBeTruthy();

    // 1b. Generic webhook channel. `config.url` points at a host that won't
    // accept the synthetic POST, so the send may fail — that's fine.
    const channel = await api(page, 'POST', '/v1/notifications', {
      kind: 'webhook',
      name: uniq('e2e-testnotif-ch', browserName),
      config: { url: 'https://example.com/hook' },
      active: true,
    });
    channelId = channel.id;
    expect(channelId).toBeTruthy();

    // 2. Attach directly to the monitor (204 no-content).
    const attach = await rawApi(page, 'POST', `/v1/monitors/${monitorId}/notifications/${channelId}`);
    expect(attach.status(), 'attach channel to monitor').toBe(204);

    // Confirm the attach landed (the monitor now lists the channel).
    const attached = await api(page, 'GET', `/v1/monitors/${monitorId}/notifications`);
    expect(attached.some(c => c.id === channelId), 'channel listed for monitor').toBe(true);

    // 3. Fire test-notifications.
    const res = await rawApi(page, 'POST', `/v1/monitors/${monitorId}/test-notifications`);
    expect(res.status(), 'test-notifications status').toBe(200);
    const body = await res.json();
    expect(Array.isArray(body.sent), 'response carries a `sent` array').toBe(true);
    expect(body.sent.length, 'at least one channel result').toBeGreaterThanOrEqual(1);
    // Our channel must be among the per-channel results. We assert the result
    // shape (channel_id + boolean ok), NOT that the send to the fake webhook
    // succeeded.
    const ours = body.sent.find(r => r.channel_id === channelId);
    expect(ours, 'our channel appears in the sent results').toBeTruthy();
    expect(typeof ours.ok, 'per-channel result carries an ok boolean').toBe('boolean');
  } finally {
    if (monitorId) await api(page, 'DELETE', `/v1/monitors/${monitorId}`).catch(() => {});
    if (channelId) await api(page, 'DELETE', `/v1/notifications/${channelId}`).catch(() => {});
  }
});
