// E2E: in-app CSV monitor import.
//
// Drives POST /v1/monitors/import-csv (monitors.rs::import_csv), which
// takes the raw request body as `text/csv`, parses it via
//   backend/crates/rampart-api/src/importers/csv_import.rs::parse_csv_and_map
// and creates one monitor per well-formed row. Rows whose kind is unknown
// (or that are missing a field their kind requires) come back in `skipped`
// rather than aborting the batch. The response is `{created, skipped:[…]}`.
//
// The CSV here has a header + three rows:
//   - an `http` monitor (needs `url`)         -> created
//   - a `tcp`  monitor (needs hostname+port)  -> created
//   - a bad `nonsense`-kind row               -> skipped (unknown kind)
// so we expect `{created: 2, skipped: [1 entry]}`.
//
// Names are uniq()-suffixed; the two created monitors are confirmed via
// GET /v1/monitors and torn down in a finally to keep the shared
// cross-browser DB clean.

import { test, expect } from './fixtures.js';
import { api, ensureLoggedIn, gotoView, uniq } from './helpers.js';

test.describe.configure({ mode: 'serial' });

test('csv import: 3 rows -> 2 created + 1 skipped, created monitors land', async ({ page, browserName }) => {
  await ensureLoggedIn(page);

  const httpName = uniq('e2e-csv-http', browserName);
  const tcpName  = uniq('e2e-csv-tcp', browserName);
  const badName  = uniq('e2e-csv-bad', browserName);

  // Recognised columns (see csv_import.rs): name,kind,url,hostname,port,
  // interval_seconds,timeout_seconds. tcp requires hostname + port.
  const csv = [
    'name,kind,url,hostname,port,interval_seconds,timeout_seconds',
    `${httpName},http,https://example.com,,,60,16`,
    `${tcpName},tcp,,example.com,443,60,16`,
    `${badName},nonsense,,,,60,16`,
  ].join('\n');

  const createdIds = [];
  try {
    const res = await page.request.post('/v1/monitors/import-csv', {
      headers: { 'content-type': 'text/csv' },
      data: csv,
    });
    expect(res.status(), 'import-csv status').toBe(200);
    const body = await res.json();
    expect(body.created, 'two rows created').toBe(2);
    expect(Array.isArray(body.skipped)).toBe(true);
    expect(body.skipped.length, 'one row skipped').toBe(1);
    // The skipped row is the bad-kind one.
    expect(body.skipped[0].row).toBe(badName);

    // Confirm the two created monitors exist via the list endpoint.
    const monitors = await api(page, 'GET', '/v1/monitors');
    const httpMon = monitors.find(m => m.name === httpName);
    const tcpMon  = monitors.find(m => m.name === tcpName);
    const badMon  = monitors.find(m => m.name === badName);
    expect(httpMon, 'http monitor created').toBeTruthy();
    expect(tcpMon, 'tcp monitor created').toBeTruthy();
    expect(badMon, 'bad-kind row must NOT have created a monitor').toBeFalsy();
    createdIds.push(httpMon.id, tcpMon.id);

    // The ImportMonitors view renders a file-picker + paste textarea.
    await gotoView(page, '#/import');
    await expect(page.getByRole('heading', { level: 1 }).first())
      .toBeVisible({ timeout: 10_000 });
    // Hidden <input type=file accept=".csv,text/csv,text/plain"> + the
    // paste textarea both render once the writable form is shown.
    await expect(page.locator('input[type="file"]')).toBeAttached();
    await expect(page.locator('textarea.textarea')).toBeVisible();
  } finally {
    for (const id of createdIds) {
      await api(page, 'DELETE', `/v1/monitors/${id}`).catch(() => {});
    }
  }
});
