# 50-routing.sh — tag-routing end to end:
#   folder tag + channel tag (same tag) ⇒ channel fires for monitors in folder
#   folder channel (attached directly to a folder)
#   per-monitor exclude (removes a channel that would otherwise fire)
#   effective-channels (read back what actually resolves)

CH="$(get /v1/notifications)"
CH_WEBHOOK="$(id_by_name "$CH" 'webhook → sink')"
CH_EMAIL="$(id_by_name "$CH" 'email → Mailpit')"
CH_NTFY="$(id_by_name "$CH" 'ntfy → sink')"

# folder tag: tag the Edge folder 'critical'
[ -n "$GRP_EDGE" ] && [ -n "$TAG_CRITICAL" ] && \
  curl -s -b "$CJ" -X POST "$API/v1/monitor-groups/$GRP_EDGE/tags/$TAG_CRITICAL" >/dev/null 2>&1
# channel tag: tag the webhook channel 'critical' → it now fires for Edge monitors
[ -n "$CH_WEBHOOK" ] && [ -n "$TAG_CRITICAL" ] && \
  curl -s -b "$CJ" -X POST "$API/v1/notifications/$CH_WEBHOOK/tags/$TAG_CRITICAL" >/dev/null 2>&1
log "tag-routing: Edge folder + webhook channel both tagged 'critical'"

# folder channel: attach the email channel directly to the Edge folder
[ -n "$GRP_EDGE" ] && [ -n "$CH_EMAIL" ] && \
  curl -s -b "$CJ" -X POST "$API/v1/monitor-groups/$GRP_EDGE/channels/$CH_EMAIL" >/dev/null 2>&1
log "folder channel: email attached to Edge folder"

# direct monitor attach: ntfy → flapping monitor
[ -n "$MON_FLAP" ] && [ -n "$CH_NTFY" ] && \
  curl -s -b "$CJ" -X POST "$API/v1/monitors/$MON_FLAP/notifications/$CH_NTFY" >/dev/null 2>&1

# monitor exclude: exclude email from the flaky target even though the folder routes it
MON_FLAKY_ID="$(id_by_name "$(get /v1/monitors)" 'edge · flaky target (503/min)')"
[ -n "$MON_FLAKY_ID" ] && [ -n "$CH_EMAIL" ] && \
  curl -s -b "$CJ" -X POST "$API/v1/monitors/$MON_FLAKY_ID/excludes/$CH_EMAIL" >/dev/null 2>&1
log "monitor exclude: email excluded from the flaky target"

# read back effective channels for the flapping monitor (proves resolution)
if [ -n "$MON_FLAP" ]; then
  EFF="$(get "/v1/monitors/$MON_FLAP/effective-channels")"
  log "effective-channels for flapping monitor: $(echo "$EFF" | jq -c '.' 2>/dev/null)"
fi
