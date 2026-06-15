import React, { lazy, Suspense, useEffect, useState } from 'react';
// Views are lazy-loaded so that the initial bundle stays lean — each view
// becomes its own async chunk and only loads when the route is visited.
const Dashboard         = lazy(() => import('./views/Dashboard.jsx'));
const MonitorDetail     = lazy(() => import('./views/MonitorDetail.jsx'));
const StatusPageBuilder = lazy(() => import('./views/StatusPageBuilder.jsx'));
const NewMonitorWizard  = lazy(() => import('./views/NewMonitorWizard.jsx'));
const ImportMonitors    = lazy(() => import('./views/ImportMonitors.jsx'));
const Login             = lazy(() => import('./views/Login.jsx'));
const Notifications     = lazy(() => import('./views/Notifications.jsx'));
const Escalations       = lazy(() => import('./views/Escalations.jsx'));
const Traces            = lazy(() => import('./views/Traces.jsx'));
const Logs              = lazy(() => import('./views/Logs.jsx'));
const Profiling         = lazy(() => import('./views/Profiling.jsx'));
const Errors            = lazy(() => import('./views/Errors.jsx'));
const Rum               = lazy(() => import('./views/Rum.jsx'));
const OnCall            = lazy(() => import('./views/OnCall.jsx'));
const AlertRules        = lazy(() => import('./views/AlertRules.jsx'));
const Detection         = lazy(() => import('./views/Detection.jsx'));
const Silences          = lazy(() => import('./views/Silences.jsx'));
const Maintenance       = lazy(() => import('./views/Maintenance.jsx'));
const DependencyGraph   = lazy(() => import('./views/DependencyGraph.jsx'));
const ApiKeys           = lazy(() => import('./views/ApiKeys.jsx'));
const Proxies           = lazy(() => import('./views/Proxies.jsx'));
const Agents            = lazy(() => import('./views/Agents.jsx'));
const Security          = lazy(() => import('./views/Security.jsx'));
const Users             = lazy(() => import('./views/Users.jsx'));
const SmtpSettings      = lazy(() => import('./views/SmtpSettings.jsx'));
const RetentionSettings = lazy(() => import('./views/RetentionSettings.jsx'));
const IngestSettings    = lazy(() => import('./views/IngestSettings.jsx'));
const Folders           = lazy(() => import('./views/Folders.jsx'));
const Tags              = lazy(() => import('./views/Tags.jsx'));
const AuditLog          = lazy(() => import('./views/AuditLog.jsx'));
const ScheduledReports  = lazy(() => import('./views/ScheduledReports.jsx'));
const DeliveryLog       = lazy(() => import('./views/DeliveryLog.jsx'));
const Metrics           = lazy(() => import('./views/Metrics.jsx'));
const MonitorTemplates  = lazy(() => import('./views/MonitorTemplates.jsx'));
const StatusPageView    = lazy(() => import('./views/StatusPageView.jsx'));
const ManageSubscription = lazy(() => import('./views/ManageSubscription.jsx'));
import { api } from './lib/api.js';
import { isAdmin, canWrite } from './lib/roles.js';
import { parseRoute } from './lib/router.js';
import { FloatingThemeToggle, FloatingLocalePicker } from './components/ThemeToggle.jsx';

// Minimal centered fallback shown while a lazy view chunk is downloading.
// Uses existing theme CSS vars so it adapts to light/dark automatically.
function ViewFallback() {
  return (
    <div style={{
      position: 'fixed', inset: 0,
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      background: 'var(--bg)', color: 'var(--text-3)',
      fontFamily: 'Inter, system-ui, sans-serif', fontSize: 13,
    }}>
      Loading…
    </div>
  );
}

const VIEW_LABEL = {
  'dashboard':     'Dashboard',
  'monitor':       'Monitor',
  'new-monitor':   'New monitor',
  'import':        'Import CSV',
  'status-page':   'Status pages',
  'notifications': 'Notifications',
  'escalations':   'Escalations',
  'traces':        'Traces',
  'logs':          'Logs',
  'profiling':     'Profiling',
  'errors':        'Errors',
  'rum':           'RUM',
  'on-call':       'On-call',
  'alert-rules':   'Alert rules',
  'detection':     'Detection',
  'silences':      'Silences',
  'tags':          'Tags',
  'maintenance':   'Maintenance',
  'dependencies':  'Dependencies',
  'api-keys':      'API keys',
  'proxies':       'Proxies',
  'agents':        'Probe agents',
  'security':      'Security',
  'users':         'Users',
  'smtp-settings':      'SMTP',
  'retention-settings': 'Retention',
  'ingest-settings':    'Ingest',
  'folders':            'Folders',
  'audit':         'Audit log',
  'reports':       'Scheduled reports',
  'delivery-log':  'Delivery log',
  'metrics':       'Metrics',
  'templates':     'Monitor templates',
  'public-status': 'Public view',
  'login':         'Login',
};

// Whether the current hash is the "bare" entry (empty or #/) — the only
// state where a custom-domain host should short-circuit to its status page.
// Any explicit in-app route (#/login, #/monitor, #/s/:slug, …) is left alone
// so deep links and the dashboard keep working on a custom domain too.
function isBareHash() {
  const h = window.location.hash;
  return h === '' || h === '#' || h === '#/';
}

export default function App() {
  const [route,   setRoute]   = useState(() => parseRoute(window.location.hash));
  const [authState, setAuthState] = useState({ loading: true, user: null, needsSetup: false });
  // Host-header routing probe. `pending` while we ask the by-domain endpoint
  // whether this hostname maps to a published status page; `host` is set only
  // once a page actually resolves, at which point we render the public view in
  // place of the dashboard shell. Any error (incl. 404) clears `pending` and
  // falls through to the normal dashboard/login flow.
  const [domainProbe, setDomainProbe] = useState({ pending: isBareHash(), host: null });

  useEffect(() => {
    const onChange = () => setRoute(parseRoute(window.location.hash));
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);

  // One-shot host-header probe on boot: if the visitor landed on the bare
  // hash, ask whether this hostname is a status-page custom domain. We only
  // short-circuit when it resolves; a 404 / network error falls through
  // silently so localhost and the normal dashboard are never disrupted.
  useEffect(() => {
    let cancelled = false;
    if (!isBareHash()) { setDomainProbe({ pending: false, host: null }); return undefined; }
    (async () => {
      try {
        const host = window.location.hostname;
        const page = await api.statusPages.byDomain(host);
        if (cancelled) return;
        if (page) setDomainProbe({ pending: false, host });
        else      setDomainProbe({ pending: false, host: null });
      } catch {
        if (!cancelled) setDomainProbe({ pending: false, host: null });
      }
    })();
    return () => { cancelled = true; };
  }, []);

  // One-shot auth check on mount. Re-run when the route changes back to
  // the dashboard (so logout → /#/login → /# refreshes the user state).
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await api.auth.me();
        if (cancelled) return;
        if (r?.needs_setup)      setAuthState({ loading: false, user: null, needsSetup: true });
        else if (r?.user)        setAuthState({ loading: false, user: r.user, needsSetup: false, mustSetup2fa: !!r.must_setup_2fa });
        else                     setAuthState({ loading: false, user: null, needsSetup: false });
      } catch (e) {
        if (cancelled) return;
        // 401 means not logged in; the request layer already redirected to
        // #/login. We still update local state so the switcher hides views.
        setAuthState({ loading: false, user: null, needsSetup: false });
      }
    })();
    return () => { cancelled = true; };
  }, [route.view]);

  // 2FA enrollment gate. When an org policy requires 2FA and this user hasn't
  // enrolled, force them to the Security view until they do — they can't use
  // the rest of the app first.
  useEffect(() => {
    if (authState.user && authState.mustSetup2fa
        && route.view !== 'security' && route.view !== 'login') {
      window.location.hash = '#/security';
    }
  }, [authState.user, authState.mustSetup2fa, route.view]);

  // Host-header routing. While the boot probe is in flight (bare hash only),
  // hold the render so we don't flash the dashboard/login before learning this
  // host is a status-page custom domain. Once resolved, render the public view
  // directly — bypassing the auth gate exactly like the #/s/:slug path.
  if (domainProbe.pending) {
    return <ViewFallback />;
  }
  if (domainProbe.host && isBareHash()) {
    return (
      <>
        <Suspense fallback={<ViewFallback />}>
          <StatusPageView byDomainHost={domainProbe.host} />
        </Suspense>
        <FloatingThemeToggle />
        <FloatingLocalePicker />
      </>
    );
  }

  // Gate: if not logged in (and not needing setup) and the user isn't already on
  // the login route, redirect there. We let /#/login render freely either way.
  // Public status pages bypass the gate entirely — that's the whole point.
  if (
    !authState.loading
    && !authState.user
    && route.view !== 'login'
    && route.view !== 'public-status'
    && route.view !== 'manage-subscription'
  ) {
    if (!window.location.hash.startsWith('#/login')) {
      window.location.hash = '#/login';
    }
    return null;
  }

  // Admin-only views. Non-admins (editor / readonly) who navigate here
  // directly are bounced to the dashboard — the backend also 403s these,
  // this is just UX so they don't land on a dead screen. `role` is the
  // source of truth on `authState.user`.
  const ADMIN_ONLY_VIEWS = new Set([
    'users', 'security', 'api-keys', 'proxies', 'agents', 'audit',
    'smtp-settings', 'retention-settings', 'ingest-settings', 'reports', 'delivery-log',
  ]);
  if (
    !authState.loading
    && authState.user
    && ADMIN_ONLY_VIEWS.has(route.view)
    && !isAdmin(authState.user)
  ) {
    if (window.location.hash !== '#/') window.location.hash = '#/';
    return null;
  }

  // Editor-or-admin views. Readonly users who deep-link here are bounced to
  // the dashboard (the backend also gates the mutating routes). Mirrors the
  // ADMIN_ONLY_VIEWS bounce above but for the looser write gate.
  const WRITE_ONLY_VIEWS = new Set(['templates']);
  if (
    !authState.loading
    && authState.user
    && WRITE_ONLY_VIEWS.has(route.view)
    && !canWrite(authState.user)
  ) {
    if (window.location.hash !== '#/') window.location.hash = '#/';
    return null;
  }

  const user = authState.user;

  let view = null;
  switch (route.view) {
    case 'login':         view = <Login />; break;
    case 'monitor':       view = <MonitorDetail monitorId={route.id} user={user} />; break;
    case 'new-monitor':   view = <NewMonitorWizard />; break;
    case 'import':        view = <ImportMonitors user={user} />; break;
    case 'status-page':   view = <StatusPageBuilder user={user} />; break;
    case 'notifications': view = <Notifications user={user} />; break;
    case 'escalations':   view = <Escalations user={user} />; break;
    case 'traces':        view = <Traces openTraceId={route.id} />; break;
    case 'logs':          view = <Logs traceId={route.id} />; break;
    case 'profiling':     view = <Profiling profileId={route.id} />; break;
    case 'errors':        view = <Errors user={user} />; break;
    case 'rum':           view = <Rum />; break;
    case 'on-call':       view = <OnCall user={user} />; break;
    case 'alert-rules':   view = <AlertRules />; break;
    case 'detection':     view = <Detection user={user} />; break;
    case 'silences':      view = <Silences />; break;
    case 'maintenance':   view = <Maintenance user={user} />; break;
    case 'dependencies':  view = <DependencyGraph />; break;
    case 'api-keys':      view = <ApiKeys user={user} />; break;
    case 'proxies':       view = <Proxies />; break;
    case 'agents':        view = <Agents />; break;
    case 'security':      view = <Security />; break;
    case 'users':         view = <Users />; break;
    case 'smtp-settings':      view = <SmtpSettings />; break;
    case 'retention-settings': view = <RetentionSettings />; break;
    case 'ingest-settings':    view = <IngestSettings />; break;
    case 'folders':            view = <Folders />; break;
    case 'tags':               view = <Tags />; break;
    case 'audit':         view = <AuditLog />; break;
    case 'reports':       view = <ScheduledReports user={user} />; break;
    case 'delivery-log':  view = <DeliveryLog user={user} />; break;
    case 'metrics':       view = <Metrics user={user} />; break;
    case 'templates':     view = <MonitorTemplates user={user} />; break;
    case 'public-status': view = <StatusPageView slug={route.id} />; break;
    case 'manage-subscription': view = <ManageSubscription token={route.id} />; break;
    case 'dashboard':
    default:            view = <Dashboard user={authState.user} onLogout={async () => {
      try { await api.auth.logout(); } catch {}
      setAuthState({ loading: false, user: null, needsSetup: false });
      window.location.hash = '#/login';
    }}/>; break;
  }

  // Theme toggle: shown on every authenticated view AND on the public
  // status surface (so a visitor can flip to dark too). Hidden on the
  // login screen because the Login view already carries the brand mark
  // chrome at the top.
  const showThemeToggle = route.view !== 'login';

  return (
    <>
      <Suspense fallback={<ViewFallback />}>
        {view}
      </Suspense>
      {showThemeToggle && <FloatingThemeToggle />}
      {showThemeToggle && <FloatingLocalePicker />}
      {route.view !== 'login' && route.view !== 'public-status' && route.view !== 'manage-subscription' && <NavDrawer current={route.view} user={authState.user} />}
    </>
  );
}

// ─── global navigation drawer ─────────────────────────────────────────────
// The single, consistent navigation across every authenticated view: a
// launcher pill that opens a left slide-in drawer with role-filtered links
// grouped into sections, a fuzzy filter box, and an active highlight. Theme
// aware (uses the global CSS vars, like the rest of the app), so no hardcoded
// colours. Replaces both the old dev-only floating switcher and the per-view
// menus that drifted out of sync.
//
// `admin: true` items are hidden for non-admins; `write: true` for readonly.
const NAV_GROUPS = [
  { section: 'Overview', items: [
    { view: 'dashboard',    hash: '#/' },
    { view: 'metrics',      hash: '#/metrics' },
    { view: 'dependencies', hash: '#/dependencies' },
  ] },
  { section: 'Observability', items: [
    { view: 'traces',    hash: '#/traces' },
    { view: 'logs',      hash: '#/logs' },
    { view: 'errors',    hash: '#/errors' },
    { view: 'rum',       hash: '#/rum' },
    { view: 'profiling', hash: '#/profiling' },
  ] },
  { section: 'Alerting & response', items: [
    { view: 'alert-rules',   hash: '#/alert-rules' },
    { view: 'detection',     hash: '#/detection' },
    { view: 'escalations',   hash: '#/escalations' },
    { view: 'on-call',       hash: '#/on-call' },
    { view: 'silences',      hash: '#/silences' },
    { view: 'notifications', hash: '#/notifications' },
    { view: 'maintenance',   hash: '#/maintenance' },
  ] },
  { section: 'Catalog', items: [
    { view: 'templates',   hash: '#/templates', write: true },
    { view: 'folders',     hash: '#/folders' },
    { view: 'tags',        hash: '#/tags' },
    { view: 'status-page', hash: '#/status-page' },
  ] },
  { section: 'Administration', items: [
    { view: 'users',        hash: '#/users',        admin: true },
    { view: 'agents',       hash: '#/agents',       admin: true },
    { view: 'api-keys',     hash: '#/api-keys',     admin: true },
    { view: 'proxies',      hash: '#/proxies',      admin: true },
    { view: 'audit',        hash: '#/audit',        admin: true },
    { view: 'delivery-log', hash: '#/delivery-log', admin: true },
    { view: 'reports',      hash: '#/reports',      admin: true },
  ] },
  { section: 'Settings', items: [
    { view: 'security',           hash: '#/security',           admin: true },
    { view: 'smtp-settings',      hash: '#/settings/smtp',      admin: true },
    { view: 'retention-settings', hash: '#/settings/retention', admin: true },
    { view: 'ingest-settings',    hash: '#/settings/ingest',    admin: true },
  ] },
];

function NavDrawer({ current, user }) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState('');
  const admin = isAdmin(user);
  const writable = canWrite(user);

  // Close on Escape; lock nothing else.
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (e) => { if (e.key === 'Escape') setOpen(false); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);

  // Any view's header can open this single drawer by dispatching the event —
  // the dashboard's header button does, so nav lives where users expect it
  // (not only the floating launcher).
  useEffect(() => {
    const onOpen = () => setOpen(true);
    window.addEventListener('rampart:nav-open', onOpen);
    return () => window.removeEventListener('rampart:nav-open', onOpen);
  }, []);

  const visible = (it) => (admin || !it.admin) && (writable || !it.write);
  const ql = q.trim().toLowerCase();
  const match = (it) => !ql || (VIEW_LABEL[it.view] || it.view).toLowerCase().includes(ql);
  const go = () => { setOpen(false); setQ(''); };

  const groups = NAV_GROUPS
    .map(g => ({ ...g, items: g.items.filter(it => visible(it) && match(it)) }))
    .filter(g => g.items.length > 0);

  return (
    <>
      {/* launcher */}
      <button
        aria-label="Open navigation"
        onClick={() => setOpen(true)}
        style={{
          position: 'fixed', right: 16, bottom: 16, zIndex: 9998,
          width: 44, height: 44, borderRadius: 12, cursor: 'pointer',
          background: 'var(--accent, #14b8a6)', color: '#fff', border: 'none',
          fontSize: 18, lineHeight: 1, boxShadow: '0 6px 18px rgba(0,0,0,.22)',
          fontFamily: 'Inter, system-ui, sans-serif',
        }}
      >☰</button>

      {open && (
        <>
          {/* backdrop */}
          <div onClick={() => setOpen(false)} style={{
            position: 'fixed', inset: 0, zIndex: 9999,
            background: 'rgba(0,0,0,.38)', backdropFilter: 'blur(1px)',
          }}/>
          {/* drawer */}
          <nav style={{
            position: 'fixed', top: 0, right: 0, bottom: 0, zIndex: 10000,
            width: 286, maxWidth: '84vw', display: 'flex', flexDirection: 'column',
            background: 'var(--surface, #fff)', color: 'var(--text, #18181b)',
            borderLeft: '1px solid var(--border, #e7e5e4)',
            boxShadow: '0 0 40px rgba(0,0,0,.25)',
            fontFamily: 'Inter, system-ui, sans-serif',
          }}>
            <div style={{
              display: 'flex', alignItems: 'center', justifyContent: 'space-between',
              gap: 8, padding: '14px 16px', borderBottom: '1px solid var(--border, #e7e5e4)',
            }}>
              <span style={{ fontSize: 13, fontWeight: 700, letterSpacing: '-.01em' }}>Navigate</span>
              <button aria-label="Close" onClick={() => setOpen(false)} style={{
                border: 'none', background: 'transparent', cursor: 'pointer',
                color: 'var(--text-3, #a8a29e)', fontSize: 18, lineHeight: 1,
              }}>✕</button>
            </div>

            <div style={{ padding: '10px 12px' }}>
              <input
                autoFocus
                value={q}
                onChange={e => setQ(e.target.value)}
                placeholder="Filter…"
                style={{
                  width: '100%', padding: '8px 10px', borderRadius: 8, fontSize: 13,
                  background: 'var(--bg, #fafaf9)', color: 'var(--text, #18181b)',
                  border: '1px solid var(--border, #e7e5e4)', outline: 'none',
                  fontFamily: 'inherit', boxSizing: 'border-box',
                }}
              />
            </div>

            <div style={{ overflowY: 'auto', padding: '0 8px 16px', flex: 1 }}>
              {groups.length === 0 && (
                <div style={{ padding: 16, fontSize: 12.5, color: 'var(--text-3, #a8a29e)' }}>No matches.</div>
              )}
              {groups.map(g => (
                <div key={g.section} style={{ marginTop: 12 }}>
                  <div style={{
                    fontSize: 10, fontWeight: 600, textTransform: 'uppercase',
                    letterSpacing: '.06em', color: 'var(--text-3, #a8a29e)',
                    padding: '0 10px 4px',
                  }}>{g.section}</div>
                  {g.items.map(it => {
                    const active = current === it.view;
                    return (
                      <a key={it.view} href={it.hash} onClick={go} style={{
                        display: 'block', padding: '8px 10px', borderRadius: 8,
                        fontSize: 13, textDecoration: 'none', marginBottom: 1,
                        color: active ? 'var(--accent-2, #0d9488)' : 'var(--text-2, #57534e)',
                        background: active ? 'var(--accent-soft, #ccfbf1)' : 'transparent',
                        fontWeight: active ? 600 : 500,
                      }}>
                        {VIEW_LABEL[it.view] || it.view}
                      </a>
                    );
                  })}
                </div>
              ))}
            </div>
          </nav>
        </>
      )}
    </>
  );
}
