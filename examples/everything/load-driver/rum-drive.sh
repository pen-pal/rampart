#!/bin/sh
# rum-drive — periodically loads the demo FRONTEND through the browserless
# `renderer` so the RUM snippet runs in a real Chromium and reports Core Web
# Vitals + browser JS errors to Rampart, continuously, without a human keeping a
# tab open. The snippet beacons same-origin (/rum/* → nginx → rampart), so it
# resolves correctly from inside the renderer container.
set -u
RENDERER="${RENDERER_URL:-http://renderer:3000}"
FRONTEND="${FRONTEND_URL:-http://demo-frontend/}"

echo "[rum-drive] waiting for renderer + frontend ..."
i=0
# browserless has no root route (/ → 404); /json/version is its health endpoint.
until curl -sf -o /dev/null "$RENDERER/json/version" 2>/dev/null && curl -sf -o /dev/null "$FRONTEND" 2>/dev/null; do
  i=$((i + 1)); [ "$i" -gt 120 ] && echo "[rum-drive] deps never came up — exiting" && exit 0
  sleep 2
done
echo "[rum-drive] driving RUM via $RENDERER → $FRONTEND"

# browserless /function script: load the page (runs the RUM snippet + the page's
# own auto-driver), interact a little (clicks → INP), fire an uncaught error
# (→ RUM error capture), then return — page teardown fires pagehide which flushes
# the web-vitals beacon.
cat > /tmp/fn.js <<JS
export default async function ({ page }) {
  await page.goto('${FRONTEND}', { waitUntil: 'networkidle2', timeout: 25000 });
  await new Promise(r => setTimeout(r, 3000));
  try { await page.click('button'); } catch (e) {}
  await page.evaluate(() => { setTimeout(() => { undefined.kaboomRUM(); }, 10); });
  await new Promise(r => setTimeout(r, 2500));
  // force the visibilitychange flush before teardown
  await page.evaluate(() => { Object.defineProperty(document,'visibilityState',{value:'hidden',configurable:true}); document.dispatchEvent(new Event('visibilitychange')); });
  await new Promise(r => setTimeout(r, 800));
  return { data: 'ok', type: 'application/json' };
}
JS

N=0
while true; do
  N=$((N + 1))
  code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$RENDERER/function" \
    -H 'content-type: application/javascript' --data-binary @/tmp/fn.js --max-time 60)
  echo "[rum-drive] render #$N → HTTP $code"
  sleep 45
done
