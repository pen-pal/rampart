# Security-review agent

A scheduled GitHub Actions job (`.github/workflows/security-review.yml`)
that runs Claude Code once a week to do the security review our other
automation **can't**: reasoning about authorization, the token-auth
surfaces, and the new attack surface introduced by recent changes. It
files findings as GitHub issues and **never touches code** — a human
triages and fixes.

## Why this exists alongside the existing scanners

We already gate the build on the mechanical layer:

| Tool | Cron | Catches |
| :--- | :--- | :--- |
| `cargo deny` (`security-audit.yml`) | weekly Wed | RUSTSEC advisories, license/ban policy |
| CodeQL (`codeql.yml`) | weekly Sat | injection-shaped bugs in the JS/TS surface |
| `dependency-review` | per-PR | risky dependency diffs |

None of those reason about **logic**. CodeQL won't tell you a `readonly`
API key can reach a writer path, or that a handler trusts a client-supplied
id without an ownership check, or that a new endpoint widened the
unauthenticated surface. That judgement is what this agent adds. It does
**not** re-run dependency scanning — that would duplicate `cargo deny`.

## What it reviews

Each run is scoped to **the diff since the last run** (trailing ~7 days),
not the whole tree — review cost scales with change, not codebase size.
Within that diff it prioritises, in order:

1. **Authorization / RBAC.** The `require_admin` / `require_editor` /
   `require_write_or_readonly_get` middleware and the read/write/admin
   API-key scopes. Did a new route land in the wrong RBAC group, or skip
   the layer entirely?
2. **Token-auth surfaces.** Agent tokens (`rmpa_…`), push tokens, ingest
   tokens, API keys — generation (CSPRNG?), hashing-at-rest, comparison
   (constant-time?), and rotation/revocation.
3. **Broken object-level authorization (IDOR).** Single-tenant means no
   `workspace_id` scoping, so the guard is "is this actor allowed to act
   on this resource id" — easy to miss on new CRUD (escalation, metrics,
   on-call, monitors).
4. **Unauthenticated surface.** Public status pages and the `/push` and
   metric/heartbeat ingest endpoints take input from anyone — check for
   injection, unbounded input, and information disclosure.
5. **SSRF.** Rampart makes outbound requests to operator-supplied URLs
   (HTTP probes, result webhooks, the probe runners). New code that
   fetches a user-controlled URL is an SSRF candidate.
6. **Secrets handling.** Channel configs, SMTP creds, tokens — encrypted
   at rest, never logged, never returned in API responses.

It also **re-checks `docs/SECURITY-DEBT.md`**: for each accepted advisory,
has the upstream blocker shipped the fix yet (e.g. `rumqttc` on `rustls`
0.23, a `scylla` ring-only feature)? This automates the "revisit on every
`cargo update`" chore we do by hand today.

## What it does NOT do — by design

- **It never modifies code, commits, or opens a code PR.** An autonomous
  job with write access that patches security-sensitive paths on a cron is
  itself an attack surface, and a wrong "fix" on an auth path is worse than
  the gap. Output is findings only; a human decides and fixes.
- **It doesn't re-run `cargo deny` / CodeQL / dependency-review.** Those
  own the mechanical layer.
- **It doesn't pen-test a running instance.** This is static + reasoning
  review of the code. DAST against a live deployment is a separate,
  infra-heavier effort (ephemeral instance + ZAP/nuclei) and is a possible
  future addition, not part of this job.

## Output

One GitHub issue per run **only when there's something to report**
(a finding at Medium+ severity, or a change in `SECURITY-DEBT` blocker
status). Each finding carries a severity (Critical / High / Medium / Low),
`file:line`, the reasoning or reproduction, and a suggested remediation.
The job de-dupes against existing open issues labelled `security-review`
rather than re-filing the same finding every week. A clean run writes to
the Actions run summary and opens nothing.

## Setup

The workflow needs one repository secret: **`ANTHROPIC_API_KEY`**
(Settings → Secrets and variables → Actions). Until it's set the job runs
but the action step fails — that's the only manual step. Trigger an
on-demand run via the Actions tab ("Run workflow", `workflow_dispatch`) to
test before relying on the weekly cron.

## Tuning

- **Cadence** — `schedule.cron` in the workflow (default Monday 08:00 UTC).
- **Model** — `--model` in `claude_args`; bump to a more capable tier for
  deeper review at higher cost.
- **Scope window** — the prompt's "since last run" lookback; widen for a
  periodic full-tree audit.
