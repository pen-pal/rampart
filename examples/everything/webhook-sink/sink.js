// webhook-sink — a tiny browser-viewable echo server. Every notification
// Rampart delivers to a sink-compatible channel (webhook, mattermost,
// rocketchat, gotify, ntfy, matrix, apprise, home_assistant, alerta, …, all
// pointed here by `provision`) lands as a POST and is shown in a live list at
// http://localhost:8099. Proof that REAL deliveries fired.
const http = require('node:http');
const received = []; // newest first, capped

function record(req, body) {
  received.unshift({
    at: new Date().toISOString(),
    method: req.method,
    path: req.url,
    headers: req.headers,
    body: body.slice(0, 8000),
  });
  if (received.length > 500) received.pop();
  console.log(`[sink] ${req.method} ${req.url} (${body.length}b)`);
}

const PAGE = (rows) => `<!doctype html><html><head><meta charset=utf-8>
<title>Rampart webhook-sink</title><meta http-equiv=refresh content=5>
<style>body{font-family:system-ui;margin:24px;max-width:1000px}h1{font-size:20px}
.e{border:1px solid #e7e5e4;border-radius:8px;padding:10px;margin:8px 0}
.m{color:#0d9488;font-weight:600}.p{color:#78716c}pre{background:#f5f5f4;padding:8px;border-radius:6px;overflow:auto;font-size:12px;margin:6px 0 0}
.t{color:#a8a29e;font-size:12px}</style></head><body>
<h1>Rampart webhook-sink — ${received.length} delivery(ies) received</h1>
<p class=p>Auto-refreshes every 5s. Each card is a real notification Rampart delivered to a sink-compatible channel.</p>
${rows}</body></html>`;

const server = http.createServer((req, res) => {
  if (req.method === 'GET' && (req.url === '/' || req.url.startsWith('/?'))) {
    const rows = received.map((e) => {
      let pretty = e.body;
      try { pretty = JSON.stringify(JSON.parse(e.body), null, 2); } catch {}
      return `<div class=e><span class=m>${e.method}</span> <code>${e.path}</code>
        <span class=t> · ${e.at}</span><pre>${pretty.replace(/[<>&]/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;' }[c]))}</pre></div>`;
    }).join('');
    res.writeHead(200, { 'content-type': 'text/html' });
    return res.end(PAGE(rows || '<p class=p>nothing yet — waiting for the first delivery…</p>'));
  }
  if (req.method === 'GET' && req.url === '/_count') {
    res.writeHead(200, { 'content-type': 'application/json' });
    return res.end(JSON.stringify({ count: received.length }));
  }
  // Everything else (any path, any verb) is a delivery target. Echo 200 OK so
  // adapters that check the response (gotify, healthchecks, apprise, …) succeed.
  let body = '';
  req.on('data', (c) => { body += c; });
  req.on('end', () => {
    record(req, body);
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end('{"ok":true,"id":"sink-' + received.length + '"}');
  });
});
server.listen(8099, () => console.log('[sink] listening on :8099'));
