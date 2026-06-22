// E2E: by-domain public status-page lookup.
//
// Drives GET /v1/public/status-pages/by-domain/{host}
// (status_pages.rs::public_view_by_domain), which resolves a page by its
// operator-set `custom_domain` and serves the same projection as the
// slug route. Backs the frontend host-header probe: a visitor hitting
// `status.acme.com` boots the public view in place of the dashboard.
// An unknown host returns 404, which the frontend treats as "not a
// custom-domain host" and falls through silently.
//
// custom_domain must be a bare lowercase hostname (letters/digits/dots/
// hyphens, per `validate_custom_domain`), so we mint one shaped like
// `e2e-<uniq>.example.test`. uniq()-suffixed + torn down in a finally so
// the unique custom_domain index stays clean for cross-browser re-runs.

import { test, expect } from './fixtures.js';
import { api, rawApi, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('custom domain: by-domain lookup resolves to the page; unknown host 404s', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  const slug = uniq('e2e-dom', browserName);
  // Lowercase-only; the uniq() suffix is already [a-z0-9-], safe for the
  // custom_domain validator.
  const domain = `${uniq('e2e', browserName)}.example.test`;

  const created = await api(page, 'POST', '/v1/status-pages', {
    slug,
    title: `E2E Custom Domain ${browserName}`,
    custom_domain: domain,
  });
  expect(created?.id).toBeTruthy();
  expect(created.custom_domain).toBe(domain);

  try {
    // by-domain resolves to the same page (slug matches).
    const view = await api(page, 'GET', `/v1/public/status-pages/by-domain/${domain}`);
    expect(view.slug, 'by-domain payload slug matches the page').toBe(slug);
    expect(view.custom_domain).toBe(domain);

    // Unknown host -> 404.
    const unknown = await rawApi(page, 'GET',
      `/v1/public/status-pages/by-domain/nope-${uniq('x', browserName)}.example.test`);
    expect(unknown.status(), 'unknown custom domain should 404').toBe(404);
  } finally {
    await api(page, 'DELETE', `/v1/status-pages/${created.id}`).catch(() => {});
  }
});
