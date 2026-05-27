// Best-effort teardown: drop the test DB so the next run starts fresh
// even if global-setup is skipped (e.g. webServer reuse). Not fatal if
// it can't reach Postgres — the next setup run will handle cleanup.

import { execSync } from 'node:child_process';

const PG_CONTAINER = process.env.RAMPART_PG_CONTAINER || 'backend-postgres-1';

export default async function globalTeardown() {
  try {
    execSync(`docker exec ${PG_CONTAINER} psql -U rampart -d postgres -c "DROP DATABASE IF EXISTS rampart_test;"`,
      { stdio: 'ignore' });
  } catch { /* tolerate */ }
}
