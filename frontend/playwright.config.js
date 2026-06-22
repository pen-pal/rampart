// Playwright e2e config.
//
// Stack under test:
//   - Postgres at localhost:5432 (docker compose service)
//   - rampart-api on a dedicated test DB (`rampart_test`), bound to port
//     3001 so it doesn't fight the dev :3000 process
//   - The React bundle is embedded by the API binary (rust-embed reads
//     from frontend/dist in debug builds → tests need `npm run build`
//     to have run; the CI workflow handles that)
//
// Run locally:    npx playwright test
// Run a single:   npx playwright test e2e/auth.spec.js
// Open the UI:    npx playwright test --ui

import { defineConfig, devices } from '@playwright/test';

const TEST_DB_URL  = 'postgres://rampart:rampart@localhost:5432/rampart_test';
const TEST_API_URL = 'http://localhost:3001';

export default defineConfig({
  testDir:    './e2e',
  // The screenshot generator mutates files on disk under docs/assets/.
  // It runs only when invoked explicitly via `npm run screenshots`, never
  // as part of the regular CI matrix.
  testIgnore: process.env.SCREENSHOTS_RUN ? [] : ['**/screenshots.spec.js'],
  fullyParallel: false,    // tests share one server + DB; serialise for sanity
  forbidOnly: !!process.env.CI,
  retries:    process.env.CI ? 1 : 0,
  workers:    1,           // ditto
  reporter:   process.env.CI ? [['github'], ['list']] : 'list',
  timeout:    30_000,
  expect:     { timeout: 5_000 },

  use: {
    baseURL:    TEST_API_URL,
    trace:      'retain-on-failure',
    screenshot: 'only-on-failure',
  },

  // Drops the test DB on suite exit so reruns start clean.
  globalTeardown: './e2e/global-teardown.js',

  // Brings up rampart-api against a freshly-recreated test DB. The
  // start-api.sh wrapper drops/recreates the DB + applies migrations
  // before exec'ing the binary, so we don't race a parallel globalSetup
  // against webServer startup.
  webServer: {
    command: './e2e/start-api.sh',
    url:     `${TEST_API_URL}/healthz`,
    timeout: 120_000,
    reuseExistingServer: !process.env.CI,
    env: {
      DATABASE_URL:       TEST_DB_URL,
      DATABASE_POOL_SIZE: '8',
      BIND_ADDR:          '0.0.0.0:3001',
      RUST_LOG:           'rampart=warn,tower_http=warn',
      SQLX_OFFLINE:       'true',
      // Trust the loopback test client so the server honours the per-context
      // X-Forwarded-For that fixtures.js stamps — gives each test its own
      // auth rate-limit bucket. Production keys on the real TCP peer; this is
      // test-env only.
      RAMPART_TRUSTED_PROXIES: '127.0.0.1,::1',
    },
  },

  projects: [
    // Engines bundled with Playwright — installed via `npx playwright
    // install`. Cover the three real rendering engines (Blink / Gecko /
    // WebKit) so Safari + Firefox-only regressions surface in CI, not
    // in production from a user's iPhone.
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox',  use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit',   use: { ...devices['Desktop Safari'] } },

    // Branded channels — same engines, real installer build. Skipped
    // unless the binary is present on the runner; CI installs them via
    // `npx playwright install msedge chrome`. Locally these no-op if
    // you don't have Edge / Chrome installed.
    //
    // Brave / Vivaldi / LibreWolf / Arc aren't bundled with Playwright,
    // but Brave + Vivaldi + Arc are Chromium forks (covered by
    // `chromium` + the `chrome` channel) and LibreWolf is a Firefox
    // fork (covered by `firefox`). Adding them as separate projects
    // would only catch repackaging differences, which a uptime
    // monitor's web UI doesn't exercise.
    {
      name: 'chrome',
      use: { ...devices['Desktop Chrome'], channel: 'chrome' },
    },
    {
      name: 'msedge',
      use: { ...devices['Desktop Edge'], channel: 'msedge' },
    },
  ],
});
