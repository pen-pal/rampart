// Demo backend — every request is auto-traced (express → pg/redis spans) and
// flows to Rampart. Also emits structured logs, SIEM auth-failure lines, real
// errors (captured by @sentry/node), Prometheus /metrics, and a battery of
// toggleable endpoints that Rampart's OWN probes hit so up/down/latency/keyword/
// json_query/synthetic monitors all reflect genuine behaviour.
//
// Run as `node -r ./otel.js server.js` so instrumentation loads first.
const express = require('express');
const { Pool } = require('pg');
const Redis = require('ioredis');
const client = require('prom-client');

const log = global.rlog || ((lvl, m) => console.log(`[${lvl}] ${m}`));
const Sentry = global.Sentry;
const pool = new Pool({ connectionString: process.env.DATABASE_URL || 'postgres://demo:demo@demo-db:5432/demo' });
const redis = new Redis(process.env.REDIS_URL || 'redis://demo-redis:6379');

// ── Prometheus metrics ───────────────────────────────────────────────────────
const registry = new client.Registry();
client.collectDefaultMetrics({ register: registry });
const httpReqs = new client.Counter({
  name: 'demo_http_requests_total', help: 'HTTP requests', labelNames: ['route', 'status'], registers: [registry],
});
const checkoutCents = new client.Counter({
  name: 'demo_checkout_cents_total', help: 'Total cents charged', registers: [registry],
});
// A gauge that climbs and stays high — the pushed metric rule keys on a series
// like this. We also publish it so Prometheus scrapes it.
const queueDepth = new client.Gauge({
  name: 'demo_queue_depth', help: 'Pending job queue depth', labelNames: ['service'], registers: [registry],
});
let q = 0;
setInterval(() => { q = 30 + Math.floor(Math.random() * 90); queueDepth.set({ service: 'demo-backend' }, q); }, 5000);

async function initDb() {
  for (let i = 0; i < 30; i++) {
    try {
      await pool.query(`CREATE TABLE IF NOT EXISTS products (
        id SERIAL PRIMARY KEY, name TEXT NOT NULL, price_cents INT NOT NULL)`);
      const { rows } = await pool.query('SELECT count(*)::int AS n FROM products');
      if (rows[0].n === 0) {
        await pool.query(
          `INSERT INTO products (name, price_cents) VALUES
           ('Widget', 1999), ('Gadget', 4999), ('Gizmo', 999), ('Doohickey', 12999)`,
        );
      }
      log('INFO', 'database ready');
      return;
    } catch (e) {
      await new Promise((r) => setTimeout(r, 1000));
    }
  }
  log('ERROR', 'database never became ready');
}

const app = express();
app.use(express.json());
// per-request metric
app.use((req, res, next) => {
  res.on('finish', () => httpReqs.inc({ route: req.path.replace(/\d+/g, ':id'), status: res.statusCode }));
  next();
});

// ── health: a STABLE one (always up) + a TOGGLEABLE one (flip via /admin) ────
let healthy = true;            // toggled by /admin/health
app.get('/api/health', (_req, res) => res.json({ ok: true }));
// The monitored endpoint Rampart's FLAPPING http monitor hits. Returns 503 when
// flipped down. A push-cron / the autodrive flips it to create real outages.
app.get('/api/ready', (_req, res) => {
  if (healthy) return res.json({ status: 'ok' });
  res.status(503).json({ status: 'unavailable' });
});
app.post('/admin/health', (req, res) => {
  healthy = !(req.body && req.body.down);
  log(healthy ? 'INFO' : 'WARN', `health toggled → ${healthy ? 'UP' : 'DOWN'}`);
  res.json({ healthy });
});

// ── Prometheus exposition (scraped by Prometheus → remote_write into Rampart) ─
app.get('/metrics', async (_req, res) => {
  res.set('content-type', registry.contentType);
  res.end(await registry.metrics());
});

// ── keyword target: a page Rampart's `keyword` monitor asserts text in ───────
app.get('/welcome', (_req, res) => {
  res.type('html').send('<!doctype html><h1>Welcome to the Rampart demo</h1><p>status: operational</p>');
});

// ── json_query target: a JSON doc Rampart's `json_query` monitor asserts on ──
app.get('/status.json', (_req, res) => {
  res.json({ status: healthy ? 'operational' : 'degraded', version: '1.0.0', region: 'demo' });
});

// ── synthetic target: a 2-step login → fetch flow the synthetic monitor runs ─
app.post('/auth/token', (req, res) => {
  const u = (req.body && req.body.user) || 'svc';
  res.json({ token: 'demo-' + Buffer.from(u).toString('hex').slice(0, 12) });
});
app.get('/auth/whoami', (req, res) => {
  const auth = req.headers.authorization || '';
  if (!auth.startsWith('Bearer demo-')) return res.status(401).json({ error: 'unauthorized' });
  res.json({ ok: true, user: 'svc' });
});

// pg-backed list
app.get('/api/products', async (_req, res) => {
  const { rows } = await pool.query('SELECT id, name, price_cents FROM products ORDER BY id');
  res.json(rows);
});

// cache-aside: redis → (miss) pg. Produces a redis span + maybe a pg span.
app.get('/api/products/:id', async (req, res) => {
  const key = `product:${req.params.id}`;
  const cached = await redis.get(key);
  if (cached) return res.json({ source: 'cache', product: JSON.parse(cached) });
  const { rows } = await pool.query('SELECT id, name, price_cents FROM products WHERE id = $1', [req.params.id]);
  if (!rows[0]) return res.status(404).json({ error: 'not found' });
  await redis.set(key, JSON.stringify(rows[0]), 'EX', 30);
  res.json({ source: 'db', product: rows[0] });
});

// multi-step checkout: pg + redis + a downstream call → a deeper trace.
app.post('/api/checkout', async (req, res) => {
  const id = (req.body && req.body.productId) || 1;
  const { rows } = await pool.query('SELECT id, name, price_cents FROM products WHERE id = $1', [id]);
  if (!rows[0]) return res.status(404).json({ error: 'no such product' });
  await redis.incr('orders:count');
  await new Promise((r) => setTimeout(r, 40 + Math.random() * 120)); // payment gateway
  checkoutCents.inc(rows[0].price_cents);
  log('INFO', `checkout ok product=${rows[0].name}`, { product_id: id });
  res.json({ ok: true, charged: rows[0].price_cents });
});

// SIEM: emit auth logs. Random failures produce repeated "failed login" lines
// a detection rule keys on (service=demo-backend, body ~ "failed login").
app.post('/api/login', (req, res) => {
  const user = (req.body && req.body.user) || 'guest';
  const ip = req.headers['x-forwarded-for'] || req.socket.remoteAddress || '0.0.0.0';
  if (Math.random() < 0.4) {
    log('WARN', `failed login for user ${user} from ${ip}`, { 'event.action': 'auth.fail', user });
    return res.status(401).json({ error: 'invalid credentials' });
  }
  log('INFO', `login ok for user ${user}`, { 'event.action': 'auth.ok', user });
  res.json({ ok: true });
});

// error tier: throw → 500. Auto-instrumentation records the error span, we log
// it, AND @sentry/node captures it → Rampart's error-tracking tier.
app.get('/api/boom', (_req, _res, next) => {
  const err = new Error('synthetic failure in /api/boom');
  err.name = 'CheckoutError';
  next(err);
});
app.get('/api/boom2', (_req, _res, next) => {
  next(new TypeError("Cannot read properties of undefined (reading 'total')"));
});

// eslint-disable-next-line no-unused-vars
app.use((err, _req, res, _next) => {
  log('ERROR', `unhandled: ${err.message}`, { 'error.type': err.name });
  if (Sentry) { try { Sentry.captureException(err); } catch {} }
  res.status(500).json({ error: err.message });
});

const PORT = process.env.PORT || 8080;
initDb().then(() => {
  app.listen(PORT, () => log('INFO', `demo-backend listening on :${PORT}`));
});
