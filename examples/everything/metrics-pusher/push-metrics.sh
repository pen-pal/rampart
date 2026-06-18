#!/bin/sh
# metrics-pusher — pushes Prometheus text to Rampart's /v1/metrics/ingest with
# the WRITE api-key `provision` captured. This is the REAL metric-push path; the
# pushed demo_queue_depth series breaches the metric rule provision created
# (demo_queue_depth{service="demo-backend"} > 40).
#
# POSIX sh (busybox ash in curlimages/curl) — no bashisms; randomness via awk.
set -u
echo "[metrics-pusher] waiting for the WRITE key from provision…"
i=0
while [ ! -f /shared/writer.env ]; do
  i=$((i + 1)); [ "$i" -gt 120 ] && echo "[metrics-pusher] no writer.env — exiting" && exit 0
  sleep 2
done
# shellcheck disable=SC1091
. /shared/writer.env
API="${RAMPART_URL:-http://rampart:3000}"
KEY="${RAMPART_WRITE_KEY:-}"
[ -z "$KEY" ] && echo "[metrics-pusher] empty WRITE key — exiting" && exit 0
echo "[metrics-pusher] pushing to $API/v1/metrics/ingest every 15s"

rnd() { awk -v min="$1" -v max="$2" 'BEGIN{srand(); print int(min+rand()*(max-min))}'; }

while true; do
  Q="$(rnd 45 115)"        # > 40 → breaches the metric rule
  JOBS="$(rnd 0 12)"
  BODY="$(printf 'demo_queue_depth{service="demo-backend"} %s\ndemo_active_jobs{service="demo-backend"} %s\ndemo_pushed_build_info{service="demo-backend",version="1.0.0"} 1\n' "$Q" "$JOBS")"
  CODE="$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/v1/metrics/ingest" \
    -H "authorization: Bearer $KEY" -H 'content-type: text/plain' --data-binary "$BODY")"
  [ "$CODE" = "200" ] || echo "[metrics-pusher] HTTP $CODE pushing metrics"
  sleep 15
done
