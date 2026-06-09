# Inbound Alert Ingestion (Alertmanager / Grafana / Datadog / PagerDuty / Opsgenie)

Rampart can accept alerts pushed from external monitoring systems and turn
them into status-page incidents. The supported sources are:

- **Prometheus Alertmanager** — `POST /v1/public/ingest/alertmanager/{token}`
- **Grafana** (unified alerting) — `POST /v1/public/ingest/grafana/{token}`
- **Datadog** (webhook integration) — `POST /v1/public/ingest/datadog/{token}`
- **PagerDuty** (webhook v3) — `POST /v1/public/ingest/pagerduty/{token}`
- **Opsgenie** (webhook integration) — `POST /v1/public/ingest/opsgenie/{token}`

They all share the same token auth, the same status-page resolution, and
the same create-or-resolve incident core; they differ only in how their
vendor payload is parsed. The Alertmanager receiver is documented first and
in full; the Grafana, Datadog, PagerDuty and Opsgenie sections below only
cover what differs.

The flow:

1. Mint a page-scoped **ingest token** through the admin API.
2. Point an Alertmanager `webhook_config` at the public ingest URL that
   embeds the token.
3. When Alertmanager fires an alert, Rampart opens an incident on the
   token's status page. When the alert resolves, Rampart resolves the
   matching incident.

The token in the URL is the only credential — there is no session, no
bearer header. Treat the full URL as a secret. A token is scoped to a
single status page and can do nothing except create / resolve incidents on
that page.

---

## 1. Mint an ingest token

Ingest tokens are managed under a status page. You need an authenticated
admin session (the same session cookie the dashboard uses).

Create a token for status page `:id`:

```bash
curl -X POST https://rampart.example.com/v1/status-pages/<STATUS_PAGE_ID>/ingest-tokens \
     -H 'Content-Type: application/json' \
     --cookie 'session=<YOUR_SESSION>' \
     -d '{"label": "alertmanager-prod"}'
```

Response (`201 Created`):

```json
{
  "id": "0192f3c1-...-...",
  "status_page_id": "0192abcd-...-...",
  "token": "ing_X0a9...40chars...",
  "label": "alertmanager-prod",
  "created_at": "2026-06-09T12:00:00Z",
  "last_used_at": null
}
```

The `token` value is shown in full here **and** on every subsequent list
call — unlike personal API keys, ingest tokens are not hashed, because you
have to paste the full value into Alertmanager's config and there is no way
to re-derive it. Still treat it as a secret.

List tokens for a page:

```bash
curl https://rampart.example.com/v1/status-pages/<STATUS_PAGE_ID>/ingest-tokens \
     --cookie 'session=<YOUR_SESSION>'
```

Revoke a token (by its `id`, not the token string):

```bash
curl -X DELETE https://rampart.example.com/v1/ingest-tokens/<TOKEN_ID> \
     --cookie 'session=<YOUR_SESSION>'
```

### Managing tokens in the UI

You don't have to use curl. In the dashboard, go to **Status pages**, edit an
existing page, and scroll to the **Alertmanager / webhook ingest** section.
There you can label and generate new tokens, copy each token's ready-to-paste
Alertmanager webhook URL, and revoke tokens you no longer need. (The section
only appears once a page is saved — tokens are scoped to a page id — and only
for admin/editor roles.)

---

## 2. The ingest URL

The public receiver lives at:

```
POST /v1/public/ingest/alertmanager/{token}
```

So the full URL for the token above is:

```
https://rampart.example.com/v1/public/ingest/alertmanager/ing_X0a9...40chars...
```

It accepts a standard Alertmanager webhook JSON body and returns
`202 Accepted` with a small summary:

```json
{ "created": 1, "resolved": 0 }
```

An unknown token returns `404 Not Found`.

---

## 3. Alertmanager configuration

Add a webhook receiver to `alertmanager.yml` and route alerts to it:

```yaml
route:
  receiver: rampart-status
  # ... your existing grouping / matchers ...

receivers:
  - name: rampart-status
    webhook_configs:
      - url: 'https://rampart.example.com/v1/public/ingest/alertmanager/ing_X0a9...40chars...'
        # Alertmanager retries on non-2xx; Rampart returns 202 on success.
        send_resolved: true
```

`send_resolved: true` is important — it tells Alertmanager to POST again
with `status: "resolved"` when the alert clears, which is what lets Rampart
close the incident automatically.

---

## 4. How alerts map to incidents

Rampart reads the standard Alertmanager payload:

```json
{
  "status": "firing",
  "alerts": [
    {
      "status": "firing",
      "labels": { "alertname": "HighErrorRate", "severity": "critical" },
      "annotations": {
        "summary": "Error rate above 5%",
        "description": "The API error rate has exceeded 5% for 5 minutes."
      },
      "startsAt": "2026-06-09T12:00:00Z",
      "endsAt": "0001-01-01T00:00:00Z"
    }
  ],
  "commonLabels": { "...": "..." },
  "groupLabels": { "...": "..." }
}
```

Each entry in `alerts` is processed independently:

| Alertmanager field | Incident mapping |
|--------------------|------------------|
| `status: "firing"` | Create a new incident on the token's status page (stamped with the dedup key). |
| `status: "resolved"` | Resolve the **active** incident on the page whose `dedup_key` matches. |
| `fingerprint` | Incident **dedup key** — the stable handle used to match a resolve back to its firing. Falls back to `alertname` (then title) when absent. |
| `labels.alertname` | Incident **title**. Falls back to `annotations.summary` if absent. |
| `annotations.description` | Incident **content** (falls back to `annotations.summary`, then empty). |
| `labels.severity` | Incident **style**: `critical` → `danger`, `warning` → `warning`, anything else → `info`. |

### Resolution / deduplication

Resolution matches on a stable **dedup key**, not the title. On firing,
Rampart stores the alert's `fingerprint` (Alertmanager >= 0.22 sends one) as
the incident's `dedup_key`. When the matching `resolved` alert arrives — it
carries the same `fingerprint` — Rampart finds the active incident on that
page with that exact key and marks it resolved. This is robust even when two
distinct alerts share a title.

If the sender omits `fingerprint`, Rampart falls back to using the
`alertname` (then the title) as the dedup key, which dedups loosely.

A partial unique index keeps at most one **active** incident per
`(status_page_id, dedup_key)`: a duplicate firing for an already-open
incident is treated as already-reported and is **not** counted in the
response `created` total (no second incident is opened).

If no matching active incident exists on a resolve (already resolved, or
never created), the resolved alert is a no-op and is not counted in the
response `resolved` total.

### Notes

- An alert with neither a non-empty `alertname` nor a `summary` is skipped.
- A missing or unexpected `status` on an alert is treated as `firing`, so
  alerts are never silently dropped.
- Subscriber email fan-out (if SMTP is configured) is handled by the normal
  incident-creation path for manually created incidents; ingest-created
  incidents are written directly via the DB layer and currently do **not**
  trigger subscriber emails. This keeps a noisy alert source from blasting
  the subscriber list. Revisit if you want ingest-driven notifications.

---

## 5. Grafana (unified alerting)

Grafana's unified-alerting webhook contact point posts a body that is
intentionally Alertmanager-shaped, so Rampart reuses the exact same parser.

The receiver lives at:

```
POST /v1/public/ingest/grafana/{token}
```

Configure a **Webhook** contact point in Grafana (Alerting → Contact points)
with the full ingest URL:

```
https://rampart.example.com/v1/public/ingest/grafana/ing_X0a9...40chars...
```

Grafana sends a payload of the form:

```json
{
  "status": "firing",
  "alerts": [
    {
      "status": "firing",
      "labels": { "alertname": "HighErrorRate", "severity": "critical" },
      "annotations": {
        "summary": "Error rate above 5%",
        "description": "The API error rate has exceeded 5% for 5 minutes."
      },
      "fingerprint": "a1b2c3d4e5f6"
    }
  ]
}
```

Mapping is identical to Alertmanager:

| Grafana field | Incident mapping |
|---------------|------------------|
| `alerts[].status: "firing"` | Create an incident (stamped with the dedup key). |
| `alerts[].status: "resolved"` | Resolve the active incident with the matching `dedup_key`. |
| `alerts[].fingerprint` | Incident **dedup key** (falls back to `alertname`/title if absent). |
| `alerts[].labels.alertname` | Incident **title** (falls back to `annotations.summary`). |
| `alerts[].annotations.description` | Incident **content** (falls back to `summary`, then empty). |
| `alerts[].labels.severity` | Incident **style** (`critical`→`danger`, `warning`→`warning`, else `info`). |

Grafana stamps each alert with a stable `fingerprint` across the firing and
resolved notifications, so resolution is exact. Returns `202 Accepted` with
the same `{ "created": N, "resolved": M }` summary; an unknown token returns
`404 Not Found`.

---

## 6. Datadog

Datadog posts a single event per webhook. The body is operator-templated;
Rampart assumes the **documented default** template (the `$EVENT_*`
variables), so configure your Datadog webhook integration to emit:

```json
{
  "alert_type": "$EVENT_TYPE",
  "title": "$EVENT_TITLE",
  "body": "$TEXT_ONLY_MSG",
  "alert_id": "$ALERT_ID",
  "alert_transition": "$ALERT_TRANSITION"
}
```

The receiver lives at:

```
POST /v1/public/ingest/datadog/{token}
```

In Datadog (Integrations → Webhooks), add a webhook whose **URL** is the full
ingest URL and whose **Payload** is the JSON template above:

```
https://rampart.example.com/v1/public/ingest/datadog/ing_X0a9...40chars...
```

Mapping:

| Datadog field | Incident mapping |
|---------------|------------------|
| `alert_transition: "Triggered"` (or anything not `Recovered`) | Create an incident. |
| `alert_transition: "Recovered"` | Resolve the active incident with the matching `dedup_key`. |
| `alert_id` | Incident **dedup key** (falls back to `title` if absent). |
| `title` | Incident **title** (falls back to the dedup key if empty). |
| `body` | Incident **content**. |
| `alert_type` | Incident **style**: `error`→`danger`, `warning`→`warning`, anything else (`success`/`info`)→`info`. |

`alert_id` is stable across the `Triggered` and `Recovered` transitions for a
given monitor alert, so resolution is exact. An event with neither an
`alert_id` nor a `title` is a no-op. Returns `202 Accepted` with the same
`{ "created": N, "resolved": M }` summary; an unknown token returns
`404 Not Found`.

---

## 7. PagerDuty (webhook v3)

PagerDuty posts a single event per webhook (V3 webhook subscription / Events
API v2 shape). Rampart reads the `event` envelope: `event.event_type`
plus `event.data.status` decide create vs. resolve, and `event.data` carries
the incident.

The receiver lives at:

```
POST /v1/public/ingest/pagerduty/{token}
```

In PagerDuty (Integrations → Generic Webhooks (v3), or a webhook subscription),
point the **URL** at the full ingest URL:

```
https://rampart.example.com/v1/public/ingest/pagerduty/ing_X0a9...40chars...
```

PagerDuty sends a payload of the form:

```json
{
  "event": {
    "event_type": "incident.triggered",
    "data": {
      "id": "PXXXXXX",
      "title": "High error rate on api-gateway",
      "status": "triggered",
      "urgency": "high",
      "html_url": "https://acme.pagerduty.com/incidents/PXXXXXX"
    }
  }
}
```

Mapping:

| PagerDuty field | Incident mapping |
|-----------------|------------------|
| `event.event_type: "incident.triggered"` (or `data.status: "triggered"`) | Create an incident (stamped with the dedup key). |
| `event.event_type: "incident.resolved"` (or `data.status: "resolved"`) | Resolve the active incident with the matching `dedup_key`. |
| `event.event_type: "incident.acknowledged"` (or any other event) | No-op — `202 Accepted` with `created:0 resolved:0`. |
| `data.id` | Incident **dedup key** (falls back to `title` if absent). |
| `data.title` | Incident **title** (falls back to the dedup key if empty). |
| `data.html_url` | Folded into the incident **content** as a short line. |
| `data.urgency` | Incident **style**: `high` → `danger`, anything else (`low`) → `warning`. |

`data.id` is stable across the triggered and resolved events for a given
incident, so resolution is exact. An acknowledgement (or any non-trigger,
non-resolve event) is intentionally a no-op so an ack never opens a second
incident. An event with neither an `id` nor a `title` is a no-op. Returns
`202 Accepted` with the same `{ "created": N, "resolved": M }` summary; an
unknown token returns `404 Not Found`.

---

## 8. Opsgenie

Opsgenie posts a single event per webhook. The top-level `action` drives
create/resolve; the `alert` block carries the alert Rampart maps.

The receiver lives at:

```
POST /v1/public/ingest/opsgenie/{token}
```

In Opsgenie (Settings → Integrations → Webhook), point the **Webhook URL** at
the full ingest URL:

```
https://rampart.example.com/v1/public/ingest/opsgenie/ing_X0a9...40chars...
```

Opsgenie sends a payload of the form:

```json
{
  "action": "Create",
  "alert": {
    "alertId": "a1b2c3d4-...-...",
    "tinyId": "42",
    "message": "High error rate on api-gateway",
    "description": "The API error rate has exceeded 5% for 5 minutes.",
    "priority": "P1"
  }
}
```

Mapping:

| Opsgenie field | Incident mapping |
|----------------|------------------|
| `action: "Create"` | Create an incident (stamped with the dedup key). |
| `action: "Close"` | Resolve the active incident with the matching `dedup_key`. |
| `action: "AckAlert"` (or any other action) | No-op — `202 Accepted` with `created:0 resolved:0`. |
| `alert.alertId` | Incident **dedup key** (falls back to `tinyId`, then `message`, if absent). |
| `alert.message` | Incident **title** (falls back to the dedup key if empty). |
| `alert.description` | Incident **content**. |
| `alert.priority` | Incident **style**: `P1`/`P2` → `danger`, `P3` → `warning`, anything else (`P4`/`P5`) → `info`. |

`alert.alertId` is stable across the `Create` and `Close` actions for a given
alert, so resolution is exact. Any action other than `Create` / `Close`
(e.g. `AckAlert`, `AddNote`) is intentionally a no-op. An alert with no
`alertId`, `tinyId`, or `message` is a no-op. Returns `202 Accepted` with the
same `{ "created": N, "resolved": M }` summary; an unknown token returns
`404 Not Found`.
