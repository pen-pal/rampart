# Result webhooks

A monitor can POST the result of every probe to a URL of your choosing — a
fire-and-forget feed of raw check outcomes you can pipe into your own systems
(a queue, a metrics sink, a custom escalation flow). It is separate from the
notification channels: notifications are about *alerts* (a monitor went
down/up); result webhooks are about *every heartbeat*.

## Enabling

Set the per-monitor config field `result_webhook` to the destination URL (the
monitor wizard exposes a "Result webhook URL" field). To sign the requests,
also set `result_webhook_secret`. Leave the URL blank to disable.

## Request

After each probe the scheduler sends, fire-and-forget, with a short timeout
(it never blocks the probe loop):

```
POST <result_webhook>
Content-Type: application/json
X-Rampart-Timestamp: 1733836800
X-Rampart-Signature: sha256=<hex>      # only when a secret is configured

{
  "monitor_id": "019eaa1c-5b05-72e2-b8b1-eac1d9867dd8",
  "name": "api health",
  "status": "up",          // "up" | "down" | other probe statuses
  "latency_ms": 142,
  "status_code": 200,       // may be null for non-HTTP probes
  "ts": 1733836800          // unix seconds; equals X-Rampart-Timestamp
}
```

The receiver should respond quickly with any 2xx. Non-2xx and timeouts are
recorded as failed deliveries (visible in the delivery log) but are not
retried automatically.

## Verifying the signature

When `result_webhook_secret` is set, each request carries
`X-Rampart-Signature: sha256=<hex>`. The signature is the lowercase hex
HMAC-SHA256, keyed by the secret, over the exact string:

```
<X-Rampart-Timestamp>.<raw request body>
```

The timestamp is part of the signed message, so a captured request cannot be
replayed under a different time — reject requests whose timestamp is too far
from now (e.g. more than 5 minutes of skew). Always compare signatures with a
constant-time comparison, and verify against the **raw** body bytes (do not
re-serialize the parsed JSON — key order/whitespace would differ).

### Python

```python
import hmac, hashlib, time

def verify(secret: str, timestamp: str, raw_body: bytes, signature: str) -> bool:
    # Reject stale timestamps (replay protection).
    if abs(time.time() - int(timestamp)) > 300:
        return False
    signed = timestamp.encode() + b"." + raw_body
    expected = "sha256=" + hmac.new(secret.encode(), signed, hashlib.sha256).hexdigest()
    return hmac.compare_digest(expected, signature)

# Flask example
# verify(SECRET, request.headers["X-Rampart-Timestamp"],
#        request.get_data(), request.headers["X-Rampart-Signature"])
```

### Node.js

```js
const crypto = require('crypto');

function verify(secret, timestamp, rawBody, signature) {
  if (Math.abs(Date.now() / 1000 - Number(timestamp)) > 300) return false;
  const signed = `${timestamp}.${rawBody}`;            // rawBody = the exact bytes received
  const expected = 'sha256=' + crypto.createHmac('sha256', secret).update(signed).digest('hex');
  const a = Buffer.from(expected), b = Buffer.from(signature);
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

// Express: use express.raw({ type: 'application/json' }) so req.body is the raw Buffer,
// then verify(SECRET, req.get('X-Rampart-Timestamp'), req.body, req.get('X-Rampart-Signature')).
```

When no secret is configured, no `X-Rampart-Signature` header is sent — the
receiver should then treat the endpoint as trusted by network placement only.
