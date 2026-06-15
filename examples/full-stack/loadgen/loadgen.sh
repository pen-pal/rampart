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
# NOTE: password must pass the policy in rampart_api::auth::validate_password —
# >=10 chars and must NOT contain the email local-part ("demo"), else register
# 400s and no admin is created.
EMAIL="demo@rampart.local"
PASS="Rampart-Live-9271"
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

# Error project for live errors; capture its id + DSN public key.
RESP=$(curl -s -b "$CJ" $J -X POST "$API/v1/error-projects" \
  -d '{"name":"live-app","platform":"python"}' 2>/dev/null)
PID=$(printf '%s' "$RESP" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
KEY=$(printf '%s' "$RESP" | grep -o '"public_key":"[^"]*"' | head -1 | cut -d'"' -f4)
log "error project id=${PID:-?}"

i=0
while true; do
  i=$((i + 1))
  NOW=$(date +%s)
  NS="${NOW}000000000"
  END="$((NOW + 1))000000000"
  TRACE=$(printf '%032x' "$i")
  SPANA=$(printf '%016x' "$i")
  SPANB=$(printf '%016x' "$((i + 100000))")
  svc=$([ $((i % 2)) -eq 0 ] && echo live-api || echo live-web)

  # OTLP traces (OTLP/JSON) — a 2-span trace.
  curl -s $J -X POST "$API/otlp/v1/traces" -d "{
    \"resourceSpans\":[{\"resource\":{\"attributes\":[{\"key\":\"service.name\",\"value\":{\"stringValue\":\"$svc\"}}]},
    \"scopeSpans\":[{\"spans\":[
      {\"traceId\":\"$TRACE\",\"spanId\":\"$SPANA\",\"name\":\"GET /\",\"kind\":2,\"startTimeUnixNano\":\"$NS\",\"endTimeUnixNano\":\"$END\",\"status\":{\"code\":1}},
      {\"traceId\":\"$TRACE\",\"spanId\":\"$SPANB\",\"parentSpanId\":\"$SPANA\",\"name\":\"SELECT 1\",\"kind\":3,\"startTimeUnixNano\":\"$NS\",\"endTimeUnixNano\":\"$END\",\"status\":{\"code\":0}}
    ]}]}]}" >/dev/null 2>&1 || true

  # OTLP logs.
  sev=$([ $((i % 7)) -eq 0 ] && echo '17,"ERROR"' || echo '9,"INFO"')
  curl -s $J -X POST "$API/otlp/v1/logs" -d "{
    \"resourceLogs\":[{\"resource\":{\"attributes\":[{\"key\":\"service.name\",\"value\":{\"stringValue\":\"$svc\"}}]},
    \"scopeLogs\":[{\"logRecords\":[{\"timeUnixNano\":\"$NS\",\"severityNumber\":${sev%,*},\"severityText\":${sev#*,},\"body\":{\"stringValue\":\"request $i handled\"}}]}]}]}" >/dev/null 2>&1 || true

  # RUM web-vitals (vary LCP so the chart moves + occasionally breaches).
  lcp=$((1500 + (i * 137) % 3000))
  curl -s $J -X POST "$API/rum/v1/events" -d "{
    \"app\":\"live-storefront\",\"url\":\"/p/$((i % 5))\",\"session\":\"s-$((i % 50))\",
    \"metrics\":{\"lcp_ms\":$lcp,\"fcp_ms\":900,\"cls\":0.05,\"inp_ms\":120,\"ttfb_ms\":210,\"load_ms\":$((lcp + 300))}}" >/dev/null 2>&1 || true

  # An error every ~5th tick.
  if [ $((i % 5)) -eq 0 ] && [ -n "${PID:-}" ] && [ -n "${KEY:-}" ]; then
    curl -s $J -X POST "$API/api/$PID/store?sentry_key=$KEY" -d "{
      \"event_id\":\"$TRACE\",\"level\":\"error\",\"platform\":\"python\",\"release\":\"2.1.0\",\"environment\":\"production\",
      \"exception\":{\"values\":[{\"type\":\"ValueError\",\"value\":\"bad input on item $((i % 13))\",
      \"stacktrace\":{\"frames\":[{\"filename\":\"worker.py\",\"function\":\"handle\",\"lineno\":61,\"in_app\":true}]}}]},
      \"user\":{\"id\":\"u-$((i % 25))\"}}" >/dev/null 2>&1 || true
  fi

  [ $((i % 6)) -eq 0 ] && log "emitted $i batches"
  sleep 10
done
