// E2E: bulk-edit (POST /v1/monitors/bulk-edit).
//
// backend/crates/rampart-api/src/routes/monitors.rs::bulk_edit takes:
//   { ids: [..], interval_seconds?, timeout_seconds?, add_tag_ids: [..],
//     remove_tag_ids: [..] }
// and returns { updated: N } — N = monitors that had at least one mutation
// applied without error. Interval / timeout are validated against the same
// ranges UpdateMonitor enforces (interval_seconds 10..=86400); an out-of-range
// value fails the whole request with a 400 BEFORE any per-id work.
//
// Flow:
//   1. Create 2 monitors + a tag.
//   2. bulk-edit { ids:[both], interval_seconds:120, add_tag_ids:[tag] }
//      -> 200 { updated:2 }; both monitors now have interval 120 + the tag.
//   3. bulk-edit { ids:[both], remove_tag_ids:[tag] } -> tag removed.
//   4. bulk-edit { ids:[both], interval_seconds:0 } -> 400 (out of range).
//
// Monitors + tag are uniq()-suffixed and removed in a finally.

import { test, expect } from '@playwright/test';
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

test('bulk-edit sets interval + adds/removes tags across monitors; rejects out-of-range interval', async ({ page, browserName }) => {
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

    // 2. Set interval + add the tag to both.
    const edit = await api(page, 'POST', '/v1/monitors/bulk-edit', {
      ids: [m1, m2],
      interval_seconds: 120,
      add_tag_ids: [tagId],
    });
    expect(edit.updated, 'both monitors updated').toBe(2);

    for (const id of [m1, m2]) {
      const m = await api(page, 'GET', `/v1/monitors/${id}`);
      expect(m.interval_seconds, `monitor ${id} interval set to 120`).toBe(120);
      expect((m.tags || []).some(t => t.id === tagId), `monitor ${id} has the tag`).toBe(true);
    }

    // 3. Remove the tag from both.
    const remove = await api(page, 'POST', '/v1/monitors/bulk-edit', {
      ids: [m1, m2],
      remove_tag_ids: [tagId],
    });
    expect(remove.updated, 'both monitors updated on tag removal').toBe(2);

    for (const id of [m1, m2]) {
      const m = await api(page, 'GET', `/v1/monitors/${id}`);
      expect((m.tags || []).some(t => t.id === tagId), `monitor ${id} no longer has the tag`).toBe(false);
    }

    // 4. Out-of-range interval (min is 10) is rejected before any per-id work.
    //
    // ACTUAL backend shape: bulk_edit validates via the shared UpdateMonitor
    // `validator::Validate` (range 10..=86400). A range failure surfaces as
    // HTTP 422 (validation), NOT 400 — only the hand-rolled
    // `ApiError::BadRequest` guards in the handler (empty ids, bad tag UUID)
    // return 400. So we assert 422 here.
    const bad = await rawApi(page, 'POST', '/v1/monitors/bulk-edit', {
      ids: [m1, m2],
      interval_seconds: 0,
    });
    expect(bad.status(), 'out-of-range interval_seconds rejected (422 validation)').toBe(422);
    // Confirm nothing changed: interval still 120 from step 2.
    const m1After = await api(page, 'GET', `/v1/monitors/${m1}`);
    expect(m1After.interval_seconds, 'rejected edit left interval untouched').toBe(120);
  } finally {
    if (m1) await api(page, 'DELETE', `/v1/monitors/${m1}`).catch(() => {});
    if (m2) await api(page, 'DELETE', `/v1/monitors/${m2}`).catch(() => {});
    if (tagId) await api(page, 'DELETE', `/v1/tags/${tagId}`).catch(() => {});
  }
});
