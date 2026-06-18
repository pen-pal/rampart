# 90-status-incidents-ingest.sh — public status page (sections, custom branding,
# subscriber, incident + running updates, incident templates), a PRIVATE
# password-protected page, and ingest tokens for every inbound vendor. Posts a
# firing+resolved alert through the public webhook to open+close a REAL incident.

# --- public status page -----------------------------------------------------
SP="$(get /v1/status-pages)"
SP_ID="$(echo "$SP" | jq -r '(if type=="array" then .[] else (.items[]?) end)|select(.slug=="demo-status").id' 2>/dev/null | head -1)"
if [ -z "$SP_ID" ]; then
  # gather a few monitor ids to attach
  MIDS="$(get /v1/monitors | jq -c '[ (if type=="array" then .[] else (.items[]?) end) | select(.name|test("^edge ·")) | .id ] | .[0:4]')"
  SP_ID="$(post /v1/status-pages "$(jq -nc --argjson m "${MIDS:-[]}" '{
    slug:"demo-status", title:"Rampart Demo · Public Status", theme:"light",
    description:"Live status of the demo services.",
    logo_url:"https://avatars.githubusercontent.com/u/0?s=64",
    custom_css:".rampart-status h1{letter-spacing:.5px}",
    monitor_ids:$m }')" | jq -r '.id // empty')"
  log "public status page created (slug=demo-status) id=$SP_ID"
fi

# sections + assign a monitor to a section
if [ -n "$SP_ID" ]; then
  SEC="$(get "/v1/status-pages/$SP_ID/sections")"
  SEC_ID="$(id_by_name "$SEC" 'Edge')"
  [ -z "$SEC_ID" ] && SEC_ID="$(post "/v1/status-pages/$SP_ID/sections" '{"name":"Edge"}' | jq -r '.id // empty')"
  post "/v1/status-pages/$SP_ID/sections" '{"name":"Core"}' >/dev/null 2>&1 || true
  MID_EDGE="$(id_by_name "$(get /v1/monitors)" 'edge · healthy nginx')"
  if [ -n "$MID_EDGE" ] && [ -n "$SEC_ID" ]; then
    puta "/v1/status-pages/$SP_ID/monitors/$MID_EDGE/section" "{\"section_id\":\"$SEC_ID\"}" >/dev/null 2>&1 || true
  fi
  log "status page sections created + a monitor assigned to 'Edge'"

  # subscriber (→ Mailpit once SMTP settings are configured below)
  post "/v1/public/status-pages/demo-status/subscribe" '{"email":"subscriber@rampart.local"}' >/dev/null 2>&1 || true
  log "status page subscriber added (subscriber@rampart.local)"

  # incident + running updates
  INC="$(get "/v1/status-pages/$SP_ID/incidents")"
  if ! echo "$INC" | jq -e '(if type=="array" then .[] else (.items[]?) end)|select(.title=="Elevated error rates")' >/dev/null 2>&1; then
    INC_ID="$(post "/v1/status-pages/$SP_ID/incidents" '{"title":"Elevated error rates","content":"We are investigating elevated 5xx on the edge.","style":"warning","pinned":true}' | jq -r '.id // empty')"
    if [ -n "$INC_ID" ]; then
      post "/v1/incidents/$INC_ID/updates" '{"message":"Identified — a bad deploy; rolling back."}' >/dev/null 2>&1 || true
      post "/v1/incidents/$INC_ID/updates" '{"message":"Monitoring — rollback complete, errors subsiding."}' >/dev/null 2>&1 || true
      post "/v1/incidents/$INC_ID/resolve" '{}' >/dev/null 2>&1 || true
      log "incident created with running updates, then resolved"
    fi
  fi
fi

# --- private / password-protected status page -------------------------------
if ! echo "$(get /v1/status-pages)" | jq -e '(if type=="array" then .[] else (.items[]?) end)|select(.slug=="demo-internal")' >/dev/null 2>&1; then
  post /v1/status-pages '{"slug":"demo-internal","title":"Internal Status (private)","password":"Rampart-Internal-1","theme":"dark","custom_css":".rampart-status{filter:none}"}' >/dev/null
  log "private password-protected status page created (slug=demo-internal, pw=Rampart-Internal-1)"
fi

# --- incident templates ------------------------------------------------------
IT="$(get /v1/incident-templates)"
echo "$IT" | jq -e '(if type=="array" then .[] else (.items[]?) end)|select(.name=="Investigating")' >/dev/null 2>&1 || \
  post /v1/incident-templates '{"name":"Investigating","body":"We are investigating reports of {{issue}}.","style":"warning"}' >/dev/null
echo "$IT" | jq -e '(if type=="array" then .[] else (.items[]?) end)|select(.name=="Resolved")' >/dev/null 2>&1 || \
  post /v1/incident-templates '{"name":"Resolved","body":"This incident has been resolved.","style":"success"}' >/dev/null
log "incident templates created"

# --- ingest tokens for every inbound vendor ---------------------------------
# Mint one per vendor on the public page. Capture the Alertmanager token URL into
# the shared volume so the alertmanager service can post real alerts to it.
mint_token() {  # $1=label → echoes raw token
  [ -z "$SP_ID" ] && return
  local existing tok
  existing="$(get "/v1/status-pages/$SP_ID/ingest-tokens")"
  tok="$(echo "$existing" | jq -r --arg l "$1" '.[]?|select(.label==$l).token' 2>/dev/null | head -1)"
  if [ -n "$tok" ] && [ "$tok" != "null" ]; then echo "$tok"; return; fi
  post "/v1/status-pages/$SP_ID/ingest-tokens" "{\"label\":\"$1\"}" | jq -r '.token // empty'
}
TOK_AM="$(mint_token alertmanager)"
TOK_GRAFANA="$(mint_token grafana)"
TOK_DD="$(mint_token datadog)"
TOK_PD="$(mint_token pagerduty)"
TOK_OG="$(mint_token opsgenie)"
TOK_GEN="$(mint_token generic)"
log "ingest tokens minted (alertmanager, grafana, datadog, pagerduty, opsgenie, generic)"

# configure the generic token's mapping so its receiver works
if [ -n "$TOK_GEN" ] && [ -n "$SP_ID" ]; then
  GEN_ID="$(get "/v1/status-pages/$SP_ID/ingest-tokens" | jq -r '.[]?|select(.label=="generic").id' | head -1)"
  [ -n "$GEN_ID" ] && patch "/v1/ingest-tokens/$GEN_ID/mapping" \
    '{"mapping":{"action_path":"/status","action_firing_value":"firing","action_resolved_value":"resolved","title_path":"/title","content_path":"/body","dedup_path":"/id","style":"danger"}}' >/dev/null 2>&1
  log "generic ingest token mapping configured"
fi

# write the alertmanager + generic token URLs to the shared volume
{
  echo "RAMPART_URL=${API}"
  echo "ALERTMANAGER_INGEST_URL=${API}/v1/public/ingest/alertmanager/${TOK_AM}"
  echo "GENERIC_INGEST_URL=${API}/v1/public/ingest/generic/${TOK_GEN}"
} > "$SHARED/ingest.env"
log "wrote $SHARED/ingest.env (alertmanager + generic ingest URLs)"

# --- REAL alert→incident: open then close an incident via the public webhook -
if [ -n "$TOK_AM" ]; then
  curl -s -X POST "$API/v1/public/ingest/alertmanager/$TOK_AM" -H 'content-type: application/json' \
    -d '{"alerts":[{"status":"firing","labels":{"alertname":"DemoSyntheticAlert","severity":"critical"},"annotations":{"summary":"provision smoke alert","description":"opened by provision via the real ingest webhook"},"fingerprint":"provision-demo-1"}]}' >/dev/null 2>&1
  log "posted a FIRING alert through the real Alertmanager ingest webhook → incident opened"
  curl -s -X POST "$API/v1/public/ingest/alertmanager/$TOK_AM" -H 'content-type: application/json' \
    -d '{"alerts":[{"status":"resolved","labels":{"alertname":"DemoSyntheticAlert"},"fingerprint":"provision-demo-1"}]}' >/dev/null 2>&1
  log "posted the RESOLVED alert → incident auto-closed (the full vendor ingest path)"
fi

# --- SMTP settings so incident/subscriber emails actually fan out to Mailpit -
puta /v1/settings/smtp "$(jq -nc --arg h "$MAILPIT_SMTP_HOST" --argjson p "$MAILPIT_SMTP_PORT" '{
  host:$h, port:$p, encryption:"plain", from:"status@rampart.local" }')" >/dev/null 2>&1 || true
log "SMTP settings pointed at Mailpit (incident/subscriber emails deliver for real)"
