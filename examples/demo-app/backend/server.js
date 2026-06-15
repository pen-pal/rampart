// Demo backend — every request is auto-traced (express → pg/redis spans) and
// flows to Rampart. Also emits structured logs, SIEM auth-failure lines, and
// errors. Run as `node -r ./otel.js server.js` so instrumentation loads first.
const express = require('express');
const { Pool } = require('pg');
const Redis = require('ioredis');

const log = global.rlog || ((lvl, m) => console.log(`[${lvl}] ${m}`));
const pool = new Pool({ connectionString: process.env.DATABASE_URL || 'postgres://demo:demo@demo-db:5432/demo' });
const redis = new Redis(process.env.REDIS_URL || 'redis://demo-redis:6379');

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

app.get('/api/health', (_req, res) => res.json({ ok: true }));

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
  // simulate a payment-gateway round trip
  await new Promise((r) => setTimeout(r, 40 + Math.random() * 120));
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

// error tier: throw → 500 (auto-instrumentation records the error span + we log)
app.get('/api/boom', () => {
  throw new Error('synthetic failure in /api/boom');
});

// eslint-disable-next-line no-unused-vars
app.use((err, _req, res, _next) => {
  log('ERROR', `unhandled: ${err.message}`, { 'error.type': err.name });
  res.status(500).json({ error: err.message });
});

const PORT = process.env.PORT || 8080;
initDb().then(() => {
  app.listen(PORT, () => log('INFO', `demo-backend listening on :${PORT}`));
});
