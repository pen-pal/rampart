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

async function request(path, { method = 'GET', body, rawBody, contentType, signal } = {}) {
  const opts = { method, signal, headers: {}, credentials: 'same-origin' };
  if (rawBody !== undefined) {
    // Non-JSON payload (e.g. a raw CSV upload) — send the string verbatim
    // under the caller's content-type rather than JSON-encoding it.
    opts.headers['Content-Type'] = contentType || 'text/plain';
    opts.body = rawBody;
  } else if (body !== undefined) {
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
    // Clone a monitor. Optional `opts` { group_id?: uuid|null, name?: string }
    // retargets the copy into a chosen folder/group and/or renames it. Omit
    // `opts` (or pass {}) for the legacy "clone into the same group" behaviour.
    clone:   (id, opts)   => request(`/v1/monitors/${id}/clone`,  { method: 'POST', ...(opts && Object.keys(opts).length ? { body: opts } : {}) }),
    regeneratePushToken: (id) => request(`/v1/monitors/${id}/regenerate-push-token`, { method: 'POST' }),
    testNow: (id)         => request(`/v1/monitors/${id}/test-now`, { method: 'POST' }),
    testNotifications: (id) => request(`/v1/monitors/${id}/test-notifications`, { method: 'POST' }),
    bulk:    (monitorIds, action) => request('/v1/monitors/bulk', { method: 'POST', body: { monitor_ids: monitorIds, ...action } }),
    /// Bulk-edit a set of monitors in one call. `payload` shape:
    /// { ids: [uuid], patch: { interval_secs?, timeout_secs?, enabled?,
    /// group_id?(null clears), tags?: [uuid] } }. Returns { updated, skipped }.
    bulkEdit: (ids, patch) => request('/v1/monitors/bulk-edit', { method: 'POST', body: { ids, patch } }),
    /// Pause / resume every monitor carrying a tag in one call. `action`
    /// is 'pause' | 'resume'. Returns { affected: n } — the count of
    /// monitors whose active state actually changed.
    bulkByTag: (tagId, action) => request('/v1/monitors/bulk-by-tag', { method: 'POST', body: { tag_id: tagId, action } }),
    /// Bulk-import monitors from a Rampart-native CSV. POSTs the raw CSV
    /// text (not JSON) to `/v1/monitors/import-csv`; the backend parses +
    /// maps it and returns `{ created, skipped: [{ row, reason }] }`.
    importCsv: (csvText) => request('/v1/monitors/import-csv', { method: 'POST', rawBody: csvText, contentType: 'text/csv' }),
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
    /// SLO error-budget burn-down: one point per day across the window.
    /// Same 404 contract as `errorBudget` — only call after confirming
    /// `monitor.slo_target_pct` is set. `windowDays` is one of 7 / 30 / 90;
    /// omitting it lets the backend default to the monitor's configured
    /// `slo_window_days` (preserves the pre-toggle behaviour).
    sloBurndown: (id, windowDays) => {
      const qs = windowDays ? `?window_days=${windowDays}` : '';
      return request(`/v1/monitors/${id}/slo/burndown${qs}`);
    },
    /// Hourly status rollups across an explicit [from, to] window. Both
    /// bounds are RFC3339 strings. Returns one bucket per hour:
    /// [{ bucket_start, up_count, down_count, other_count, sample_count,
    /// avg_latency_ms }]. Backs the long-horizon views that outlive raw
    /// heartbeat retention.
    rollups: (id, from, to) => {
      const qs = new URLSearchParams();
      if (from) qs.set('from', from);
      if (to)   qs.set('to',   to);
      const s = qs.toString();
      return request(`/v1/monitors/${id}/rollups${s ? '?' + s : ''}`);
    },
    /// Daily uptime percentage over a trailing range. `range` is one of
    /// '30d' | '90d' | '1y'. Returns [{ day(date), uptime_pct, sample_count }],
    /// oldest-first. Works past raw retention because it reads from rollups.
    uptimeHistory: (id, range) => request(`/v1/monitors/${id}/uptime-history?range=${encodeURIComponent(range || '30d')}`),
    /// Preview a bulk-edit WITHOUT mutating. Same body as `bulkEdit`; the
    /// `?dry_run=true` flag makes the backend return what it WOULD do:
    /// { would_update, would_skip, preview: [{ id, name, changes: {
    /// <field>: { from, to } } }] }.
    bulkEditPreview: (ids, patch) => request('/v1/monitors/bulk-edit?dry_run=true', { method: 'POST', body: { ids, patch } }),
  },
  // ─── monitor templates ───────────────────────────────────────────────────
  // Reusable monitor blueprints. `spec` is a monitor-creation body (kind +
  // url/config + interval_seconds etc.); `instantiate` materialises a real
  // monitor from one with an optional name override and returns the created
  // monitor. Distinct from monitorPresets (those are header/TLS config bags).
  monitorTemplates: {
    list:        ()              => request('/v1/monitor-templates'),
    get:         (id)            => request(`/v1/monitor-templates/${id}`),
    create:      (input)         => request('/v1/monitor-templates', { method: 'POST', body: input }),
    remove:      (id)            => request(`/v1/monitor-templates/${id}`, { method: 'DELETE' }),
    instantiate: (id, name)      => request(`/v1/monitor-templates/${id}/instantiate`, { method: 'POST', body: name ? { name } : {} }),
  },
  // ─── monitor presets ────────────────────────────────────────────────────
  // Reusable config bags the New-Monitor wizard applies with a click:
  // saved HTTP header sets (kind 'http_headers', data { headers: {…} }) and
  // saved TLS posture (kind 'tls', data { ignore_tls: bool }). Nested under
  // the monitors router on the backend, hence the /v1/monitors/presets paths.
  monitorPresets: {
    list:   ()      => request('/v1/monitors/presets'),
    get:    (id)    => request(`/v1/monitors/presets/${id}`),
    create: (input) => request('/v1/monitors/presets', { method: 'POST', body: input }),
    remove: (id)    => request(`/v1/monitors/presets/${id}`, { method: 'DELETE' }),
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
    create:      (kind, name, config, templateId, cooldownSeconds = 0, digestWindowSecs = 0, extra = {}) => request('/v1/notifications', { method: 'POST', body: { kind, name, config, active: true, template_id: templateId || null, cooldown_seconds: Number(cooldownSeconds) || 0, digest_window_secs: Number(digestWindowSecs) || 0, ...extra } }),
    update:      (id, patch)                         => request(`/v1/notifications/${id}`, { method: 'PATCH', body: patch }),
    remove:      (id)                                => request(`/v1/notifications/${id}`, { method: 'DELETE' }),
    test:        (id)                                => request(`/v1/notifications/${id}/test`, { method: 'POST' }),
    counts:      ()                                  => request('/v1/notifications/counts'),
    forMonitor:  (mid)                               => request(`/v1/monitors/${mid}/notifications`),
    attach:      (mid, nid)                          => request(`/v1/monitors/${mid}/notifications/${nid}`, { method: 'POST' }),
    detach:      (mid, nid)                          => request(`/v1/monitors/${mid}/notifications/${nid}`, { method: 'DELETE' }),
  },
  // ─── delivery log ────────────────────────────────────────────────────────
  // Read-only paginated feed of recent notification deliveries. Each row is
  // { channel_kind, event_kind, monitor_id, ok, error, sent_at }. Keyset
  // pagination (newest-first): pass `before` = the RFC3339 `sent_at` of the
  // last row of the previous page to fetch the next one. The backend has no
  // offset param — it returns rows strictly older than `before`.
  deliveryLog: {
    list: (opts = {}) => {
      const qs = new URLSearchParams();
      if (opts.limit  != null) qs.set('limit',  String(opts.limit));
      if (opts.before != null) qs.set('before', String(opts.before));
      const s = qs.toString();
      return request(`/v1/delivery-log${s ? '?' + s : ''}`);
    },
    /// Re-send a previously-failed (or any) delivery. Returns the new
    /// DeliveryLogEntry (200); 409 if the original channel was deleted;
    /// 404 if the id is unknown. Admin-only on the backend.
    retry: (id) => request(`/v1/delivery-log/${id}/retry`, { method: 'POST' }),
  },
  // ─── external metrics + alert rules ──────────────────────────────────────
  // Prometheus-style samples pushed to /v1/metrics/ingest. `series` lists the
  // known {name, labels} pairs (newest activity first, max 1000); `query`
  // returns step-bucketed {ts, avg, min, max, samples} aggregates for one of
  // them. Rules fire notification channels when a metric breaches a threshold
  // for `for_seconds`.
  metrics: {
    series: () => request('/v1/metrics/series'),
    query: (name, { labels, from, to, stepSeconds } = {}) => {
      const qs = new URLSearchParams({ name });
      if (labels && Object.keys(labels).length) qs.set('labels', JSON.stringify(labels));
      if (from)        qs.set('from', from);
      if (to)          qs.set('to',   to);
      if (stepSeconds) qs.set('step_seconds', String(stepSeconds));
      return request(`/v1/metrics/query?${qs.toString()}`);
    },
    listRules:  ()          => request('/v1/metrics/rules'),
    createRule: (input)     => request('/v1/metrics/rules', { method: 'POST', body: input }),
    updateRule: (id, patch) => request(`/v1/metrics/rules/${id}`, { method: 'PATCH', body: patch }),
    removeRule: (id)        => request(`/v1/metrics/rules/${id}`, { method: 'DELETE' }),
  },
  scheduledReports: {
    list:    ()          => request('/v1/scheduled-reports'),
    get:     (id)        => request(`/v1/scheduled-reports/${id}`),
    create:  (input)     => request('/v1/scheduled-reports', { method: 'POST', body: input }),
    update:  (id, patch) => request(`/v1/scheduled-reports/${id}`, { method: 'PATCH', body: patch }),
    remove:  (id)        => request(`/v1/scheduled-reports/${id}`, { method: 'DELETE' }),
    /// Render + email the report immediately. Returns 204 (null) on success.
    send:    (id)        => request(`/v1/scheduled-reports/${id}/send`, { method: 'POST' }),
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
    create:         (email, name, password, role)         => request('/v1/users', { method: 'POST', body: { email, name, password, role } }),
    setAdmin:       (id, isAdmin)                         => request(`/v1/users/${id}/admin`, { method: 'POST', body: { is_admin: !!isAdmin } }),
    setRole:        (id, role)                            => request(`/v1/users/${id}/role`, { method: 'PATCH', body: { role } }),
    remove:         (id)                                  => request(`/v1/users/${id}`, { method: 'DELETE' }),
    changePassword: (current, next)                       => request('/v1/users/me/password', { method: 'POST', body: { current_password: current, new_password: next } }),
  },
  // ─── self-service preferences ──────────────────────────────────────────
  // Per-user UI prefs (saved dashboard views + default folder). The blob is
  // opaque to the backend; the dashboard owns its shape. getPrefs returns the
  // stored object ({} when never set); setPrefs PUTs the full document and
  // echoes it back.
  me: {
    getPrefs: ()      => request('/v1/me/prefs'),
    setPrefs: (prefs) => request('/v1/me/prefs', { method: 'PUT', body: { prefs } }),
  },
  proxies: {
    list:      ()                  => request('/v1/proxies'),
    create:    (input)             => request('/v1/proxies', { method: 'POST', body: input }),
    remove:    (id)                => request(`/v1/proxies/${id}`, { method: 'DELETE' }),
    setActive: (id, active)        => request(`/v1/proxies/${id}/active`, { method: 'POST', body: { active } }),
  },
  // ─── remote probe agents ─────────────────────────────────────────────────
  // Self-hosted probe runners in other networks/regions. `create` returns
  // { agent, token } — the bearer token is shown exactly once. PATCH with
  // location "" clears it; DELETE makes the agent's monitors fall back to
  // local probing.
  agents: {
    list:   ()          => request('/v1/agents'),
    create: (input)     => request('/v1/agents', { method: 'POST', body: input }),
    update: (id, patch) => request(`/v1/agents/${id}`, { method: 'PATCH', body: patch }),
    remove: (id)        => request(`/v1/agents/${id}`, { method: 'DELETE' }),
  },
  apiKeys: {
    list:   ()                  => request('/v1/api-keys'),
    // `rateLimit` is the optional per-hour request budget (integer, default
    // 1000 server-side); omit it (null/undefined) to take the default.
    create: (name, scope, exp, rateLimit) => request('/v1/api-keys', { method: 'POST', body: { name, scope: scope || 'read', expires_at: exp || null, ...(rateLimit != null ? { rate_limit_per_hour: Number(rateLimit) } : {}) } }),
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
  ingestTokens: {
    list:   (pageId)        => request(`/v1/status-pages/${pageId}/ingest-tokens`),
    create: (pageId, label) => request(`/v1/status-pages/${pageId}/ingest-tokens`, { method: 'POST', body: { label } }),
    remove: (tokenId)       => request(`/v1/ingest-tokens/${tokenId}`, { method: 'DELETE' }),
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
  incidentTemplates: {
    list:   ()      => request('/v1/incident-templates'),
    create: (input) => request('/v1/incident-templates', { method: 'POST', body: input }),
    remove: (id)    => request(`/v1/incident-templates/${id}`, { method: 'DELETE' }),
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
    // Component sections (sub-section grouping on the public page).
    listSections:  (pageId)              => request(`/v1/status-pages/${pageId}/sections`),
    createSection: (pageId, name)        => request(`/v1/status-pages/${pageId}/sections`, { method: 'POST', body: { name } }),
    updateSection: (pageId, sectionId, patch) => request(`/v1/status-pages/${pageId}/sections/${sectionId}`, { method: 'PATCH', body: patch }),
    deleteSection: (pageId, sectionId)   => request(`/v1/status-pages/${pageId}/sections/${sectionId}`, { method: 'DELETE' }),
    // Assign a monitor to a section, or null to un-assign (ungrouped).
    assignSection: (pageId, monitorId, sectionId) => request(`/v1/status-pages/${pageId}/monitors/${monitorId}/section`, { method: 'PUT', body: { section_id: sectionId } }),
    publicView: (slug)              => request(`/v1/public/status-pages/${slug}`),
    // Resolve a public page by the request's Host header (custom domain).
    // Backs the host-header routing probe in App.jsx: returns the same
    // PublicStatusPage payload as `publicView`, or 404s when the host has no
    // page (the caller falls through to the dashboard on any error).
    byDomain:   (host)              => request(`/v1/public/status-pages/by-domain/${encodeURIComponent(host)}`),
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
  // ─── escalation policies ─────────────────────────────────────────────────
  // Multi-step alert ladders. A monitor with a policy routes its Down alerts
  // through the ladder instead of its attached channels: step 1 pages
  // immediately, each later step pages `wait_seconds` after the previous
  // unless someone acknowledges or the monitor recovers. `episode` returns
  // the monitor's open episode or null; `ack` stops further escalation
  // (404 when nothing is open / already acked).
  escalation: {
    list:    ()          => request('/v1/escalation-policies'),
    create:  (input)     => request('/v1/escalation-policies', { method: 'POST', body: input }),
    update:  (id, patch) => request(`/v1/escalation-policies/${id}`, { method: 'PATCH', body: patch }),
    remove:  (id)        => request(`/v1/escalation-policies/${id}`, { method: 'DELETE' }),
    episode: (monitorId) => request(`/v1/monitors/${monitorId}/escalation`),
    ack:     (monitorId) => request(`/v1/monitors/${monitorId}/escalation/ack`, { method: 'POST' }),
  },
  // Error tracking — projects hold the DSN key + alert channels; issues group
  // events by fingerprint.
  errorProjects: {
    list:   ()          => request('/v1/error-projects'),
    create: (input)     => request('/v1/error-projects', { method: 'POST', body: input }),
    update: (id, patch) => request(`/v1/error-projects/${id}`, { method: 'PATCH', body: patch }),
    remove: (id)        => request(`/v1/error-projects/${id}`, { method: 'DELETE' }),
    issues: (id, status) => request(`/v1/error-projects/${id}/issues${status ? `?status=${encodeURIComponent(status)}` : ''}`),
  },
  errorIssues: {
    get:       (id) => request(`/v1/error-issues/${id}`),
    events:    (id) => request(`/v1/error-issues/${id}/events`),
    resolve:   (id) => request(`/v1/error-issues/${id}/resolve`,   { method: 'POST' }),
    ignore:    (id) => request(`/v1/error-issues/${id}/ignore`,    { method: 'POST' }),
    unresolve: (id) => request(`/v1/error-issues/${id}/unresolve`, { method: 'POST' }),
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
