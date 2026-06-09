// E2E: per-channel digest window field (migration 0053).
//
// notifications.digest_window_secs coalesces a channel's events into one
// combined message every N seconds. Contract (rampart-db notifications.rs +
// migration 0053):
//   - 0   = immediate (legacy default).
//   - N   = coalesce into one message per N seconds.
//   - The db layer clamps writes to 0..=3600 (`clamp_digest_window`); the DB
//     CHECK is a backstop. So an out-of-range value should be CLAMPED to 3600
//     on persist rather than rejected — but we accept a 400 too, in case the
//     API rejects it before the clamp.
//
// Flow:
//   1. Create a webhook channel with digest_window_secs: 60 -> persists on GET.
//   2. PATCH to 0 -> persists (immediate).
//   3. PATCH to 99999 (out of range) -> the channel lands at 3600 (clamped)
//      OR the request is rejected with 400. Assert whichever the backend does.
//
// uniq()-suffixed name; channel removed in a finally.

import { test, expect } from '@playwright/test';
import { api, rawApi, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('channel digest_window_secs round-trips and clamps (or rejects) out-of-range values', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  let channelId = null;
  try {
    // 1. Create with a 60s digest window.
    const created = await api(page, 'POST', '/v1/notifications', {
      kind: 'webhook',
      name: uniq('e2e-digest-ch', browserName),
      config: { url: 'https://example.com/hook' },
      active: true,
      digest_window_secs: 60,
    });
    channelId = created.id;
    expect(channelId).toBeTruthy();
    expect(created.digest_window_secs, 'create persists digest_window_secs').toBe(60);

    const get1 = await api(page, 'GET', `/v1/notifications/${channelId}`);
    expect(get1.digest_window_secs, 'GET reflects 60').toBe(60);

    // 2. Update to 0 (immediate).
    const upd0 = await api(page, 'PATCH', `/v1/notifications/${channelId}`, {
      digest_window_secs: 0,
    });
    expect(upd0.digest_window_secs, 'update to 0 persists').toBe(0);
    const get0 = await api(page, 'GET', `/v1/notifications/${channelId}`);
    expect(get0.digest_window_secs, 'GET reflects 0').toBe(0);

    // 3. Out-of-range (99999). Backend clamps 0..=3600 on write, so expect it
    //    to land at 3600 — but tolerate a 400 if the API rejects instead.
    const raw = await rawApi(page, 'PATCH', `/v1/notifications/${channelId}`, {
      digest_window_secs: 99999,
    });
    if (raw.status() === 400) {
      // Rejected outright — acceptable. Confirm the stored value is unchanged.
      const after = await api(page, 'GET', `/v1/notifications/${channelId}`);
      expect(after.digest_window_secs, 'rejected update left value untouched').toBe(0);
    } else {
      expect(raw.status(), 'out-of-range update accepted (clamped)').toBe(200);
      const body = await raw.json();
      expect(body.digest_window_secs, 'out-of-range value clamped to 3600').toBe(3600);
      const after = await api(page, 'GET', `/v1/notifications/${channelId}`);
      expect(after.digest_window_secs, 'GET reflects the clamped 3600').toBe(3600);
    }
  } finally {
    if (channelId) await api(page, 'DELETE', `/v1/notifications/${channelId}`).catch(() => {});
  }
});
