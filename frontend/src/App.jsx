import React, { useEffect, useState } from 'react';
import Dashboard         from './views/Dashboard.jsx';
import MonitorDetail     from './views/MonitorDetail.jsx';
import StatusPageBuilder from './views/StatusPageBuilder.jsx';
import NewMonitorWizard  from './views/NewMonitorWizard.jsx';
import Login             from './views/Login.jsx';
import Notifications     from './views/Notifications.jsx';
import Maintenance       from './views/Maintenance.jsx';
import ApiKeys           from './views/ApiKeys.jsx';
import Proxies           from './views/Proxies.jsx';
import Security          from './views/Security.jsx';
import Users             from './views/Users.jsx';
import SmtpSettings      from './views/SmtpSettings.jsx';
import AuditLog          from './views/AuditLog.jsx';
import StatusPageView    from './views/StatusPageView.jsx';
import { api } from './lib/api.js';
import { parseRoute } from './lib/router.js';

const VIEW_LABEL = {
  'dashboard':     'Dashboard',
  'monitor':       'Monitor',
  'new-monitor':   'New monitor',
  'status-page':   'Status pages',
  'notifications': 'Notifications',
  'maintenance':   'Maintenance',
  'api-keys':      'API keys',
  'proxies':       'Proxies',
  'security':      'Security',
  'users':         'Users',
  'smtp-settings': 'SMTP',
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
    case 'smtp-settings': view = <SmtpSettings />; break;
    case 'audit':         view = <AuditLog />; break;
    case 'public-status': view = <StatusPageView slug={route.id} />; break;
    case 'dashboard':
    default:            view = <Dashboard user={authState.user} onLogout={async () => {
      try { await api.auth.logout(); } catch {}
      setAuthState({ loading: false, user: null, needsSetup: false });
      window.location.hash = '#/login';
    }}/>; break;
  }

  return (
    <>
      {view}
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
    { hash: '#/settings/smtp', view: 'smtp-settings' },
    { hash: '#/audit',         view: 'audit'         },
    { hash: '#/status-page',   view: 'status-page'   },
    { hash: '#/new-monitor',   view: 'new-monitor'   },
  ];
  return (
    <div style={{
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
