// E2E: admin API-keys CRUD lifecycle.
//
// Counterpart to the other admin-surface specs (`subscribers`,
// `status-page-admin`). Drives the `/v1/api-keys` surface end-to-end:
// create through the JSON API (so we can capture the once-shown
// plaintext token), render the list at `#/api-keys`, confirm the row
// hydrated with the key prefix, then revoke via DELETE and confirm
// the row disappears from both the API and the UI on reload.
//
// The plaintext `token` is the load-bearing detail of the create
// endpoint — the server hashes the secret half on insert, so this
// is the only window to see it. The spec asserts the prefix matches
// `rmp_` (the documented bearer-token shape) and that the same
// prefix surfaces in the list row.
//
// Cross-browser projects share a single DB, so names are uniq()-suffixed
// and cleanup runs in a finally so a mid-spec panic still removes the row.
//
// API shapes are documented in
//   backend/crates/rampart-api/src/routes/api_keys.rs
// and the response struct lives in
//   backend/crates/rampart-core/src/api_key.rs (IssuedApiKey { key, token }).

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, gotoView, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('admin api-keys CRUD: create (capture plaintext), list, revoke', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  // 1. Create via the admin API. Name is uniq()-suffixed so parallel
  //    browser projects against the shared DB don't collide.
  const name = uniq('e2e-api-key', browserName);
  const issued = await api(page, 'POST', '/v1/api-keys', {
    name,
    scopes: [],
    expires_at: null,
  });

  // Response is `IssuedApiKey { key, token }` — `token` is the only
  // shot at the plaintext. Capture + sanity-check it now.
  expect(issued?.key?.id).toBeTruthy();
  expect(issued.key.name).toBe(name);
  expect(typeof issued.token).toBe('string');
  expect(issued.token.startsWith('rmp_')).toBe(true);
  // Capture the prefix the UI will display on the list row.
  const prefix = issued.key.key_prefix;
  expect(typeof prefix).toBe('string');
  expect(prefix.length).toBeGreaterThan(0);

  const createdId = issued.key.id;

  try {
    // 2. Confirm the list endpoint exposes the row WITHOUT the secret half.
    //    `ApiKey` is the read shape — no `token` field, just `key_prefix`.
    const list = await api(page, 'GET', '/v1/api-keys');
    expect(Array.isArray(list)).toBe(true);
    const row = list.find(k => k.id === createdId);
    expect(row).toBeTruthy();
    expect(row.name).toBe(name);
    // Defensive: the list response must never include the plaintext.
    expect(row.token).toBeUndefined();

    // 3. Navigate to the admin list view at `#/api-keys`. The row renders
    //    the name (bold) + the key_prefix in a mono span — wait on both.
    await gotoView(page, '#/api-keys');
    await expect(page.getByRole('heading', { name: /api keys/i }))
      .toBeVisible({ timeout: 10_000 });
    await expect(page.getByText(name, { exact: false }))
      .toBeVisible({ timeout: 10_000 });
    // The list row shows `<prefix>… · created …` in the mono span. Match
    // on the prefix substring — that's the unique identifier the user
    // sees once the plaintext is gone.
    await expect(page.getByText(new RegExp(prefix.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))))
      .toBeVisible();

    // 4. Revoke via DELETE. The UI's Revoke button fires a confirm()
    //    dialog before calling the API, but driving via the API is
    //    deterministic and matches the pattern used by the other admin
    //    CRUD specs (state setup/teardown through JSON, UI for assertions).
    await api(page, 'DELETE', `/v1/api-keys/${createdId}`);

    // 5. Reload — the list polls every 30s, but reload makes the
    //    assertion deterministic without waiting on the next tick.
    await page.reload();
    await expect(page.getByText(name, { exact: false }))
      .toHaveCount(0, { timeout: 10_000 });

    // 6. Confirm the API agrees the row is gone.
    const after = await api(page, 'GET', '/v1/api-keys');
    expect(after.find(k => k.id === createdId)).toBeFalsy();
  } finally {
    // Belt-and-suspenders: if any assertion above blew up before the
    // explicit DELETE, still try to remove the row so the next run
    // lands on a clean slate. The shared cross-browser DB punishes
    // any drift.
    await api(page, 'DELETE', `/v1/api-keys/${createdId}`).catch(() => {});
  }
});
