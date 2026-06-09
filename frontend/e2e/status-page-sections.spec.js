// E2E: status-page component sections (component grouping).
//
// Drives the section CRUD + monitor-assignment routes in
//   backend/crates/rampart-api/src/routes/status_pages.rs
//   POST   /v1/status-pages/{id}/sections                       {name} -> 201
//   PUT    /v1/status-pages/{id}/monitors/{monitor_id}/section  {section_id} -> 204
//   DELETE /v1/status-pages/{id}/sections/{section_id}          -> 204
// and asserts the public projection in PublicStatusPage.sections, which
// buckets each attached monitor under its section header (a synthetic
// leading entry with name == null carries the ungrouped monitors).
//
// Flow:
//   1. Create 2 monitors + a page with both attached.
//   2. Create 2 sections; assign one monitor to each.
//   3. Public view -> data.sections groups each monitor under its section
//      name (no ungrouped bucket once both are assigned).
//   4. Re-assign a monitor's section to null -> it returns to ungrouped.
//   5. DELETE a section -> its monitors fall back to ungrouped (the FK uses
//      ON DELETE SET NULL, so the monitor is NOT detached from the page).
//
// section_id assignment uses rampart_core::AssignSectionReq { section_id }.
// Public section monitors are keyed by monitor `name`.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

function monitorBody(browserName, tag) {
  return {
    name: uniq(`e2e-sec-mon-${tag}`, browserName),
    kind: 'http',
    url: 'https://example.com',
    interval_seconds: 60,
  };
}

// Find the public section bucket that holds a monitor with the given name.
function sectionHolding(view, monName) {
  return (view.sections || []).find(s => (s.monitors || []).some(m => m.name === monName));
}

test('status-page sections: group monitors, unassign to ungrouped, delete falls back', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  let mon1 = null, mon2 = null, sp = null;
  try {
    mon1 = await api(page, 'POST', '/v1/monitors', monitorBody(browserName, 'a'));
    mon2 = await api(page, 'POST', '/v1/monitors', monitorBody(browserName, 'b'));
    expect(mon1?.id && mon2?.id).toBeTruthy();

    const slug = uniq('e2e-sections', browserName);
    sp = await api(page, 'POST', '/v1/status-pages', {
      slug,
      title: `E2E Sections ${browserName}`,
      monitor_ids: [mon1.id, mon2.id],
    });
    expect(sp?.id).toBeTruthy();

    // Two labelled sections.
    const secAName = uniq('API', browserName);
    const secBName = uniq('DB', browserName);
    const secA = await api(page, 'POST', `/v1/status-pages/${sp.id}/sections`, { name: secAName });
    const secB = await api(page, 'POST', `/v1/status-pages/${sp.id}/sections`, { name: secBName });
    expect(secA?.id && secB?.id).toBeTruthy();

    // Assign each monitor to a section.
    expect(await api(page, 'PUT', `/v1/status-pages/${sp.id}/monitors/${mon1.id}/section`,
      { section_id: secA.id }), 'assign mon1 -> A returns 204').toBeNull();
    expect(await api(page, 'PUT', `/v1/status-pages/${sp.id}/monitors/${mon2.id}/section`,
      { section_id: secB.id }), 'assign mon2 -> B returns 204').toBeNull();

    // Public view groups each monitor under its section name.
    const view1 = await api(page, 'GET', `/v1/public/status-pages/${slug}`);
    const holdsM1 = sectionHolding(view1, mon1.name);
    const holdsM2 = sectionHolding(view1, mon2.name);
    expect(holdsM1?.name, 'mon1 grouped under section A').toBe(secAName);
    expect(holdsM2?.name, 'mon2 grouped under section B').toBe(secBName);
    // Both assigned => no synthetic ungrouped (name == null) bucket present.
    expect((view1.sections || []).find(s => s.name === null), 'no ungrouped bucket when all assigned')
      .toBeFalsy();

    // Unassign mon1 (section_id: null) -> returns to ungrouped.
    expect(await api(page, 'PUT', `/v1/status-pages/${sp.id}/monitors/${mon1.id}/section`,
      { section_id: null }), 'unassign mon1 returns 204').toBeNull();
    const view2 = await api(page, 'GET', `/v1/public/status-pages/${slug}`);
    const m1After = sectionHolding(view2, mon1.name);
    expect(m1After?.name, 'mon1 now in ungrouped (name == null)').toBeNull();
    // mon2 still under B.
    expect(sectionHolding(view2, mon2.name)?.name, 'mon2 still under B').toBe(secBName);

    // Delete section B -> mon2 falls back to ungrouped, still on the page.
    expect(await api(page, 'DELETE', `/v1/status-pages/${sp.id}/sections/${secB.id}`),
      'delete section B returns 204').toBeNull();
    const view3 = await api(page, 'GET', `/v1/public/status-pages/${slug}`);
    const m2After = sectionHolding(view3, mon2.name);
    expect(m2After, 'mon2 still present on page after section delete').toBeTruthy();
    expect(m2After?.name, 'mon2 fell back to ungrouped (not detached)').toBeNull();
    // Section B label is gone from the grouping.
    expect((view3.sections || []).find(s => s.name === secBName), 'section B label gone').toBeFalsy();
    // Section A still exists.
    expect((view3.sections || []).find(s => s.name === secAName), 'section A still present').toBeTruthy();
  } finally {
    if (sp?.id) await api(page, 'DELETE', `/v1/status-pages/${sp.id}`).catch(() => {});
    if (mon1?.id) await api(page, 'DELETE', `/v1/monitors/${mon1.id}`).catch(() => {});
    if (mon2?.id) await api(page, 'DELETE', `/v1/monitors/${mon2.id}`).catch(() => {});
  }
});
