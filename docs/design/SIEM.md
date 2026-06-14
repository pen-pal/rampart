# SIEM / syslog export

Rampart is **not** a SIEM (no detection-rule engine, threat intel, or
correlation — see [TUNNELING.md](TUNNELING.md) for the same stance on staying in
our lane). But it *is* the security-event source a blue team wants to feed into
their SIEM: the **audit log** records logins, failed logins, 2FA failures, and
every config change, in a tamper-evident hash chain. This feature streams that
log out to an external sink.

## Model

A single leader-gated background loop tails the audit log forward and forwards
new rows to the configured sink. Off by default.

- **Config** (`settings.siem_export`): `{ enabled, kind, target }`.
  - `kind = "webhook"` → HTTP `POST` a JSON array of audit entries to `target`
    (a URL). The natural fit for SIEMs with an HTTP collector (Splunk HEC,
    Elastic, Datadog logs, a Vector/Logstash HTTP input).
  - `kind = "syslog"` → send one **RFC5424** line per entry over **UDP** to
    `target` (`host:port`), pri `134` (local0.info), with the audit JSON as the
    message. The classic path into rsyslog / syslog-ng / a SIEM syslog input.
- **Cursor** (`settings.siem_export_cursor`): the id of the last successfully
  forwarded row. The loop fetches `audit_log WHERE id > cursor ORDER BY id ASC`
  in batches and advances the cursor **only after a successful send** — so a
  down sink is retried, never skipped, and a backlog drains in one tick.
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

## Configure

Admin → **Settings → Ingest token** page → *SIEM / syslog export*, or the API:

```
PUT /v1/settings/siem-export   { "enabled": true, "kind": "webhook",
                                 "target": "https://siem.example.com/ingest" }
```

## Out of scope / follow-ups

- TCP + TLS syslog (v1 is UDP). Webhook already rides HTTPS.
- A dead-letter buffer for prolonged sink outages (today the cursor just waits).
- Per-event-kind filtering (today the whole audit stream ships).
