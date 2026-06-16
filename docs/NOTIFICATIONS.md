# Notification channels

128 channel adapters — including the Apprise gateway (one channel fans
out to 80+ downstream services) and the HMAC-signed Generic Webhook — to
fan out. Every channel takes a JSON config blob, persisted as
`notifications.config`. Secret-shaped fields (tokens, passwords, API
keys) are stored as plain text in the database — protect Postgres
accordingly.

Two cross-cutting options on every channel: a per-channel **cooldown**
(seconds; suppresses repeat fires within the window — useful for
flap-prone monitors paired with SMS/paging), and, for the Generic
Webhook, an optional **HMAC secret** that signs the body with
`X-Rampart-Signature: sha256=<hex>` over the raw bytes.

When subject + body are rendered, every channel runs the message
through the **Liquid template** layer first. Custom templates live at
`#/notifications` → Templates tab. See [TEMPLATES.md](#templates) at
the bottom for variables + examples.

## Index

- [Chat platforms](#chat-platforms)
- [Push notifications](#push-notifications)
- [Email](#email)
- [SMS gateways](#sms-gateways)
- [Incident management](#incident-management)
- [Error trackers + observability](#error-trackers--observability)
- [Issue trackers / task management](#issue-trackers--task-management)
- [Self-hosted / generic](#self-hosted--generic)
- [Cloud message buses](#cloud-message-buses)
- [Liquid templates](#templates)

---

## Chat platforms

| Channel             | Required config                                                  |
| ---                 | ---                                                              |
| Slack               | `webhook_url` (`https://hooks.slack.com/...`), optional `channel`|
| Discord             | `webhook_url`, optional `username`                               |
| MS Teams            | `webhook_url`                                                    |
| Mattermost          | `webhook_url`                                                    |
| Rocket.Chat         | `webhook_url`                                                    |
| Telegram            | `bot_token`, `chat_id`                                           |
| Matrix              | `homeserver`, `access_token`, `room_id`                          |
| Google Chat         | `webhook_url`                                                    |
| WeCom (企业微信)    | `bot_key`, optional `mentioned_mobile_list`                      |
| DingTalk (钉钉)     | `access_token`, optional `secret`, `at_mobiles`, `at_all`        |
| Feishu (飞书)       | `webhook_url`                                                    |
| Lark                | `webhook_url`                                                    |
| LINE Messenger      | `channel_access_token`, `to`                                     |
| Mastodon            | `server`, `access_token`, optional `visibility`                  |
| Pumble              | `webhook_url`                                                    |
| Bitrix24            | `webhook_url`, `user_id`                                         |
| Stackfield          | `webhook_url`                                                    |
| Cisco Webex         | `bot_token`, `room_id`                                           |
| Flock               | `webhook_url`                                                    |
| ZohoCliq            | `webhook_url`                                                    |
| Zulip               | `server`, `bot_email`, `bot_key`, `kind` (stream/private), `to`, `topic` |
| Signal              | `api_url` (signal-cli REST), `number`, `recipients[]`            |
| WhatsApp (WAHA)     | `base_url`, `session`, `chat_id`, optional `api_key`             |
| WhatsApp (whapi)    | `api_token`, `to`, optional `base_url`                           |
| WhatsApp (360messenger) | `api_key`, `phone`                                           |
| WhatsApp (Evolution) | `base_url`, `api_key`, `instance`, `number`                     |
| Threema             | `gateway_id`, `secret`, `to` (Threema ID, email, or phone)       |
| Bale                | `bot_token`, `chat_id`                                           |
| Kook                | `bot_token`, `target_type` (GROUP/PERSON), `target_id`           |
| OneBot              | `http_url`, `kind` (group/private), `target_id`, optional `access_token` |
| OneChat (TH)        | `bot_token`, `chat_id`                                           |
| MAX Messenger (RU)  | `access_token`, `chat_id`                                        |
| Nostr               | `bridge_url` (HTTP relay), `recipient`, optional `api_key`       |
| RingCentral         | `webhook_url`                                                    |
| VK                  | `access_token`, `peer_id`, optional `api_version`                |
| YZJ (云之家)        | `webhook_url`                                                    |

## Push notifications

| Channel       | Required config                                                |
| ---           | ---                                                            |
| ntfy.sh       | `url` (incl. topic), optional `auth_token`                     |
| Gotify        | `url`, `token`                                                 |
| Pushover      | `user_key`, `app_token`                                        |
| Pushbullet    | `access_token`, optional `device_iden`                         |
| Bark (iOS)    | `device_key`, optional `server`, `group`, `sound`              |
| Pushy         | `api_key`, `to[]` (device tokens)                              |
| Gorush        | `server`, `platform` (ios/android), `tokens[]`, optional `topic`|
| Pushcut       | `api_key`, `notification_name`                                 |
| PushDeer      | `push_key`, optional `server`                                  |
| PushPlus      | `token`, optional `topic`                                      |
| SpugPush      | `template_code`                                                |
| Spug (Server酱) — see ServerChan |                                              |
| ServerChan    | `send_key`                                                     |
| WPush.cn      | `api_key`, `channel` (csv: wechat/email/sms/dingtalk)          |
| Notifery      | `api_token`, `group`                                           |
| CallMeBot     | `endpoint_url`                                                 |
| Apprise       | `server_url` (sidecar), `urls[]`                               |
| Web Push      | optional `subject` (VAPID contact, e.g. `mailto:…`). Browsers subscribe per-device via the **Enable push** button on the channel row; the shared VAPID keypair is auto-generated. RFC 8291 `aes128gcm`. |

## Email

| Channel    | Required config                                              |
| ---        | ---                                                          |
| Email/SMTP | `smtp_host`, `smtp_port`, `encryption` (tls/starttls/plain), `username`, `password`, `from`, `to` |
| SendGrid   | `api_key`, `from_email`, optional `from_name`, `to` (csv or array) |
| Resend     | `api_key`, `from`, `to`                                     |
| Brevo      | `api_key`, `from_email`, optional `from_name`, `to_email`, optional `to_name` |
| Mailgun    | `api_key`, `domain`, optional `base_url` (EU override), `from`, `to` |
| Mailjet    | `api_key`, `api_secret`, `from_email`, optional `from_name`, `to_email`, optional `to_name` |
| Postmark   | `server_token`, `from`, `to`, optional `message_stream`     |
| Mandrill   | `api_key`, `from_email`, optional `from_name`, `to_email`   |
| SparkPost  | `api_key`, `from`, `to`, optional `base_url` (EU)           |

## SMS gateways

| Channel            | Required config                                            |
| ---                | ---                                                        |
| Twilio             | `account_sid`, `auth_token`, `from`, `to` (csv)            |
| MessageBird        | `access_key`, `originator`, `recipients` (csv)             |
| Plivo              | `auth_id`, `auth_token`, `from`, `to` (`+1...<+44...>`)    |
| Vonage / Nexmo     | `api_key`, `api_secret`, `from`, `to`                      |
| Bandwidth          | `account_id`, `username`, `password`, `application_id`, `from`, `to` (csv) |
| Telnyx             | `api_key`, `from`, `to` (csv)                              |
| ClickSend          | `username`, `api_key`, `from`, `to` (csv)                  |
| 46elks             | `api_username`, `api_password`, `from`, `to` (csv)         |
| SMSGlobal          | `api_key`, `api_secret`, `origin`, `destination` (csv)     |
| seven.io           | `api_key`, `to` (csv), optional `from`                     |
| Cellsynt (SE)      | `username`, `password`, `originator`, `destination` (csv)  |
| GtxMessaging       | `api_key`, `sender_id`, `to` (csv)                         |
| SmsManager.cz      | `api_key`, `numbers` (csv), `quality` (lowcost/economy/high), optional `sender_id` |
| SMSEagle           | `base_url`, `access_token`, `to` (csv)                     |
| Octopush           | `api_login`, `api_key`, `sender`, `to` (csv)               |
| SerwerSMS.pl       | `username`, `password`, `sender`, `phone` (csv)            |
| SMSPlanet.pl       | `api_key`, `sender`, `to` (csv)                            |
| SMSC.ru            | `login`, `psw`, `phones` (csv)                             |
| Aliyun SMS         | `access_key_id`, `access_key_secret`, `sign_name`, `template_code`, `phone_numbers`, optional `template_param` JSON |
| SMS.ir             | `api_key`, `line_number`, `mobiles` (csv)                  |
| Free Mobile (FR)   | `user`, `pass` (delivers only to the account holder)       |
| PromoSMS.pl        | `username`, `password`, `sender`, `to` (csv), optional `kind` (1/3) |
| SMSPartner.fr      | `api_key`, `sender`, `to` (csv)                            |
| Teltonika SMS      | `base_url`, `username`, `password`, `number`               |

## Incident management

| Channel               | Required config                                          |
| ---                   | ---                                                      |
| PagerDuty             | `integration_key`                                        |
| Opsgenie              | `api_key`, optional `region` (us/eu), `priority`         |
| PagerTree             | `integration_url`, `severity`                            |
| Squadcast             | `webhook_url`                                            |
| GoAlert               | `integration_url`                                        |
| Alerta                | `api_url`, `api_key`, optional `environment`             |
| AlertNow              | `webhook_url`                                            |
| AlertOps              | `integration_url`                                        |
| SIGNL4                | `team_secret`                                            |
| Heii On-Call          | `trigger_url`, optional `close_url`                      |
| Splunk On-Call        | `integration_url`                                        |
| Grafana OnCall        | `webhook_url`                                            |
| Spike.sh              | `integration_url`                                        |
| Zenduty               | `integration_url`                                        |
| iLert                 | `integration_key`                                        |
| FlashDuty (快猫星云)  | `integration_url`, optional `severity`                   |
| Halo PSA              | `base_url`, `client_id`, `client_secret`, `team`, `ticket_type_id` |
| Jira Service Mgmt     | `site_url`, `email`, `api_token`, `project_key`, optional `issue_type` |
| BetterStack           | `integration_url`                                        |
| Statuspage.io         | `api_key`, `page_id`                                     |
| Splash                | `webhook_url`                                            |

## Error trackers + observability

| Channel        | Required config                                                 |
| ---            | ---                                                             |
| Sentry         | `dsn` (full project DSN)                                        |
| Rollbar        | `access_token`, optional `environment`                          |
| Honeybadger    | `api_key`, optional `environment`                               |
| Datadog Events | `api_key`, `site` (us1/us3/us5/eu/us1-fed)                      |
| New Relic      | `insert_key`, `account_id`, `region` (us/eu)                    |
| Healthchecks.io| `ping_url` — Up pings `/<uuid>`, Down pings `/<uuid>/fail`      |

## Issue trackers / task management

| Channel       | Required config                                                |
| ---           | ---                                                            |
| GitHub Issue  | `token` (PAT), `owner`, `repo`, optional `labels[]`            |
| GitLab Issue  | optional `base_url` (default gitlab.com), `token`, `project_id`|
| Linear        | `api_key`, `team_id`                                           |
| ClickUp       | `api_token`, `list_id`                                         |
| Trello        | `key`, `token`, `list_id`                                      |
| Asana         | `access_token`, `workspace`, `project`                         |
| Notion        | `api_token`, `database_id` (title prop must be `Name`)         |
| Google Sheets | `webhook_url` (Apps Script Web App deployed by the user)       |
| Home Assistant| `base_url`, `long_lived_token`, `notify_service`               |

## Self-hosted / generic

| Channel        | Required config                                               |
| ---            | ---                                                           |
| Generic Webhook| `url`, optional `method`, `headers`, `body_template`, optional `secret` (HMAC-SHA256 → `X-Rampart-Signature`) |
| Apprise gateway| `server_url`, `urls[]` (Apprise notation)                    |
| Fluxer         | `webhook_url`                                                 |

---

## Cloud message buses

Publish alerts onto a cloud pub/sub or queue for downstream consumers.
Auth is signed per request — no long-lived bearer tokens on the wire.

| Channel            | Required config                                               |
| ---                | ---                                                           |
| AWS SNS            | `region`, `access_key_id`, `secret_access_key`, exactly one of `topic_arn` / `phone_number`, optional `session_token` (STS). SigV4-signed. |
| Azure Service Bus  | `namespace`, `entity` (queue/topic), `sas_key_name`, `sas_key`, optional `ttl_seconds` (default 300). SAS-token auth. |
| GCP Pub/Sub        | `project_id`, `topic`, `client_email`, `private_key` (PEM from the service-account JSON). Mints + caches an OAuth2 token from a signed JWT. |

---

## Templates

Notification subject + body run through Liquid. The pre-Liquid
`{{placeholder}}` syntax works unchanged.

Top-level variables:

| Variable          | Meaning                                                |
| ---               | ---                                                    |
| `monitor.name`    | display name                                           |
| `monitor.url`     | target URL (empty for hostname-based kinds)            |
| `monitor.kind`    | http / tcp / ping / dns / push / tls / domain / postgres / mysql / mssql / redis / mongodb / grpc / mqtt / docker / steam / kafka / radius / keyword / json_query |
| `monitor.id`      | UUID                                                   |
| `monitor.hostname`| for kinds that use hostname instead of URL             |
| `monitor.port`    | when set                                               |
| `status`          | current heartbeat status string                        |
| `prev_status`     | previous status, or "unknown"                          |
| `latency_ms`      | int, empty if absent                                   |
| `status_code`     | int, empty if absent                                   |
| `msg`             | probe-supplied message                                 |
| `retries`         | retry count of this heartbeat                          |
| `ts`              | heartbeat timestamp (RFC 3339)                         |

Default subject: `[{{ status }}] {{ monitor.name }}`

Default body:

```liquid
{{ monitor.name }} is now {{ status }} (was {{ prev_status }}).

Kind:     {{ monitor.kind }}
Target:   {{ monitor.url }}
Latency:  {{ latency_ms }}ms
Code:     {{ status_code }}
Message:  {{ msg }}
Time:     {{ ts }}
Monitor:  {{ monitor.id }}
```

Liquid lets you do conditionals and filters too:

```liquid
{% if status == "down" %}🔴 OUTAGE{% else %}🟢 RECOVERED{% endif %}
{{ monitor.name }} — {{ msg | default: "no message" }}
Status: {{ status | upcase }}
Time:   {{ ts }}
```

Note: Liquid is strict on missing variables — a typo like
`{{ doesnotexist }}` renders as `[template render error: Unknown
variable …]`. For nil-safe fallbacks, route a *known* variable through
`default:` (e.g. `{{ msg | default: "n/a" }}`).

---

## Adding a new channel

1. Add a new variant to `rampart_core::notification::ChannelKind`.
2. Migration: `ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS '<snake>';`
3. New file under `backend/crates/rampart-notifier/src/channels/<name>.rs`
   implementing the `Channel` trait. Pattern after `slack.rs` for
   webhook-style, `email.rs` for SMTP-style, `opsgenie.rs` for
   incident-with-resolve.
4. Wire into `channels/mod.rs::dispatch`.
5. UI: add to `SUPPORTED` list and `ConfigForm` cases in
   `frontend/src/views/Notifications.jsx`.
6. Document the new channel in this file.
