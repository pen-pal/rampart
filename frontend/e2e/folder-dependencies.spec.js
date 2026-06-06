// E2E: folder + monitor dependency-edge wiring.
//
// The dependency-edge API lives at /v1/monitors/{id}/dependencies/{parent_id}
// (mounted from monitor_groups::dep_router); it operates on monitor IDs.
// This spec also seeds two folders so the folder-create surface is touched
// while we're at it, then walks the dep edges:
//   * attach a dep edge child → parent
//   * read it back via GET and assert parent.id appears in `parents`
//   * confirm the cycle guard rejects the reverse edge
//   * tear it all back down
//
// Mirrors the self-contained shape of routing.spec.js.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test('folder + monitor dependency edges attach, list, and refuse cycles', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  // 1. Two folders for the monitor-group surface (cleaned up at end).
  const parentFolderName = uniq('e2e-deps-parent', browserName);
  const childFolderName  = uniq('e2e-deps-child',  browserName);
  const parentFolder = await api(page, 'POST', '/v1/monitor-groups',
    { name: parentFolderName, parent_id: null });
  const childFolder  = await api(page, 'POST', '/v1/monitor-groups',
    { name: childFolderName,  parent_id: null });
  expect(parentFolder.id).toBeTruthy();
  expect(childFolder.id).toBeTruthy();

  // 2. Two monitors — the dep edges are between monitors, even though the
  //    handler lives in the monitor_groups module.
  const mkMon = (name) => ({
    name, kind: 'http', url: 'https://deps.example.com',
    interval_seconds: 60, retry_interval_sec: 30, max_retries: 0,
    timeout_seconds: 10, resend_interval_sec: 0, upside_down: false,
    http_method: 'GET', accepted_statuses: [200],
    follow_redirect: true, ignore_tls: false,
  });
  const parentMon = await api(page, 'POST', '/v1/monitors',
    mkMon(uniq('e2e-deps-parent-mon', browserName)));
  const childMon  = await api(page, 'POST', '/v1/monitors',
    mkMon(uniq('e2e-deps-child-mon',  browserName)));

  try {
    // 3. Attach the edge: child depends on parent.
    await api(page, 'POST',
      `/v1/monitors/${childMon.id}/dependencies/${parentMon.id}`);

    // 4. Read deps back; the handler returns { parents, children }. The
    //    edge we just wrote should land in `parents` (what the child
    //    waits on).
    const deps = await api(page, 'GET',
      `/v1/monitors/${childMon.id}/dependencies`);
    const parents = Array.isArray(deps) ? deps : (deps.parents ?? []);
    expect(parents).toContain(parentMon.id);

    // 5. Cycle guard: reverse edge (parent → child) must be rejected.
    //    api() throws on non-2xx, which is exactly what we want to catch.
    let cycleRejected = false;
    try {
      await api(page, 'POST',
        `/v1/monitors/${parentMon.id}/dependencies/${childMon.id}`);
    } catch (_err) {
      cycleRejected = true;
    }
    expect(cycleRejected).toBe(true);

    // 6. Detach the edge cleanly.
    await api(page, 'DELETE',
      `/v1/monitors/${childMon.id}/dependencies/${parentMon.id}`);
    const after = await api(page, 'GET',
      `/v1/monitors/${childMon.id}/dependencies`);
    const parentsAfter = Array.isArray(after) ? after : (after.parents ?? []);
    expect(parentsAfter).not.toContain(parentMon.id);
  } finally {
    // 7. Always clean up monitors + folders so reruns stay green.
    await api(page, 'DELETE', `/v1/monitors/${childMon.id}`).catch(() => {});
    await api(page, 'DELETE', `/v1/monitors/${parentMon.id}`).catch(() => {});
    await api(page, 'DELETE', `/v1/monitor-groups/${childFolder.id}`).catch(() => {});
    await api(page, 'DELETE', `/v1/monitor-groups/${parentFolder.id}`).catch(() => {});
  }
});
