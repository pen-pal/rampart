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
    },
  },

  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    // Firefox + WebKit kept for CI cross-browser runs; locally `chromium`
    // is the default and others can be opted into with --project=firefox.
    { name: 'firefox',  use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit',   use: { ...devices['Desktop Safari'] } },
  ],
});
