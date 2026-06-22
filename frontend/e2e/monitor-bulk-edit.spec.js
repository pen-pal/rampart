// E2E: bulk-edit (POST /v1/monitors/bulk-edit).
//
// backend/crates/rampart-api/src/routes/monitors.rs::bulk_edit takes:
//   { ids: [..], patch: { interval_secs?, timeout_secs?, enabled?,
//                         group_id?(null clears), tags?:[..] (REPLACE set) } }
// and returns { updated: N, skipped: M } — N = monitors mutated, M = ids that
// matched no monitor. The whole batch runs in one transaction. Interval /
// timeout are range-checked up front via the shared UpdateMonitor validator
// (interval 10..=86400); an out-of-range value fails the whole request before
// any per-id work.
//
// Flow:
//   1. Create 2 monitors + a tag.
//   2. bulk-edit { ids:[both], patch:{ interval_secs:120, tags:[tag] } }
//      -> 200 { updated:2 }; both monitors now have interval 120 + the tag.
//   3. bulk-edit { ids:[both], patch:{ tags:[] } } -> tag set replaced empty.
//   4. bulk-edit { ids:[both], patch:{ interval_secs:0 } } -> rejected, no change.
//
// Monitors + tag are uniq()-suffixed and removed in a finally.

import { test, expect } from './fixtures.js';
import { api, rawApi, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

function newMonitorBody(browserName, n) {
  return {
    name: uniq(`e2e-bulk-mon${n}`, browserName),
    kind: 'http',
    url: 'https://example.com',
    interval_seconds: 60,
  };
}

test('bulk-edit sets interval + replaces tags across monitors; rejects out-of-range interval', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  let m1 = null;
  let m2 = null;
  let tagId = null;
  try {
    const mon1 = await api(page, 'POST', '/v1/monitors', newMonitorBody(browserName, 1));
    const mon2 = await api(page, 'POST', '/v1/monitors', newMonitorBody(browserName, 2));
    m1 = mon1.id;
    m2 = mon2.id;
    expect(m1 && m2).toBeTruthy();

    const tag = await api(page, 'POST', '/v1/tags', {
      name: uniq('e2e-bulk-tag', browserName),
      color: '#ff8800',
    });
    tagId = tag.id;
    expect(tagId).toBeTruthy();

    // 2. Set interval + the tag set to [tag] on both.
    const edit = await api(page, 'POST', '/v1/monitors/bulk-edit', {
      ids: [m1, m2],
      patch: { interval_secs: 120, tags: [tagId] },
    });
    expect(edit.updated, 'both monitors updated').toBe(2);

    for (const id of [m1, m2]) {
      const m = await api(page, 'GET', `/v1/monitors/${id}`);
      expect(m.interval_seconds, `monitor ${id} interval set to 120`).toBe(120);
      expect((m.tags || []).some(t => t.id === tagId), `monitor ${id} has the tag`).toBe(true);
    }

    // 3. Replace the tag set with empty -> tag removed from both.
    const cleared = await api(page, 'POST', '/v1/monitors/bulk-edit', {
      ids: [m1, m2],
      patch: { tags: [] },
    });
    expect(cleared.updated, 'both monitors updated on tag clear').toBe(2);

    for (const id of [m1, m2]) {
      const m = await api(page, 'GET', `/v1/monitors/${id}`);
      expect((m.tags || []).some(t => t.id === tagId), `monitor ${id} no longer has the tag`).toBe(false);
    }

    // 3b. An unknown id is counted in `skipped`, real ones still update.
    const mixed = await api(page, 'POST', '/v1/monitors/bulk-edit', {
      ids: [m1, '00000000-0000-0000-0000-000000000000'],
      patch: { interval_secs: 150 },
    });
    expect(mixed.updated, 'one real monitor updated').toBe(1);
    expect(mixed.skipped, 'one unknown id skipped').toBe(1);

    // 4. Out-of-range interval (min is 10) is rejected before any per-id work.
    // The shared UpdateMonitor validator surfaces a range failure as a 4xx
    // (validation). Assert it is rejected and left nothing changed.
    const bad = await rawApi(page, 'POST', '/v1/monitors/bulk-edit', {
      ids: [m1, m2],
      patch: { interval_secs: 0 },
    });
    expect(bad.status(), 'out-of-range interval rejected').toBeGreaterThanOrEqual(400);
    // Confirm nothing changed: interval still 150 from step 3b for m1.
    const m1After = await api(page, 'GET', `/v1/monitors/${m1}`);
    expect(m1After.interval_seconds, 'rejected edit left interval untouched').toBe(150);
  } finally {
    if (m1) await api(page, 'DELETE', `/v1/monitors/${m1}`).catch(() => {});
    if (m2) await api(page, 'DELETE', `/v1/monitors/${m2}`).catch(() => {});
    if (tagId) await api(page, 'DELETE', `/v1/tags/${tagId}`).catch(() => {});
  }
});
