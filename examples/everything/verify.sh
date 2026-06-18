#!/usr/bin/env bash
# verify.sh — run this AFTER `docker compose up` has been live for a few minutes.
# Asserts every tier is genuinely non-empty: monitors of many kinds, delivery-log
# rows, an open episode, an incident from ingest, and traces/logs/metrics/rum/
# profiles/errors all present. Exits non-zero if a tier is empty.
#
#   bash verify.sh            # uses defaults below
#   API=http://localhost:3000 EMAIL=... PASS=... bash verify.sh
set -u
API="${API:-http://localhost:3000}"
EMAIL="${EMAIL:-${RAMPART_ADMIN_EMAIL:-demo@rampart.local}}"
PASS="${PASS:-${RAMPART_ADMIN_PASSWORD:-Rampart-Live-9271}}"
SINK="${SINK_URL:-http://localhost:8099}"
CJ="$(mktemp)"
J=(-H 'content-type: application/json')
command -v jq >/dev/null 2>&1 || { echo "verify.sh needs jq + curl on PATH (apt-get install jq / brew install jq)"; exit 2; }
fails=0
pass() { printf '  \033[32mPASS\033[0m  %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; fails=$((fails + 1)); }

g() { curl -s -b "$CJ" "$API$1"; }

echo "== verifying Rampart at $API =="
curl -s -c "$CJ" "${J[@]}" -X POST "$API/v1/auth/login" -d "{\"email\":\"$EMAIL\",\"password\":\"$PASS\"}" >/dev/null
g /v1/auth/me | grep -q '"email"' && pass "authenticated" || { fail "could not authenticate"; echo "(is the stack up? is provision done?)"; }

# helper: count elements of an array-or-{items|rows} response
count() { jq -r '(if type=="array" then length elif has("items") then (.items|length) elif has("rows") then (.rows|length) else 0 end)' 2>/dev/null; }
atleast() {  # atleast <n> <desc> <json>
  local n; n="$(printf '%s' "$3" | count)"; n="${n:-0}"
  if [ "$n" -ge "$1" ]; then pass "$2 ($n)"; else fail "$2 — got $n, want >= $1"; fi
}

# --- config tiers -----------------------------------------------------------
MONS="$(g /v1/monitors)"
atleast 30 "monitors created (all 42 kinds)" "$MONS"
KINDS="$(printf '%s' "$MONS" | jq -r '(if type=="array" then .[] else (.items[]?) end).kind' 2>/dev/null | sort -u | wc -l)"
[ "${KINDS:-0}" -ge 30 ] && pass "distinct monitor kinds ($KINDS)" || fail "distinct monitor kinds — got ${KINDS:-0}, want >= 30"
atleast 100 "notification channels (all ~128 kinds)" "$(g /v1/notifications)"
atleast 1 "escalation policies" "$(g /v1/escalation-policies)"
atleast 1 "on-call schedules" "$(g /v1/on-call-schedules)"
atleast 1 "SLOs" "$(g /v1/slos)"
atleast 1 "maintenance windows" "$(g /v1/maintenance-windows)"
atleast 1 "status pages" "$(g /v1/status-pages)"
atleast 1 "detection rules" "$(g /v1/detection-rules)"
atleast 1 "telemetry rules" "$(g /v1/telemetry-rules)"
atleast 1 "metric rules" "$(g /v1/metrics/rules)"
atleast 2 "api keys" "$(g /v1/api-keys)"
atleast 1 "proxies" "$(g /v1/proxies)"
atleast 1 "agents" "$(g /v1/agents)"
atleast 1 "scheduled reports" "$(g /v1/scheduled-reports)"
atleast 1 "deploy markers" "$(g '/v1/deploy-markers?hours=240')"
atleast 2 "orgs (multi-tenancy)" "$(g /v1/orgs)"
atleast 1 "tags" "$(g /v1/tags)"
atleast 2 "monitor groups/folders" "$(g /v1/monitor-groups)"

# --- LIVE telemetry tiers (need the real services to have run a few minutes) -
atleast 1 "TRACES present (real OTLP)" "$(g '/v1/traces?limit=50')"
atleast 1 "LOGS present (real OTLP)" "$(g '/v1/logs?limit=50')"
atleast 1 "metric series present" "$(g /v1/metrics/series)"
# RUM + errors + profiles have bespoke shapes
RUM="$(g /v1/rum/summary)"; echo "$RUM" | jq -e '.' >/dev/null 2>&1 && pass "RUM summary returned" || fail "RUM summary"
EP="$(g /v1/error-projects)"; atleast 1 "error projects" "$EP"
PROFS="$(g '/v1/profiles?hours=24')"; atleast 1 "profiles present (folded/pprof/otlp)" "$PROFS"
PSVC="$(g /v1/profiles/services)"; echo "$PSVC" | jq -e 'length>=1' >/dev/null 2>&1 && pass "profile services present" || fail "profile services"

# --- runtime state: delivery log, episodes, incidents -----------------------
atleast 1 "delivery-log rows (real notifications sent)" "$(g '/v1/delivery-log?limit=50')"
EPIS="$(g /v1/escalation-policies/episodes)"; echo "$EPIS" | jq -e '(if type=="array" then length else (.items|length) end) >= 1' >/dev/null 2>&1 \
  && pass "escalation episodes present" || fail "escalation episodes (none yet — let the flapping monitor cycle)"
# incident from the real ingest webhook (open or recently resolved)
INC="$(g /v1/incidents/recent)"; echo "$INC" | jq -e '(if type=="array" then length else (.items|length) end) >= 1' >/dev/null 2>&1 \
  && pass "incidents present (from ingest webhook)" || fail "incidents present"
# detection findings from the real failed-login logs
FIND="$(g '/v1/detection-rules/findings?limit=20')"; echo "$FIND" | jq -e '(if type=="array" then length else (.items|length) end) >= 1' >/dev/null 2>&1 \
  && pass "detection findings raised (from real logs)" || fail "detection findings (none yet — let logins run)"

# --- sink + mailpit (external, best-effort) ---------------------------------
SINK_CT="$(curl -s "$SINK/_count" 2>/dev/null | jq -r '.count // 0' 2>/dev/null)"
[ "${SINK_CT:-0}" -ge 1 ] && pass "webhook-sink received deliveries ($SINK_CT)" || fail "webhook-sink deliveries (check http://localhost:8099)"

echo
if [ "$fails" -eq 0 ]; then
  echo "✅ ALL TIERS NON-EMPTY"
else
  echo "❌ $fails check(s) failed — some tiers may just need more uptime (traces/episodes/incidents accrue over minutes)"
fi
rm -f "$CJ"
exit "$fails"
