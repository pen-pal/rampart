# 10-secrets.sh — mint the runtime secrets real services consume, and write
# them to the shared volume. Captured: WRITE + ADMIN api-keys, an error-project
# DSN (built from its public_key + id), and an OTLP base for the app.
#
# api-key creation is admin-only; we hold an admin session. The raw token is in
# the create response's `.token` (never shown again) — capture it now.

# --- error-tracking project → DSN -------------------------------------------
EP_NAME="demo-backend"
EXISTING_EP="$(get /v1/error-projects)"
EP_ID="$(echo "$EXISTING_EP" | jq -r --arg n "$EP_NAME" '(if type=="array" then .[] else (.items[]?) end)|select(.name==$n).id' 2>/dev/null | head -1)"
if [ -z "$EP_ID" ]; then
  EP_JSON="$(post /v1/error-projects '{"name":"demo-backend","platform":"node"}')"
  EP_ID="$(echo "$EP_JSON" | jq -r '.id // empty')"
  EP_KEY="$(echo "$EP_JSON" | jq -r '.public_key // empty')"
else
  # public_key isn't re-listed; if we can't recover it, leave DSN unset (the app
  # still runs without Sentry). On a fresh stack the create branch always runs.
  EP_JSON="$(get "/v1/error-projects")"
  EP_KEY="$(echo "$EXISTING_EP" | jq -r --arg n "$EP_NAME" '(if type=="array" then .[] else (.items[]?) end)|select(.name==$n).public_key' 2>/dev/null | head -1)"
fi
# DSN that @sentry/node posts to: https://<public_key>@<host>/<project_id>.
# The app talks to the in-network rampart:3000 (no scheme/TLS issues over http).
if [ -n "$EP_ID" ] && [ -n "$EP_KEY" ] && [ "$EP_KEY" != "null" ]; then
  RAMPART_HOSTPORT="$(echo "$API" | sed -E 's#^https?://##')"
  DSN="http://${EP_KEY}@${RAMPART_HOSTPORT}/${EP_ID}"
  log "error-project '$EP_NAME' id=$EP_ID → DSN captured"
else
  DSN=""
  log "WARN: could not capture error-project DSN (app will run without Sentry)"
fi

# --- WRITE-scoped api-key (metrics push, rule edits) ------------------------
# Always mint a fresh one on a fresh stack; on re-run we can't recover the raw
# token, so we mint a uniquely-named one if the standard name is gone.
WKEY=""
if ! echo "$(get /v1/api-keys)" | jq -e '.[]?|select(.name=="demo-writer")' >/dev/null 2>&1; then
  WKEY="$(post /v1/api-keys '{"name":"demo-writer","scope":"write","rate_limit_per_hour":100000}' | jq -r '.token // empty')"
  [ -n "$WKEY" ] && log "WRITE api-key minted (demo-writer)"
fi

# --- ADMIN-scoped api-key (read tier shows all key scopes) ------------------
if ! echo "$(get /v1/api-keys)" | jq -e '.[]?|select(.name=="demo-admin-key")' >/dev/null 2>&1; then
  post /v1/api-keys '{"name":"demo-admin-key","scope":"admin","rate_limit_per_hour":100000}' >/dev/null
  log "ADMIN api-key minted (demo-admin-key)"
fi
# --- READ-scoped api-key ----------------------------------------------------
if ! echo "$(get /v1/api-keys)" | jq -e '.[]?|select(.name=="demo-read-key")' >/dev/null 2>&1; then
  post /v1/api-keys '{"name":"demo-read-key","scope":"read","rate_limit_per_hour":100000}' >/dev/null
  log "READ api-key minted (demo-read-key)"
fi

# --- write the app's env (sourced by demo-app/backend/entrypoint.sh) --------
{
  echo "# written by provision $(date -u +%FT%TZ)"
  echo "SENTRY_DSN=${DSN}"
  echo "RAMPART_OTLP=${API}/otlp"
  echo "RAMPART_PROFILES=${API}/profiles"
} > "$SHARED/demo.env"
log "wrote $SHARED/demo.env"

# --- write the WRITE key for the metrics-pusher cron ------------------------
if [ -n "$WKEY" ]; then
  { echo "RAMPART_WRITE_KEY=${WKEY}"; echo "RAMPART_URL=${API}"; } > "$SHARED/writer.env"
  log "wrote $SHARED/writer.env (WRITE api-key for metrics-pusher)"
else
  log "NOTE: WRITE key not minted this run (already existed) — metrics-pusher uses any prior key"
fi
