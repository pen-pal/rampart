#!/usr/bin/env bash
# =============================================================================
# Rampart "everything" demo — one-shot provisioner (orchestrator).
#
# Runs AFTER rampart is healthy. Creates CONFIG for every feature via the HTTP
# API and captures runtime secrets (push token, agent rmpa_ token, write
# api-key, error-project DSN, ingest tokens) into the shared volume so the
# demo-app / agent / crons can consume them.
#
# It NEVER fabricates telemetry/uptime/errors — those come from real running
# services. API calls here are CONFIG ONLY (plus opening/closing one incident
# via the public ingest webhook, which IS the real alert→incident path).
#
# Idempotent: GET-before-POST / name checks throughout; safe to re-run.
# Each section is a sourced file under sections/. bash + curl + jq.
# =============================================================================
set -u
export API="${RAMPART_URL:-http://rampart:3000}"
export EMAIL="${RAMPART_ADMIN_EMAIL:-demo@rampart.local}"
export PASS="${RAMPART_ADMIN_PASSWORD:-Rampart-Live-9271}"
export SINK="${SINK_URL:-http://webhook-sink:8099}"
export MAILPIT_SMTP_HOST="${MAILPIT_SMTP_HOST:-mailpit}"
export MAILPIT_SMTP_PORT="${MAILPIT_SMTP_PORT:-1025}"
export PUBLIC_BASE="${PUBLIC_BASE:-http://localhost:3000}"
export SHARED="${SHARED_DIR:-/shared}"
export CJ=/tmp/cj
export HERE="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$SHARED"
log()  { echo "[provision] $*"; }
J=(-H 'content-type: application/json')

# --- tiny HTTP helpers (cookie-jar session) ---------------------------------
api()  { curl -s -b "$CJ" -c "$CJ" "${J[@]}" "$@"; }
get()  { curl -s -b "$CJ" "$API$1"; }
post() { curl -s -b "$CJ" -c "$CJ" "${J[@]}" -X POST "$API$1" --data-binary "$2"; }
patch(){ curl -s -b "$CJ" -c "$CJ" "${J[@]}" -X PATCH "$API$1" --data-binary "$2"; }
del()  { curl -s -b "$CJ" -c "$CJ" -X DELETE "$API$1"; }
puta() { curl -s -b "$CJ" -c "$CJ" "${J[@]}" -X PUT "$API$1" --data-binary "$2"; }
# POST raw text body with an explicit content-type (CSV / prom-text).
post_text() { curl -s -b "$CJ" -c "$CJ" -H "content-type: $2" -X POST "$API$1" --data-binary "$3"; }
export -f log api get post patch del puta post_text

# id of the first array/.{items} element whose .name == $2
id_by_name() { echo "$1" | jq -r --arg n "$2" '(if type=="array" then .[] else (.items[]?) end)|select(.name==$n).id' 2>/dev/null | head -1; }
export -f id_by_name

# create-monitor-if-absent: $1=name $2=json-body → echoes the monitor id
ensure_monitor() {
  local name="$1" body="$2" existing id
  existing="$(get /v1/monitors)"
  id="$(id_by_name "$existing" "$name")"
  if [ -n "$id" ]; then echo "$id"; return; fi
  id="$(post /v1/monitors "$body" | jq -r '.id // empty')"
  echo "$id"
}
export -f ensure_monitor

# create-channel-if-absent: $1=name $2=json-body → echoes id (best-effort)
ensure_channel() {
  local name="$1" body="$2" existing id
  existing="$(get /v1/notifications)"
  id="$(id_by_name "$existing" "$name")"
  if [ -n "$id" ]; then echo "$id"; return; fi
  post /v1/notifications "$body" | jq -r '.id // empty'
}
export -f ensure_channel

# =============================================================================
log "waiting for Rampart at $API ..."
until curl -sf "$API/healthz" >/dev/null 2>&1; do sleep 2; done
until curl -sf "$API/readyz"  >/dev/null 2>&1; do sleep 2; done
log "rampart is up"

# --- first-run admin + session ----------------------------------------------
# The running server does NOT auto-create an admin (only seed-demo does), so the
# first register call here becomes the Admin. Idempotent once it exists.
post /v1/auth/register "{\"email\":\"$EMAIL\",\"name\":\"Demo Admin\",\"password\":\"$PASS\"}" >/dev/null 2>&1 || true
post /v1/auth/login    "{\"email\":\"$EMAIL\",\"password\":\"$PASS\"}" >/dev/null 2>&1 || true
if ! get /v1/auth/me 2>/dev/null | grep -q '"email"'; then
  log "FATAL: no admin session — check the password policy (>=10 chars, must not contain the email local-part)"
  exit 1
fi
log "authenticated as $EMAIL"

# --- run the sections in order ----------------------------------------------
for s in \
  10-secrets.sh \
  20-tags-groups.sh \
  30-channels.sh \
  40-monitors.sh \
  50-routing.sh \
  60-escalation-oncall-slo.sh \
  70-maintenance-silence.sh \
  80-rules.sh \
  90-status-incidents-ingest.sh \
  95-orgs-keys-proxy-agent-misc.sh \
  99-finalize.sh
do
  if [ -f "$HERE/sections/$s" ]; then
    log "── section $s ──"
    # shellcheck disable=SC1090
    . "$HERE/sections/$s" || log "section $s reported an error (continuing)"
  else
    log "WARN: section $s missing"
  fi
done

log "provisioning complete. Secrets written to $SHARED:"
ls -1 "$SHARED" 2>/dev/null | sed 's/^/[provision]   /'
