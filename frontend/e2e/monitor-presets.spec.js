// E2E: monitor presets (saved config bags) + bulk-by-tag pause/resume.
//
// Two slices of backend/crates/rampart-api/src/routes/monitors.rs:
//
//   Presets (CRUD without PATCH — a preset is an immutable bag):
//     POST   /v1/monitors/presets        {name, kind, data} -> 201
//     GET    /v1/monitors/presets        -> list
//     DELETE /v1/monitors/presets/{id}   -> 204
//   `kind` is the snake_case MonitorPresetKind: "http_headers" | "tls".
//
//   Bulk-by-tag (POST /v1/monitors/bulk-by-tag {tag_id, action}):
//     action "pause"  -> flips matching monitors inactive, returns {affected}
//     action "resume" -> flips them active again
//   Both reuse `set_active_by_tag`; `affected` counts genuine transitions
//   only (monitors already in the target state aren't recounted), so the
//   pause-from-fresh and resume both report affected:2.
//
// Tags attach via /v1/monitors/{id}/tags/{tag_id} (POST). Monitor `active`
// is read back off the monitor row.

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

function monitorBody(browserName, tag) {
  return {
    name: uniq(`e2e-preset-mon-${tag}`, browserName),
    kind: 'http',
    url: 'https://example.com',
    interval_seconds: 60,
  };
}

test('monitor presets CRUD', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  let id = null;
  try {
    const name = uniq('e2e-preset', browserName);
    const created = await api(page, 'POST', '/v1/monitors/presets', {
      name,
      kind: 'http_headers',
      data: { headers: { 'X-Api-Version': '2', 'User-Agent': 'rampart-e2e' } },
    });
    expect(created?.id).toBeTruthy();
    expect(created.name).toBe(name);
    expect(created.kind).toBe('http_headers');
    id = created.id;

    const list = await api(page, 'GET', '/v1/monitors/presets');
    expect(list.find(p => p.id === id), 'preset in list').toBeTruthy();

    expect(await api(page, 'DELETE', `/v1/monitors/presets/${id}`), 'delete -> 204').toBeNull();
    const cleared = id; id = null;
    const list2 = await api(page, 'GET', '/v1/monitors/presets');
    expect(list2.find(p => p.id === cleared), 'preset gone after delete').toBeFalsy();
  } finally {
    if (id) await api(page, 'DELETE', `/v1/monitors/presets/${id}`).catch(() => {});
  }
});

test('bulk-by-tag pauses + resumes every tagged monitor', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  let mon1 = null, mon2 = null, tag = null;
  try {
    mon1 = await api(page, 'POST', '/v1/monitors', monitorBody(browserName, 'a'));
    mon2 = await api(page, 'POST', '/v1/monitors', monitorBody(browserName, 'b'));
    expect(mon1?.id && mon2?.id).toBeTruthy();

    tag = await api(page, 'POST', '/v1/tags', { name: uniq('e2e-bt', browserName) });
    expect(tag?.id).toBeTruthy();

    // Attach the tag to both monitors.
    await api(page, 'POST', `/v1/monitors/${mon1.id}/tags/${tag.id}`);
    await api(page, 'POST', `/v1/monitors/${mon2.id}/tags/${tag.id}`);

    // Pause -> affected:2.
    const paused = await api(page, 'POST', '/v1/monitors/bulk-by-tag', {
      tag_id: tag.id,
      action: 'pause',
    });
    expect(paused.affected, 'pause affected count').toBe(2);
    expect((await api(page, 'GET', `/v1/monitors/${mon1.id}`)).active, 'mon1 paused').toBe(false);
    expect((await api(page, 'GET', `/v1/monitors/${mon2.id}`)).active, 'mon2 paused').toBe(false);

    // Resume -> affected:2.
    const resumed = await api(page, 'POST', '/v1/monitors/bulk-by-tag', {
      tag_id: tag.id,
      action: 'resume',
    });
    expect(resumed.affected, 'resume affected count').toBe(2);
    expect((await api(page, 'GET', `/v1/monitors/${mon1.id}`)).active, 'mon1 active').toBe(true);
    expect((await api(page, 'GET', `/v1/monitors/${mon2.id}`)).active, 'mon2 active').toBe(true);
  } finally {
    if (mon1?.id) await api(page, 'DELETE', `/v1/monitors/${mon1.id}`).catch(() => {});
    if (mon2?.id) await api(page, 'DELETE', `/v1/monitors/${mon2.id}`).catch(() => {});
    if (tag?.id) await api(page, 'DELETE', `/v1/tags/${tag.id}`).catch(() => {});
  }
});
