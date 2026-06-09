import React, { lazy, Suspense, useEffect, useState } from 'react';
// Views are lazy-loaded so that the initial bundle stays lean — each view
// becomes its own async chunk and only loads when the route is visited.
const Dashboard         = lazy(() => import('./views/Dashboard.jsx'));
const MonitorDetail     = lazy(() => import('./views/MonitorDetail.jsx'));
const StatusPageBuilder = lazy(() => import('./views/StatusPageBuilder.jsx'));
const NewMonitorWizard  = lazy(() => import('./views/NewMonitorWizard.jsx'));
const Login             = lazy(() => import('./views/Login.jsx'));
const Notifications     = lazy(() => import('./views/Notifications.jsx'));
const Maintenance       = lazy(() => import('./views/Maintenance.jsx'));
const ApiKeys           = lazy(() => import('./views/ApiKeys.jsx'));
const Proxies           = lazy(() => import('./views/Proxies.jsx'));
const Security          = lazy(() => import('./views/Security.jsx'));
const Users             = lazy(() => import('./views/Users.jsx'));
const SmtpSettings      = lazy(() => import('./views/SmtpSettings.jsx'));
const RetentionSettings = lazy(() => import('./views/RetentionSettings.jsx'));
const Folders           = lazy(() => import('./views/Folders.jsx'));
const Tags              = lazy(() => import('./views/Tags.jsx'));
const AuditLog          = lazy(() => import('./views/AuditLog.jsx'));
const StatusPageView    = lazy(() => import('./views/StatusPageView.jsx'));
import { api } from './lib/api.js';
import { parseRoute } from './lib/router.js';
import { FloatingThemeToggle } from './components/ThemeToggle.jsx';

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
  'status-page':   'Status pages',
  'notifications': 'Notifications',
  'tags':          'Tags',
  'maintenance':   'Maintenance',
  'api-keys':      'API keys',
  'proxies':       'Proxies',
  'security':      'Security',
  'users':         'Users',
  'smtp-settings':      'SMTP',
  'retention-settings': 'Retention',
  'folders':            'Folders',
  'audit':         'Audit log',
  'public-status': 'Public view',
  'login':         'Login',
};

export default function App() {
  const [route,   setRoute]   = useState(() => parseRoute(window.location.hash));
  const [authState, setAuthState] = useState({ loading: true, user: null, needsSetup: false });

  useEffect(() => {
    const onChange = () => setRoute(parseRoute(window.location.hash));
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
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
        else if (r?.user)        setAuthState({ loading: false, user: r.user, needsSetup: false });
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

  // Gate: if not logged in (and not needing setup) and the user isn't already on
  // the login route, redirect there. We let /#/login render freely either way.
  // Public status pages bypass the gate entirely — that's the whole point.
  if (
    !authState.loading
    && !authState.user
    && route.view !== 'login'
    && route.view !== 'public-status'
  ) {
    if (!window.location.hash.startsWith('#/login')) {
      window.location.hash = '#/login';
    }
    return null;
  }

  let view = null;
  switch (route.view) {
    case 'login':         view = <Login />; break;
    case 'monitor':       view = <MonitorDetail monitorId={route.id} />; break;
    case 'new-monitor':   view = <NewMonitorWizard />; break;
    case 'status-page':   view = <StatusPageBuilder />; break;
    case 'notifications': view = <Notifications />; break;
    case 'maintenance':   view = <Maintenance />; break;
    case 'api-keys':      view = <ApiKeys />; break;
    case 'proxies':       view = <Proxies />; break;
    case 'security':      view = <Security />; break;
    case 'users':         view = <Users />; break;
    case 'smtp-settings':      view = <SmtpSettings />; break;
    case 'retention-settings': view = <RetentionSettings />; break;
    case 'folders':            view = <Folders />; break;
    case 'tags':               view = <Tags />; break;
    case 'audit':         view = <AuditLog />; break;
    case 'public-status': view = <StatusPageView slug={route.id} />; break;
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
      {route.view !== 'login' && route.view !== 'public-status' && <ViewSwitcher current={route.view} />}
    </>
  );
}

// ─── floating dev-only switcher ───────────────────────────────────────────
function ViewSwitcher({ current }) {
  const [open, setOpen] = useState(false);
  const links = [
    { hash: '#/',              view: 'dashboard'     },
    { hash: '#/monitor',       view: 'monitor'       },
    { hash: '#/notifications', view: 'notifications' },
    { hash: '#/maintenance',   view: 'maintenance'   },
    { hash: '#/api-keys',      view: 'api-keys'      },
    { hash: '#/proxies',       view: 'proxies'       },
    { hash: '#/security',      view: 'security'      },
    { hash: '#/users',         view: 'users'         },
    { hash: '#/folders',            view: 'folders'           },
    { hash: '#/tags',               view: 'tags'              },
    { hash: '#/settings/smtp',      view: 'smtp-settings'      },
    { hash: '#/settings/retention', view: 'retention-settings' },
    { hash: '#/audit',         view: 'audit'         },
    { hash: '#/status-page',   view: 'status-page'   },
    { hash: '#/new-monitor',   view: 'new-monitor'   },
  ];
  return (
    <div className="rampart-view-switcher" style={{
      position: 'fixed', right: 16, bottom: 16, zIndex: 10000,
      fontFamily: 'Inter, system-ui, sans-serif', fontSize: 12,
    }}>
      {open && (
        <div style={{
          marginBottom: 8, padding: 8, minWidth: 180,
          background: '#18181b', color: '#fafafa',
          borderRadius: 10, boxShadow: '0 10px 32px rgba(0,0,0,.25)',
        }}>
          <div style={{
            fontSize: 10, color: '#a1a1aa', textTransform: 'uppercase',
            letterSpacing: '.06em', padding: '4px 8px 8px',
          }}>Rampart views</div>
          {links.map(v => (
            <a key={v.hash} href={v.hash} onClick={() => setOpen(false)}
              style={{
                display: 'block', padding: '8px 10px', borderRadius: 6,
                color: 'inherit', textDecoration: 'none',
                background: current === v.view ? '#27272a' : 'transparent',
              }}>
              {VIEW_LABEL[v.view]}
            </a>
          ))}
        </div>
      )}
      <button onClick={() => setOpen(o => !o)} style={{
        background: '#14b8a6', color: 'white', border: 'none',
        padding: '10px 14px', borderRadius: 999, cursor: 'pointer',
        fontWeight: 600, boxShadow: '0 4px 12px rgba(20,184,166,.4)',
        display: 'flex', alignItems: 'center', gap: 6,
      }}>
        ⌘ {open ? 'Close' : 'Views'}
      </button>
    </div>
  );
}
