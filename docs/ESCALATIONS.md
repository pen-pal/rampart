# Escalation policies

An **escalation policy** turns "blast every channel at once" into an
ordered ladder: page the on-call channel immediately, and only wake the
wider team if nobody reacts. PagerDuty semantics, sized for a team that
self-hosts.

```
monitor goes Down
   │
   ▼ immediately
 step 1  →  #oncall (Slack) + on-call SMS
   │  10 min, unacknowledged
   ▼
 step 2  →  team email + manager push
   │  30 min, unacknowledged
   ▼
 step 3  →  everything with a pulse
```

## How it behaves

- A monitor that references a policy routes its **Down-flips through the
  ladder** instead of its regular attached/tag-routed channels. (Its SLO
  and maintenance events keep their normal routing; monitors without a
  policy are completely unaffected.)
- **Step 1 fires the moment the monitor flips Down.** Each later step
  fires `wait_seconds` after the previous one — unless someone
  **acknowledges** or the monitor **recovers** first.
- **Acknowledge** (button on the monitor page, or
  `POST /v1/monitors/{id}/escalation/ack`) stops the climb and records
  who took it. The monitor staying Down after an ack pages no one else.
- **Recovery auto-resolves** the episode and sends a recovery notice to
  every step that was already paged — including after an ack.
- Escalation pages are **direct sends**: they bypass digest coalescing
  and per-channel quiet hours by design. A page that waits for a digest
  window is not a page. (Per-channel cooldown/rate-limit don't apply on
  this path either — the ladder's waits are the pacing.)
- One **episode** per monitor at a time (a database invariant): flapping
  can't stack ladders. A fresh outage after recovery opens a fresh
  episode from step 1.
- Dependency suppression still applies upstream: if the monitor's parent
  is down, no episode opens — one root cause, one ladder.

## Setting it up

1. **Create a policy**: dashboard → *Escalations*, or:

   ```json
   POST /v1/escalation-policies
   {
     "name": "ops ladder",
     "steps": [
       { "wait_seconds": 0,    "channel_ids": ["<oncall-slack>", "<oncall-sms>"] },
       { "wait_seconds": 600,  "channel_ids": ["<team-email>"] },
       { "wait_seconds": 1800, "channel_ids": ["<everyone>"] }
     ]
   }
   ```

   Rules: 1–10 steps; every step needs at least one channel; step 1's
   wait must be `0` (it fires at open); later waits are 0–86400s.

2. **Attach it to monitors**: the *Escalation policy* select in the
   monitor edit modal / creation wizard, or `escalation_policy_id` on
   the monitor API (`null` detaches).

3. **Acknowledge from the monitor page** when an episode banner shows.

Deleting a policy returns its monitors to regular channel fan-out and
discards its open episodes. Editing a policy applies to *future* steps
of running episodes (the ladder re-reads the policy at each advance).

## Notes

- Escalation sends appear in the **delivery log** with event kind
  `escalation`, so the audit trail of who was paged when is queryable.
- The advance scan runs on the scheduler's ~30s tick; step waits are
  honoured with that granularity.
- If the process restarts mid-episode, state is in Postgres — the ladder
  resumes where it left off. An episode whose monitor recovered during
  downtime is closed (not paged) on the next scan.
