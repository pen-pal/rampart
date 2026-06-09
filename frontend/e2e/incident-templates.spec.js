// E2E: incident-template CRUD round-trip.
//
// Drives the global incident-update template library in
//   backend/crates/rampart-api/src/routes/incident_templates.rs
//   POST   /v1/incident-templates           -> 201 { id, name, body, style, created_at }
//   GET    /v1/incident-templates           -> list
//   PATCH  /v1/incident-templates/{id}       -> updated row
//   DELETE /v1/incident-templates/{id}       -> 204
// Templates are global (not page-scoped). `style` reuses the lowercase
// IncidentStyle enum (info|warning|danger|primary|success). RBAC/no-auth
// gating is covered by rbac.spec.js — this spec is the CRUD round-trip.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('incident-templates: create -> list -> patch -> delete round-trip', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  const name = uniq('e2e-tmpl', browserName);
  let id = null;
  try {
    // --- create -> 201 ---
    const created = await api(page, 'POST', '/v1/incident-templates', {
      name,
      body: 'We are investigating an issue.',
      style: 'warning',
    });
    expect(created?.id).toBeTruthy();
    expect(created.name).toBe(name);
    expect(created.style).toBe('warning');
    id = created.id;

    // --- list contains it ---
    const list1 = await api(page, 'GET', '/v1/incident-templates');
    expect(list1.find(t => t.id === id), 'template appears in list').toBeTruthy();

    // --- patch updates body + style ---
    const patched = await api(page, 'PATCH', `/v1/incident-templates/${id}`, {
      body: 'Identified the root cause.',
      style: 'danger',
    });
    expect(patched.body).toBe('Identified the root cause.');
    expect(patched.style).toBe('danger');
    expect(patched.name, 'name unchanged by partial patch').toBe(name);

    // --- delete removes it ---
    const del = await api(page, 'DELETE', `/v1/incident-templates/${id}`);
    expect(del, 'delete returns 204 / null body').toBeNull();
    const cleared = id; id = null;

    const list2 = await api(page, 'GET', '/v1/incident-templates');
    expect(list2.find(t => t.id === cleared), 'template gone after delete').toBeFalsy();
  } finally {
    if (id) await api(page, 'DELETE', `/v1/incident-templates/${id}`).catch(() => {});
  }
});
