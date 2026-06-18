# 40-monitors.sh — one monitor of EVERY one of the 42 MonitorKinds, against the
# real in-network targets. Common kinds probe live default services; heavy/exotic
# kinds probe services that only exist under `docker compose --profile heavy`
# (their monitors simply show Down until you bring the profile up — that's real
# state, not a fixture). Egress kinds (domain WHOIS, rdap, doh, steam, snmp)
# need internet or a stub and are clearly named.
#
# Special cases wired here:
#   - a FLAPPING http monitor on the demo backend's /api/ready (real Down/Up)
#   - an http with check_cert + cert_expiry_days=14 against the ~10d TLS cert → Warn
#   - a tls monitor against the EXPIRED cert → Down
#   - a push monitor (token captured → push-cron flips it Down for real)
#   - a synthetic 2-step login→whoami flow
#   - a remote-agent-only target (private), assigned to the agent below

m() {  # m <name> <json-body>  → ensures + echoes id
  ensure_monitor "$1" "$2"
}

# --- common / default-profile monitors --------------------------------------

# Always-healthy edge target
m "edge · healthy nginx" \
  "{\"name\":\"edge · healthy nginx\",\"kind\":\"http\",\"url\":\"http://target-healthy/\",\"interval_seconds\":20,\"group_id\":\"$GRP_EDGE\"}" >/dev/null

# FLAPPING http → real Down/Up cycles, drives episode/escalation/on-call/SLO.
export MON_FLAP="$(m 'edge · flapping ready probe' \
  "{\"name\":\"edge · flapping ready probe\",\"kind\":\"http\",\"url\":\"http://demo-backend:8080/api/ready\",\"interval_seconds\":15,\"max_retries\":1,\"group_id\":\"$GRP_EDGE\",\"slo_target_pct\":99.0,\"slo_window_days\":7}")"

# Also a genuinely-flapping target (503 for 25s/min) so even without UI toggles
# there's a real outage every minute.
export MON_FLAKY="$(m 'edge · flaky target (503/min)' \
  "{\"name\":\"edge · flaky target (503/min)\",\"kind\":\"http\",\"url\":\"http://target-flaky:8080/\",\"interval_seconds\":15,\"group_id\":\"$GRP_EDGE\"}")"

# keyword: asserts text in the /welcome page
m "edge · keyword (welcome)" \
  "{\"name\":\"edge · keyword (welcome)\",\"kind\":\"keyword\",\"url\":\"http://demo-backend:8080/welcome\",\"config\":{\"keyword\":\"operational\"},\"interval_seconds\":30,\"group_id\":\"$GRP_EDGE\"}" >/dev/null

# json_query: asserts $.status == operational
m "edge · json_query (status)" \
  "{\"name\":\"edge · json_query (status)\",\"kind\":\"json_query\",\"url\":\"http://demo-backend:8080/status.json\",\"config\":{\"json_path\":\"status\",\"expected_value\":\"operational\"},\"interval_seconds\":30,\"group_id\":\"$GRP_EDGE\"}" >/dev/null

# http + check_cert against the ~10-day cert → Warn (days-left < 14).
m "edge · cert-expiry warn" \
  "{\"name\":\"edge · cert-expiry warn\",\"kind\":\"http\",\"url\":\"https://tls-target:8443/\",\"ignore_tls\":false,\"check_cert\":true,\"cert_expiry_days\":14,\"interval_seconds\":60,\"group_id\":\"$GRP_EDGE\"}" >/dev/null

# tls against the EXPIRED cert → Down.
m "edge · tls expired (down)" \
  "{\"name\":\"edge · tls expired (down)\",\"kind\":\"tls\",\"hostname\":\"tls-target\",\"config\":{\"port\":9443,\"warn_days\":14},\"interval_seconds\":60,\"group_id\":\"$GRP_EDGE\"}" >/dev/null

# tls healthy (the warn cert is still valid → Up, just warns soon)
m "edge · tls healthy" \
  "{\"name\":\"edge · tls healthy\",\"kind\":\"tls\",\"hostname\":\"tls-target\",\"config\":{\"port\":8443,\"warn_days\":3},\"interval_seconds\":60,\"group_id\":\"$GRP_EDGE\"}" >/dev/null

# push: server mints a token; push-cron sends real heartbeats + a Down flip.
export MON_PUSH_ID="$(m 'jobs · nightly backup (push)' \
  "{\"name\":\"jobs · nightly backup (push)\",\"kind\":\"push\",\"interval_seconds\":120,\"group_id\":\"$GRP_CORE\"}")"

# synthetic: 2-step login → authenticated fetch with a var extraction.
m "core · synthetic login flow" "$(jq -nc --arg g "$GRP_CORE" '{
  name:"core · synthetic login flow", kind:"synthetic", interval_seconds:60, group_id:$g,
  config:{ steps:[
    { name:"login", method:"POST", url:"http://demo-backend:8080/auth/token",
      headers:{"content-type":"application/json"}, body:"{\"user\":\"svc\"}",
      extract:[{var:"tok", from:"json", path:"token"}],
      assert:[{kind:"status", op:"eq", value:"200"}] },
    { name:"whoami", method:"GET", url:"http://demo-backend:8080/auth/whoami",
      headers:{"authorization":"Bearer {{tok}}"},
      assert:[{kind:"json", path:"ok", op:"eq", value:"true"}] }
  ]}}')" >/dev/null

# tcp / ping / dns against in-network + public-ish targets
m "core · postgres tcp"  "{\"name\":\"core · postgres tcp\",\"kind\":\"tcp\",\"hostname\":\"rampart-db\",\"port\":5432,\"interval_seconds\":30,\"group_id\":\"$GRP_DATASTORES\"}" >/dev/null
m "net · ping gateway"   "{\"name\":\"net · ping gateway\",\"kind\":\"ping\",\"hostname\":\"target-healthy\",\"interval_seconds\":30,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null
m "net · dns A record"   "{\"name\":\"net · dns A record\",\"kind\":\"dns\",\"hostname\":\"target-healthy\",\"config\":{\"record_type\":\"A\"},\"interval_seconds\":60,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null

# datastores (default profile ships postgres + redis the demo-app uses)
m "data · postgres"  "{\"name\":\"data · postgres\",\"kind\":\"postgres\",\"hostname\":\"demo-db\",\"port\":5432,\"config\":{\"user\":\"demo\",\"password\":\"demo\",\"database\":\"demo\"},\"interval_seconds\":30,\"group_id\":\"$GRP_DATASTORES\"}" >/dev/null
m "data · redis"     "{\"name\":\"data · redis\",\"kind\":\"redis\",\"hostname\":\"demo-redis\",\"port\":6379,\"interval_seconds\":30,\"group_id\":\"$GRP_DATASTORES\"}" >/dev/null

# grpc / websocket / banner protocols against the demo backend / mailpit
m "net · websocket"  "{\"name\":\"net · websocket\",\"kind\":\"websocket\",\"url\":\"ws://demo-backend:8080/\",\"interval_seconds\":60,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null
m "net · smtp banner (mailpit)" "{\"name\":\"net · smtp banner (mailpit)\",\"kind\":\"smtp\",\"hostname\":\"mailpit\",\"port\":1025,\"interval_seconds\":60,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null

# docker: probe a container via the mounted socket (compose mounts it ro).
m "infra · docker container" "{\"name\":\"infra · docker container\",\"kind\":\"docker\",\"hostname\":\"unix:///var/run/docker.sock\",\"config\":{\"container\":\"rampart-everything-target-healthy\",\"host\":\"unix:///var/run/docker.sock\"},\"interval_seconds\":60,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null

# --- the remaining kinds (heavy targets behind --profile heavy) -------------
# These probe service names that exist only when `--profile heavy` is up. Until
# then they show Down (genuine), demonstrating every remaining kind in the UI.
m "heavy · mysql"        "{\"name\":\"heavy · mysql\",\"kind\":\"mysql\",\"hostname\":\"mysql\",\"port\":3306,\"config\":{\"user\":\"demo\",\"password\":\"demo\",\"database\":\"demo\"},\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · mssql"        "{\"name\":\"heavy · mssql\",\"kind\":\"mssql\",\"hostname\":\"mssql\",\"port\":1433,\"config\":{\"user\":\"sa\",\"password\":\"Rampart-Demo-9271\",\"database\":\"master\",\"trust_cert\":true},\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · mongodb"      "{\"name\":\"heavy · mongodb\",\"kind\":\"mongodb\",\"hostname\":\"mongo\",\"port\":27017,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · elasticsearch" "{\"name\":\"heavy · elasticsearch\",\"kind\":\"elasticsearch\",\"url\":\"http://elasticsearch:9200\",\"config\":{\"allow_yellow\":true},\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · vault"        "{\"name\":\"heavy · vault\",\"kind\":\"vault\",\"url\":\"http://vault:8200\",\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · etcd"         "{\"name\":\"heavy · etcd\",\"kind\":\"etcd\",\"url\":\"http://etcd:2379\",\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · kafka (redpanda)" "{\"name\":\"heavy · kafka (redpanda)\",\"kind\":\"kafka\",\"hostname\":\"redpanda\",\"port\":9092,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · cassandra"    "{\"name\":\"heavy · cassandra\",\"kind\":\"cassandra\",\"hostname\":\"cassandra\",\"port\":9042,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · ldap"         "{\"name\":\"heavy · ldap\",\"kind\":\"ldap\",\"url\":\"ldap://openldap:389\",\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · amqp (rabbit)" "{\"name\":\"heavy · amqp (rabbit)\",\"kind\":\"amqp\",\"url\":\"amqp://guest:guest@rabbitmq:5672/\",\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · nats"         "{\"name\":\"heavy · nats\",\"kind\":\"nats\",\"url\":\"nats://nats:4222\",\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · memcached"    "{\"name\":\"heavy · memcached\",\"kind\":\"memcached\",\"hostname\":\"memcached\",\"port\":11211,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · mqtt"         "{\"name\":\"heavy · mqtt\",\"kind\":\"mqtt\",\"hostname\":\"mosquitto\",\"port\":1883,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · radius"       "{\"name\":\"heavy · radius\",\"kind\":\"radius\",\"hostname\":\"freeradius\",\"config\":{\"username\":\"demo\",\"password\":\"demo\",\"secret\":\"testing123\"},\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · ntp"          "{\"name\":\"heavy · ntp\",\"kind\":\"ntp\",\"hostname\":\"ntp\",\"interval_seconds\":120,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · snmp"         "{\"name\":\"heavy · snmp\",\"kind\":\"snmp\",\"hostname\":\"snmp\",\"config\":{\"community\":\"public\",\"oid\":\"1.3.6.1.2.1.1.1.0\"},\"interval_seconds\":120,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · ssh banner"   "{\"name\":\"heavy · ssh banner\",\"kind\":\"ssh\",\"hostname\":\"openssh\",\"port\":2222,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · ftp banner"   "{\"name\":\"heavy · ftp banner\",\"kind\":\"ftp\",\"hostname\":\"ftp\",\"port\":21,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · imap banner"  "{\"name\":\"heavy · imap banner\",\"kind\":\"imap\",\"hostname\":\"dovecot\",\"port\":143,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · pop3 banner"  "{\"name\":\"heavy · pop3 banner\",\"kind\":\"pop3\",\"hostname\":\"dovecot\",\"port\":110,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "heavy · grpc health"  "{\"name\":\"heavy · grpc health\",\"kind\":\"grpc\",\"hostname\":\"grpcbin\",\"port\":9000,\"interval_seconds\":60,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "lan · mdns"           "{\"name\":\"lan · mdns\",\"kind\":\"mdns\",\"hostname\":\"mdns.local\",\"interval_seconds\":120,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null
m "lan · ssdp"           "{\"name\":\"lan · ssdp\",\"kind\":\"ssdp\",\"hostname\":\"239.255.255.250\",\"interval_seconds\":120,\"group_id\":\"$GRP_HEAVY\"}" >/dev/null

# --- egress / internet kinds (need outbound internet; clearly named) --------
m "egress · domain WHOIS (needs internet)" "{\"name\":\"egress · domain WHOIS (needs internet)\",\"kind\":\"domain\",\"hostname\":\"example.com\",\"config\":{\"warn_days\":30},\"interval_seconds\":3600,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null
m "egress · rdap (needs internet)"  "{\"name\":\"egress · rdap (needs internet)\",\"kind\":\"rdap\",\"url\":\"https://rdap.org/\",\"config\":{\"domain\":\"example.com\",\"warn_days\":30},\"interval_seconds\":3600,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null
m "egress · doh (needs internet)"   "{\"name\":\"egress · doh (needs internet)\",\"kind\":\"doh\",\"url\":\"https://cloudflare-dns.com/dns-query\",\"config\":{\"query\":\"example.com\",\"rtype\":\"A\"},\"interval_seconds\":300,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null
m "egress · steam (needs internet)" "{\"name\":\"egress · steam (needs internet)\",\"kind\":\"steam\",\"hostname\":\"208.78.164.10\",\"interval_seconds\":300,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null
m "egress · browser (needs renderer)" "{\"name\":\"egress · browser (needs renderer)\",\"kind\":\"browser\",\"url\":\"http://demo-backend:8080/welcome\",\"config\":{\"renderer_url\":\"http://renderer:3000\",\"keyword\":\"operational\"},\"interval_seconds\":300,\"group_id\":\"$GRP_NETWORK\"}" >/dev/null

# --- a private-only target the REMOTE AGENT probes --------------------------
# Reachable only from inside the network on a separate name; we assign it to the
# agent below so the probe genuinely comes from the agent, not the server.
export MON_AGENT="$(m 'agent · private-only target' \
  "{\"name\":\"agent · private-only target\",\"kind\":\"http\",\"url\":\"http://agent-target/\",\"interval_seconds\":30,\"group_id\":\"$GRP_NETWORK\"}")"

# --- capture the push token for the push-cron -------------------------------
if [ -n "${MON_PUSH_ID:-}" ]; then
  PTOK="$(get "/v1/monitors/$MON_PUSH_ID" | jq -r '.push_token // empty')"
  if [ -z "$PTOK" ]; then
    PTOK="$(post "/v1/monitors/$MON_PUSH_ID/regenerate-push-token" '{}' | jq -r '.push_token // empty')"
  fi
  if [ -n "$PTOK" ]; then
    { echo "RAMPART_URL=${API}"; echo "PUSH_TOKEN=${PTOK}"; } > "$SHARED/push.env"
    log "captured push token → $SHARED/push.env"
  else
    log "WARN: could not capture push token"
  fi
fi

# --- attach tags to monitors (for tag-routing in the next section) ----------
attach_tag() {  # $1=monitor-name $2=tag-id
  local mid; mid="$(id_by_name "$(get /v1/monitors)" "$1")"
  [ -n "$mid" ] && [ -n "$2" ] && curl -s -b "$CJ" -X POST "$API/v1/monitors/$mid/tags/$2" >/dev/null 2>&1
}
attach_tag "edge · flapping ready probe" "$TAG_CRITICAL"
attach_tag "edge · flapping ready probe" "$TAG_EDGE"
attach_tag "edge · flaky target (503/min)" "$TAG_EDGE"
attach_tag "data · postgres" "$TAG_DATA"
attach_tag "data · redis"    "$TAG_DATA"
log "monitors created for all 42 kinds; tags attached"

# --- monitor dependencies: edge depends on postgres -------------------------
MID_EDGE="$(id_by_name "$(get /v1/monitors)" 'edge · healthy nginx')"
MID_PG="$(id_by_name "$(get /v1/monitors)" 'data · postgres')"
if [ -n "$MID_EDGE" ] && [ -n "$MID_PG" ]; then
  curl -s -b "$CJ" -X POST "$API/v1/monitors/$MID_EDGE/dependencies/$MID_PG" >/dev/null 2>&1
  log "dependency: 'edge · healthy nginx' depends on 'data · postgres'"
fi

# persist a couple ids for later sections
{ echo "MON_FLAP=$MON_FLAP MON_FLAKY=$MON_FLAKY MON_AGENT=$MON_AGENT MON_PUSH_ID=$MON_PUSH_ID"; } >> "$SHARED/ids.env"
