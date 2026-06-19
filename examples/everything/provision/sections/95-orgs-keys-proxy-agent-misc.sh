# 95-orgs-keys-proxy-agent-misc.sh — multi-org, proxy, remote agent, scheduled
# report, deploy markers, presets, templates (monitor + notification/Liquid),
# CSV import round-trip, and bulk ops.

# --- 2nd org + member + re-role + switch + non-member 404 -------------------
ORGS="$(get /v1/orgs)"
ORG2_ID="$(echo "$ORGS" | jq -r '.[]?|select(.slug=="demo-team").id' 2>/dev/null | head -1)"
if [ -z "$ORG2_ID" ]; then
  ORG2_ID="$(post /v1/orgs '{"slug":"demo-team","name":"Demo Team"}' | jq -r '.id // empty')"
  log "2nd org created: Demo Team ($ORG2_ID)"
fi
if [ -n "$ORG2_ID" ]; then
  # add a member (must be an existing user — register one first)
  post /v1/auth/register '{"email":"member@rampart.local","name":"Member","password":"Rampart-Member-1"}' >/dev/null 2>&1 || true
  post "/v1/orgs/$ORG2_ID/members" '{"email":"member@rampart.local","role":"editor"}' >/dev/null 2>&1 || true
  MEM_UID="$(get "/v1/orgs/$ORG2_ID/members" | jq -r '.[]?|select(.email=="member@rampart.local").user_id // .[]?|select(.email=="member@rampart.local").id' 2>/dev/null | head -1)"
  if [ -n "$MEM_UID" ]; then
    patch "/v1/orgs/$ORG2_ID/members/$MEM_UID" '{"role":"readonly"}' >/dev/null 2>&1 || true
    log "member added to Demo Team, then re-roled editor→readonly"
  fi
  # switch active org → works since P4 shipped (cookie session)
  curl -s -b "$CJ" -c "$CJ" -X POST "$API/v1/orgs/$ORG2_ID/switch" >/dev/null 2>&1
  log "switched active org to Demo Team"

  # --- populate Demo Team as a REAL second tenant -------------------------
  # While switched IN, writes are org-stamped to Demo Team (Phase 4). These
  # monitors + telemetry are isolated from Default — and with RAMPART_RLS=1
  # that isolation is ENFORCED at the Postgres layer (defense-in-depth on top
  # of the app-level scoping). ensure_monitor checks the CURRENT org's list,
  # so re-runs are idempotent per-org.
  m_dt1="$(ensure_monitor 'demo-team · web' \
    '{"name":"demo-team · web","kind":"http","url":"http://target-healthy/","interval_seconds":30}')"
  m_dt2="$(ensure_monitor 'demo-team · api ready' \
    '{"name":"demo-team · api ready","kind":"http","url":"http://target-healthy/","interval_seconds":30}')"
  m_dt3="$(ensure_monitor 'demo-team · keyword' \
    '{"name":"demo-team · keyword","kind":"keyword","url":"http://target-healthy/","config":{"keyword":"Welcome"},"interval_seconds":60}')"
  log "Demo Team monitors created (demo-team · web / api ready / keyword)"

  # Mint a per-org ingest key (returns the raw token ONCE) so telemetry sent
  # with it lands in Demo Team — proving per-org telemetry isolation.
  DT_KEY="$(post /v1/ingest-keys '{"label":"demo-team ingest"}' | jq -r '.token // empty')"
  if [ -n "$DT_KEY" ]; then
    log "Demo Team ingest key minted (label='demo-team ingest')"
    # Push a couple of REAL logs via the key (OTLP/JSON → /otlp/v1/logs,
    # Bearer-authenticated). resolve_ingest_org maps the key → Demo Team.
    NOW_NS="$(date -u +%s)000000000"
    DT_LOGS="$(jq -nc --arg ts "$NOW_NS" '{
      resourceLogs:[{
        resource:{attributes:[{key:"service.name",value:{stringValue:"demo-team-svc"}}]},
        scopeLogs:[{logRecords:[
          {timeUnixNano:$ts,severityNumber:9,severityText:"INFO",
           body:{stringValue:"demo-team checkout completed"}},
          {timeUnixNano:$ts,severityNumber:17,severityText:"ERROR",
           body:{stringValue:"demo-team payment gateway timeout"}}
        ]}]}]}')"
    LCODE="$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/otlp/v1/logs" \
      -H "authorization: Bearer $DT_KEY" -H 'content-type: application/json' \
      --data-binary "$DT_LOGS")"
    log "Demo Team logs pushed via ingest key → HTTP $LCODE (expected 200)"
    # A few metric samples too. /v1/metrics/ingest is session-authed and files
    # under the ACTIVE org — we're still switched into Demo Team, so these land
    # there as well (a second, independent proof of per-org telemetry).
    DT_METRICS="$(printf 'demo_team_queue_depth{service="demo-team-svc"} 7\ndemo_team_orders_total{service="demo-team-svc"} 42\n')"
    MCODE="$(curl -s -b "$CJ" -o /dev/null -w '%{http_code}' -H 'content-type: text/plain' \
      -X POST "$API/v1/metrics/ingest" --data-binary "$DT_METRICS")"
    log "Demo Team metrics pushed (active-org session) → HTTP $MCODE (expected 200)"
  else
    log "WARN: could not mint Demo Team ingest key (telemetry isolation demo skipped)"
  fi

  # non-member org → 404 (probe a random uuid)
  CODE="$(curl -s -b "$CJ" -o /dev/null -w '%{http_code}' "$API/v1/orgs/00000000-0000-0000-0000-000000000000")"
  log "non-member org GET returned HTTP $CODE (expected 404)"
  # switch BACK to the default org so the rest of provisioning lands there
  DEF_ID="$(get /v1/orgs | jq -r 'map(select(.slug=="default")) | (.[0].id // empty)' 2>/dev/null | head -1)"
  [ -z "$DEF_ID" ] && DEF_ID="$(get /v1/orgs | jq -r '.[0].id // empty' 2>/dev/null | head -1)"
  [ -n "$DEF_ID" ] && curl -s -b "$CJ" -c "$CJ" -X POST "$API/v1/orgs/$DEF_ID/switch" >/dev/null 2>&1
  log "switched back to the default org"
fi

# --- proxy + a monitor that routes through it -------------------------------
PROXIES="$(get /v1/proxies)"
PROXY_ID="$(echo "$PROXIES" | jq -r '.[]?|select(.host=="proxy.demo.local").id' 2>/dev/null | head -1)"
if [ -z "$PROXY_ID" ]; then
  PROXY_ID="$(post /v1/proxies '{"protocol":"http","host":"proxy.demo.local","port":3128,"active":true}' | jq -r '.id // empty')"
  log "proxy created: http://proxy.demo.local:3128"
fi
if [ -n "$PROXY_ID" ]; then
  m_proxy="$(ensure_monitor 'edge · via proxy' "{\"name\":\"edge · via proxy\",\"kind\":\"http\",\"url\":\"http://target-healthy/\",\"proxy_id\":\"$PROXY_ID\",\"interval_seconds\":60,\"group_id\":\"$GRP_EDGE\"}")"
  log "monitor 'edge · via proxy' routes through the proxy"
fi

# --- remote agent register (capture rmpa_ token for the agent service) ------
AGENTS="$(get /v1/agents)"
AG_ID="$(echo "$AGENTS" | jq -r '.[]?|select(.name=="demo-remote-agent").id' 2>/dev/null | head -1)"
if [ -z "$AG_ID" ]; then
  AG_JSON="$(post /v1/agents '{"name":"demo-remote-agent","location":"in-network · isolated segment"}')"
  AG_ID="$(echo "$AG_JSON" | jq -r '.agent.id // .id // empty')"
  AG_TOK="$(echo "$AG_JSON" | jq -r '.token // empty')"
  if [ -n "$AG_TOK" ]; then
    { echo "RAMPART_URL=${API}"; echo "RAMPART_AGENT_TOKEN=${AG_TOK}"; } > "$SHARED/agent.env"
    log "remote agent registered; rmpa_ token → $SHARED/agent.env"
  fi
fi
# assign the private-only monitor to the agent so it probes from the agent
if [ -n "$AG_ID" ] && [ -n "${MON_AGENT:-}" ]; then
  patch "/v1/monitors/$MON_AGENT" "{\"agent_id\":\"$AG_ID\"}" >/dev/null 2>&1 || true
  log "assigned 'agent · private-only target' to the remote agent"
fi

# --- scheduled report -------------------------------------------------------
SR="$(get /v1/scheduled-reports)"
echo "$SR" | jq -e '.[]?|select(.name=="Weekly Uptime")' >/dev/null 2>&1 || \
  post /v1/scheduled-reports '{"name":"Weekly Uptime","recipients":["ops@rampart.local"],"cadence":"weekly"}' >/dev/null
log "scheduled report created (Weekly Uptime)"

# --- deploy markers ---------------------------------------------------------
post /v1/deploy-markers '{"title":"Deploy demo-backend v1.0.0","description":"initial rollout","service":"demo-backend"}' >/dev/null 2>&1 || true
post /v1/deploy-markers '{"title":"Config change","description":"raised checkout timeout","service":"demo-backend"}' >/dev/null 2>&1 || true
log "deploy markers created"

# --- monitor presets --------------------------------------------------------
PRE="$(get /v1/monitors/presets)"
echo "$PRE" | jq -e '.[]?|select(.name=="std headers")' >/dev/null 2>&1 || \
  post /v1/monitors/presets '{"name":"std headers","kind":"http_headers","data":{"headers":{"User-Agent":"Rampart-Probe","X-Demo":"1"}}}' >/dev/null
echo "$PRE" | jq -e '.[]?|select(.name=="lax tls")' >/dev/null 2>&1 || \
  post /v1/monitors/presets '{"name":"lax tls","kind":"tls","data":{"ignore_tls":true}}' >/dev/null
log "monitor presets created (http_headers, tls)"

# --- monitor template + instantiate -----------------------------------------
MT="$(get /v1/monitor-templates)"
MT_ID="$(id_by_name "$MT" 'standard http')"
if [ -z "$MT_ID" ]; then
  MT_ID="$(post /v1/monitor-templates '{"name":"standard http","spec":{"name":"templated http","kind":"http","url":"http://target-healthy/","interval_seconds":60}}' | jq -r '.id // empty')"
fi
if [ -n "$MT_ID" ]; then
  post "/v1/monitor-templates/$MT_ID/instantiate" '{"name":"edge · from template"}' >/dev/null 2>&1 || true
  log "monitor template created + instantiated"
fi

# --- notification (Liquid) template + attach to a channel -------------------
NT="$(get /v1/notification-templates)"
NT_ID="$(id_by_name "$NT" 'Down alert (Liquid)')"
if [ -z "$NT_ID" ]; then
  NT_ID="$(post /v1/notification-templates "$(jq -nc '{
    name:"Down alert (Liquid)", channel_kinds:["webhook","email"], event_kind:"monitor_down",
    subject_template:"[{{ monitor.name }}] is {{ status }}",
    body_template:"{{ monitor.name }} went {{ status }} at {{ checked_at }}. URL: {{ monitor.url }}" }')" | jq -r '.id // empty')"
fi
if [ -n "$NT_ID" ]; then
  CH_WEBHOOK="$(id_by_name "$(get /v1/notifications)" 'webhook → sink')"
  [ -n "$CH_WEBHOOK" ] && patch "/v1/notifications/$CH_WEBHOOK" "{\"template_id\":\"$NT_ID\"}" >/dev/null 2>&1
  log "Liquid notification template created + attached to the webhook channel"
fi

# --- CSV import round-trip --------------------------------------------------
# export current monitors, then import a tiny CSV (idempotent: name-guarded).
if ! echo "$(get /v1/monitors)" | jq -e '(if type=="array" then .[] else (.items[]?) end)|select(.name=="csv · imported")' >/dev/null 2>&1; then
  CSV='name,kind,url,interval_seconds
csv · imported,http,http://target-healthy/,60'
  post_text /v1/monitors/import-csv "text/csv" "$CSV" >/dev/null 2>&1 || true
  log "CSV import round-trip (created 'csv · imported')"
fi
get /v1/monitors/export >/dev/null 2>&1 && log "monitor export verified (GET /v1/monitors/export)"

# --- bulk ops: pause then resume the heavy folder's monitors ----------------
HEAVY_IDS="$(get /v1/monitors | jq -c '[ (if type=="array" then .[] else (.items[]?) end) | select(.name|test("^heavy ·")) | .id ]')"
if [ "$(echo "$HEAVY_IDS" | jq 'length')" -gt 0 ] 2>/dev/null; then
  post /v1/monitors/bulk "$(jq -nc --argjson ids "$HEAVY_IDS" '{monitor_ids:$ids, action:"pause"}')" >/dev/null 2>&1 || true
  post /v1/monitors/bulk "$(jq -nc --argjson ids "$HEAVY_IDS" '{monitor_ids:$ids, action:"resume"}')" >/dev/null 2>&1 || true
  log "bulk ops: paused then resumed the heavy-folder monitors"
fi
