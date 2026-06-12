# On-call schedules

An **on-call schedule** is a rotation: an ordered ring of notification
channels that hand off the pager on a fixed cadence. Escalation steps
point at a schedule instead of (or alongside) fixed channels, so "page
whoever is on call this week" stays correct as the rotation turns —
without editing the policy every Monday.

```
schedule "primary"  ──  weekly handoff, anchored Mon 2026-06-15 09:00 UTC
  participant ring: [ Alice-SMS, Bob-SMS, Carol-SMS ]

  week of Jun 15 → Alice-SMS on call
  week of Jun 22 → Bob-SMS
  week of Jun 29 → Carol-SMS
  week of Jul 06 → Alice-SMS  (ring wraps)
```

## Channels, not people

Rampart addresses notifications by **channel** — there is no user→contact
link in the data model. So a rotation rotates *channels*, not users. In
practice you make one channel per responder (Alice's SMS, Bob's Pushover,
the team's PagerDuty integration) and list them in rotation order. The
channel is the pager; the schedule decides which pager is live.

## How "who's on call" is computed

There are no per-shift rows to generate, store, or prune. On-call is pure
arithmetic over three fields:

| Field | Meaning |
| :--- | :--- |
| `anchor` | UTC instant `participant_ids[0]`'s first shift begins. |
| `rotation_seconds` | Handoff cadence — how long each shift lasts (300 s – 365 days). |
| `participant_ids` | The rotation ring, in order (1–50 channels). |

```
period  = floor((T − anchor) / rotation_seconds)
on_call = participant_ids[ period mod len(participant_ids) ]
```

Because the division is euclidean, a schedule resolves for **any** instant
— including before the anchor (it wraps backward) — so there is never a
"gap" with no one on call. `GET /v1/on-call-schedules/{id}/current` returns
who's live right now.

To land handoffs at a civilised local time (e.g. Monday 09:00, not
midnight), set the `anchor` to the first handoff instant you want in UTC;
every later handoff rolls forward from there. Daylight-saving shifts aren't
tracked — the cadence is a fixed number of seconds.

## Wiring a schedule into an escalation ladder

A schedule does nothing on its own; it becomes live when an
[escalation step](ESCALATIONS.md) references it. Each step carries both
`channel_ids` (fixed) and `schedule_ids` (rotating); a step needs at least
one of the two. When the step fires, every schedule in `schedule_ids` is
resolved to its current on-call channel and paged alongside the fixed
channels.

```jsonc
// escalation policy "production"
"steps": [
  { "wait_seconds": 0,    "schedule_ids": ["<primary>"] },          // page on-call now
  { "wait_seconds": 600,  "schedule_ids": ["<primary>", "<secondary>"] },
  { "wait_seconds": 1800, "channel_ids":  ["<team-slack>"] }        // whole team
]
```

Resolution happens **at page time**, so rotating the schedule (or fixing a
typo'd participant) takes effect on the next page with no change to the
policy.

## Edge cases

- **Deleted or emptied schedule.** A step pointing at a schedule that no
  longer resolves (deleted, or its ring emptied) logs a warning and is
  skipped — exactly like an unresolvable channel id. The rest of the
  step's targets still page; the ladder never hard-fails on a bad
  reference.
- **Single participant.** A one-channel ring is always-on-call — a
  perfectly valid "no rotation yet, just me" schedule.
- **Overrides / shift swaps** ("I'll cover for you Saturday") are **not**
  in this version. The rotation is purely cadence-based. Planned
  follow-up.

## API

All routes are editor-gated (readonly users can `GET`):

| Method & path | Purpose |
| :--- | :--- |
| `GET /v1/on-call-schedules` | List schedules. |
| `POST /v1/on-call-schedules` | Create — `{name, rotation_seconds, anchor, participant_ids}`. |
| `PATCH /v1/on-call-schedules/{id}` | Update any subset of the fields. |
| `DELETE /v1/on-call-schedules/{id}` | Delete. |
| `GET /v1/on-call-schedules/{id}/current` | `{ "on_call": "<channel-uuid>" }` right now. |

`anchor` is RFC 3339 (UTC); `participant_ids` are notification-channel ids
from `GET /v1/notifications`.
