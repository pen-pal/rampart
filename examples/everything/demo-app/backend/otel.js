// OpenTelemetry + Sentry + profiling bootstrap. Required via `node -r ./otel.js`
// so it initialises before the app loads. Ships the demo backend's REAL
// telemetry to Rampart:
//   - traces:    auto-instrumented (http, express, pg, ioredis) → /otlp/v1/traces
//   - logs:      OTLP logs (incl. the SIEM auth-fail lines)      → /otlp/v1/logs
//   - profiling: periodic V8 CPU profile → folded text          → /profiles/v1/folded
//   - errors:    @sentry/node, DSN captured from a real Rampart error-project
//
// All endpoints come from env that the `provision` one-shot writes into the
// shared volume (RAMPART_OTLP, RAMPART_PROFILES, SENTRY_DSN).
const { NodeSDK } = require('@opentelemetry/sdk-node');
const { getNodeAutoInstrumentations } = require('@opentelemetry/auto-instrumentations-node');
const { OTLPTraceExporter } = require('@opentelemetry/exporter-trace-otlp-http');
const { OTLPLogExporter } = require('@opentelemetry/exporter-logs-otlp-http');
const { BatchLogRecordProcessor } = require('@opentelemetry/sdk-logs');
const { Resource } = require('@opentelemetry/resources');
const { ATTR_SERVICE_NAME } = require('@opentelemetry/semantic-conventions');
const logsApi = require('@opentelemetry/api-logs');

const SERVICE = process.env.OTEL_SERVICE_NAME || 'demo-backend';
const BASE = (process.env.RAMPART_OTLP || 'http://rampart:3000/otlp').replace(/\/$/, '');
const PROFILES_BASE = (process.env.RAMPART_PROFILES || 'http://rampart:3000/profiles').replace(/\/$/, '');
const resource = new Resource({ [ATTR_SERVICE_NAME]: SERVICE });

// ── errors: @sentry/node → Rampart's Sentry-compatible ingest ───────────────
// SENTRY_DSN is written by `provision` (real error-project public_key + id).
// Without a DSN this is a no-op so the app still boots standalone.
let Sentry = null;
if (process.env.SENTRY_DSN) {
  try {
    Sentry = require('@sentry/node');
    // Rampart mints error-project ids as UUIDs (e.g. 019edc0a-5f23-…) and its
    // documented DSN is `http://<public_key>@<host>/<uuid>`. But the Sentry
    // SDK's DSN parser only keeps the LEADING NUMERIC run of the project id —
    // it turns `019edc0a-…` into just `019` — so the SDK would POST to
    // `/api/019/envelope/` and Rampart returns 404 (no such project). To deliver
    // backend errors reliably we route the envelope through Sentry's `tunnel`:
    // the SDK posts the raw envelope VERBATIM to the tunnel URL (no project-id
    // mangling). We point the tunnel at the full-UUID envelope endpoint and
    // carry the public key as `?sentry_key=` (the tunnel transport doesn't send
    // the X-Sentry-Auth header, so Rampart authenticates off the query param).
    const m = process.env.SENTRY_DSN.match(/^(https?):\/\/([^@]+)@([^/]+)\/(.+)$/);
    const tunnel = m
      ? `${m[1]}://${m[3]}/api/${m[4]}/envelope/?sentry_key=${m[2]}`
      : undefined;
    Sentry.init({
      dsn: process.env.SENTRY_DSN,
      tunnel,
      // Rampart's ingest is plain HTTP on the docker network — disable TLS checks
      // are not needed (http), but keep the SDK lean.
      tracesSampleRate: 0,
      environment: process.env.NODE_ENV || 'production',
      release: 'demo-backend@1.0.0',
      defaultIntegrations: false,
      // CRITICAL: @sentry/node v8 is built on OpenTelemetry and, by default,
      // registers its OWN global OTel tracer/context/propagation during init.
      // That hijacks the API before our NodeSDK starts ("duplicate registration
      // of API: trace/context/propagation"), so the auto-instrumented Express/
      // pg/redis spans flow into Sentry's no-op tracer instead of our OTLP
      // exporter — demo-backend traces never reach Rampart. We use Sentry ONLY
      // as an error reporter here, so tell it to leave OTel alone and let the
      // NodeSDK own tracing.
      skipOpenTelemetrySetup: true,
    });
    console.log('[otel] Sentry initialised → ' + process.env.SENTRY_DSN.replace(/\/\/.*@/, '//***@')
      + (tunnel ? ' (tunnel)' : ''));
  } catch (e) {
    console.log('[otel] Sentry init failed: ' + e.message);
  }
}
global.Sentry = Sentry;

// ── traces + logs ────────────────────────────────────────────────────────
// IMPORTANT: the OTLP *logs* exporter MUST be handed to the NodeSDK via
// `logRecordProcessors`. NodeSDK.start() registers its OWN global
// LoggerProvider (an env-default one that ships to localhost:4318), and the
// OpenTelemetry logs API silently refuses to overwrite an already-registered
// global provider — so a later `logs.setGlobalLoggerProvider(...)` is a no-op
// and every demo-backend log line would be dropped (ECONNREFUSED → :4318,
// never reaching Rampart). Passing the processor into the SDK makes Rampart's
// exporter the global provider that `logs.getLogger()` resolves to.
const sdk = new NodeSDK({
  resource,
  traceExporter: new OTLPTraceExporter({ url: BASE + '/v1/traces' }),
  logRecordProcessors: [
    new BatchLogRecordProcessor(new OTLPLogExporter({ url: BASE + '/v1/logs' })),
  ],
  instrumentations: [getNodeAutoInstrumentations()],
});
sdk.start();

const otelLogger = logsApi.logs.getLogger(SERVICE);
const SEV = { INFO: 9, WARN: 13, ERROR: 17 };
// Exposed to the app for structured logging (the SIEM auth lines use this).
global.rlog = (severityText, body, attributes = {}) => {
  otelLogger.emit({ severityNumber: SEV[severityText] || 9, severityText, body, attributes });
  console.log(`[${severityText}] ${body}`);
};

// ── profiling ───────────────────────────────────────────────────────────────
// Take a short V8 CPU profile every 30s, fold it (Brendan-Gregg `stack count`),
// and POST to Rampart's folded ingest. Pure built-ins (node:inspector + http).
const inspector = require('node:inspector');
const http = require('node:http');

function foldCpuProfile(profile) {
  const byId = new Map(profile.nodes.map((n) => [n.id, n]));
  const frame = (n) => {
    const f = n.callFrame;
    const fn = f.functionName || '(anonymous)';
    const file = (f.url || '').split('/').pop();
    return file ? `${fn} (${file}:${f.lineNumber + 1})` : fn;
  };
  const parent = new Map();
  for (const n of profile.nodes) for (const c of n.children || []) parent.set(c, n.id);
  const stackOf = (id) => {
    const frames = [];
    let cur = id;
    while (cur != null) {
      const n = byId.get(cur);
      if (!n) break;
      frames.unshift(frame(n));
      cur = parent.get(cur);
    }
    return frames.join(';');
  };
  const lines = [];
  for (const n of profile.nodes) {
    if (n.hitCount > 0) lines.push(`${stackOf(n.id)} ${n.hitCount}`);
  }
  return lines.join('\n');
}

function postFolded(folded) {
  if (!folded) return;
  const u = new URL(`${PROFILES_BASE}/v1/folded?service=${encodeURIComponent(SERVICE)}&type=cpu`);
  const req = http.request(
    { method: 'POST', hostname: u.hostname, port: u.port || 80, path: u.pathname + u.search,
      headers: { 'content-type': 'text/plain', 'content-length': Buffer.byteLength(folded) } },
    (res) => res.resume(),
  );
  req.on('error', () => {});
  req.write(folded);
  req.end();
}

function profileOnce() {
  const session = new inspector.Session();
  try { session.connect(); } catch { return; }
  session.post('Profiler.enable', () => {
    session.post('Profiler.start', () => {
      setTimeout(() => {
        session.post('Profiler.stop', (err, { profile } = {}) => {
          if (!err && profile) { try { postFolded(foldCpuProfile(profile)); } catch {} }
          try { session.disconnect(); } catch {}
        });
      }, 3000); // sample 3s
    });
  });
}
setInterval(profileOnce, 30000);
setTimeout(profileOnce, 8000);

// flush on exit. sdk.shutdown() now flushes BOTH traces and logs (the log
// processor is owned by the NodeSDK), so no separate provider shutdown needed.
process.on('SIGTERM', async () => {
  try { await sdk.shutdown(); } catch {}
  process.exit(0);
});
