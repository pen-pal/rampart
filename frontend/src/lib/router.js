// Hash-based router. Tiny, dependency-free, and pure: parseRoute is a
// pure function so it's easy to unit-test without spinning up the DOM.
//
// Patterns:
//   #/                          → Dashboard
//   #/login                     → Login (or first-run signup)
//   #/notifications             → Notifications
//   #/status-page               → StatusPageBuilder
//   #/new-monitor               → NewMonitorWizard
//   #/maintenance               → Maintenance windows
//   #/s/<slug>                  → Public status page (no auth gate)
//   #/monitor                   → MonitorDetail with id=null
//   #/monitor/<id>              → MonitorDetail(id)

export function parseRoute(hash) {
  const h = hash || '#/';
  if (h.startsWith('#/login'))      return { view: 'login',         id: null };
  if (h.startsWith('#/s/'))         return { view: 'public-status', id: h.slice('#/s/'.length) };
  if (h.startsWith('#/manage/'))    return { view: 'manage-subscription', id: h.slice('#/manage/'.length) };
  if (h.startsWith('#/monitor/'))   return { view: 'monitor',       id: h.slice('#/monitor/'.length) };
  if (h === '#/monitor')            return { view: 'monitor',       id: null };
  if (h === '#/new-monitor')        return { view: 'new-monitor',   id: null };
  if (h === '#/import')             return { view: 'import',        id: null };
  if (h === '#/status-page')        return { view: 'status-page',   id: null };
  if (h === '#/notifications')      return { view: 'notifications', id: null };
  if (h === '#/escalations')        return { view: 'escalations',   id: null };
  if (h.startsWith('#/traces/'))    return { view: 'traces',        id: h.slice('#/traces/'.length) };
  if (h === '#/traces')             return { view: 'traces',        id: null };
  if (h.startsWith('#/logs/trace/')) return { view: 'logs',         id: h.slice('#/logs/trace/'.length) };
  if (h === '#/logs')               return { view: 'logs',          id: null };
  if (h.startsWith('#/profiling/')) return { view: 'profiling',     id: h.slice('#/profiling/'.length) };
  if (h === '#/profiling' || h.startsWith('#/profiling?')) return { view: 'profiling', id: null };
  if (h.startsWith('#/errors/'))    return { view: 'errors',        id: h.slice('#/errors/'.length) };
  if (h === '#/errors')             return { view: 'errors',        id: null };
  if (h === '#/rum')                return { view: 'rum',           id: null };
  if (h === '#/on-call')            return { view: 'on-call',       id: null };
  if (h === '#/alert-rules')        return { view: 'alert-rules',   id: null };
  if (h === '#/detection')          return { view: 'detection',     id: null };
  if (h === '#/slos')               return { view: 'slos',          id: null };
  if (h === '#/silences')           return { view: 'silences',      id: null };
  if (h === '#/maintenance')        return { view: 'maintenance',   id: null };
  if (h === '#/dependencies')       return { view: 'dependencies',  id: null };
  if (h === '#/dashboards')         return { view: 'dashboards',    id: null };
  if (h === '#/api-keys')           return { view: 'api-keys',      id: null };
  if (h === '#/proxies')            return { view: 'proxies',       id: null };
  if (h === '#/agents')             return { view: 'agents',        id: null };
  if (h === '#/security')           return { view: 'security',      id: null };
  if (h === '#/users')              return { view: 'users',         id: null };
  if (h === '#/organizations')      return { view: 'organizations', id: null };
  if (h === '#/folders')            return { view: 'folders',            id: null };
  if (h === '#/tags')               return { view: 'tags',               id: null };
  if (h === '#/settings/smtp')      return { view: 'smtp-settings',      id: null };
  if (h === '#/settings/retention') return { view: 'retention-settings', id: null };
  if (h === '#/settings/ingest')    return { view: 'ingest-settings',    id: null };
  if (h === '#/settings/ingest-keys') return { view: 'ingest-keys',      id: null };
  if (h === '#/audit')              return { view: 'audit',         id: null };
  if (h === '#/reports')            return { view: 'reports',       id: null };
  if (h === '#/templates')          return { view: 'templates',     id: null };
  if (h === '#/delivery-log')       return { view: 'delivery-log',  id: null };
  if (h === '#/metrics')            return { view: 'metrics',       id: null };
  return { view: 'dashboard', id: null };
}
