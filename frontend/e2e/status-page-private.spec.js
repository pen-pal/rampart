// E2E: password-protected (private) status pages.
//
// Backed by migration 0051 (status_pages.password_hash) and the public
// `/unlock` route in
//   backend/crates/rampart-api/src/routes/status_pages.rs
//
// Contract:
//   - PATCH /v1/status-pages/{id} accepts a write-only `password` field.
//     Some(string) sets it (page becomes private); null clears it (public).
//   - The public projection NEVER serializes the hash; it exposes a derived
//     read-only `private` boolean.
//   - GET /v1/public/status-pages/{slug} on a private page returns a LOCKED
//     STUB: { private: true, slug, title } with empty monitors / incidents.
//   - POST /v1/public/status-pages/{slug}/unlock { password } -> 401 on a
//     wrong password, 200 + the FULL public payload on the right one.
//   - PATCH password:null flips the page back to public; the plain GET then
//     returns the full payload with no unlock.
//
// The unauthenticated checks run through a FRESH APIRequestContext with no
// session cookies (the admin `page` is authenticated). Raw JSON is posted to
// the public endpoints with an explicit content-type. The page is uniq()-
// suffixed and removed in a finally.

import { test, expect, request as pwRequest } from './fixtures.js';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

const JSON_HEADERS = { 'content-type': 'application/json' };
const PAGE_PASSWORD = 'open-sesame-correct-horse';

test('private status page: locked stub for the public, unlock gates the full payload, back-to-public removes the gate', async ({ page, browserName, baseURL }) => {
  await ensureLoggedIn(page);

  const slug = uniq('e2e-private', browserName);
  let pageId = null;
  let monitorId = null;
  let anon = null; // unauthenticated APIRequestContext (no session)
  try {
    // Create a public page with one monitor attached so "monitors leaked"
    // is a meaningful assertion when locked.
    const monitor = await api(page, 'POST', '/v1/monitors', {
      name: uniq('e2e-private-mon', browserName),
      kind: 'http',
      url: 'https://example.com',
      interval_seconds: 60,
    });
    monitorId = monitor.id;
    const sp = await api(page, 'POST', '/v1/status-pages', {
      slug,
      title: `E2E Private ${browserName}`,
      monitor_ids: [monitor.id],
    });
    pageId = sp.id;
    expect(pageId).toBeTruthy();
    expect(sp.private, 'fresh page is public').toBe(false);

    // Flip private by setting a password.
    const patched = await api(page, 'PATCH', `/v1/status-pages/${pageId}`, {
      password: PAGE_PASSWORD,
    });
    expect(patched.private, 'page is now private').toBe(true);
    // Hash must never be serialized.
    expect(patched.password_hash, 'password_hash never serialized').toBeUndefined();

    // Fresh unauthenticated context — public endpoints don't need a session.
    anon = await pwRequest.newContext({ baseURL });

    // Plain public GET returns the locked stub: no monitors / incidents.
    const lockedRes = await anon.get(`/v1/public/status-pages/${slug}`);
    expect(lockedRes.status(), 'public GET of private page is 200 (stub)').toBe(200);
    const locked = await lockedRes.json();
    expect(locked.private, 'stub flagged private').toBe(true);
    expect(locked.slug).toBe(slug);
    expect(locked.monitors?.length ?? 0, 'no monitors leaked in stub').toBe(0);
    expect(locked.incidents?.length ?? 0, 'no incidents leaked in stub').toBe(0);

    // Unlock with the WRONG password -> 401.
    const wrong = await anon.post(`/v1/public/status-pages/${slug}/unlock`, {
      headers: JSON_HEADERS,
      data: { password: 'definitely-not-it' },
    });
    expect(wrong.status(), 'wrong password 401').toBe(401);

    // Unlock with the RIGHT password -> 200 + full payload.
    const right = await anon.post(`/v1/public/status-pages/${slug}/unlock`, {
      headers: JSON_HEADERS,
      data: { password: PAGE_PASSWORD },
    });
    expect(right.status(), 'correct password 200').toBe(200);
    const full = await right.json();
    expect(full.private, 'unlocked payload still reports private:true').toBe(true);
    expect(full.monitors?.length ?? 0, 'unlocked payload carries the monitor').toBeGreaterThanOrEqual(1);

    // Flip back to public by clearing the password.
    //
    // NOTE on the actual backend shape: UpdateStatusPage.password is
    // `Option<Option<String>>` but is declared with a plain `#[serde(default)]`
    // and NO `double_option` deserializer (unlike UpdateNotification.template_id).
    // With serde's default behaviour a JSON `null` deserializes to the OUTER
    // `None` — i.e. "field omitted", a NO-OP — so `password: null` does NOT
    // clear the hash. The db `update` treats an empty string as a clear
    // (`Some(pw) if !pw.is_empty() => hash, _ => None`), so we send "" to make
    // the page public again. (The comments in the source describe Some(None)
    // clear semantics that the missing double_option deserializer never
    // actually produces over the wire.)
    const reopened = await api(page, 'PATCH', `/v1/status-pages/${pageId}`, {
      password: '',
    });
    expect(reopened.private, 'page is public again').toBe(false);

    // Plain public GET now returns the full payload with no unlock.
    const publicRes = await anon.get(`/v1/public/status-pages/${slug}`);
    expect(publicRes.status()).toBe(200);
    const pub = await publicRes.json();
    expect(pub.private, 'public page reports private:false').toBe(false);
    expect(pub.monitors?.length ?? 0, 'public payload carries the monitor').toBeGreaterThanOrEqual(1);
  } finally {
    if (anon) await anon.dispose().catch(() => {});
    if (pageId) await api(page, 'DELETE', `/v1/status-pages/${pageId}`).catch(() => {});
    // The page→monitor edge cascades with the page, but the monitor row
    // itself doesn't; delete it explicitly. Best-effort.
    if (monitorId) await api(page, 'DELETE', `/v1/monitors/${monitorId}`).catch(() => {});
  }
});
