// E2E: generic JSON-path ingest receiver.
//
// Drives the operator-mapped public receiver in
//   backend/crates/rampart-api/src/routes/ingest.rs::generic
//   POST /v1/public/ingest/generic/{token}
// Unlike the named vendor receivers, this one has no fixed payload shape:
// the ingest token carries a stored `IngestMapping` of RFC 6901 JSON
// Pointers (action_path / title_path / content_path / dedup_path + the
// firing/resolved discriminator values). The handler pulls the normalized
// incident fields out of an arbitrary inbound body via those pointers and
// funnels through the same create-or-resolve core as every vendor.
//
// Flow:
//   1. Create a status page + mint an ingest token.
//   2. PATCH /v1/ingest-tokens/{id}/mapping to set the JSON-pointer mapping.
//   3. POST a custom body matching the mapping (firing) -> 202 created>=1;
//      GET the public page -> the incident is active.
//   4. POST the resolved variant -> 202 resolved>=1; incident leaves active
//      and lands in history.
//   5. Unknown token -> 404.
//   6. A *different* token with NO mapping -> 400.
//
// Mapping fields are read straight from rampart_core::ingest_token::
// IngestMapping; the receiver coerces scalars to text via pointer_str so a
// string discriminator works as-is.

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

const JSON_HEADERS = { 'content-type': 'application/json' };

test('generic webhook: mapped firing -> incident -> resolved; 404/400 guards', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  const slug = uniq('e2e-generic', browserName);
  const sp = await api(page, 'POST', '/v1/status-pages', {
    slug,
    title: `E2E Generic ${browserName}`,
  });
  expect(sp?.id).toBeTruthy();

  try {
    // Token WITH a mapping (the happy path).
    const tok = await api(page, 'POST', `/v1/status-pages/${sp.id}/ingest-tokens`, {
      label: uniq('e2e-generic-tok', browserName),
    });
    expect(tok?.token).toBeTruthy();
    expect(tok?.id).toBeTruthy();

    // A second token left mapping-less to prove the 400 path.
    const tokNoMap = await api(page, 'POST', `/v1/status-pages/${sp.id}/ingest-tokens`, {
      label: uniq('e2e-generic-nomap', browserName),
    });
    expect(tokNoMap?.token).toBeTruthy();

    // PATCH the mapping. JSON pointers index into the custom body below.
    const updated = await api(page, 'PATCH', `/v1/ingest-tokens/${tok.id}/mapping`, {
      mapping: {
        action_path: '/event/state',
        action_firing_value: 'open',
        action_resolved_value: 'closed',
        title_path: '/event/name',
        content_path: '/event/detail',
        dedup_path: '/event/key',
        style: 'danger',
      },
    });
    expect(updated?.mapping?.action_path).toBe('/event/state');

    const title = `GEN-${uniq('inc', browserName)}`;
    const dedup = `gen-key-${uniq('k', browserName)}`;

    // --- firing -> 202 created>=1 ---
    const fire = await page.request.post(`/v1/public/ingest/generic/${tok.token}`, {
      headers: JSON_HEADERS,
      data: { event: { state: 'open', name: title, detail: 'something broke', key: dedup } },
    });
    expect(fire.status(), 'generic firing status').toBe(202);
    expect((await fire.json()).created, 'generic firing created').toBeGreaterThanOrEqual(1);

    // --- public page shows the active incident ---
    const view1 = await api(page, 'GET', `/v1/public/status-pages/${slug}`);
    const active = (view1.incidents || []).find(i => i.title === title);
    expect(active, 'generic active incident on public page').toBeTruthy();

    // --- resolved -> 202 resolved>=1 ---
    const res = await page.request.post(`/v1/public/ingest/generic/${tok.token}`, {
      headers: JSON_HEADERS,
      data: { event: { state: 'closed', name: title, detail: 'fixed', key: dedup } },
    });
    expect(res.status(), 'generic resolved status').toBe(202);
    expect((await res.json()).resolved, 'generic resolved count').toBeGreaterThanOrEqual(1);

    // --- incident left active, now in history ---
    const view2 = await api(page, 'GET', `/v1/public/status-pages/${slug}`);
    expect((view2.incidents || []).find(i => i.title === title), 'no longer active').toBeFalsy();
    expect((view2.incident_history || []).find(i => i.title === title), 'now in history').toBeTruthy();

    // --- unknown token -> 404 (possession of a valid token IS the auth) ---
    const unknown = await page.request.post(
      `/v1/public/ingest/generic/not-a-real-token-${browserName}`,
      { headers: JSON_HEADERS, data: { event: { state: 'open', name: 'x', key: 'y' } } });
    expect(unknown.status(), 'unknown generic token -> 404').toBe(404);

    // --- valid token with NO mapping -> 400 ---
    const noMap = await page.request.post(
      `/v1/public/ingest/generic/${tokNoMap.token}`,
      { headers: JSON_HEADERS, data: { event: { state: 'open', name: 'x', key: 'y' } } });
    expect(noMap.status(), 'mapping-less generic token -> 400').toBe(400);
  } finally {
    // Cascade drops both ingest tokens + the incident with the page.
    await api(page, 'DELETE', `/v1/status-pages/${sp.id}`).catch(() => {});
  }
});
