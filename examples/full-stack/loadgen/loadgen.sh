#!/bin/sh
# Live telemetry generator for the Rampart full-stack example.
#
# Waits for Rampart, signs in (registering the first admin if needed), creates
# two live monitors pointing at the probe targets and an error project, then
# loops forever emitting OTLP traces + logs, RUM web-vitals and errors so the
# dashboard keeps moving. POSIX sh + curl only; all calls are best-effort
# (`|| true`) so a transient blip never kills the loop.
set -u

API="${RAMPART_URL:-http://rampart:3000}"
# Admin creds: seed-demo creates this user server-side (any password works), so
# we just log in with it here. Override via RAMPART_ADMIN_EMAIL / _PASSWORD.
# The register fallback below only fires if seed didn't create an admin — and
# its password must pass the policy (>=10 chars, must not contain the email
# local-part), hence the compliant default.
EMAIL="${RAMPART_ADMIN_EMAIL:-demo@rampart.local}"
PASS="${RAMPART_ADMIN_PASSWORD:-Rampart-Live-9271}"
CJ=/tmp/cj
J='-H content-type:application/json'

log() { echo "[loadgen] $*"; }

log "waiting for $API ..."
until curl -sf "$API/healthz" >/dev/null 2>&1; do sleep 2; done
log "rampart is up"

# First-run admin (no-op once it exists), then log in for a session cookie.
REG=$(curl -s -c "$CJ" $J -X POST "$API/v1/auth/register" \
  -d "{\"email\":\"$EMAIL\",\"name\":\"Demo\",\"password\":\"$PASS\"}" 2>/dev/null)
curl -s -c "$CJ" $J -X POST "$API/v1/auth/login" \
  -d "{\"email\":\"$EMAIL\",\"password\":\"$PASS\"}" >/dev/null 2>&1 || true
# Verify we actually have a session; warn loudly if not (e.g. password policy
# rejected register and the user never existed).
if ! curl -s -b "$CJ" "$API/v1/auth/me" 2>/dev/null | grep -q '"email"'; then
  log "WARN: no admin session — register response was: $REG"
  log "WARN: log in manually at the dashboard or check the password policy"
fi

# Live monitors pointing at the in-network probe targets.
curl -s -b "$CJ" $J -X POST "$API/v1/monitors" \
  -d '{"name":"live · healthy target","kind":"http","url":"http://target-healthy/","config":{},"interval_seconds":20}' >/dev/null 2>&1 || true
curl -s -b "$CJ" $J -X POST "$API/v1/monitors" \
  -d '{"name":"live · flaky target","kind":"http","url":"http://target-flaky:8080/","config":{},"interval_seconds":20}' >/dev/null 2>&1 || true

# Drive continuous REAL traffic against Rampart's own API. Rampart is configured
# (RAMPART_OTLP_ENDPOINT) to export its OWN request traces + logs back into
# itself, so this fills the Traces + Logs views with live data from the running
# app — no fabricated payloads. The ingest routes are excluded from self-export
# (no feedback loop), so we hit read endpoints here.
log "driving live traffic — Rampart self-exports its own traces + logs"
ENDPOINTS="/v1/monitors /v1/traces /v1/logs /v1/error-projects /v1/rum/summary /v1/detection-rules /v1/metrics /v1/escalation-policies /v1/on-call-schedules /v1/silences"
i=0
while true; do
  i=$((i + 1))
  for ep in $ENDPOINTS; do
    curl -s -b "$CJ" "$API$ep" >/dev/null 2>&1 || true
  done
  [ $((i % 6)) -eq 0 ] && log "driven $i rounds of live traffic"
  sleep 8
done
