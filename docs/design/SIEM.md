# SIEM / syslog export

Rampart is a security-event source a blue team feeds into their SIEM: the
**audit log** records logins, failed logins, 2FA failures, and every config
change in a tamper-evident hash chain, and the [detection engine](DETECTION.md)
raises **findings** from log patterns. This feature streams both out to an
external sink. (Correlation, threat intel and long-term retention stay in the
real SIEM — see [TUNNELING.md](TUNNELING.md) for the same stay-in-our-lane
stance.)

## Model

A single leader-gated background loop runs two forward tails — the audit log and
detection findings — to the one configured sink. Off by default. Each event is
tagged with its source (RFC5424 APP-NAME `audit` or `detection`; the webhook
JSON is shaped per source) so a collector can route by origin.

- **Config** (`settings.siem_export`): `{ enabled, kind, target, format }`.
  - `kind = "webhook"` → HTTP `POST` to `target` (a URL). For `format = "json"`
    the body is a JSON array of events (the historical shape); for `cef`/`leef`
    it is newline-delimited records as `text/plain`. The natural fit for SIEMs
    with an HTTP collector (Splunk HEC, Elastic, Datadog logs, a Vector/Logstash
    HTTP input).
  - `kind = "syslog"` → send one **RFC5424** line per entry over **UDP** to
    `target` (`host:port`), pri `134` (local0.info), with the formatted event as
    the message. The classic path into rsyslog / syslog-ng / a SIEM syslog input.
  - `kind = "syslog_tcp"` → the same RFC5424 line newline-framed over a **TCP**
    stream — for collectors that want reliable delivery, and the usual route to a
    TLS-terminating sidecar (stunnel).
  - `format` (default `"json"`): the per-event wire format — see below.
- **Cursors**: `settings.siem_export_cursor` holds the id of the last forwarded
  audit row (`audit_log WHERE id > cursor ORDER BY id ASC`);
  `settings.siem_export_findings_cursor` holds the `created_at` of the last
  forwarded finding (`detection_findings WHERE created_at > cursor ORDER BY
  created_at ASC`, since the findings PK is a UUID). Both advance **only after a
  successful send** — so a down sink is retried, never skipped, and a backlog
  drains in one tick.
- **Leader-gated**: only the elected leader forwards (same `Leadership` as the
  scheduler / prune loop), so an HA deployment doesn't double-ship.
- **Best-effort**: a failing send logs a warning and retries next tick (15s);
  it never blocks the request path or the audit write.

## Why the audit log specifically

The audit log already is the consolidated security-event store — auth outcomes
(`auth.login`, `auth.login_failed`, `auth.totp_failed`) plus every mutating
admin action, each with actor, source IP, and timestamp, redaction-safe (secret
fields are stripped at write time). Forwarding it gives the SIEM the security
signal without exposing the rest of the observability tiers. The in-app
[security insights](../design/ERROR-TRACKING.md) view covers the at-a-glance
case; this covers the "send it to the system of record" case.

## Event formats

`format` controls how each event is serialized on the wire. The transport
(`kind`) and the format are independent: any format works over any sink.

- **`json`** (default) — the raw Rampart event object, unchanged from prior
  versions. Webhook posts a JSON array; syslog frames one JSON object per line.
- **`cef`** — ArcSight **Common Event Format**, the lingua franca for Splunk and
  ArcSight: `CEF:0|Rampart|Rampart|<version>|<signatureId>|<name>|<severity>|<extension>`.
  `signatureId`/`name` come from the event's `action` (audit) or `rule_id`/
  `rule_name` (finding); `severity` (0-10) maps from a finding's severity bucket
  (`critical`→10, `high`→8, `medium`→6, `low`→3) or defaults by source. The
  extension is space-joined `key=value` pairs of the event's fields, with the
  standard `src` alias for the source IP; pipes are escaped in the header and
  `=`/newlines in extension values.
- **`leef`** — IBM QRadar **Log Event Extended Format**:
  `LEEF:1.0|Rampart|Rampart|<version>|<eventId>|<tab-delimited key=value attrs>`.
  Same field mapping as CEF; attributes are tab-delimited per the LEEF spec.

CEF/LEEF let Splunk/QRadar/ArcSight parse fields natively instead of treating
each event as opaque JSON. Mapping is generic over the event shape (the event is
JSON-ified, then rendered), so new audit/finding fields appear automatically.

## Configure

Admin → **Settings → Ingest token** page → *SIEM / syslog export*, or the API:

```
PUT /v1/settings/siem-export   { "enabled": true, "kind": "webhook",
                                 "target": "https://siem.example.com/ingest",
                                 "format": "cef" }
```

## Out of scope / follow-ups

- TLS syslog (UDP + TCP ship today; TCP is the path to a TLS-terminating
  sidecar). Webhook already rides HTTPS.
- A dead-letter buffer for prolonged sink outages (today the cursor just waits).
- Per-event-kind filtering (today the whole audit stream ships).
