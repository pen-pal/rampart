// Thin fetch wrapper for the Rampart API.
//
// All endpoints live under /v1 and the same host as the page (the Rust
// binary serves both API and UI), so we use relative paths and let the
// browser handle the host. In `npm run dev`, Vite proxies /v1 to :3000.
//
// Errors come back as ApiError so callers can branch on status. We never
// throw a plain Error — every failure has a code + message.

export class ApiError extends Error {
  constructor(status, code, message) {
    super(message || code || `HTTP ${status}`);
    this.status  = status;
    this.code    = code;
    this.payload = { status, code, message };
  }
}

async function request(path, { method = 'GET', body, signal } = {}) {
  const opts = { method, signal, headers: {}, credentials: 'same-origin' };
  if (body !== undefined) {
    opts.headers['Content-Type'] = 'application/json';
    opts.body = JSON.stringify(body);
  }

  let resp;
  try {
    resp = await fetch(path, opts);
  } catch (e) {
    if (e.name === 'AbortError') throw e;
    throw new ApiError(0, 'network', e.message || 'network error');
  }

  // 204 No Content — common for pause / resume / delete / logout.
  if (resp.status === 204) return null;

  const text = await resp.text();
  let json = null;
  if (text) {
    try { json = JSON.parse(text); }
    catch { /* non-JSON body — leave json null and fall through */ }
  }

  if (!resp.ok) {
    const err = json && json.error;
    const apiErr = new ApiError(resp.status, err?.code || 'http_error', err?.message || text || resp.statusText);
    // Auto-redirect on 401 so views don't have to handle it individually.
    // Exception: requests already going to /v1/auth/* (login, me) need to
    // surface 401 to the caller so the login form can show "wrong password".
    if (resp.status === 401 && !path.startsWith('/v1/auth/')) {
      if (!window.location.hash.startsWith('#/login')) {
        window.location.hash = '#/login';
      }
    }
    throw apiErr;
  }

  return json;
}

// ─── monitors ───────────────────────────────────────────────────────────────
export const api = {
  monitors: {
    list:    ()           => request('/v1/monitors'),
    get:     (id)         => request(`/v1/monitors/${id}`),
    create:  (input)      => request('/v1/monitors', { method: 'POST', body: input }),
    update:  (id, patch)  => request(`/v1/monitors/${id}`, { method: 'PATCH', body: patch }),
    remove:  (id)         => request(`/v1/monitors/${id}`, { method: 'DELETE' }),
    pause:   (id)         => request(`/v1/monitors/${id}/pause`,  { method: 'POST' }),
    resume:  (id)         => request(`/v1/monitors/${id}/resume`, { method: 'POST' }),
    clone:   (id)         => request(`/v1/monitors/${id}/clone`,  { method: 'POST' }),
    regeneratePushToken: (id) => request(`/v1/monitors/${id}/regenerate-push-token`, { method: 'POST' }),
    testNow: (id)         => request(`/v1/monitors/${id}/test-now`, { method: 'POST' }),
    bulk:    (monitorIds, action) => request('/v1/monitors/bulk', { method: 'POST', body: { monitor_ids: monitorIds, ...action } }),
    summary: (windowSec)  => request(`/v1/monitors/summary?window=${windowSec ?? 86400}`),
    history: (per)        => request(`/v1/monitors/history?per=${per ?? 60}`),
    heartbeats: (id, limit, before) => {
      const qs = new URLSearchParams({ limit: String(limit ?? 100) });
      if (before) qs.set('before', before);
      return request(`/v1/monitors/${id}/heartbeats?${qs.toString()}`);
    },
    /// MTBF / MTTR + downtime event count over a trailing window.
    /// `windowDays` is one of 7 / 30 / 90; omitting it lets the backend
    /// default to 30 (preserves the pre-toggle behaviour). Backend
    /// computes everything from server-side heartbeat history so the
    /// figures don't depend on what the client has lazy-loaded.
    reliability: (id, windowDays) => {
      const qs = windowDays ? `?window_days=${windowDays}` : '';
      return request(`/v1/monitors/${id}/reliability${qs}`);
    },
    /// SLO error-budget over the monitor's configured window. Returns
    /// 404 when the monitor has no SLO target set — callers should
    /// only invoke this after checking `monitor.slo_target_pct`.
    errorBudget: (id)  => request(`/v1/monitors/${id}/slo/error-budget`),
  },
  health: {
    live:  () => request('/healthz'),
    ready: () => request('/readyz'),
  },
  auth: {
    me:       ()                          => request('/v1/auth/me'),
    register: (email, name, password)     => request('/v1/auth/register', { method: 'POST', body: { email, name, password } }),
    login:    (email, password)           => request('/v1/auth/login',    { method: 'POST', body: { email, password } }),
    logout:   ()                          => request('/v1/auth/logout',   { method: 'POST' }),
    totpSetup:   ()                       => request('/v1/auth/2fa/setup',   { method: 'POST' }),
    totpEnable:  (code)                   => request('/v1/auth/2fa/enable',  { method: 'POST', body: { code } }),
    totpDisable: (password, code)         => request('/v1/auth/2fa/disable', { method: 'POST', body: { password, code } }),
    totpVerify:  (challengeToken, code)   => request('/v1/auth/2fa/verify',  { method: 'POST', body: { challenge_token: challengeToken, code } }),
  },
  notifications: {
    list:        ()                                  => request('/v1/notifications'),
    get:         (id)                                => request(`/v1/notifications/${id}`),
    create:      (kind, name, config, templateId, cooldownSeconds = 0) => request('/v1/notifications', { method: 'POST', body: { kind, name, config, active: true, template_id: templateId || null, cooldown_seconds: Number(cooldownSeconds) || 0 } }),
    update:      (id, patch)                         => request(`/v1/notifications/${id}`, { method: 'PATCH', body: patch }),
    remove:      (id)                                => request(`/v1/notifications/${id}`, { method: 'DELETE' }),
    test:        (id)                                => request(`/v1/notifications/${id}/test`, { method: 'POST' }),
    counts:      ()                                  => request('/v1/notifications/counts'),
    forMonitor:  (mid)                               => request(`/v1/monitors/${mid}/notifications`),
    attach:      (mid, nid)                          => request(`/v1/monitors/${mid}/notifications/${nid}`, { method: 'POST' }),
    detach:      (mid, nid)                          => request(`/v1/monitors/${mid}/notifications/${nid}`, { method: 'DELETE' }),
  },
  templates: {
    list:    ()              => request('/v1/notification-templates'),
    get:     (id)            => request(`/v1/notification-templates/${id}`),
    create:  (input)         => request('/v1/notification-templates', { method: 'POST', body: input }),
    update:  (id, patch)     => request(`/v1/notification-templates/${id}`, { method: 'PATCH', body: patch }),
    remove:  (id)            => request(`/v1/notification-templates/${id}`, { method: 'DELETE' }),
    preview: (subject, body) => request('/v1/notification-templates/preview', { method: 'POST', body: { subject_template: subject, body_template: body } }),
  },
  users: {
    list:           ()                                    => request('/v1/users'),
    create:         (email, name, password, isAdmin)      => request('/v1/users', { method: 'POST', body: { email, name, password, is_admin: !!isAdmin } }),
    setAdmin:       (id, isAdmin)                         => request(`/v1/users/${id}/admin`, { method: 'POST', body: { is_admin: !!isAdmin } }),
    remove:         (id)                                  => request(`/v1/users/${id}`, { method: 'DELETE' }),
    changePassword: (current, next)                       => request('/v1/users/me/password', { method: 'POST', body: { current_password: current, new_password: next } }),
  },
  proxies: {
    list:      ()                  => request('/v1/proxies'),
    create:    (input)             => request('/v1/proxies', { method: 'POST', body: input }),
    remove:    (id)                => request(`/v1/proxies/${id}`, { method: 'DELETE' }),
    setActive: (id, active)        => request(`/v1/proxies/${id}/active`, { method: 'POST', body: { active } }),
  },
  apiKeys: {
    list:   ()                  => request('/v1/api-keys'),
    create: (name, scopes, exp) => request('/v1/api-keys', { method: 'POST', body: { name, scopes: scopes || [], expires_at: exp || null } }),
    revoke: (id)                => request(`/v1/api-keys/${id}`, { method: 'DELETE' }),
  },
  audit: {
    list: (limit, before, kind, action, actor, from, to) => {
      const qs = new URLSearchParams();
      if (limit)  qs.set('limit',  String(limit));
      if (before) qs.set('before', String(before));
      if (kind)   qs.set('kind',   kind);
      if (action) qs.set('action', action);
      if (actor)  qs.set('actor',  actor);
      if (from)   qs.set('from',   from);
      if (to)     qs.set('to',     to);
      const s = qs.toString();
      return request(`/v1/audit-log${s ? '?' + s : ''}`);
    },
  },
  subscribers: {
    subscribe:   (slug, email) => request(`/v1/public/status-pages/${slug}/subscribe`, { method: 'POST', body: { email } }),
    listForPage: (pageId)      => request(`/v1/status-pages/${pageId}/subscribers`),
    remove:      (id)          => request(`/v1/subscribers/${id}`, { method: 'DELETE' }),
  },
  smtp: {
    get: () => request('/v1/settings/smtp'),
    put: (cfg) => request('/v1/settings/smtp', { method: 'PUT', body: cfg }),
  },
  retention: {
    get: () => request('/v1/settings/retention'),
    put: (heartbeats, auditLog) => request('/v1/settings/retention', { method: 'PUT', body: { heartbeats: Number(heartbeats), audit_log: Number(auditLog) } }),
  },
  incidents: {
    listForPage:  (pageId)              => request(`/v1/status-pages/${pageId}/incidents`),
    create:       (pageId, input)       => request(`/v1/status-pages/${pageId}/incidents`, { method: 'POST', body: input }),
    update:       (id, patch)           => request(`/v1/incidents/${id}`, { method: 'PATCH', body: patch }),
    remove:       (id)                  => request(`/v1/incidents/${id}`, { method: 'DELETE' }),
    resolve:      (id)                  => request(`/v1/incidents/${id}/resolve`, { method: 'POST' }),
    listUpdates:  (id)                  => request(`/v1/incidents/${id}/updates`),
    postUpdate:   (id, message)         => request(`/v1/incidents/${id}/updates`, { method: 'POST', body: { message } }),
  },
  tags: {
    list:   ()                  => request('/v1/tags'),
    create: (name, color)       => request('/v1/tags', { method: 'POST', body: { name, color } }),
    update: (id, patch)         => request(`/v1/tags/${id}`, { method: 'PATCH', body: patch }),
    remove: (id)                => request(`/v1/tags/${id}`, { method: 'DELETE' }),
    usage:  ()                  => request('/v1/tags/usage'),
    forMonitor: (mid)           => request(`/v1/monitors/${mid}/tags`),
    attach: (mid, tagId)        => request(`/v1/monitors/${mid}/tags/${tagId}`, { method: 'POST' }),
    detach: (mid, tagId)        => request(`/v1/monitors/${mid}/tags/${tagId}`, { method: 'DELETE' }),
  },
  statusPages: {
    list:       ()                  => request('/v1/status-pages'),
    get:        (id)                => request(`/v1/status-pages/${id}`),
    create:     (input)             => request('/v1/status-pages', { method: 'POST', body: input }),
    update:     (id, patch)         => request(`/v1/status-pages/${id}`, { method: 'PATCH', body: patch }),
    remove:     (id)                => request(`/v1/status-pages/${id}`, { method: 'DELETE' }),
    publicView: (slug)              => request(`/v1/public/status-pages/${slug}`),
    // Per-hour latency for one UTC calendar day, scoped to the monitor at
    // `monitorIdx` on the public page. `isoDate` is `YYYY-MM-DD`.
    dayLatency: (slug, monitorIdx, isoDate) =>
      request(`/v1/public/status-pages/${slug}/day-latency?monitor_idx=${encodeURIComponent(monitorIdx)}&date=${encodeURIComponent(isoDate)}`),
  },
  webpush: {
    vapidKey:    ()                          => request('/v1/webpush/vapid-key'),
    subscribe:   (notificationId, sub)        => request('/v1/webpush/subscriptions', { method: 'POST', body: { notification_id: notificationId, endpoint: sub.endpoint, keys: sub.keys } }),
    unsubscribe: (endpoint)                   => request('/v1/webpush/subscriptions', { method: 'DELETE', body: { endpoint } }),
  },
  monitorGroups: {
    list:   ()             => request('/v1/monitor-groups'),
    create: (name, sortOrder = 0) => request('/v1/monitor-groups', { method: 'POST', body: { name, sort_order: Number(sortOrder) || 0 } }),
    update: (id, patch)    => request(`/v1/monitor-groups/${id}`, { method: 'PATCH', body: patch }),
    remove: (id)           => request(`/v1/monitor-groups/${id}`, { method: 'DELETE' }),
    // Tags on a folder.
    tags:      (id)            => request(`/v1/monitor-groups/${id}/tags`),
    addTag:    (id, tagId)     => request(`/v1/monitor-groups/${id}/tags/${tagId}`, { method: 'POST' }),
    delTag:    (id, tagId)     => request(`/v1/monitor-groups/${id}/tags/${tagId}`, { method: 'DELETE' }),
    // Channels attached at the folder level.
    channels:  (id)            => request(`/v1/monitor-groups/${id}/channels`),
    addChannel:(id, notifId)   => request(`/v1/monitor-groups/${id}/channels/${notifId}`, { method: 'POST' }),
    delChannel:(id, notifId)   => request(`/v1/monitor-groups/${id}/channels/${notifId}`, { method: 'DELETE' }),
  },
  // Tag-based routing: tags on channels, per-monitor excludes, resolved set.
  routing: {
    channelTags:   (notifId)          => request(`/v1/notifications/${notifId}/tags`),
    addChannelTag: (notifId, tagId)   => request(`/v1/notifications/${notifId}/tags/${tagId}`, { method: 'POST' }),
    delChannelTag: (notifId, tagId)   => request(`/v1/notifications/${notifId}/tags/${tagId}`, { method: 'DELETE' }),
    excludes:        (mid)            => request(`/v1/monitors/${mid}/excludes`),
    addExclude:      (mid, notifId)   => request(`/v1/monitors/${mid}/excludes/${notifId}`, { method: 'POST' }),
    delExclude:      (mid, notifId)   => request(`/v1/monitors/${mid}/excludes/${notifId}`, { method: 'DELETE' }),
    effective:       (mid)            => request(`/v1/monitors/${mid}/effective-channels`),
  },
  dependencies: {
    list:    (mid)              => request(`/v1/monitors/${mid}/dependencies`),
    attach:  (childId, parentId) => request(`/v1/monitors/${childId}/dependencies/${parentId}`, { method: 'POST' }),
    detach:  (childId, parentId) => request(`/v1/monitors/${childId}/dependencies/${parentId}`, { method: 'DELETE' }),
  },
  maintenance: {
    list:        ()                  => request('/v1/maintenance-windows'),
    get:         (id)                => request(`/v1/maintenance-windows/${id}`),
    create:      (input)             => request('/v1/maintenance-windows', { method: 'POST', body: input }),
    update:      (id, patch)         => request(`/v1/maintenance-windows/${id}`, { method: 'PATCH', body: patch }),
    remove:      (id)                => request(`/v1/maintenance-windows/${id}`, { method: 'DELETE' }),
    setActive:   (id, active)        => request(`/v1/maintenance-windows/${id}/active`, { method: 'POST', body: { active } }),
    attach:      (id, monitorId)     => request(`/v1/maintenance-windows/${id}/monitors/${monitorId}`, { method: 'POST' }),
    detach:      (id, monitorId)     => request(`/v1/maintenance-windows/${id}/monitors/${monitorId}`, { method: 'DELETE' }),
  },
};

// ─── tiny hooks ─────────────────────────────────────────────────────────────
// Two patterns we need across views: fire-and-render a one-shot fetch, and
// re-poll every N seconds. Keep it minimal — react-query is overkill at this
// scale (one process, ~tens of monitors).

import { useEffect, useRef, useState } from 'react';

export function useApi(fn, deps = [], { pollMs = 0 } = {}) {
  const [state, setState] = useState({ data: null, error: null, loading: true });
  // Stash the latest fn in a ref so we don't re-fire when the caller passes
  // an inline arrow function on every render.
  const fnRef = useRef(fn);
  fnRef.current = fn;

  useEffect(() => {
    let cancelled = false;
    const ctl = new AbortController();
    let timer = null;

    const run = async (isInitial) => {
      if (isInitial) setState(s => ({ ...s, loading: true }));
      try {
        const data = await fnRef.current(ctl.signal);
        if (!cancelled) setState({ data, error: null, loading: false });
      } catch (e) {
        if (e.name === 'AbortError' || cancelled) return;
        setState(s => ({ ...s, error: e, loading: false }));
      }
    };

    run(true);
    if (pollMs > 0) timer = setInterval(() => run(false), pollMs);

    return () => {
      cancelled = true;
      ctl.abort();
      if (timer) clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  return state;
}

// ─── small format helpers used across views ─────────────────────────────────
export function formatRelative(iso) {
  if (!iso) return '—';
  // Backend serializes time via the `time` crate as an int array.
  // Convert defensively for both shapes.
  const date = iso instanceof Array ? offsetDateTimeArrayToDate(iso) : new Date(iso);
  const deltaSec = Math.round((Date.now() - date.getTime()) / 1000);
  const future = deltaSec < 0;
  const sec = Math.abs(deltaSec);
  const fmt = sec < 60        ? `${sec}s`
            : sec < 3600      ? `${Math.round(sec / 60)}m`
            : sec < 86400     ? `${Math.round(sec / 3600)}h`
            :                   `${Math.round(sec / 86400)}d`;
  return future ? `in ${fmt}` : `${fmt} ago`;
}

export function formatClock(iso) {
  if (!iso) return '—';
  const date = iso instanceof Array ? offsetDateTimeArrayToDate(iso) : new Date(iso);
  return date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

// The `time` crate serializes OffsetDateTime as a tuple:
//   [year, ordinal_day, hour, minute, second, nanosecond, offset_h, offset_m, offset_s]
// Convert via UTC since the backend uses UTC throughout.
export function offsetDateTimeArrayToDate(a) {
  if (!a || a.length < 6) return new Date(NaN);
  const [year, ordinal, h, m, s, ns] = a;
  const jan1 = Date.UTC(year, 0, 1);
  return new Date(jan1 + (ordinal - 1) * 86_400_000 + h * 3_600_000 + m * 60_000 + s * 1000 + Math.round(ns / 1e6));
}

export function statusToClass(status) {
  switch (status) {
    case 'up':           return 'up';
    case 'down':         return 'down';
    case 'warn':         return 'warn';
    case 'paused':       return 'paused';
    case 'maintenance':  return 'maint';
    case 'pending':      return 'paused';
    default:             return 'paused';
  }
}
