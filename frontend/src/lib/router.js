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
  if (h.startsWith('#/monitor/'))   return { view: 'monitor',       id: h.slice('#/monitor/'.length) };
  if (h === '#/monitor')            return { view: 'monitor',       id: null };
  if (h === '#/new-monitor')        return { view: 'new-monitor',   id: null };
  if (h === '#/status-page')        return { view: 'status-page',   id: null };
  if (h === '#/notifications')      return { view: 'notifications', id: null };
  if (h === '#/maintenance')        return { view: 'maintenance',   id: null };
  if (h === '#/api-keys')           return { view: 'api-keys',      id: null };
  if (h === '#/proxies')            return { view: 'proxies',       id: null };
  if (h === '#/security')           return { view: 'security',      id: null };
  if (h === '#/users')              return { view: 'users',         id: null };
  if (h === '#/settings/smtp')      return { view: 'smtp-settings', id: null };
  if (h === '#/audit')              return { view: 'audit',         id: null };
  return { view: 'dashboard', id: null };
}
