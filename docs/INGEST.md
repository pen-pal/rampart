# Inbound Alert Ingestion (Prometheus Alertmanager)

Rampart can accept alerts pushed from external monitoring systems and turn
them into status-page incidents. The first supported source is
**Prometheus Alertmanager**, via its native webhook receiver.

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
| `status: "firing"` | Create a new incident on the token's status page. |
| `status: "resolved"` | Resolve the most recent **active** incident on the page whose title matches the dedup key. |
| `labels.alertname` | Incident **title** (also the dedup key for resolution). Falls back to `annotations.summary` if absent. |
| `annotations.description` | Incident **content** (falls back to `annotations.summary`, then empty). |
| `labels.severity` | Incident **style**: `critical` → `danger`, `warning` → `warning`, anything else → `info`. |

### Resolution / deduplication

Resolution matches on the incident **title**, which is set from
`alertname`. When a `resolved` alert arrives, Rampart finds the newest
active incident on that page with the same title and marks it resolved. So
keep `alertname` stable between the firing and resolved payloads (which
Alertmanager does by default).

If no matching active incident exists (already resolved, or never created),
the resolved alert is a no-op and is simply not counted in the response
`resolved` total.

### Notes

- An alert with neither a non-empty `alertname` nor a `summary` is skipped.
- A missing or unexpected `status` on an alert is treated as `firing`, so
  alerts are never silently dropped.
- Subscriber email fan-out (if SMTP is configured) is handled by the normal
  incident-creation path for manually created incidents; ingest-created
  incidents are written directly via the DB layer and currently do **not**
  trigger subscriber emails. This keeps a noisy alert source from blasting
  the subscriber list. Revisit if you want ingest-driven notifications.
