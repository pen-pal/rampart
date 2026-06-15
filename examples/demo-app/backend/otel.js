// OpenTelemetry bootstrap — required via `node -r ./otel.js` so it initialises
// before the app loads. Ships the demo backend's real telemetry to Rampart:
//   - traces: auto-instrumented (http, express, pg, ioredis) → /otlp/v1/traces
//   - logs:   OTLP logs (incl. the SIEM auth-fail lines) → /otlp/v1/logs
//   - profiling: periodic V8 CPU profile → folded text → /profiles/v1/folded
const { NodeSDK } = require('@opentelemetry/sdk-node');
const { getNodeAutoInstrumentations } = require('@opentelemetry/auto-instrumentations-node');
const { OTLPTraceExporter } = require('@opentelemetry/exporter-trace-otlp-http');
const { OTLPLogExporter } = require('@opentelemetry/exporter-logs-otlp-http');
const { LoggerProvider, BatchLogRecordProcessor } = require('@opentelemetry/sdk-logs');
const { Resource } = require('@opentelemetry/resources');
const { ATTR_SERVICE_NAME } = require('@opentelemetry/semantic-conventions');
const logsApi = require('@opentelemetry/api-logs');

const SERVICE = process.env.OTEL_SERVICE_NAME || 'demo-backend';
const BASE = (process.env.RAMPART_OTLP || 'http://rampart:3000/otlp').replace(/\/$/, '');
const PROFILES_BASE = (process.env.RAMPART_PROFILES || 'http://rampart:3000/profiles').replace(/\/$/, '');
const resource = new Resource({ [ATTR_SERVICE_NAME]: SERVICE });

// ── traces ─────────────────────────────────────────────────────────────────
const sdk = new NodeSDK({
  resource,
  traceExporter: new OTLPTraceExporter({ url: BASE + '/v1/traces' }),
  instrumentations: [getNodeAutoInstrumentations()],
});
sdk.start();

// ── logs ─────────────────────────────────────────────────────────────────
const loggerProvider = new LoggerProvider({ resource });
loggerProvider.addLogRecordProcessor(
  new BatchLogRecordProcessor(new OTLPLogExporter({ url: BASE + '/v1/logs' })),
);
logsApi.logs.setGlobalLoggerProvider(loggerProvider);
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
  // stack for a node = path from root → node
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

// flush on exit
process.on('SIGTERM', async () => {
  try { await sdk.shutdown(); } catch {}
  try { await loggerProvider.shutdown(); } catch {}
  process.exit(0);
});
