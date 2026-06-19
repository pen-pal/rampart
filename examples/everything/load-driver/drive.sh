#!/bin/sh
# load-driver — keeps the instrumented demo backend under continuous, realistic
# traffic so EVERY telemetry tier (traces, logs, metrics, errors, SIEM
# detection) shows live data without anyone keeping a browser tab open.
#
# This is the missing piece that made the demo look "empty": the frontend page
# auto-drives traffic, but only while a browser has it open. This service drives
# the backend directly, forever, on the compose network.
#
# RUM (web vitals + JS errors) still needs a real browser; the `rum-driver`
# service handles that by periodically rendering the frontend through the
# browserless `renderer`.
set -u
API="${DEMO_BASE:-http://demo-backend:8080}"
echo "[load-driver] waiting for demo backend at $API ..."
i=0
until curl -sf -o /dev/null "$API/api/health"; do
  i=$((i + 1)); [ "$i" -gt 120 ] && echo "[load-driver] backend never came up — exiting" && exit 0
  sleep 2
done
echo "[load-driver] backend up — driving traffic"

USERS="alice bob carol dave eve"
N=0
while true; do
  N=$((N + 1))
  pick=$(echo $USERS | tr ' ' '\n' | sed -n "$(( (N % 5) + 1 ))p")
  pid=$(( (N % 4) + 1 ))

  # happy paths → traces (express+pg+redis spans), logs, metrics
  curl -s -o /dev/null "$API/api/products"
  curl -s -o /dev/null "$API/api/products/$pid"
  curl -s -o /dev/null -X POST -H 'content-type: application/json' -d "{\"productId\":$pid}" "$API/api/checkout"
  curl -s -o /dev/null "$API/welcome"
  curl -s -o /dev/null "$API/status.json"

  # auth → SIEM detection (random failed logins the detection rule keys on)
  curl -s -o /dev/null -X POST -H 'content-type: application/json' -d "{\"user\":\"$pick\"}" "$API/api/login"

  # errors → @sentry/node capture (every ~6th + ~9th cycle, two error types)
  [ $(( N % 6 )) -eq 0 ] && curl -s -o /dev/null "$API/api/boom"
  [ $(( N % 9 )) -eq 0 ] && curl -s -o /dev/null "$API/api/boom2"

  # NOTE: we deliberately do NOT toggle /admin/health here — the dedicated
  # `target-flaky` service (503 for ~25s/min) and the push-cron already produce
  # genuine Down/Up cycles that drive incidents / escalation / on-call / SLO.

  [ $(( N % 20 )) -eq 0 ] && echo "[load-driver] $N cycles"
  sleep 3
done
