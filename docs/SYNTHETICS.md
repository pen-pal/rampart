# Synthetic transactions

A **synthetic** monitor runs an ordered sequence of HTTP steps instead of a
single request. Each step makes a request, optionally pulls values out of the
response into named variables, and asserts on the response; variables flow
into later steps via `{{name}}`. This covers the classic multi-step check a
single HTTP monitor can't: **log in → capture a token → call an authed API →
assert the result.**

```
step 1  POST /login        assert status == 200
        extract token ← json data.token
   │
step 2  GET  /me           header Authorization: Bearer {{token}}
        assert json data.active == true
        assert body contains "welcome"
```

It's just another monitor kind, so it rides the whole existing pipeline —
retries, notifications, SLO, result webhooks, status pages, the response-time
chart (which plots total sequence wall-clock).

## The model

The step list lives in the monitor's `config.steps` JSONB (no new columns).
Each step:

| Field | Meaning |
| :--- | :--- |
| `name` | Optional label, shown in failure messages ("step 2 (login)"). |
| `method` | HTTP method. |
| `url` | Request URL. `{{var}}` placeholders allowed. |
| `headers` | Object of header → value; values may contain `{{var}}`. |
| `body` | Optional request body; may contain `{{var}}`. |
| `extract` | Values pulled from the response into variables (below). |
| `assert` | Pass/fail checks against the response (below). |

**Extractions** (`extract`) — each `{ var, from, path }`:
- `from: "json"` — `path` is a dotted/indexed path into the JSON body
  (`data.items.0.id`). Array indices are numeric segments.
- `from: "header"` — `path` is the response header name (case-insensitive).
- `from: "status"` — the numeric status code (`path` ignored).

A missing extraction leaves the variable unset, so its `{{name}}` stays
literal in later steps — the failure is visible rather than silently blank.

**Assertions** (`assert`) — each `{ kind, path, op, value }`:
- `kind: "status"` — compare the status code (`path` ignored).
- `kind: "json"` — compare the value at `path` in the JSON body.
- `kind: "header"` — compare the response header named by `path`.
- `kind: "body_contains"` — raw-body substring match (`op`/`path` ignored).
- `op` is one of `eq`, `ne`, `lt`, `lte`, `gt`, `gte`, `contains`. The
  ordering ops parse both sides as numbers; `eq`/`ne`/`contains` are string
  ops. `op` defaults to `eq`.

## Execution semantics

- Steps run **in order**, carrying a shared variable bag.
- A step **sends → extracts → asserts**. The **first failed assertion** (or a
  transport error / timeout) stops the run and produces a Down heartbeat whose
  message names the failing step and reason, e.g.
  `step 2 (me): json data.active == "true" (got "false")`.
- A clean sweep is **Up**, with `latency_ms` = total wall-clock across all
  steps and `status_code` = the last step's code.
- `timeout_seconds` applies **per step**. `follow_redirect` and `ignore_tls`
  (monitor settings) apply to every step.

## Building one

In the dashboard, pick **Synthetic transaction** in the new-monitor wizard and
add steps with the inline builder (request, extractions, assertions). Or POST
to `/v1/monitors` directly with `kind: "synthetic"` and a `config.steps` array:

```jsonc
{
  "name": "Checkout flow",
  "kind": "synthetic",
  "interval_seconds": 300,
  "timeout_seconds": 15,
  "config": {
    "steps": [
      {
        "name": "login",
        "method": "POST",
        "url": "https://api.example.com/login",
        "headers": { "Content-Type": "application/json" },
        "body": "{\"user\":\"probe\",\"pass\":\"…\"}",
        "extract": [{ "var": "token", "from": "json", "path": "data.token" }],
        "assert": [{ "kind": "status", "op": "eq", "value": "200" }]
      },
      {
        "name": "me",
        "method": "GET",
        "url": "https://api.example.com/me",
        "headers": { "Authorization": "Bearer {{token}}" },
        "assert": [
          { "kind": "json", "path": "data.active", "op": "eq", "value": "true" },
          { "kind": "body_contains", "value": "welcome" }
        ]
      }
    ]
  }
}
```

Bounds: at most **20 steps**, and at most **15 combined extract+assert rules**
per step. Config is validated lazily at probe time (matching `cron` /
`json_query`): a malformed `config.steps` surfaces as a Down heartbeat with a
clear message rather than being rejected at create time.

## v1 limitations (follow-ups)

- **No automatic cookie jar.** Carry session state explicitly: extract the
  token/cookie value and interpolate it into a later header. (A per-run cookie
  jar is a planned addition.)
- **`upside_down` is not applied** to synthetic monitors — it has no clean
  meaning for a multi-assertion sequence.
- **Editing** the step sequence after creation isn't in the monitor edit modal
  yet; recreate, or PATCH `config` via the API.
- JSON paths are dotted/indexed only (no filters/wildcards), matching the
  `json_query` helper.
