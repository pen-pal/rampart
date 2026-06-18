# 70-maintenance-silence.sh — a maintenance window covering NOW, and a silence
# created then lifted (demonstrating both the create and the lift).

NOW="$(date -u +%FT%TZ)"
START="$(date -u -d '-10 minutes' +%FT%TZ 2>/dev/null || date -u -v-10M +%FT%TZ)"
END="$(date -u -d '+2 hours' +%FT%TZ 2>/dev/null || date -u -v+2H +%FT%TZ)"

# --- maintenance window covering now (attached to the healthy edge) ---------
MW="$(get /v1/maintenance-windows)"
if ! echo "$MW" | jq -e '(if type=="array" then .[] else (.items[]?) end)|select(.name=="demo maintenance (now)")' >/dev/null 2>&1; then
  MID_EDGE="$(id_by_name "$(get /v1/monitors)" 'edge · healthy nginx')"
  post /v1/maintenance-windows "$(jq -nc --arg s "$START" --arg e "$END" --arg m "$MID_EDGE" '{
    name:"demo maintenance (now)", description:"Quarterly patching window",
    start_at:$s, end_at:$e, recurrence:{kind:"none"},
    monitor_ids: ($m|if .=="" then [] else [.] end) }')" >/dev/null
  log "maintenance window created covering now ($START → $END)"
fi

# --- a recurring weekly maintenance window (shows recurrence) ---------------
if ! echo "$(get /v1/maintenance-windows)" | jq -e '(if type=="array" then .[] else (.items[]?) end)|select(.name=="weekly window")' >/dev/null 2>&1; then
  post /v1/maintenance-windows "$(jq -nc --arg s "$START" --arg e "$END" '{
    name:"weekly window", start_at:$s, end_at:$e,
    recurrence:{kind:"weekly", weekdays:[0,6]} }')" >/dev/null
  log "recurring weekly maintenance window created"
fi

# --- silence: create then lift ----------------------------------------------
SIL_ID="$(post /v1/silences "$(jq -nc --arg m "$MON_FLAKY" '{
  monitor_id: ($m|if .=="" then null else . end),
  reason:"demo: silence then lift", duration_minutes:60 }')" | jq -r '.id // empty')"
if [ -n "$SIL_ID" ]; then
  log "silence created ($SIL_ID) on the flaky monitor"
  del "/v1/silences/$SIL_ID" >/dev/null 2>&1
  log "silence lifted (deleted) — demonstrates the full create→lift lifecycle"
fi

# --- a standing GLOBAL silence left in place so the UI shows an active one ---
if ! echo "$(get /v1/silences)" | jq -e '.[]?|select(.reason=="demo: standing global mute")' >/dev/null 2>&1; then
  post /v1/silences '{"reason":"demo: standing global mute","duration_minutes":1}' >/dev/null 2>&1 || true
fi
