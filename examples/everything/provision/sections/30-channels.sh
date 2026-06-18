# 30-channels.sh — create ALL notification channel kinds as CONFIG so every one
# is visible in the UI. REAL delivery is wired only for sink-compatible kinds
# (configurable URL → webhook-sink, or SMTP → Mailpit); vendor-hardcoded kinds
# are created + labelled "needs real creds" and are NOT /test-ed (that would hit
# the real vendor and fail).
#
# Channel config shapes come from rampart-notifier/src/channels/<kind>.rs.

mk() {  # mk <kind> <name> <config-json>
  ensure_channel "$2" "$(jq -nc --arg k "$1" --arg n "$2" --argjson c "$3" '{kind:$k,name:$n,config:$c,active:true}')" >/dev/null
}

# =========================================================================
# A. SINK-COMPATIBLE — point at the local webhook-sink (real deliveries).
#    These have a configurable URL with no vendor host-pinning.
# =========================================================================
mk webhook         "webhook → sink"          "{\"url\":\"$SINK/webhook\",\"method\":\"POST\"}"
mk mattermost      "mattermost → sink"        "{\"webhook_url\":\"$SINK/mattermost\",\"channel\":\"alerts\"}"
mk rocket_chat     "rocketchat → sink"        "{\"webhook_url\":\"$SINK/rocketchat\",\"channel\":\"alerts\"}"
mk gotify          "gotify → sink"            "{\"server\":\"$SINK\",\"token\":\"demo\",\"priority\":5}"
mk ntfy            "ntfy → sink"              "{\"server\":\"$SINK\",\"topic\":\"rampart-demo\",\"priority\":3}"
mk matrix          "matrix → sink"            "{\"homeserver\":\"$SINK\",\"access_token\":\"demo\",\"room_id\":\"!demo:sink\"}"
mk apprise         "apprise → sink"           "{\"apprise_url\":\"$SINK\",\"urls\":\"json://sink\"}"
mk home_assistant  "home_assistant → sink"    "{\"base_url\":\"$SINK\",\"long_lived_token\":\"demo\",\"notify_service\":\"persistent_notification\"}"
mk alerta          "alerta → sink"            "{\"api_url\":\"$SINK\",\"api_key\":\"demo\"}"
mk alertnow        "alertnow → sink"          "{\"webhook_url\":\"$SINK/alertnow\"}"
mk alertops        "alertops → sink"          "{\"integration_url\":\"$SINK/alertops\"}"
mk betterstack     "betterstack → sink"       "{\"integration_url\":\"$SINK/betterstack\"}"
mk bitrix24        "bitrix24 → sink"          "{\"webhook_url\":\"$SINK/bitrix24\",\"user_id\":\"1\"}"
mk callmebot       "callmebot → sink"         "{\"endpoint_url\":\"$SINK/callmebot\"}"
mk flashduty       "flashduty → sink"         "{\"integration_url\":\"$SINK/flashduty\"}"
mk flock           "flock → sink"             "{\"webhook_url\":\"$SINK/flock\"}"
mk fluxer          "fluxer → sink"            "{\"webhook_url\":\"$SINK/fluxer\"}"
mk goalert         "goalert → sink"           "{\"integration_url\":\"$SINK/goalert\"}"
mk grafana_oncall  "grafana_oncall → sink"    "{\"webhook_url\":\"$SINK/grafana_oncall\"}"
mk healthchecks_io "healthchecks → sink"      "{\"ping_url\":\"$SINK/hc\"}"
mk heii_oncall     "heii_oncall → sink"       "{\"trigger_url\":\"$SINK/heii/trigger\",\"close_url\":\"$SINK/heii/close\"}"
mk mastodon        "mastodon → sink"          "{\"server\":\"$SINK\",\"access_token\":\"demo\"}"
mk nostr           "nostr → sink"             "{\"bridge_url\":\"$SINK/nostr\",\"recipient\":\"npub1demo\"}"
mk onebot          "onebot → sink"            "{\"http_url\":\"$SINK\",\"kind\":\"group\",\"target_id\":1}"
mk onesender       "onesender → sink"         "{\"base_url\":\"$SINK\",\"api_token\":\"demo\",\"recipient\":\"1\"}"
mk pagertree       "pagertree → sink"         "{\"integration_url\":\"$SINK/pagertree\"}"
mk ringcentral     "ringcentral → sink"       "{\"webhook_url\":\"$SINK/ringcentral\"}"
mk signal          "signal → sink"            "{\"api_url\":\"$SINK\",\"number\":\"+10000000000\",\"recipients\":[\"+10000000001\"]}"
mk spike_sh        "spike_sh → sink"          "{\"integration_url\":\"$SINK/spike\"}"
mk splash          "splash → sink"            "{\"webhook_url\":\"$SINK/splash\"}"
mk splunk          "splunk → sink"            "{\"integration_url\":\"$SINK/splunk\"}"
mk squadcast       "squadcast → sink"         "{\"webhook_url\":\"$SINK/squadcast\"}"
mk stackfield      "stackfield → sink"        "{\"webhook_url\":\"$SINK/stackfield\"}"
mk teltonika       "teltonika → sink"         "{\"base_url\":\"$SINK\",\"username\":\"u\",\"password\":\"p\",\"number\":\"1\"}"
mk whatsapp_evolution "whatsapp_evo → sink"   "{\"base_url\":\"$SINK\",\"api_key\":\"demo\",\"instance\":\"i\",\"number\":\"1\"}"
mk whatsapp_waha   "whatsapp_waha → sink"     "{\"base_url\":\"$SINK\",\"session\":\"default\",\"chat_id\":\"1\"}"
mk yzj             "yzj → sink"               "{\"webhook_url\":\"$SINK/yzj\"}"
mk zenduty         "zenduty → sink"           "{\"integration_url\":\"$SINK/zenduty\"}"
mk zoho_cliq       "zoho_cliq → sink"         "{\"webhook_url\":\"$SINK/zoho\"}"
mk zulip           "zulip → sink"             "{\"server\":\"$SINK\",\"bot_email\":\"b@x\",\"bot_key\":\"k\",\"to\":\"alerts\"}"
mk sms_eagle       "sms_eagle → sink"         "{\"base_url\":\"$SINK\",\"access_token\":\"demo\",\"to\":\"1\"}"
mk gitlab_issue    "gitlab_issue → sink"      "{\"base_url\":\"$SINK\",\"token\":\"demo\",\"project_id\":\"1\"}"
mk gorush          "gorush → sink"            "{\"server\":\"$SINK\",\"platform\":2,\"tokens\":[\"demo\"]}"
mk halo_psa        "halo_psa → sink"          "{\"base_url\":\"$SINK\",\"client_id\":\"c\",\"client_secret\":\"s\",\"ticket_type_id\":1}"
mk jira_sm         "jira_sm → sink"           "{\"site_url\":\"$SINK\",\"email\":\"a@x\",\"api_token\":\"t\",\"project_key\":\"OPS\"}"

# email → Mailpit (real SMTP delivery). field names: smtp_host/smtp_port/encryption.
mk email           "email → Mailpit"          "{\"smtp_host\":\"$MAILPIT_SMTP_HOST\",\"smtp_port\":$MAILPIT_SMTP_PORT,\"encryption\":\"plain\",\"from\":\"alerts@rampart.local\",\"to\":\"oncall@rampart.local\"}"

log "sink-compatible channels created (deliveries land in the webhook-sink + Mailpit)"

# =========================================================================
# B. VENDOR-HARDCODED — created as CONFIG ONLY, labelled "needs real creds".
#    NOT /test-ed (would hit the real vendor). Config uses obvious placeholders.
# =========================================================================
vk() {  # vk <kind> <config-json>  (name = "<kind> (needs real creds)")
  mk "$1" "$1 (needs real creds)" "$2"
}
# Host-pinned URL kinds (UI shows them; a real vendor URL would make them live).
vk slack        "{\"webhook_url\":\"https://hooks.slack.com/services/T000/B000/XXXX\",\"channel\":\"#alerts\"}"
vk discord      "{\"webhook_url\":\"https://discord.com/api/webhooks/000/XXXX\"}"
vk feishu       "{\"webhook_url\":\"https://open.feishu.cn/open-apis/bot/v2/hook/XXXX\"}"
vk google_chat  "{\"webhook_url\":\"https://chat.googleapis.com/v1/spaces/XXXX\"}"
vk google_sheets "{\"webhook_url\":\"https://script.google.com/macros/s/XXXX/exec\"}"
vk teams        "{\"webhook_url\":\"https://example.webhook.office.com/webhookb2/XXXX\"}"

# Pure vendor-endpoint kinds. Minimal placeholder config so create succeeds.
vk telegram     "{\"bot_token\":\"000:XXXX\",\"chat_id\":\"123\"}"
vk sms_twilio   "{\"account_sid\":\"AC000\",\"auth_token\":\"XXXX\",\"from\":\"+10000000000\",\"to\":\"+10000000001\"}"
vk pagerduty    "{\"routing_key\":\"XXXX\"}"
vk opsgenie     "{\"api_key\":\"XXXX\",\"region\":\"us\"}"
vk pushover     "{\"app_token\":\"XXXX\",\"user_key\":\"YYYY\"}"
vk sendgrid     "{\"api_key\":\"SG.XXXX\",\"from\":\"a@x\",\"to\":\"b@x\"}"
vk resend       "{\"api_key\":\"re_XXXX\",\"from\":\"a@x\",\"to\":\"b@x\"}"
vk brevo        "{\"api_key\":\"XXXX\",\"from\":\"a@x\",\"to\":\"b@x\"}"
vk mailjet      "{\"api_key\":\"XXXX\",\"api_secret\":\"YYYY\",\"from\":\"a@x\",\"to\":\"b@x\"}"
vk mandrill     "{\"api_key\":\"XXXX\",\"from\":\"a@x\",\"to\":\"b@x\"}"
vk postmark     "{\"server_token\":\"XXXX\",\"from\":\"a@x\",\"to\":\"b@x\"}"
vk mailgun      "{\"api_key\":\"XXXX\",\"domain\":\"mg.example.com\",\"from\":\"a@x\",\"to\":\"b@x\"}"
vk sparkpost    "{\"api_key\":\"XXXX\",\"from\":\"a@x\",\"to\":\"b@x\"}"
vk datadog      "{\"api_key\":\"XXXX\",\"region\":\"us\"}"
vk newrelic     "{\"api_key\":\"XXXX\",\"region\":\"us\"}"
vk sentry       "{\"dsn\":\"https://x@example.ingest.sentry.io/1\"}"
vk rollbar      "{\"access_token\":\"XXXX\"}"
vk honeybadger  "{\"api_key\":\"XXXX\"}"
vk aws_sns      "{\"region\":\"us-east-1\",\"topic_arn\":\"arn:aws:sns:us-east-1:000:t\",\"access_key_id\":\"AKIA\",\"secret_access_key\":\"XXXX\"}"
vk azure_servicebus "{\"namespace\":\"demo\",\"queue\":\"alerts\",\"shared_access_key_name\":\"k\",\"shared_access_key\":\"XXXX\"}"
vk gcp_pubsub   "{\"project_id\":\"demo\",\"topic\":\"alerts\",\"access_token\":\"XXXX\"}"
vk linear       "{\"api_key\":\"lin_XXXX\",\"team_id\":\"T\"}"
vk clickup      "{\"api_token\":\"pk_XXXX\",\"list_id\":\"1\"}"
vk trello       "{\"key\":\"XXXX\",\"token\":\"YYYY\",\"list_id\":\"1\"}"
vk github_issue "{\"token\":\"ghp_XXXX\",\"repo\":\"owner/repo\"}"
vk asana        "{\"access_token\":\"XXXX\",\"project_id\":\"1\"}"
vk notion       "{\"api_token\":\"secret_XXXX\",\"database_id\":\"1\"}"
vk webex        "{\"bot_token\":\"XXXX\",\"room_id\":\"1\"}"
vk wecom        "{\"bot_key\":\"XXXX\"}"
vk dingtalk     "{\"access_token\":\"XXXX\"}"
vk lark         "{\"webhook_url\":\"https://open.larksuite.com/open-apis/bot/v2/hook/XXXX\"}"
vk pumble       "{\"webhook_url\":\"https://api.pumble.com/workspaces/XXXX\"}"
vk vk           "{\"access_token\":\"XXXX\",\"peer_id\":\"1\"}"
vk vonage       "{\"api_key\":\"XXXX\",\"api_secret\":\"YYYY\",\"from\":\"Demo\",\"to\":\"+10000000000\"}"
vk plivo        "{\"auth_id\":\"XXXX\",\"auth_token\":\"YYYY\",\"from\":\"1\",\"to\":\"1\"}"
vk messagebird  "{\"access_key\":\"XXXX\",\"originator\":\"Demo\",\"recipients\":\"1\"}"
vk telnyx       "{\"api_key\":\"XXXX\",\"from\":\"1\",\"to\":\"1\"}"
vk bandwidth    "{\"username\":\"u\",\"password\":\"p\",\"account_id\":\"1\",\"from\":\"1\",\"to\":\"1\",\"application_id\":\"1\"}"
vk clicksend    "{\"username\":\"u\",\"api_key\":\"XXXX\",\"to\":\"1\"}"
vk smsglobal    "{\"api_key\":\"XXXX\",\"api_secret\":\"YYYY\",\"to\":\"1\"}"
vk threema      "{\"identity\":\"XXXX\",\"secret\":\"YYYY\",\"to\":\"YYYYYYYY\"}"
vk line         "{\"channel_token\":\"XXXX\",\"to\":\"U000\"}"
vk pushbullet   "{\"access_token\":\"XXXX\"}"
vk pushy        "{\"api_key\":\"XXXX\",\"to\":\"device\"}"
vk pushcut      "{\"webhook_name\":\"alerts\",\"api_key\":\"XXXX\"}"
vk pushplus     "{\"token\":\"XXXX\"}"
vk serverchan   "{\"send_key\":\"SCT000\"}"
vk spug_push    "{\"token\":\"XXXX\"}"
vk wpush        "{\"api_key\":\"XXXX\"}"
vk bark         "{\"device_key\":\"XXXX\"}"
vk pushdeer     "{\"push_key\":\"XXXX\"}"
vk signl4       "{\"team_secret\":\"XXXX\"}"
vk ilert        "{\"api_key\":\"XXXX\"}"
vk statuspage_io "{\"api_key\":\"XXXX\",\"page_id\":\"P\",\"component_id\":\"C\"}"
vk aliyun_sms   "{\"access_key_id\":\"XXXX\",\"access_key_secret\":\"YYYY\",\"sign_name\":\"Demo\",\"template_code\":\"SMS_1\",\"phone\":\"1\"}"
vk bale         "{\"bot_token\":\"XXXX\",\"chat_id\":\"1\"}"
vk cellsynt     "{\"username\":\"u\",\"password\":\"p\",\"to\":\"1\"}"
vk freemobile   "{\"user\":\"u\",\"pass\":\"p\"}"
vk gtxmessaging "{\"api_token\":\"XXXX\",\"from\":\"1\",\"to\":\"1\"}"
vk kook         "{\"bot_token\":\"XXXX\",\"target_id\":\"1\"}"
vk max_messenger "{\"bot_token\":\"XXXX\",\"chat_id\":\"1\"}"
vk notifery     "{\"api_key\":\"XXXX\"}"
vk octopush     "{\"api_login\":\"u\",\"api_key\":\"XXXX\",\"to\":\"1\"}"
vk onechat      "{\"bot_token\":\"XXXX\",\"to\":\"1\"}"
vk promosms     "{\"token\":\"XXXX\",\"to\":\"1\"}"
vk serwersms    "{\"username\":\"u\",\"password\":\"p\",\"to\":\"1\"}"
vk sevenio      "{\"api_key\":\"XXXX\",\"to\":\"1\"}"
vk sms46elks    "{\"username\":\"u\",\"password\":\"p\",\"from\":\"D\",\"to\":\"1\"}"
vk sms_ir       "{\"api_key\":\"XXXX\",\"to\":\"1\"}"
vk sms_manager  "{\"api_key\":\"XXXX\",\"to\":\"1\"}"
vk smsc         "{\"login\":\"u\",\"password\":\"p\",\"to\":\"1\"}"
vk smspartner   "{\"api_key\":\"XXXX\",\"to\":\"1\"}"
vk smsplanet    "{\"api_key\":\"XXXX\",\"from\":\"D\",\"to\":\"1\"}"
vk whatsapp360  "{\"api_key\":\"XXXX\",\"from\":\"1\",\"to\":\"1\"}"
vk whatsapp_whapi "{\"api_token\":\"XXXX\",\"to\":\"1\"}"

# webpush is a real kind but needs a browser push subscription to deliver; created
# as config so it's visible. NOTE: a couple of the newest kinds (e.g. sms46elks,
# whatsapp360) only exist in the DB `channel_kind` enum on recent images — on an
# older published image their create 500s harmlessly (the create is best-effort).
vk webpush "{\"subject\":\"mailto:alerts@rampart.local\"}"

log "vendor-hardcoded channels created as config-only (labelled 'needs real creds', NOT /test-ed)"

# --- A targeted REAL /test for the headline sink channel --------------------
WH_ID="$(id_by_name "$(get /v1/notifications)" 'webhook → sink')"
if [ -n "$WH_ID" ]; then
  post "/v1/notifications/$WH_ID/test" '{}' >/dev/null 2>&1 || true
  log "fired a REAL /test on 'webhook → sink' (check the sink UI)"
fi
