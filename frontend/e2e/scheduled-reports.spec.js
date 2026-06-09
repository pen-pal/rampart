// E2E: scheduled-report CRUD round-trip.
//
// Drives the admin-gated routes in
//   backend/crates/rampart-api/src/routes/scheduled_reports.rs
//   POST   /v1/scheduled-reports        {name, recipients, cadence} -> 201
//   GET    /v1/scheduled-reports        -> list
//   PATCH  /v1/scheduled-reports/{id}    -> updated row
//   DELETE /v1/scheduled-reports/{id}    -> 204
// A report is a named set of recipients + cadence; the scheduler renders +
// sends the weekly digest on its own slow tick. This spec only exercises
// the CRUD surface — it never triggers an actual send.
//
// Shapes from rampart_db::scheduled_reports: NewScheduledReport
// {name, recipients[], cadence} and UpdateScheduledReport (all optional).
// The row carries last_sent_at (null until first send) + created_at.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('scheduled-reports: create -> list -> patch -> delete', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  const name = uniq('e2e-report', browserName);
  let id = null;
  try {
    const created = await api(page, 'POST', '/v1/scheduled-reports', {
      name,
      recipients: ['ops@example.com'],
      cadence: 'weekly',
    });
    expect(created?.id).toBeTruthy();
    expect(created.name).toBe(name);
    expect(created.recipients).toEqual(['ops@example.com']);
    expect(created.cadence).toBe('weekly');
    expect(created.last_sent_at, 'never sent yet').toBeFalsy();
    id = created.id;

    const list1 = await api(page, 'GET', '/v1/scheduled-reports');
    expect(list1.find(r => r.id === id), 'report in list').toBeTruthy();

    const patched = await api(page, 'PATCH', `/v1/scheduled-reports/${id}`, {
      recipients: ['ops@example.com', 'oncall@example.com'],
    });
    expect(patched.recipients, 'recipients updated').toEqual(['ops@example.com', 'oncall@example.com']);
    expect(patched.name, 'name unchanged by partial patch').toBe(name);

    expect(await api(page, 'DELETE', `/v1/scheduled-reports/${id}`), 'delete -> 204').toBeNull();
    const cleared = id; id = null;
    const list2 = await api(page, 'GET', '/v1/scheduled-reports');
    expect(list2.find(r => r.id === cleared), 'report gone after delete').toBeFalsy();
  } finally {
    if (id) await api(page, 'DELETE', `/v1/scheduled-reports/${id}`).catch(() => {});
  }
});
