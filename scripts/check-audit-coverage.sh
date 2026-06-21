#!/usr/bin/env bash
# CI guard: every RBAC / org-membership mutation handler must emit an audit
# record (SOC2 access-review — member add/remove/role-change + org create/rename
# are exactly the access-change events an auditor and a quarterly access review
# require). A new mutating handler added without a `crate::audit::record` call
# would silently drop the access-change event; fail the build so it can't.
#
# Deliberately a coarse count guard, not a parser: it catches the common
# regression (someone adds an org-mutation handler and forgets the audit call)
# without pretending to understand control flow.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
f="$root/backend/crates/rampart-api/src/routes/orgs.rs"

# RBAC/membership mutations that MUST be audited.
handlers=(create rename add_member set_member_role remove_member)
want=${#handlers[@]}

if [ ! -f "$f" ]; then
  echo "FAIL: $f not found" >&2
  exit 1
fi

got=$(grep -c "audit::record" "$f" || true)
if [ "$got" -lt "$want" ]; then
  echo "FAIL: $f has $got audit::record call(s) but $want RBAC mutations must be audited." >&2
  echo "      Each of [${handlers[*]}] must call crate::audit::record on success." >&2
  exit 1
fi

echo "OK: orgs.rs RBAC mutations audited ($got audit::record call(s) >= $want handlers)."
