// E2E: inbound alert ingestion across all five webhook receivers.
//
// Drives the public, token-authed receivers in
//   backend/crates/rampart-api/src/routes/ingest.rs
//   POST /v1/public/ingest/{vendor}/{token}
// for Alertmanager, Grafana, Datadog, PagerDuty, and Opsgenie. Each
// vendor handler normalizes its payload into the shared `NormalizedAlert`
// shape and funnels through `apply_alert`: a firing payload opens an
// incident (created>=1), the matching resolved payload closes it
// (resolved>=1). Both return 202 with `{created, resolved}`.
//
// For every vendor the spec:
//   1. POSTs a firing payload  -> 202, created>=1
//   2. GETs the public page    -> the incident shows under `incidents`
//   3. POSTs the resolved pair -> 202, resolved>=1
//   4. re-GETs the public page -> the incident moved to `incident_history`
//
// Plus a 404 check on an unknown token (possession of a valid token IS
// the entire auth check — anything else is Not Found).
//
// Setup state (status page + ingest token) is created through the admin
// JSON API; the page is uniq()-suffixed and torn down in a finally so the
// shared cross-browser DB stays clean.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

// Per-vendor: the firing + resolved payloads and the unique alertname /
// title we assert surfaces on the public page. dedup keys are unique per
// vendor so the incidents don't collide on the page's (page, dedup_key)
// active index.
function vendorCases(browserName) {
  const tag = uniq('E2E', browserName); // short cross-browser-unique tag
  return [
    {
      vendor: 'alertmanager',
      title: `AM-${tag}`,
      firing: {
        status: 'firing',
        alerts: [{
          status: 'firing',
          labels: { alertname: `AM-${tag}`, severity: 'critical' },
          annotations: { summary: 'x', description: 'y' },
          fingerprint: `fp-am-${tag}`,
        }],
      },
      resolved: {
        status: 'resolved',
        alerts: [{
          status: 'resolved',
          labels: { alertname: `AM-${tag}`, severity: 'critical' },
          annotations: { summary: 'x', description: 'y' },
          fingerprint: `fp-am-${tag}`,
        }],
      },
    },
    {
      vendor: 'grafana',
      title: `GR-${tag}`,
      firing: {
        status: 'firing',
        alerts: [{
          status: 'firing',
          labels: { alertname: `GR-${tag}`, severity: 'critical' },
          annotations: { summary: 'x', description: 'y' },
          fingerprint: `fp-gr-${tag}`,
        }],
      },
      resolved: {
        status: 'resolved',
        alerts: [{
          status: 'resolved',
          labels: { alertname: `GR-${tag}`, severity: 'critical' },
          annotations: { summary: 'x', description: 'y' },
          fingerprint: `fp-gr-${tag}`,
        }],
      },
    },
    {
      vendor: 'datadog',
      title: `DD-${tag}`,
      firing: {
        alert_type: 'error',
        title: `DD-${tag}`,
        body: 'b',
        alert_id: `dd-${tag}`,
        alert_transition: 'Triggered',
      },
      resolved: {
        alert_type: 'error',
        title: `DD-${tag}`,
        body: 'b',
        alert_id: `dd-${tag}`,
        alert_transition: 'Recovered',
      },
    },
    {
      vendor: 'pagerduty',
      title: `PD-${tag}`,
      firing: {
        event: {
          event_type: 'incident.triggered',
          data: { id: `pd-${tag}`, title: `PD-${tag}`, status: 'triggered', urgency: 'high', html_url: 'https://x' },
        },
      },
      resolved: {
        event: {
          event_type: 'incident.resolved',
          data: { id: `pd-${tag}`, title: `PD-${tag}`, status: 'resolved', urgency: 'high', html_url: 'https://x' },
        },
      },
    },
    {
      vendor: 'opsgenie',
      title: `OG-${tag}`,
      firing: {
        action: 'Create',
        alert: { alertId: `og-${tag}`, message: `OG-${tag}`, description: 'd', priority: 'P1' },
      },
      resolved: {
        action: 'Close',
        alert: { alertId: `og-${tag}`, message: `OG-${tag}`, description: 'd', priority: 'P1' },
      },
    },
  ];
}

const JSON_HEADERS = { 'content-type': 'application/json' };

test('ingest: firing -> public incident -> resolved -> history, all five vendors', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  // 1. Create the status page + mint an ingest token that authorizes it.
  const slug = uniq('e2e-ingest', browserName);
  const sp = await api(page, 'POST', '/v1/status-pages', {
    slug,
    title: `E2E Ingest ${browserName}`,
  });
  expect(sp?.id).toBeTruthy();

  const created = sp;
  try {
    const tok = await api(page, 'POST', `/v1/status-pages/${created.id}/ingest-tokens`, {
      label: uniq('e2e-tok', browserName),
    });
    expect(tok?.token).toBeTruthy();
    const token = tok.token;

    const cases = vendorCases(browserName);

    for (const c of cases) {
      // --- firing -> 202 created>=1 ---
      // Hit the receiver directly through the page's request context so we
      // can set an explicit `content-type: application/json` (mirroring a
      // real webhook sender) rather than going through the JSON helpers.
      const fireRes = await page.request.post(
        `/v1/public/ingest/${c.vendor}/${token}`,
        { headers: JSON_HEADERS, data: c.firing });
      expect(fireRes.status(), `${c.vendor} firing status`).toBe(202);
      const fireBody = await fireRes.json();
      expect(fireBody.created, `${c.vendor} firing created`).toBeGreaterThanOrEqual(1);

      // --- public page shows the active incident ---
      const view1 = await api(page, 'GET', `/v1/public/status-pages/${slug}`);
      const active = (view1.incidents || []).find(i => i.title === c.title);
      expect(active, `${c.vendor} active incident on public page`).toBeTruthy();

      // --- resolved -> 202 resolved>=1 ---
      const resRes = await page.request.post(
        `/v1/public/ingest/${c.vendor}/${token}`,
        { headers: JSON_HEADERS, data: c.resolved });
      expect(resRes.status(), `${c.vendor} resolved status`).toBe(202);
      const resBody = await resRes.json();
      expect(resBody.resolved, `${c.vendor} resolved count`).toBeGreaterThanOrEqual(1);

      // --- incident moved out of active and into history ---
      const view2 = await api(page, 'GET', `/v1/public/status-pages/${slug}`);
      const stillActive = (view2.incidents || []).find(i => i.title === c.title);
      expect(stillActive, `${c.vendor} incident should no longer be active`).toBeFalsy();
      const inHistory = (view2.incident_history || []).find(i => i.title === c.title);
      expect(inHistory, `${c.vendor} incident should be in history`).toBeTruthy();
    }

    // --- 404 on an unknown token (possession of a valid token IS auth) ---
    const bad = await page.request.post(
      `/v1/public/ingest/alertmanager/not-a-real-token-${browserName}`,
      { headers: JSON_HEADERS, data: cases[0].firing });
    expect(bad.status(), 'unknown ingest token should 404').toBe(404);
  } finally {
    // Cascade should drop the ingest token + incidents with the page.
    await api(page, 'DELETE', `/v1/status-pages/${created.id}`).catch(() => {});
  }
});
