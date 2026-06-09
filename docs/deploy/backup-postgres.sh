#!/usr/bin/env bash
# Rampart · Postgres backup script.
#
# Dumps the rampart database via `pg_dump --format=custom` and rotates
# old dumps. Safe to run while Rampart is live — pg_dump takes a
# snapshot consistent at start-time and lets writes proceed in
# parallel. The custom format is the same format `pg_restore` consumes;
# it compresses ~3-5× better than plain SQL and supports per-table
# selective restore.
#
# Usage (one-shot):
#   ./backup-postgres.sh
#
# Usage (cron, daily at 03:14):
#   14 3 * * *  /opt/rampart/docs/deploy/backup-postgres.sh
#
# Env vars (all optional — sensible defaults derived from the compose
# stack):
#   RAMPART_PG_CONTAINER  Compose service name. Default: rampart-postgres-1
#                         (the auto-derived name; override if you use
#                         project-name or run compose standalone).
#   PGUSER, PGPASSWORD, PGDATABASE
#                         Standard libpq env vars. Defaults map to the
#                         compose.yaml defaults: rampart/rampart/rampart.
#   BACKUP_DIR            Where dumps land. Default: /var/backups/rampart
#   BACKUP_KEEP_DAYS      Delete dumps older than this. Default: 14
#
# Exit codes:
#   0  success (file written, old files pruned)
#   1  pg_dump failed (no file written)
#   2  the named container isn't running

set -euo pipefail

PG_CONTAINER="${RAMPART_PG_CONTAINER:-rampart-postgres-1}"
PG_USER="${PGUSER:-rampart}"
PG_PASS="${PGPASSWORD:-rampart}"
PG_DB="${PGDATABASE:-rampart}"
BACKUP_DIR="${BACKUP_DIR:-/var/backups/rampart}"
BACKUP_KEEP_DAYS="${BACKUP_KEEP_DAYS:-14}"

if ! docker ps --format '{{.Names}}' | grep -qx "${PG_CONTAINER}"; then
  echo "error: postgres container '${PG_CONTAINER}' is not running" >&2
  echo "set RAMPART_PG_CONTAINER to the correct compose service name" >&2
  exit 2
fi

mkdir -p "${BACKUP_DIR}"
chmod 700 "${BACKUP_DIR}"

STAMP="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
OUT="${BACKUP_DIR}/rampart-${STAMP}.dump"

echo "rampart: dumping ${PG_DB} (container ${PG_CONTAINER}) → ${OUT}"

# `docker exec` runs pg_dump inside the postgres container so we don't
# need the postgres-client tools on the host. The custom format
# (-Fc) compresses and supports selective per-table restore.
docker exec -i \
  -e PGPASSWORD="${PG_PASS}" \
  "${PG_CONTAINER}" \
  pg_dump -U "${PG_USER}" -d "${PG_DB}" -Fc -Z 6 \
  > "${OUT}"

chmod 600 "${OUT}"
SIZE="$(du -h "${OUT}" | cut -f1)"
echo "rampart: ok ${SIZE}"

# Rotate. -mtime is +N for "modified more than N*24h ago"; this keeps
# today's dump even at the edge of the window.
if [ -n "${BACKUP_KEEP_DAYS}" ] && [ "${BACKUP_KEEP_DAYS}" -gt 0 ]; then
  find "${BACKUP_DIR}" -name 'rampart-*.dump' -type f \
    -mtime "+${BACKUP_KEEP_DAYS}" -print -delete \
    | sed 's/^/rampart: pruned old backup: /'
fi
