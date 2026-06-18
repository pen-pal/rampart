#!/bin/sh
# push-cron — drives the REAL push-monitor heartbeat path for the push monitor
# provision created (token captured into /shared/push.env). Most cycles run a
# green job (run → complete); every ~Nth cycle it /fail's → a genuine Down flip
# that fans out through the delivery-log / escalation / on-call / tag-routing.
#
# UP   = POST /push/<token>/complete   (or ?status=up)
# DOWN = POST /push/<token>/fail       (or ?status=down)
set -u
echo "[push-cron] waiting for the push token from provision…"
i=0
while [ ! -f /shared/push.env ]; do
  i=$((i + 1)); [ "$i" -gt 120 ] && echo "[push-cron] no push.env — exiting" && exit 0
  sleep 2
done
# shellcheck disable=SC1091
. /shared/push.env
API="${RAMPART_URL:-http://rampart:3000}"
TOK="${PUSH_TOKEN:-}"
[ -z "$TOK" ] && echo "[push-cron] empty push token — exiting" && exit 0
echo "[push-cron] heartbeating push monitor at $API/push/$TOK"

n=0
while true; do
  n=$((n + 1))
  if [ $(( n % 6 )) -eq 0 ]; then
    # every 6th cycle: a real failure → Down flip (pages via the pipeline)
    curl -s -o /dev/null "$API/push/$TOK/fail?msg=backup+failed" || true
    echo "[push-cron] cycle $n → FAIL (Down)"
    sleep 30
    # recover so the next cycle is green again (Up → resolves the episode)
    curl -s -o /dev/null "$API/push/$TOK/complete?msg=recovered" || true
    echo "[push-cron] recovered → complete (Up)"
  else
    curl -s -o /dev/null "$API/push/$TOK/run" || true       # opens the run clock
    sleep 3
    curl -s -o /dev/null "$API/push/$TOK/complete?ping=42" || true   # green
    echo "[push-cron] cycle $n → run+complete (Up)"
  fi
  sleep 45
done
