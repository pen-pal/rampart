// E2E: admin status-page CRUD lifecycle.
//
// Counterpart to `subscribers.spec.js`, which exercises the *public*
// subscribe flow at `#/s/<slug>`. This spec stays on the admin side:
// create a page, render it in the StatusPageBuilder list at
// `#/status-page`, patch the title, reload, and verify the new title
// surfaces in the UI. Delete cleans up.
//
// State setup goes through the JSON API (same cookie jar as the
// browser session) — keeps the spec tight on what's unique to the
// admin lifecycle rather than re-driving the Editor form, which
// other specs/screenshots already cover.

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, gotoView, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('admin status-page CRUD: create, rename, delete', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  // 1. Create the page via the admin API. Unique slug so cross-browser
  //    runs against the shared DB don't collide.
  const slug         = uniq('e2e-admin-sp', browserName);
  const initialTitle = `E2E Admin SP ${browserName}`;
  const created = await api(page, 'POST', '/v1/status-pages', {
    slug,
    title:    initialTitle,
    subtitle: 'admin crud e2e',
    public:   true,
  });
  expect(created?.id).toBeTruthy();

  try {
    // 2. Navigate to the admin builder list. Route is singular —
    //    `#/status-page` (see frontend/src/lib/router.js).
    await gotoView(page, '#/status-page');

    // The list row renders title + the public URL containing the slug.
    // Wait on the title — it's the primary signal that the row hydrated.
    await expect(page.getByText(initialTitle, { exact: false }))
      .toBeVisible({ timeout: 10_000 });
    // The slug appears inside the auto-generated public URL link.
    await expect(page.getByText(new RegExp(`/#/s/${slug}\\b`)))
      .toBeVisible();

    // 3. Rename via PATCH. UpdateStatusPage accepts an optional `title`.
    const updatedTitle = `${initialTitle} updated`;
    const patched = await api(page, 'PATCH', `/v1/status-pages/${created.id}`, {
      title: updatedTitle,
    });
    expect(patched.title).toBe(updatedTitle);

    // 4. Reload — StatusPageBuilder polls every 30s but a reload makes
    //    the assertion deterministic without waiting on the poll tick.
    await page.reload();
    await expect(page.getByText(updatedTitle, { exact: false }))
      .toBeVisible({ timeout: 10_000 });
    // Old title should be gone (the row is the same, just with a new title).
    await expect(page.getByText(initialTitle, { exact: true }))
      .toHaveCount(0);

    // 5. Delete via API and confirm the row disappears on next list call.
    await api(page, 'DELETE', `/v1/status-pages/${created.id}`);
    const remaining = await api(page, 'GET', '/v1/status-pages');
    expect(remaining.find(p => p.id === created.id)).toBeFalsy();
  } finally {
    // Belt-and-suspenders: if any assertion above blew up before the
    // explicit DELETE, still try to remove the row so the next run
    // lands on a clean slate.
    await api(page, 'DELETE', `/v1/status-pages/${created.id}`).catch(() => {});
  }
});
