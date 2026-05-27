#!/usr/bin/env bash
# Runs as Playwright's `webServer.command`. Recreates the test DB,
# applies migrations, then `exec`s the rampart-api binary so it takes
# over PID 1 of the wrapper (Playwright kills it via SIGTERM on exit).

set -euo pipefail

PG_CONTAINER="${RAMPART_PG_CONTAINER:-backend-postgres-1}"
DB_NAME="${RAMPART_TEST_DB:-rampart_test}"
TEST_URL="${DATABASE_URL:-postgres://rampart:rampart@localhost:5432/${DB_NAME}}"

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BACKEND="${REPO_ROOT}/backend"

echo "[start-api] dropping + recreating ${DB_NAME}"
docker exec "$PG_CONTAINER" psql -U rampart -d postgres -c \
  "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '${DB_NAME}';" \
  >/dev/null 2>&1 || true
docker exec "$PG_CONTAINER" psql -U rampart -d postgres -c \
  "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null
docker exec "$PG_CONTAINER" psql -U rampart -d postgres -c \
  "CREATE DATABASE ${DB_NAME} OWNER rampart;" >/dev/null

echo "[start-api] applying migrations"
DATABASE_URL="$TEST_URL" sqlx migrate run --source "${BACKEND}/migrations"

echo "[start-api] launching rampart-api on ${BIND_ADDR:-0.0.0.0:3001}"
export DATABASE_URL="$TEST_URL"
export BIND_ADDR="${BIND_ADDR:-0.0.0.0:3001}"
export RUST_LOG="${RUST_LOG:-rampart=warn,tower_http=warn}"
export SQLX_OFFLINE="${SQLX_OFFLINE:-true}"
exec "${BACKEND}/target/debug/rampart-api"
