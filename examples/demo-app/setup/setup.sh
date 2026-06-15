#!/bin/sh
# One-shot: wait for Rampart, sign in, and create the pieces that make the
# demo app's telemetry light up the SIEM + uptime tiers — a detection rule that
# matches the backend's repeated "failed login" logs, and an uptime monitor for
# the backend. Idempotent-ish (dupes are ignored).
set -u
API="${RAMPART_URL:-http://rampart:3000}"
EMAIL="${RAMPART_ADMIN_EMAIL:-demo@rampart.local}"
PASS="${RAMPART_ADMIN_PASSWORD:-Rampart-Live-9271}"
CJ=/tmp/cj
J='-H content-type:application/json'

echo "[setup] waiting for Rampart at $API ..."
until curl -sf "$API/healthz" >/dev/null 2>&1; do sleep 2; done

curl -s -c "$CJ" $J -X POST "$API/v1/auth/register" \
  -d "{\"email\":\"$EMAIL\",\"name\":\"Demo\",\"password\":\"$PASS\"}" >/dev/null 2>&1 || true
curl -s -c "$CJ" $J -X POST "$API/v1/auth/login" \
  -d "{\"email\":\"$EMAIL\",\"password\":\"$PASS\"}" >/dev/null 2>&1 || true

# SIEM rule keyed on the backend's auth-failure logs (service=demo-backend).
curl -s -b "$CJ" $J -X POST "$API/v1/detection-rules" -d '{
  "name":"demo-app · repeated failed logins","severity":"high",
  "service":"demo-backend","min_level":13,"body_regex":"failed login",
  "threshold":3,"window_seconds":600}' >/dev/null 2>&1 || true

# Uptime monitor for the backend.
curl -s -b "$CJ" $J -X POST "$API/v1/monitors" -d '{
  "name":"demo-app backend","kind":"http",
  "url":"http://demo-backend:8080/api/health","config":{},"interval_seconds":20}' >/dev/null 2>&1 || true

echo "[setup] done — detection rule + monitor created"
