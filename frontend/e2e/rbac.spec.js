// E2E: RBAC enforcement across the three roles (admin / editor / readonly).
//
// Counterpart to the backend integration tests in
//   backend/crates/rampart-api/tests/rbac.rs
// but driven through the live HTTP surface so we cover the real middleware
// wiring (require_admin + require_write_or_readonly_get) end-to-end.
//
// Flow:
//   1. Admin (the seeded first-run user) provisions an editor + a readonly
//      user via POST /v1/users {email,password,role}. We capture their
//      plaintext credentials so we can open separate authenticated sessions.
//   2. A readonly session (its own cookie jar via loginAs) can GET monitors
//      but is 403'd on every mutation and on the admin-only /v1/users.
//   3. An editor session can write monitors (POST) but is still 403'd on the
//      admin-only /v1/users surface (GET + POST).
//   4. The admin PATCHes the editor down to readonly; we re-verify the now
//      readonly user is 403'd on a monitor write — proving the role change
//      takes effect for fresh requests.
//
// API shapes:
//   POST   /v1/users            users.rs::create  -> 201 { id, email, role, ... }
//   GET    /v1/users            users.rs::list    (require_admin)
//   PATCH  /v1/users/{id}/role  users.rs::set_role {role} -> 204
//   DELETE /v1/users/{id}       users.rs::remove  -> 204
//   POST   /v1/monitors         monitors.rs::create (require_write_or_readonly_get)
//   DELETE /v1/monitors/{id}    monitors.rs::delete_one
//
// Cross-browser projects share one DB, so emails are uniq()-suffixed and all
// created users are removed in a finally so a mid-spec panic still cleans up.

import { test, expect } from '@playwright/test';
import { api, rawApi, ensureLoggedIn, loginAs, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

const PASSWORD = 'rbac-correct-horse-battery';

function newMonitorBody(browserName) {
  return {
    name: uniq('rbac-mon', browserName),
    kind: 'http',
    url: 'https://example.com',
    interval_seconds: 60,
  };
}

test('rbac: admin provisions roled users; readonly/editor gating + role flip', async ({ page, browserName, baseURL }) => {
  await ensureLoggedIn(page);

  // 1. Admin creates an editor + a readonly user. Capture credentials.
  const editorEmail   = `${uniq('rbac-editor', browserName)}@example.com`;
  const readonlyEmail = `${uniq('rbac-readonly', browserName)}@example.com`;

  const editor = await api(page, 'POST', '/v1/users', {
    email: editorEmail,
    password: PASSWORD,
    role: 'editor',
  });
  expect(editor?.id).toBeTruthy();
  expect(editor.role).toBe('editor');

  const readonly = await api(page, 'POST', '/v1/users', {
    email: readonlyEmail,
    password: PASSWORD,
    role: 'readonly',
  });
  expect(readonly?.id).toBeTruthy();
  expect(readonly.role).toBe('readonly');

  // Sessions opened per-role; disposed in finally. Tracked here so cleanup
  // can run even if a session-open or assertion throws mid-test.
  let roCtx = null;
  let edCtx = null;
  // A monitor the readonly user will attempt (and fail) to DELETE. Created by
  // the admin so the 403 isn't a "not found" masquerading as success.
  let adminMonitorId = null;

  try {
    // Seed one monitor as admin so readonly's DELETE targets a real row.
    const seeded = await api(page, 'POST', '/v1/monitors', newMonitorBody(browserName));
    adminMonitorId = seeded.id;
    expect(adminMonitorId).toBeTruthy();

    // 2. Readonly session: GET ok, every mutation + admin-only route 403.
    roCtx = await loginAs(readonlyEmail, PASSWORD, baseURL);

    const roGet = await rawApi(roCtx, 'GET', '/v1/monitors');
    expect(roGet.status(), 'readonly GET /v1/monitors').toBe(200);

    const roPost = await rawApi(roCtx, 'POST', '/v1/monitors', newMonitorBody(browserName));
    expect(roPost.status(), 'readonly POST /v1/monitors should 403').toBe(403);

    const roDelete = await rawApi(roCtx, 'DELETE', `/v1/monitors/${adminMonitorId}`);
    expect(roDelete.status(), 'readonly DELETE monitor should 403').toBe(403);

    const roUsers = await rawApi(roCtx, 'GET', '/v1/users');
    expect(roUsers.status(), 'readonly GET /v1/users (admin-only) should 403').toBe(403);

    // The admin's monitor must still exist — readonly's DELETE was rejected.
    const stillThere = await api(page, 'GET', `/v1/monitors/${adminMonitorId}`);
    expect(stillThere?.id).toBe(adminMonitorId);

    // 3. Editor session: can write monitors, blocked on admin-only /v1/users.
    edCtx = await loginAs(editorEmail, PASSWORD, baseURL);

    const edPost = await rawApi(edCtx, 'POST', '/v1/monitors', newMonitorBody(browserName));
    expect([200, 201], 'editor POST /v1/monitors should succeed')
      .toContain(edPost.status());
    // Track the editor's own monitor for cleanup.
    const editorMonitor = await edPost.json();
    const editorMonitorId = editorMonitor.id;

    const edUsersGet = await rawApi(edCtx, 'GET', '/v1/users');
    expect(edUsersGet.status(), 'editor GET /v1/users (admin-only) should 403').toBe(403);

    const edUsersPost = await rawApi(edCtx, 'POST', '/v1/users', {
      email: `${uniq('rbac-editor-spawn', browserName)}@example.com`,
      password: PASSWORD,
      role: 'editor',
    });
    expect(edUsersPost.status(), 'editor POST /v1/users (admin-only) should 403').toBe(403);

    // Remove the editor's monitor now (admin has the rights to clean up).
    if (editorMonitorId) {
      await api(page, 'DELETE', `/v1/monitors/${editorMonitorId}`).catch(() => {});
    }

    // 4. Admin flips the editor down to readonly, then re-verify a write 403s.
    await api(page, 'PATCH', `/v1/users/${editor.id}/role`, { role: 'readonly' });

    // The set_role endpoint returns 204; confirm via the admin list that the
    // role actually changed.
    const usersList = await api(page, 'GET', '/v1/users');
    const flipped = usersList.find(u => u.id === editor.id);
    expect(flipped?.role, 'editor should now be readonly').toBe('readonly');

    // Re-login (fresh session reflects the new role) and confirm the write
    // is now forbidden.
    await edCtx.dispose();
    edCtx = await loginAs(editorEmail, PASSWORD, baseURL);
    const afterFlipPost = await rawApi(edCtx, 'POST', '/v1/monitors', newMonitorBody(browserName));
    expect(afterFlipPost.status(), 'now-readonly user POST /v1/monitors should 403').toBe(403);
  } finally {
    // Tear down sessions + every row we created. Best-effort; the shared
    // cross-browser DB punishes drift.
    if (roCtx) await roCtx.dispose().catch(() => {});
    if (edCtx) await edCtx.dispose().catch(() => {});
    if (adminMonitorId) {
      await api(page, 'DELETE', `/v1/monitors/${adminMonitorId}`).catch(() => {});
    }
    if (editor?.id) {
      await api(page, 'DELETE', `/v1/users/${editor.id}`).catch(() => {});
    }
    if (readonly?.id) {
      await api(page, 'DELETE', `/v1/users/${readonly.id}`).catch(() => {});
    }
  }
});
