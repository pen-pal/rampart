#!/usr/bin/env bash
#
# check-openapi.sh — CI guard keeping docs/openapi.yaml in sync with the
# REST routes the axum router actually registers.
#
# WHY: docs/openapi.yaml is hand-curated and served verbatim at
# /openapi.yaml + /openapi.json (routes/openapi.rs). It silently drifts
# whenever a new route lands without a matching spec entry. axum 0.8 does
# not cheaply expose its route table, so instead of building the router we
# statically derive the registered path set from the route source and diff
# it against the `paths:` keys in the spec.
#
# SCOPE: this catches "you added a route and forgot to document it". It does
# NOT enforce field-level accuracy (request/response bodies, params) — that
# would make the check brittle and high-friction. Path-level coverage only.
#
# A registered path that is MISSING from the spec fails the build. Paths in
# the spec that are not (yet) registered are reported as warnings only — the
# spec may legitimately describe planned or aliased surface.
#
# Run locally:  bash scripts/check-openapi.sh
# Exit 0 = every registered route is documented. Non-zero = gaps found.

set -euo pipefail

# Resolve repo root relative to this script so it works from any CWD / CI.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

ROUTES_DIR="${REPO_ROOT}/backend/crates/rampart-api/src/routes"
LIB_RS="${REPO_ROOT}/backend/crates/rampart-api/src/lib.rs"
SPEC="${REPO_ROOT}/docs/openapi.yaml"

for p in "${ROUTES_DIR}" "${LIB_RS}" "${SPEC}"; do
  if [[ ! -e "${p}" ]]; then
    echo "check-openapi: required path not found: ${p}" >&2
    exit 2
  fi
done

PY="${PYTHON:-python3}"
if ! command -v "${PY}" >/dev/null 2>&1; then
  echo "check-openapi: python3 not found on PATH" >&2
  exit 2
fi

exec "${PY}" "${SCRIPT_DIR}/check_openapi.py" \
  --routes-dir "${ROUTES_DIR}" \
  --lib-rs "${LIB_RS}" \
  --spec "${SPEC}"
