// E2E: clone a monitor into a chosen group/folder.
//
// Drives POST /v1/monitors/{id}/clone in
//   backend/crates/rampart-api/src/routes/monitors.rs::clone_one
// with the optional CloneRequest body { group_id, name }. group_id is
// tri-state:
//   - omitted        -> inherit the source's group
//   - null           -> clone into "ungrouped" (group_id = None)
//   - set (must exist)-> clone into that group; a non-existent group 400s
// `name` overrides the default "<name> (copy)" label. The clone is a fresh
// probe surface (no heartbeat history / tags / deps copied). The response
// is the new Monitor row, carrying group_id + name.
//
// Flow:
//   1. Create a monitor + a monitor-group.
//   2. Clone {group_id, name} -> clone lands in that group with that name.
//   3. Clone {group_id: null} -> clone is ungrouped (group_id == null).
//   4. Clone {group_id: <bogus uuid>} -> 400.
// All created monitors + the group are torn down in finally.

import { test, expect } from './fixtures.js';
import { api, rawApi, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

function monitorBody(browserName) {
  return {
    name: uniq('e2e-clone-src', browserName),
    kind: 'http',
    url: 'https://example.com',
    interval_seconds: 60,
  };
}

test('clone-to-folder: targeted group, ungrouped null, bad group 400', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  let src = null, group = null;
  const clones = [];
  try {
    src = await api(page, 'POST', '/v1/monitors', monitorBody(browserName));
    expect(src?.id).toBeTruthy();

    group = await api(page, 'POST', '/v1/monitor-groups', { name: uniq('e2e-clone-grp', browserName) });
    expect(group?.id).toBeTruthy();

    // --- clone into the group with an explicit name ---
    const cloneName = uniq('e2e-clone-into-grp', browserName);
    const inGroup = await api(page, 'POST', `/v1/monitors/${src.id}/clone`, {
      group_id: group.id,
      name: cloneName,
    });
    expect(inGroup?.id).toBeTruthy();
    clones.push(inGroup.id);
    // Read it back to confirm the persisted group_id + name.
    const inGroupRead = await api(page, 'GET', `/v1/monitors/${inGroup.id}`);
    expect(inGroupRead.group_id, 'clone landed in the target group').toBe(group.id);
    expect(inGroupRead.name, 'clone took the override name').toBe(cloneName);

    // --- clone with group_id: null -> ungrouped ---
    const ungrouped = await api(page, 'POST', `/v1/monitors/${src.id}/clone`, {
      group_id: null,
      name: uniq('e2e-clone-ungrouped', browserName),
    });
    expect(ungrouped?.id).toBeTruthy();
    clones.push(ungrouped.id);
    const ungroupedRead = await api(page, 'GET', `/v1/monitors/${ungrouped.id}`);
    expect(ungroupedRead.group_id, 'null group_id -> ungrouped clone').toBeFalsy();

    // --- bad (non-existent) group_id -> 400 ---
    const bogus = '00000000-0000-7000-8000-000000000000';
    const bad = await rawApi(page, 'POST', `/v1/monitors/${src.id}/clone`, { group_id: bogus });
    expect(bad.status(), 'clone into non-existent group -> 400').toBe(400);
  } finally {
    for (const cid of clones) {
      await api(page, 'DELETE', `/v1/monitors/${cid}`).catch(() => {});
    }
    if (src?.id) await api(page, 'DELETE', `/v1/monitors/${src.id}`).catch(() => {});
    if (group?.id) await api(page, 'DELETE', `/v1/monitor-groups/${group.id}`).catch(() => {});
  }
});
