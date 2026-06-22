// E2E: maintenance windows CRUD + active-toggle flow.
//
// Admin creates a maintenance window via the API, navigates to the
// `#/maintenance` UI, asserts the window renders, flips `active` to
// false through the dedicated `/active` endpoint, reloads, and verifies
// the row now carries the `pill-paused` marker that `Maintenance.jsx`
// renders for inactive windows (see `windowState()` — `!w.active`
// short-circuits to `'paused'`).
//
// Window name is `uniq()`-prefixed so cross-browser runs against a
// shared DB don't collide, and the spec deletes the window at the end
// to keep the row count from drifting upward across re-runs.

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, gotoView, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('maintenance window CRUD + active toggle reflects in the UI', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  // 1. Create a one-shot window via the admin API. The backend (see
  //    `NewMaintenanceWindow` in `rampart-core/src/maintenance.rs`)
  //    expects `start_at` / `end_at` as RFC3339 strings, and rejects
  //    `end_at <= start_at`. Anchor 1h in the future for a 2h window
  //    so the row lands in the `upcoming` state on first render.
  const name = uniq('e2e-maint', browserName);
  const now      = Date.now();
  const startsAt = new Date(now + 60 * 60_000).toISOString();
  const endsAt   = new Date(now + 3 * 60 * 60_000).toISOString();

  const created = await api(page, 'POST', '/v1/maintenance-windows', {
    name,
    description: 'e2e maintenance window',
    recurrence:  { kind: 'none' },
    start_at:    startsAt,
    end_at:      endsAt,
  });
  expect(created?.id).toBeTruthy();
  expect(created.active).toBe(true);

  try {
    // 2. Visit the Maintenance view and confirm the row rendered.
    await gotoView(page, '#/maintenance', 'h1:has-text("Maintenance windows")');
    await expect(page.getByText(name)).toBeVisible({ timeout: 10_000 });

    // 3. Pause via the dedicated active toggle endpoint, then reload —
    //    the view does a full `window.location.reload()` on its own
    //    mutations, so we mimic that to pick up the new state.
    await api(page, 'POST', `/v1/maintenance-windows/${created.id}/active`, { active: false });
    await page.reload();

    // 4. UI reflects the paused state. `Maintenance.jsx` renders a
    //    `<span class="pill pill-paused">paused</span>` for inactive
    //    windows; scope the assertion to the row that owns our name so
    //    we don't accidentally match an unrelated paused window.
    const row = page.locator('.row').filter({ hasText: name });
    await expect(row).toBeVisible({ timeout: 10_000 });
    await expect(row.locator('.pill-paused')).toBeVisible();
    // The pause/resume button label flips to "Resume" once `active` is
    // false — extra signal that the toggle landed.
    await expect(row.getByRole('button', { name: /resume/i })).toBeVisible();
  } finally {
    // 5. Clean up so re-runs against a shared DB stay idempotent.
    await api(page, 'DELETE', `/v1/maintenance-windows/${created.id}`).catch(() => {});
  }
});
