// E2E: public status page subscribe flow.
//
// Walks the path a real user would: admin creates a status page,
// visitor lands on the public view at `#/s/<slug>`, drops in an
// email, clicks Subscribe, sees the success message. The spec
// then verifies the subscriber row exists via the admin API.
//
// The public route is unauthenticated — `/v1/public/status-pages/
// <slug>/subscribe` is the documented endpoint — so the spec
// drives the SubscribeBox form straight from the rendered DOM
// without any session setup beyond admin login for the page-
// creation step.

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, gotoView, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('public status page accepts a subscriber via the SubscribeBox', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  // 1. Create a status page via the admin API. Uniquely-slugged so
  //    cross-browser runs against a shared DB don't collide.
  const slug = uniq('e2e-status', browserName);
  const title = `E2E Status · ${browserName}`;
  const page1 = await api(page, 'POST', '/v1/status-pages', {
    slug,
    title,
    subtitle: 'subscriber e2e',
    public: true,
  });
  expect(page1?.id).toBeTruthy();

  // 2. Visit the public view at `#/s/<slug>`. SubscribeBox is rendered
  //    by `StatusPageView.jsx` near the footer.
  await gotoView(page, `#/s/${slug}`);
  // Wait for the page title to render — confirms the route resolved
  // and the public view fetched the page data successfully.
  await expect(page.getByRole('heading', { name: new RegExp(title, 'i') }))
    .toBeVisible({ timeout: 5_000 });

  // 3. Fill the email + click Subscribe. The form binds on email input
  //    type=email so we target it by placeholder.
  const email = `subscriber-${browserName}-${Date.now()}@example.com`;
  await page.getByPlaceholder(/you@example\.com/i).fill(email);
  await page.getByRole('button', { name: /^subscribe$/i }).click();

  // 4. Success message renders inline — `state === 'ok'` branch in
  //    SubscribeBox.
  await expect(page.getByText(/subscribed\./i)).toBeVisible({ timeout: 5_000 });

  // 5. Verify the subscriber persisted via the admin API. The list
  //    endpoint scopes to a specific status page id, so we fetch the
  //    page we just created and assert our email shows up.
  const subs = await api(page, 'GET', `/v1/status-pages/${page1.id}/subscribers`);
  expect(Array.isArray(subs)).toBe(true);
  expect(subs.find(s => s.destination === email)).toBeTruthy();

  // 6. Clean up — delete the subscriber + the status page so a re-run
  //    of the same browser project against the shared DB lands on a
  //    fresh slate. `unique` slugs already prevent re-run collision,
  //    but the cleanup keeps the row count from drifting upward across
  //    runs.
  const sub = subs.find(s => s.destination === email);
  await api(page, 'DELETE', `/v1/subscribers/${sub.id}`).catch(() => {});
  await api(page, 'DELETE', `/v1/status-pages/${page1.id}`).catch(() => {});
});
