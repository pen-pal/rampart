# Cross-tier correlation

Rampart's observability tiers are not silos — each signal links to the others
that share its context, so you can pivot from a symptom to its cause without
copy-pasting ids between tools. This page maps the full correlation web.

## The links

| From | To | How |
| :--- | :--- | :--- |
| **Log line** | Trace | A log carrying a `trace_id` links straight to that trace's waterfall; the `span_id` is shown alongside. |
| **Trace** | Logs | The trace detail view embeds the logs emitted under its `trace_id`. |
| **Error issue** | Trace | An error event with trace context (`contexts.trace.trace_id`) links to the originating trace. |
| **Trace** | Errors | The trace detail view lists error issues touched by that trace. |
| **Trace span** | Profiling | Each span deep-links to a flamegraph scoped to its service and exact `[start, end]` window (epoch-ms). |
| **RUM page-load** | Trace | A beacon carrying a backend `trace_id` links the browser page-load to its server trace. |
| **Service-map edge** | Traces | Clicking a caller → callee edge opens the traces list filtered to that callee service. |

These are bidirectional where it makes sense (log ↔ trace, error ↔ trace), so
a pivot in one direction has a return path.

## How the ids flow

- **`trace_id` / `span_id`** ride on OTLP spans and on log records (OTLP logs set
  them from the active span). The error tier captures them from the Sentry
  envelope's trace context. The browser RUM snippet picks up a `trace_id`
  best-effort from `window.__rampartTraceId` or a `<meta name="traceparent">`
  (the RUM snippet) so a page-load can name its backend trace.
- **Service + absolute time** is the join for the trace → profiling pivot:
  profiles are stored folded with timestamps, so a span's `[start, end]` selects
  exactly the samples taken while that span ran (`from_ms` / `to_ms` on
  `/v1/profiles/flamegraph`).
- **Service name** joins spans into the dependency map (cross-service
  parent/child span pairs) and back out to the filtered trace list.

## Why it matters

The point of one tool holding all the tiers is that the joins are free. A
log-volume alert → open the noisy logs → jump to a slow trace → see the SQL in
the waterfall → profile that span's window → find the hot function. Or: an SLO
budget burns → the metric/monitor behind it → the traces for that service →
the error issue spiking. No id juggling, no second pane of glass.

## Related

- [Traces](design/TRACES.md) · [Logs](design/LOGS.md) · [Error tracking](design/ERROR-TRACKING.md)
- [RUM](design/RUM.md) · [Profiling](design/PROFILING.md) · [SLOs & error budgets](SLOS.md)
