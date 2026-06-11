# Remote probe agents

A **probe agent** is a lightweight `rampart-agent` worker you run somewhere
other than the Rampart server — another region, another cloud, or a private
network segment behind a firewall. The agent probes the monitors assigned
to it using the same probe runners the server uses (all 38 kinds), and
reports heartbeats back over the HTTP API.

Why you'd want one:

- **Multi-location checks** — probe your service from where your users
  are, not just from the box Rampart happens to run on.
- **Private-network monitoring** — monitor an internal database or admin
  panel that the Rampart server can't reach. The agent runs inside the
  network and **dials out** to the server; no inbound holes, no VPN.

## How it works

```
┌──────────────┐   GET  /v1/agent/monitors    ┌─────────────────┐
│ rampart-agent│ ───────────────────────────► │ Rampart server  │
│  (eu-west)   │ ◄─────────────────────────── │                 │
│              │   POST /v1/agent/heartbeats  │  scheduler skips│
│ probes run   │ ───────────────────────────► │  agent-assigned │
│ locally      │                              │  monitors       │
└──────────────┘                              └─────────────────┘
```

- The agent **pulls** its assignment list every 30s (configurable) and
  runs one probe loop per monitor on the monitor's own interval.
- Results are **batched** (1s / 100 results) and POSTed back. The server
  stamps receive time — agent clocks are never trusted.
- Reported heartbeats flow through the **same pipeline** as local probes:
  status-flip notifications, SLO breach detection, per-monitor result
  webhooks, SSE live streams, retention — all identical.
- The server's scheduler **skips** agent-assigned monitors, so nothing is
  probed twice.

### Liveness + the stale-agent watchdog

Every pull/report bumps the agent's `last_seen_at`; the dashboard shows an
agent **online** while it has polled within the last 90 seconds.

If an assigned monitor receives no heartbeat for **2× its interval + 30s**
(agent crashed, link down), the server synthesizes a `Down` heartbeat —
`no report from agent "<name>"` — and fires the monitor's notification
channels exactly as a real outage would. An unplugged agent pages you; it
never fails silent.

## Setting up an agent

1. **Register it** (admin): dashboard → *Agents* → *New agent*, or:

   ```bash
   curl -X POST https://status.example.com/v1/agents \
     -H "Authorization: Bearer rmp_<admin api key>" \
     -H "Content-Type: application/json" \
     -d '{"name": "eu-west-1", "location": "Hetzner FSN"}'
   ```

   The response contains the agent token (`rmpa_…`) **exactly once** —
   only its SHA-256 hash is stored. Copy it now.

2. **Run the binary** on the remote box:

   ```bash
   RAMPART_URL=https://status.example.com \
   RAMPART_AGENT_TOKEN=rmpa_… \
   rampart-agent
   ```

   Build it from the workspace with `cargo build --release -p rampart-agent`
   — it's a single static-ish binary with the same pure-Rust TLS stack as
   the server (no OpenSSL on the box required).

   | Env var | Default | Meaning |
   |---------|---------|---------|
   | `RAMPART_URL` | — (required) | Base URL of the Rampart server |
   | `RAMPART_AGENT_TOKEN` | — (required) | The `rmpa_…` token from registration |
   | `RAMPART_AGENT_POLL_SECS` | `30` | How often to re-pull assignments |
   | `RAMPART_AGENT_HOST_METRICS_SECS` | `60` | Host metric cadence; `0` disables |
   | `RUST_LOG` | `rampart_agent=info,info` | Log filter |

3. **Assign monitors**: open a monitor's *Edit* modal (or step 3 of the
   creation wizard) and pick the agent under **Probe agent**. Set it back
   to *Local* to return the monitor to the server's own scheduler.

## Host metrics

The agent doubles as a host monitor: every 60 seconds
(`RAMPART_AGENT_HOST_METRICS_SECS`, `0` disables) it samples the box it
runs on and pushes the result through `POST /v1/agent/metrics`:

| Metric | Meaning |
|--------|---------|
| `host_cpu_pct` | global CPU usage % |
| `host_mem_used_pct` / `host_mem_total_bytes` / `host_mem_used_bytes` | memory |
| `host_disk_used_pct{mount="…"}` / `host_disk_total_bytes{mount="…"}` | per real filesystem |
| `host_load1` / `host_load5` / `host_load15` | load averages |
| `host_uptime_seconds` | time since boot |

Every sample is stored with an `agent="<name>"` label injected
server-side, so two agents' metrics never collide and a threshold rule
can target one host: metric `host_disk_used_pct`, labels
`{"mount": "/", "agent": "eu-west-1"}`. Charts live in the dashboard's
**Metrics** view; alerting works through the standard
[threshold rules](METRICS.md#threshold-alert-rules).

## Semantics worth knowing

- **Push monitors can't be assigned** — they're inbound-only by
  definition; the API rejects the assignment with `400`.
- **Proxies are not applied on agents** (yet): a monitor's `proxy_id` is
  ignored when probed by an agent — the agent connects directly. Assign
  proxied monitors to the local scheduler if you need the proxy hop.
- **Revoking an agent** kills its token immediately and returns its
  monitors to local probing (the dashboard shows them as locally probed;
  the scheduler picks them up within seconds).
- **Maintenance windows** are enforced server-side at ingestion: results
  reported during an active window are recorded as `Maintenance` and
  never alert, same as local probes.
- **Editors** can see the agent list (it feeds the assignment picker);
  only **admins** can register, rename, or revoke agents.

## API surface

| Route | Auth | Purpose |
|-------|------|---------|
| `GET /v1/agents` | session/key (editor+) | List agents + liveness |
| `POST /v1/agents` | session/key (admin) | Register; returns token once |
| `PATCH /v1/agents/{id}` | session/key (admin) | Rename / relocate |
| `DELETE /v1/agents/{id}` | session/key (admin) | Revoke |
| `GET /v1/agent/monitors` | `Bearer rmpa_…` | Pull assigned monitor specs |
| `POST /v1/agent/heartbeats` | `Bearer rmpa_…` | Report a result batch |
| `POST /v1/agent/metrics` | `Bearer rmpa_…` | Push host/custom metrics |

See [`openapi.yaml`](./openapi.yaml) for request/response shapes.
