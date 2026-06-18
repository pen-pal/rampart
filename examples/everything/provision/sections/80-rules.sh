# 80-rules.sh — detection (SIEM), telemetry, and metric rules. These fire from
# the REAL logs/traces/metrics the demo-app + metrics-pusher emit.

CH="$(get /v1/notifications)"
CH_WEBHOOK="$(id_by_name "$CH" 'webhook → sink')"

# --- detection rule: the backend's repeated "failed login" logs -------------
DR="$(get /v1/detection-rules)"
if ! echo "$DR" | jq -e '(if type=="array" then .[] else (.items[]?) end)|select(.name=="demo · repeated failed logins")' >/dev/null 2>&1; then
  post /v1/detection-rules "$(jq -nc --arg c "$CH_WEBHOOK" '{
    name:"demo · repeated failed logins", severity:"high", service:"demo-backend",
    min_level:13, body_regex:"failed login", threshold:3, window_seconds:600,
    cooldown_seconds:120, enabled:true,
    channel_ids: ($c|if .=="" then [] else [.] end) }')" >/dev/null
  log "detection rule: failed-login SIEM rule (low cooldown so it refires)"
fi

# --- telemetry rules: error rate + trace latency + log volume ---------------
TR="$(get /v1/telemetry-rules)"
mk_tr() {  # $1=name $2=body-json
  echo "$TR" | jq -e --arg n "$1" '(if type=="array" then .[] else (.items[]?) end)|select(.name==$n)' >/dev/null 2>&1 && return
  post /v1/telemetry-rules "$2" >/dev/null
}
mk_tr "demo · error spike" "$(jq -nc --arg c "$CH_WEBHOOK" '{
  name:"demo · error spike", kind:"error_rate", target:"demo-backend", op:"gt",
  threshold:1.0, window_seconds:300, for_seconds:0, enabled:true,
  channel_ids: ($c|if .=="" then [] else [.] end) }')"
mk_tr "demo · slow traces" '{"name":"demo · slow traces","kind":"trace_latency","target":"demo-backend","op":"gt","threshold":250.0,"window_seconds":300,"for_seconds":0,"enabled":true}'
mk_tr "demo · log volume"  '{"name":"demo · log volume","kind":"log_volume","target":"demo-backend","op":"gt","threshold":10.0,"window_seconds":300,"for_seconds":0,"enabled":true}'
log "telemetry rules: error_rate, trace_latency, log_volume"

# --- metric rule: breached by the pushed demo_queue_depth series ------------
# metrics-pusher pushes demo_queue_depth{service="demo-backend"} ~30-120; this
# rule fires > 40 → real metric alert.
MR="$(get /v1/metrics/rules)"
if ! echo "$MR" | jq -e '(if type=="array" then .[] else (.items[]?) end)|select(.name=="demo · queue too deep")' >/dev/null 2>&1; then
  post /v1/metrics/rules "$(jq -nc --arg c "$CH_WEBHOOK" '{
    name:"demo · queue too deep", metric:"demo_queue_depth", labels:{service:"demo-backend"},
    op:"gt", threshold:40.0, for_seconds:30, enabled:true,
    channel_ids: ($c|if .=="" then [] else [.] end) }')" >/dev/null
  log "metric rule: demo_queue_depth > 40 (breached by the pushed series)"
fi
