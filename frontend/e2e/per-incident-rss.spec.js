// E2E: per-incident RSS/Atom feeds.
//
// Drives the public per-incident feed endpoints in
//   backend/crates/rampart-api/src/routes/status_pages.rs
//   GET /v1/public/status-pages/{slug}/incidents/{incident_id}/feed.atom
//   GET /v1/public/status-pages/{slug}/incidents/{incident_id}/feed.rss
// Where the page-level feed emits one entry per incident, these scope to a
// single incident's update thread. `resolve_incident` validates that the
// incident belongs to the given slug — a mismatched / bogus id 404s.
//
// Flow:
//   1. Create a status page + an incident (POST the page-scoped incident
//      create route).
//   2. GET the per-incident feed.atom -> 200 + atom content-type.
//   3. GET the per-incident feed.rss  -> 200 + rss content-type.
//   4. A bogus incident id on that slug -> 404.
//
// Feeds are public/unauthed, so they go through a fresh APIRequestContext.

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, uniq } from './helpers.js';
import { request as pwRequest } from './fixtures.js';

test.describe.configure({ mode: 'serial' });

test('per-incident feeds: atom + rss 200, bogus incident 404', async ({ page, browserName, baseURL }) => {
  await ensureLoggedIn(page);

  const slug = uniq('e2e-feed', browserName);
  const sp = await api(page, 'POST', '/v1/status-pages', {
    slug,
    title: `E2E Feed ${browserName}`,
  });
  expect(sp?.id).toBeTruthy();

  let pub = null;
  try {
    // Create an incident on the page (page-scoped create route).
    const inc = await api(page, 'POST', `/v1/status-pages/${sp.id}/incidents`, {
      title: `FEED-${uniq('inc', browserName)}`,
      content: 'investigating an issue',
      style: 'danger',
    });
    expect(inc?.id).toBeTruthy();

    // Public, unauthed context for the feed reads.
    pub = await pwRequest.newContext({ baseURL });

    const atom = await pub.get(`/v1/public/status-pages/${slug}/incidents/${inc.id}/feed.atom`);
    expect(atom.status(), 'per-incident feed.atom status').toBe(200);
    expect(atom.headers()['content-type'] || '', 'atom content-type').toContain('atom+xml');
    expect(await atom.text(), 'atom body is a feed').toContain('<feed');

    const rss = await pub.get(`/v1/public/status-pages/${slug}/incidents/${inc.id}/feed.rss`);
    expect(rss.status(), 'per-incident feed.rss status').toBe(200);
    expect(rss.headers()['content-type'] || '', 'rss content-type').toContain('rss+xml');
    expect(await rss.text(), 'rss body is a feed').toContain('<rss');

    // Bogus incident id on a real slug -> 404.
    const bogus = '00000000-0000-7000-8000-000000000000';
    const missing = await pub.get(`/v1/public/status-pages/${slug}/incidents/${bogus}/feed.atom`);
    expect(missing.status(), 'bogus incident id -> 404').toBe(404);
  } finally {
    if (pub) await pub.dispose().catch(() => {});
    await api(page, 'DELETE', `/v1/status-pages/${sp.id}`).catch(() => {});
  }
});
