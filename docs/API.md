# API reference

The Rampart HTTP API serves the embedded React dashboard, the public status pages, the push-monitor heartbeat sink, and Prometheus / health checkers. This document is a hand-curated catalogue of every endpoint surfaced by the binary at the time of the last release; every entry below cross-references the source file where the contract is canonical.

If you want a deep dive into request / response shapes, open the named route file in [`backend/crates/rampart-api/src/routes/`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/) and read the `#[derive(Serialize, Deserialize)]` types adjacent to the handler — they document themselves.

## OpenAPI spec

A hand-curated, machine-readable OpenAPI 3.1 description of the **primary** REST surface lives at [`docs/openapi.yaml`](./openapi.yaml) (`info.version` is pinned to the release line). It is representative rather than exhaustive — every resource group with its main routes and request/response shapes, but not every query parameter or every one of the ~100 handlers. The same document is also checked in as [`docs/openapi.json`](./openapi.json).

The running binary serves both files at root level, unauthenticated:

| Method · path        | Returns                          |
| :------------------- | :------------------------------- |
| `GET /openapi.yaml`  | the spec, `text/yaml`            |
| `GET /openapi.json`  | the spec, `application/json`     |

Wired in [`routes/openapi.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/openapi.rs); both files are embedded via `include_str!` so the served spec always matches the built source tree. There is no in-app Swagger-UI: the app CSP is `default-src 'self'`, which a CDN-hosted Swagger-UI bundle would violate.

**View it rendered** with any local previewer pointed at the served YAML (or the file directly), e.g.:

```bash
# Redoc / Swagger-UI preview from the checked-in file
npx @redocly/cli preview-docs docs/openapi.yaml

# …or against a running instance
curl -s http://localhost:8080/openapi.yaml | npx @redocly/cli preview-docs -
```

**Regenerate `openapi.json` after editing the YAML** (`openapi.yaml` is the source of truth):

```bash
python3 -c "import yaml,json,sys; json.dump(yaml.safe_load(open('docs/openapi.yaml')), open('docs/openapi.json','w'), indent=2)"
```

Generate a typed client with `openapi-generator` / `oapi-codegen` / `openapi-typescript` against either served file.

---

## URL families

| Prefix             | Authentication        | Routed in                                                      |
| :----------------- | :-------------------- | :------------------------------------------------------------- |
| `/healthz`, `/readyz`, `/metrics` | none                  | `routes/health.rs`                                  |
| `/push/:token`     | the token IS the auth | `routes/push.rs`                                               |
| `/v1/public/*`     | none — public surface | `routes/status_pages.rs::public_router`, `routes/subscribers.rs::public_router`, `routes/auth.rs::register/login/me`, `routes/totp.rs::public_router`, `routes/webpush.rs::vapid_key` |
| `/v1/*`            | session cookie OR API key | every other module's `router()`                            |

The session cookie is named `rampart_session`, is HTTP-only, same-site `Strict`, and `Secure` when the request arrives over HTTPS. API keys go in `Authorization: Bearer <key>` and resolve to the same `AuthUser` extractor as the cookie.

---

## Health + ops

| Method · path | Returns | Source |
| :--- | :--- | :--- |
| `GET /healthz`    | `{"status":"alive","version":"<x.y.z>"}` — liveness probe; always 200 if the process can answer | [`routes/health.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/health.rs) |
| `GET /readyz`     | 200 once the database is reachable                                                              | (same) |
| `GET /metrics`    | Prometheus text exposition: `rampart_build_info`, `rampart_monitors{status}`, `rampart_monitors_by_kind{kind}`, `rampart_channels_active`, `rampart_webpush_subscribers`, `rampart_heartbeats_24h{status}`, `rampart_incidents_open`, alerting-pipeline gauges (`rampart_metric_rules`, `rampart_metric_rules_firing`, `rampart_telemetry_rules`, `rampart_telemetry_rules_firing`, `rampart_detection_rules_enabled`, `rampart_detection_findings_open`, `rampart_escalations_open`, `rampart_error_issues_unresolved`), `rampart_ingest_24h{tier}` and `rampart_table_bytes{table}` (per-tier on-disk size) | (same) |

The version baked into `/healthz` is the source of truth for "what build is running"; the dashboard reads it from there to render the header pill.

---

## Authentication

| Method · path                       | Body / params                            | Source |
| :--- | :--- | :--- |
| `GET  /v1/auth/me`                  | —                                        | [`routes/auth.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/auth.rs) |
| `POST /v1/auth/register`            | `{email, name, password}` (first-run only) | (same) |
| `POST /v1/auth/login`               | `{email, password}` → `{challenge_token?}` on 2FA gate | (same) |
| `POST /v1/auth/logout`              | —                                        | (same) |
| `POST /v1/auth/2fa/setup`           | — → `{secret, otpauth_uri}`              | [`routes/totp.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/totp.rs) |
| `POST /v1/auth/2fa/enable`          | `{code}` → `{recovery_codes}`            | (same) |
| `POST /v1/auth/2fa/verify`          | `{challenge_token, code}` (login completion) | (same) |
| `POST /v1/auth/2fa/disable`         | `{password, code}`                       | (same) |
| `POST /v1/users/me/password`        | `{old_password, new_password}`           | [`routes/users.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/users.rs) |

Failed-login costs a constant-time Argon2 hash compare regardless of whether the user exists — defends against user-enumeration via timing.

---

## Monitors

| Method · path                                    | Notes                                                  |
| :--- | :--- |
| `GET    /v1/monitors`                            | List all. No pagination (the dashboard fits on one page). |
| `POST   /v1/monitors`                            | Create.                                                |
| `GET    /v1/monitors/{id}`                       | Single monitor.                                        |
| `PATCH  /v1/monitors/{id}`                       | Partial update.                                        |
| `DELETE /v1/monitors/{id}`                       | Hard delete — cascades to heartbeats + attached channels. |
| `POST   /v1/monitors/{id}/pause`                 | Idempotent; no-op if already paused.                   |
| `POST   /v1/monitors/{id}/resume`                | Idempotent.                                            |
| `POST   /v1/monitors/{id}/clone`                 | Returns the new monitor row.                           |
| `POST   /v1/monitors/{id}/regenerate-push-token` | Push monitors only; rotates the URL secret.            |
| `POST   /v1/monitors/{id}/test-now`              | Fires one out-of-cycle probe; reply is the resulting heartbeat. |
| `GET    /v1/monitors/{id}/heartbeats`            | `?before=<rfc3339>&limit=<n>` cursor; defaults to 50.  |
| `GET    /v1/monitors/{id}/heartbeats.csv`        | Same filter knobs; streamed CSV.                        |
| `GET    /v1/monitors/summary`                    | `?window=<seconds>` rollup for the dashboard cards.    |
| `GET    /v1/monitors/history`                    | Per-monitor 60-minute history for the dashboard chart. |
| `POST   /v1/monitors/bulk`                       | `{monitor_ids[], action: pause|resume|delete|move|attach_channel|detach_channel}` |

Source: [`routes/monitors.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/monitors.rs).

### Push monitor

| Method · path                  | Body | Notes |
| :--- | :--- | :--- |
| `POST /push/{token}`           | —    | Cron-friendly heartbeat sink — the token IS the auth (rotate via the regenerate route above). |
| `GET  /push/{token}`           | —    | Same as POST for shell clients that can't easily POST. |

Source: [`routes/push.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/push.rs).

---

## Notification channels + templates + routing

| Method · path                                            | Notes |
| :--- | :--- |
| `GET / POST  /v1/notifications`                          | List / create channels. |
| `GET / PATCH / DELETE /v1/notifications/{id}`            | Single channel. |
| `POST /v1/notifications/{id}/test`                       | Send a synthetic alert through the real provider. |
| `GET  /v1/notifications/counts`                          | Per-monitor channel-count map for the dashboard bell badge. |
| `GET / POST / DELETE /v1/monitors/{mid}/notifications[/{nid}]` | Explicit attach / detach + per-monitor list. |
| `GET / POST / PATCH / DELETE /v1/notification-templates` | Liquid-templated subject + body editor. |
| `POST /v1/notification-templates/preview`                | Render a template against a fake heartbeat. |
| `GET  /v1/monitor-groups/{id}/effective-channels`        | Resolves the tag-routing rule chain. |
| `GET  /v1/monitor-groups/{id}/channels`                  | Folder-attached channels. |
| `GET  /v1/monitor-groups/{id}/excludes`                  | Per-monitor exclusions inside a folder. |

Routing logic (tag union, folder inheritance, per-monitor excludes) lives in `rampart-db::routing`; the resolver is called once per alert by `rampart-notifier`.

Sources: [`routes/notifications.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/notifications.rs), [`routes/templates.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/templates.rs), [`routes/routing.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/routing.rs).

---

## Folders + tags + dependencies

| Method · path                                  | Notes |
| :--- | :--- |
| `GET / POST / PATCH / DELETE /v1/monitor-groups[/{id}]` | Nested folders. |
| `GET  /v1/monitor-groups/{id}/dependencies`    | Folder dep edges. |
| `POST / DELETE /v1/monitor-groups/{id}/dependencies/{parent_id}` | Add / remove a dep edge; cycle-guarded. |
| `GET / POST / PATCH / DELETE /v1/tags[/{id}]`  | Coloured chip tags. |
| `GET  /v1/tags/usage`                          | Tag → monitor-count rollup. |
| `GET / POST / DELETE /v1/monitors/{id}/tags[/{tag_id}]` | Per-monitor tag attach. |

Sources: [`routes/monitor_groups.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/monitor_groups.rs), [`routes/tags.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/tags.rs).

---

## Status pages + incidents + subscribers

| Method · path                                                 | Auth | Notes |
| :--- | :--- | :--- |
| `GET / POST / PATCH / DELETE /v1/status-pages[/{id}]`         | session | Admin CRUD. |
| `GET  /v1/public/status-pages/{slug}`                         | none    | Visitor-facing payload (monitors + sections + incidents + theme). |
| `GET / POST  /v1/status-pages/{page_id}/incidents`            | session | List + post incident under a page. |
| `PATCH / DELETE /v1/incidents/{id}`                           | session | Edit / remove an incident. |
| `POST /v1/incidents/{id}/resolve`                             | session | Mark resolved (incident stays for history). |
| `GET / POST /v1/incidents/{id}/updates`                       | session | Threaded incident updates. |
| `POST /v1/public/status-pages/{slug}/subscribe`               | none    | `{email}` — single-opt-in v1 (no confirm email yet). |
| `GET  /v1/public/subscribe/unsubscribe/{token}`               | none    | Tokened unsub URL emailed to the subscriber. |
| `GET / DELETE /v1/status-pages/{id}/subscribers[/{sub_id}]`   | session | Admin sub list + revoke. |

Sources: [`routes/status_pages.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/status_pages.rs), [`routes/incidents.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/incidents.rs), [`routes/subscribers.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/subscribers.rs).

---

## Maintenance windows

| Method · path                                                 | Notes |
| :--- | :--- |
| `GET / POST  /v1/maintenance-windows`                         | List / create. |
| `GET / PATCH / DELETE /v1/maintenance-windows/{id}`           | Single window. |
| `POST /v1/maintenance-windows/{id}/active`                    | Toggle a window on / off. |
| `POST / DELETE /v1/maintenance-windows/{id}/monitors/{monitor_id}` | Attach / detach a monitor. |

Sources: [`routes/maintenance.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/maintenance.rs).

---

## Web push (RFC 8291)

| Method · path                  | Notes |
| :--- | :--- |
| `GET  /v1/public/webpush/vapid-key`     | Public VAPID applicationServerKey for the dashboard / status-page subscribe button. |
| `POST /v1/public/webpush/subscriptions` | `{endpoint, keys:{p256dh, auth}}` — subscriber persists into `webpush_subscriptions`. |

Source: [`routes/webpush.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/webpush.rs). The VAPID + payload encryption stack is pure-Rust (`p256` ECDH + `aes-gcm` + `hkdf`); RFC 8291 §5 known-answer test pinned in `rampart-notifier::channels::webpush_crypto::tests::rfc8291_section5_roundtrip`.

---

## Live stream + audit + ops

| Method · path                          | Notes |
| :--- | :--- |
| `GET /v1/stream/heartbeats`            | Server-Sent Events stream of heartbeats; lagging subscribers get a `lag` event. |
| `GET /v1/audit-log`                    | `?limit=&before=&kind=&action=&actor=` filters. |
| `GET /v1/audit-log/csv`                | Same filters; CSV export capped at 50 000 rows. |
| `GET / POST / DELETE /v1/api-keys[/{id}]` | API-key issuance / revocation. |
| `GET / POST / DELETE /v1/proxies[/{id}]`  | Outbound HTTP proxy registry; selectable per-monitor. |
| `POST /v1/proxies/{id}/active`         | Toggle a proxy on / off. |
| `GET / DELETE /v1/users[/{id}]`        | Admin user CRUD. |
| `POST /v1/users/{id}/admin`            | Promote / demote admin flag. |

Sources: [`routes/stream.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/stream.rs), [`routes/audit.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/audit.rs), [`routes/api_keys.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/api_keys.rs), [`routes/proxies.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/proxies.rs), [`routes/users.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/users.rs).

---

## Settings

| Method · path                       | Notes |
| :--- | :--- |
| `GET / PUT /v1/settings/smtp`       | Per-deploy SMTP credentials for the email channel. |
| `GET / PUT /v1/settings/retention`  | `{heartbeats, audit_log}` in days; the prune loop honours it on the next hourly tick. |

Sources: [`routes/health.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/routes/health.rs)-adjacent settings handlers (see `mod.rs` for the nest path).

---

## Pagination + cursoring

Endpoints that paginate use a **cursor** (the last-seen primary key or RFC 3339 timestamp), not page numbers. Two patterns:

- **Audit log**: `?before=<i64>&limit=<n>` — `before` is the integer `id` of the oldest row from the previous page.
- **Heartbeats**: `?before=<rfc3339>&limit=<n>` — `before` is the `ts` of the oldest heartbeat from the previous page.

Both return rows in **descending** order. The frontend appends each response to its accumulator and stops loading when `rows.length < limit`.

---

## Errors

Error responses use a stable JSON envelope:

```json
{ "error": "bad_request", "message": "human-readable detail" }
```

`error` codes map to:

| Code | Status | Notes |
| :--- | :----- | :---- |
| `bad_request`      | 400 | Validation error; `message` carries the field-level reason. |
| `unauthorized`     | 401 | Missing / bad session / API key. |
| `forbidden`        | 403 | Authenticated but not authorised (e.g. non-admin hitting an admin route). |
| `not_found`        | 404 | Resource missing. |
| `conflict`         | 409 | Duplicate name, optimistic-concurrency miss, etc. |
| `internal`         | 500 | Caught DB / channel-send failure. |
| `service_unavailable` | 503 | DB pool exhausted. |

Source: [`error.rs`](https://github.com/pen-pal/rampart/blob/main/backend/crates/rampart-api/src/error.rs).

---

## Conventions

- All timestamps are RFC 3339 in UTC.
- All IDs are UUIDv7 strings.
- Cookie attributes: `HttpOnly`, `SameSite=Strict`, `Secure` on HTTPS.
- Request IDs are minted by the `tower-http::request_id` layer and echoed in the `x-request-id` response header; the value also lands on every server-side log line under the `request_id` field (text formatter or JSON formatter — toggle via `RAMPART_LOG_FORMAT=json`).
- CORS allows any origin / method / header — Rampart sits behind a reverse proxy in production; per-origin restriction would be a future iteration.

---

## What this doc isn't

- **An auto-generated, exhaustive OpenAPI spec.** A hand-curated OpenAPI 3.1 description of the primary surface ships at [`docs/openapi.yaml`](./openapi.yaml) (see [OpenAPI spec](#openapi-spec) above), but it is deliberately *representative* — we don't annotate every handler with `utoipa` macros, since that maintenance cost would exceed the value when the dashboard is the only known API client. For the full, edge-case-accurate contract the route files remain the source of truth: every input / output type derives `Serialize` / `Deserialize`, so reading them as Rust gives you exactly the JSON contract.
- **A versioned commitment.** The `/v1/` prefix exists because all endpoints today live there, not because there is a v2 in the wings. Breaking changes happen in major releases; see [`CHANGELOG.md`](../CHANGELOG.md) for the exact set per version.
- **Exhaustive about every error edge.** Read the handler + the error module if you need the full surface.
