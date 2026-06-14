# Tunneling & private-network reach

> **Stance:** Rampart is **not** an inline tunnel or proxy data plane. It does
> not forward arbitrary TCP, expose private services to the dashboard live, or
> hold a persistent reverse tunnel open. That is a network product (WireGuard,
> Tailscale, ngrok, cloudflared) — distinct from a monitoring/observability one,
> and a security surface Rampart deliberately doesn't take on.

This doc exists because "tunneling" is a recurring ask. The short answer: the
monitoring need behind it — *reach a service that the Rampart server can't* — is
**already solved**, by [remote probe agents](../AGENTS.md). What's left over
after agents is genuine tunneling, and that's out of scope.

## The need, and how agents already meet it

The real requirement is almost always: *"my database / admin panel / internal
API lives in a private network the Rampart box can't route to — monitor it
anyway."*

A **probe agent** does exactly this, and it ships today:

- Deploy `rampart-agent` **inside** the private network.
- It **dials out** to the Rampart server (outbound HTTPS only) — no inbound
  holes, no VPN, no port-forward, works behind NAT / a corporate firewall.
- Assign monitors to it (`agent_id` on the monitor); the agent probes them
  locally with the same 38 probe runners and reports heartbeats back.

That is the tunneling outcome — internal target reached, zero inbound exposure —
without a tunnel. The data plane is "the agent runs the check where the target
is," not "Rampart pipes bytes into your network."

## What a real tunnel would add — and why it's out of scope

Beyond agents, a tunnel would provide:

| Capability | Verdict |
|---|---|
| Ad-hoc / interactive reach (one-off `curl` from your laptop through the network) | Out — that's an operator-VPN job, not monitoring. |
| Arbitrary TCP port-forwarding | Out — Rampart isn't a network relay; use WireGuard / Tailscale. |
| Live-proxying a private dashboard through Rampart | Out — turns the server into an inbound proxy (auth + blast-radius surface). |
| Persistent reverse tunnel held open from the network | Out — operational + security cost with no monitoring payoff agents don't already deliver. |

Each turns Rampart into a network data plane: a new authn/authz surface, a new
attack surface, and identity drift away from "self-hosted observability." The
projects that do this (WireGuard, Tailscale, ngrok, cloudflared) do it better
and are composable — run one *alongside* Rampart if you need a real tunnel.

## If you genuinely need a tunnel

1. **Recommended:** deploy a [probe agent](../AGENTS.md) — covers private-network
   monitoring with no inbound exposure.
2. Need ad-hoc/interactive reach too? Run **WireGuard or Tailscale** between the
   Rampart host and the network, or **cloudflared** for a specific service. Point
   Rampart's normal probes at the now-routable address.

## Deferred candidate (not built)

One narrow, fits-the-model extension could be worth it later: an
**agent-relayed one-off check** — trigger a single ad-hoc probe through a chosen
agent from the UI (e.g. "test this URL from agent X right now") without creating
a persistent monitor. It reuses the agent's existing outbound channel and probe
runners, adds no new data plane, and scratches the "let me poke that thing from
inside the network" itch. Tracked as a candidate; deliberately not in scope for
the monitoring core today.
