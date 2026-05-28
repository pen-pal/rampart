// EventSource wrapper for the live heartbeat stream.
//
// `useHeartbeatStream(onHb)` opens `/v1/stream/heartbeats`, parses each
// `heartbeat` event as JSON, and invokes `onHb(heartbeat)`. The browser
// EventSource handles reconnect with backoff for free; we only add:
//   - `withCredentials` so the session cookie tags along
//   - a debounced "live tick" so callers can wire a single state bump
//     instead of one per event (heartbeats are bursty around flushes)
//
// Returns `{ connected, lastEventAt }` so the dashboard can surface a
// live/idle dot without a separate ping.

import { useEffect, useRef, useState } from 'react';

export function useHeartbeatStream(onHb) {
  const [connected, setConnected] = useState(false);
  const [lastEventAt, setLastEventAt] = useState(null);
  // Latest callback in a ref so re-renders don't tear down the source.
  const cbRef = useRef(onHb);
  cbRef.current = onHb;

  useEffect(() => {
    let es;
    try {
      es = new EventSource('/v1/stream/heartbeats', { withCredentials: true });
    } catch (e) {
      // Older browsers without EventSource; the polled fallback still works.
      return;
    }
    es.addEventListener('open', () => setConnected(true));
    es.addEventListener('error', () => setConnected(false));
    es.addEventListener('heartbeat', ev => {
      setLastEventAt(Date.now());
      try {
        const hb = JSON.parse(ev.data);
        cbRef.current && cbRef.current(hb);
      } catch { /* malformed payload — ignore */ }
    });
    return () => { try { es.close(); } catch {} };
  }, []);

  return { connected, lastEventAt };
}

// Debounced "something changed" pulse for callers that want to refetch
// polled data on SSE activity but coalesce bursts. Returns a tick value
// that increments at most once per `waitMs`.
export function useDebouncedTick(signal, waitMs = 500) {
  const [tick, setTick] = useState(0);
  const timerRef = useRef(null);
  useEffect(() => {
    if (signal == null) return;
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setTick(t => t + 1), waitMs);
    return () => { if (timerRef.current) clearTimeout(timerRef.current); };
  }, [signal, waitMs]);
  return tick;
}
