# 60-escalation-oncall-slo.sh — on-call schedule (short rotation) + escalation
# policy bound to the flapping monitor, and a tight SLO on it.

CH="$(get /v1/notifications)"
CH_WEBHOOK="$(id_by_name "$CH" 'webhook → sink')"
CH_NTFY="$(id_by_name "$CH" 'ntfy → sink')"
# The admin user is always present — use them as a guaranteed on-call participant
# (a schedule requires >=1 participant; channels are an additional ring member).
ADMIN_UID="$(get /v1/auth/me | jq -r '.user.id // .id // empty')"

# --- on-call schedule (5-min rotation = the tight floor) --------------------
SCHED="$(get /v1/on-call-schedules)"
SCHED_ID="$(id_by_name "$SCHED" 'demo on-call (5m rotation)')"
if [ -z "$SCHED_ID" ]; then
  SCHED_ID="$(post /v1/on-call-schedules "$(jq -nc --arg u "$ADMIN_UID" --arg c "$CH_WEBHOOK" '{
    name:"demo on-call (5m rotation)", rotation_seconds:300, anchor:"2026-01-01T00:00:00Z",
    participant_user_ids: ($u|if .=="" then [] else [.] end),
    participant_ids: ($c|if .=="" then [] else [.] end)
  }')" | jq -r '.id // empty')"
fi
log "on-call schedule: ${SCHED_ID:-<none>} (5-minute rotation, admin + webhook in the ring)"

# --- escalation policy: immediate channel → after 60s page the on-call ------
# Each step needs >=1 of channel_ids/schedule_ids — filter out any empty step.
POL="$(get /v1/escalation-policies)"
POL_ID="$(id_by_name "$POL" 'demo escalation')"
if [ -z "$POL_ID" ]; then
  STEP0_CH="${CH_NTFY:-$CH_WEBHOOK}"
  POL_ID="$(post /v1/escalation-policies "$(jq -nc --arg c "$STEP0_CH" --arg s "$SCHED_ID" '{
    name:"demo escalation",
    steps:( [
      { wait_seconds:0,  channel_ids: ($c|if .=="" then [] else [.] end), schedule_ids:[] },
      { wait_seconds:60, channel_ids:[], schedule_ids: ($s|if .=="" then [] else [.] end) }
    ] | map(select((.channel_ids|length>0) or (.schedule_ids|length>0))) )
  }')" | jq -r '.id // empty')"
fi
log "escalation policy: ${POL_ID:-<none>} (step0 channel, step1 on-call after 60s)"

# --- bind the policy to the flapping monitor --------------------------------
if [ -n "${MON_FLAP:-}" ] && [ -n "$POL_ID" ]; then
  patch "/v1/monitors/$MON_FLAP" "{\"escalation_policy_id\":\"$POL_ID\"}" >/dev/null 2>&1
  log "bound escalation policy to the flapping monitor → real paging on Down"
fi

# --- tight SLO on the flapping monitor (will breach → error budget burns) ---
SLOS="$(get /v1/slos)"
if ! echo "$SLOS" | jq -e '(if type=="array" then .[] else (.items[]?,.rows[]?) end)|select(.name=="flapping uptime 99.99")' >/dev/null 2>&1; then
  if [ -n "${MON_FLAP:-}" ]; then
    post /v1/slos "$(jq -nc --arg m "$MON_FLAP" '{
      name:"flapping uptime 99.99", sli_kind:"monitor", monitor_id:$m,
      objective_pct:99.99, window_days:7, enabled:true }')" >/dev/null
    log "SLO: tight 99.99% on the flapping monitor (budget will burn for real)"
  fi
fi
