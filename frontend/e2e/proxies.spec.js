// E2E: admin proxies CRUD lifecycle.
//
// Counterpart to the other admin-surface specs (`subscribers`,
// `status-page-admin`, `api-keys`). Drives the `/v1/proxies` surface
// end-to-end: create through the JSON API, render the list at
// `#/proxies`, flip `active` via the POST `/{id}/active` toggle,
// reload, assert the new state surfaces in the UI, then DELETE and
// confirm the row disappears.
//
// The active toggle is the load-bearing detail of this surface — the
// read response redacts `password` (#[serde(skip_serializing)] in
// rampart-core/src/proxy.rs) so we can't assert on creds, but the
// `active` boolean drives the `pill-active` / `pill-paused` chip and
// the Pause/Resume button label. That's the deterministic signal.
//
// Cross-browser projects share a single DB, so host names are uniq()-
// suffixed and cleanup runs in a finally so a mid-spec panic still
// removes the row.
//
// API shapes are documented in
//   backend/crates/rampart-api/src/routes/proxies.rs

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, gotoView, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('admin proxies CRUD: create, active-toggle, delete', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  // 1. Create via the admin API. Host is uniq()-suffixed so parallel
  //    browser projects against the shared DB don't collide. The host
  //    is what surfaces in the list row (`protocol://host:port`), so
  //    using a uniq value makes the DOM assertion specific.
  const host = `${uniq('e2e-proxy', browserName)}.invalid`;
  const created = await api(page, 'POST', '/v1/proxies', {
    protocol: 'http',
    host,
    port:     8080,
    username: null,
    password: null,
  });
  expect(created?.id).toBeTruthy();
  expect(created.host).toBe(host);
  // `proxies::create` defaults `active=true`. Confirm so the toggle
  // step below has a known starting state.
  expect(created.active).toBe(true);

  try {
    // 2. Navigate to the admin list view at `#/proxies`. The row renders
    //    `<protocol>://<host>:<port>` in a mono span + an active/paused
    //    pill chip. Wait on the heading first, then assert the row.
    await gotoView(page, '#/proxies');
    await expect(page.getByRole('heading', { name: /^proxies$/i }))
      .toBeVisible({ timeout: 10_000 });
    // The mono span is `http://<host>:8080` — host alone is enough
    // (it's uniq) to identify the row.
    await expect(page.getByText(host, { exact: false }))
      .toBeVisible({ timeout: 10_000 });
    // Active pill should be present somewhere on the page. There may be
    // other proxies in the shared DB; this just confirms ours rendered
    // in the active state.
    await expect(page.getByText('active', { exact: true }).first())
      .toBeVisible();

    // 3. Toggle to paused via the dedicated POST `/{id}/active` route.
    //    Driving via the API matches the pattern used by the other
    //    admin CRUD specs (state setup/teardown through JSON, UI for
    //    assertions). The endpoint returns 204 NoContent.
    await api(page, 'POST', `/v1/proxies/${created.id}/active`, { active: false });

    // Confirm the API surface flipped.
    const afterToggle = await api(page, 'GET', '/v1/proxies');
    const toggled = afterToggle.find(p => p.id === created.id);
    expect(toggled).toBeTruthy();
    expect(toggled.active).toBe(false);

    // 4. Reload — the list polls every 30s, reload is deterministic.
    //    The row should now render the `paused` pill and the host
    //    should still be visible (delete hasn't happened yet).
    await page.reload();
    await expect(page.getByText(host, { exact: false }))
      .toBeVisible({ timeout: 10_000 });
    // The same row now exposes a "Resume" button (label flips from
    // Pause → Resume when active=false). Asserting on the button label
    // is sturdier than scanning every pill on the page for "paused".
    await expect(page.getByRole('button', { name: /resume/i }).first())
      .toBeVisible({ timeout: 10_000 });

    // 5. Delete via the admin API and confirm the row disappears.
    await api(page, 'DELETE', `/v1/proxies/${created.id}`);
    await page.reload();
    await expect(page.getByText(host, { exact: false }))
      .toHaveCount(0, { timeout: 10_000 });

    // 6. Confirm the API agrees the row is gone.
    const after = await api(page, 'GET', '/v1/proxies');
    expect(after.find(p => p.id === created.id)).toBeFalsy();
  } finally {
    // Belt-and-suspenders: if any assertion above blew up before the
    // explicit DELETE, still try to remove the row so the next run
    // lands on a clean slate. The shared cross-browser DB punishes
    // any drift.
    await api(page, 'DELETE', `/v1/proxies/${created.id}`).catch(() => {});
  }
});
