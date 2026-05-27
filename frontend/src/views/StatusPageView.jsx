// Unauthenticated public status page.
// Rendered at #/s/:slug. No login chrome, no auth gating in App.jsx.

import React, { useEffect, useState } from 'react';
import { CheckCircle2, AlertCircle, AlertTriangle, Calendar, Loader2 } from 'lucide-react';
import { api } from '../lib/api.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap');

  .public {
    --bg:#fafaf9; --surface:#ffffff;
    --border:#e7e5e4;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    --warn:#f59e0b; --warn-soft:#fef3c7;
    --maint:#6366f1; --maint-soft:#e0e7ff;

    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
  }
  .public.dark {
    --bg:#0c0a09; --surface:#1c1917;
    --border:#292524;
    --text:#fafaf9; --text-2:#a8a29e; --text-3:#78716c;
  }
  .public * { box-sizing: border-box; }

  .card {
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 12px;
  }
  .row {
    display: flex; align-items: center; gap: 12px;
    padding: 16px 22px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .badge {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 3px 8px; border-radius: 999px;
    font-size: 11px; font-weight: 600;
  }
  .badge-up      { background: var(--up-soft);    color: #047857; }
  .badge-down    { background: var(--down-soft);  color: #b91c1c; }
  .badge-warn    { background: var(--warn-soft);  color: #92400e; }
  .badge-paused  { background: var(--border);     color: var(--text-2); }
  .badge-pending { background: var(--border);     color: var(--text-3); }
  .badge-maint   { background: var(--maint-soft); color: #4338ca; }

  .summary-banner {
    padding: 22px 24px; border-radius: 12px;
    display: flex; align-items: center; gap: 14px;
    font-size: 16px; font-weight: 500;
  }
  .summary-up   { background: var(--up-soft);    color: #047857; }
  .summary-down { background: var(--down-soft);  color: #b91c1c; }
  .summary-warn { background: var(--warn-soft);  color: #92400e; }
  .summary-maint{ background: var(--maint-soft); color: #4338ca; }
`;

const STATUS_LABEL = {
  up:          'Operational',
  down:        'Down',
  warn:        'Degraded',
  paused:      'Paused',
  pending:     'Pending',
  maintenance: 'Maintenance',
};

function statusIcon(status) {
  if (status === 'up')          return <CheckCircle2 size={14}/>;
  if (status === 'down')        return <AlertCircle  size={14}/>;
  if (status === 'warn')        return <AlertTriangle size={14}/>;
  if (status === 'maintenance') return <Calendar size={14}/>;
  return <CheckCircle2 size={14}/>;
}

// Rolled-up overall: worst-wins. Maintenance is treated as informational
// (overrides Up, but Down wins over Maintenance — a real outage still surfaces).
function overall(monitors) {
  if (monitors.some(m => m.current_status === 'down')) return 'down';
  if (monitors.some(m => m.current_status === 'warn')) return 'warn';
  if (monitors.some(m => m.current_status === 'maintenance')) return 'maintenance';
  return 'up';
}

export default function StatusPageView({ slug }) {
  const [data,    setData]    = useState(null);
  const [error,   setError]   = useState(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const r = await api.statusPages.publicView(slug);
        if (!cancelled) { setData(r); setError(null); setLoading(false); }
      } catch (e) {
        if (!cancelled) { setError(e); setLoading(false); }
      }
    };
    load();
    // Light polling so the page updates without a manual refresh.
    const t = setInterval(load, 30_000);
    return () => { cancelled = true; clearInterval(t); };
  }, [slug]);

  if (loading) {
    return (
      <div className="public">
        <style>{css}</style>
        <div style={{ maxWidth: 720, margin: '0 auto', padding: '80px 24px', textAlign: 'center', color: 'var(--text-3)' }}>
          <Loader2 size={20}/>
        </div>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="public">
        <style>{css}</style>
        <div style={{ maxWidth: 560, margin: '0 auto', padding: '80px 24px', textAlign: 'center' }}>
          <AlertCircle size={32} color="var(--text-3)" style={{ marginBottom: 12 }}/>
          <h1 style={{ fontSize: 22, fontWeight: 600, margin: '0 0 6px' }}>Page not found</h1>
          <p style={{ fontSize: 14, color: 'var(--text-2)', margin: 0 }}>
            No status page is published at <span style={{ fontFamily: 'monospace' }}>/{slug}</span>.
          </p>
        </div>
      </div>
    );
  }

  const themeClass = data.theme === 'dark' ? 'public dark' : 'public';
  const status = overall(data.monitors);
  const summary = {
    up:   ['All systems operational', 'summary-up'],
    warn: ['Some systems degraded',   'summary-warn'],
    down: ['Service disruption',      'summary-down'],
    maintenance: ['Scheduled maintenance in progress', 'summary-maint'],
  }[status];

  return (
    <div className={themeClass}>
      <style>{css}</style>

      <div style={{ maxWidth: 760, margin: '0 auto', padding: '48px 24px 80px' }}>
        <h1 style={{ fontSize: 30, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>
          {data.title}
        </h1>
        {data.description && (
          <p style={{ fontSize: 14, color: 'var(--text-2)', margin: '0 0 32px' }}>{data.description}</p>
        )}

        <div className={`summary-banner ${summary[1]}`} style={{ marginBottom: 32 }}>
          {statusIcon(status)} {summary[0]}
        </div>

        {(data.incidents || []).length > 0 && (
          <>
            <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-2)', textTransform: 'uppercase', letterSpacing: '.04em', margin: '0 0 12px' }}>
              Active incidents
            </h2>
            <div style={{ marginBottom: 28 }}>
              {data.incidents.map((inc, i) => <Incident key={i} incident={inc}/>)}
            </div>
          </>
        )}

        <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-2)', textTransform: 'uppercase', letterSpacing: '.04em', margin: '0 0 12px' }}>
          Components
        </h2>

        <div className="card" style={{ overflow: 'hidden' }}>
          {data.monitors.length === 0 ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
              No monitors attached yet.
            </div>
          ) : data.monitors.map((m, i) => (
            <div className="row" key={i}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 14, fontWeight: 500 }}>{m.name}</div>
                <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                  {m.uptime_90d == null ? 'no data yet' : `${m.uptime_90d.toFixed(2)}% uptime · 90d`}
                </div>
              </div>
              <span className={`badge badge-${m.current_status === 'maintenance' ? 'maint' : m.current_status}`}>
                {statusIcon(m.current_status)} {STATUS_LABEL[m.current_status] || m.current_status}
              </span>
            </div>
          ))}
        </div>

        <div style={{ marginTop: 30 }}>
          <SubscribeBox slug={slug}/>
        </div>

        <p style={{ marginTop: 24, fontSize: 11, color: 'var(--text-3)', textAlign: 'center' }}>
          Last updated {new Date(data.generated_at).toLocaleString()}
        </p>
      </div>
    </div>
  );
}

function SubscribeBox({ slug }) {
  const [email, setEmail] = useState('');
  const [state, setState] = useState('idle'); // idle | sending | ok | err
  const [err,   setErr]   = useState(null);
  const submit = async (e) => {
    e.preventDefault();
    if (!email.includes('@')) { setErr('Enter a valid email.'); setState('err'); return; }
    setState('sending'); setErr(null);
    try {
      await api.subscribers.subscribe(slug, email.trim());
      setState('ok');
    } catch (e2) {
      setErr(e2.message || 'Subscribe failed.');
      setState('err');
    }
  };
  if (state === 'ok') {
    return (
      <div style={{ textAlign: 'center', fontSize: 13, color: 'var(--text-2)' }}>
        Subscribed. You'll get an email when an incident is posted.
      </div>
    );
  }
  return (
    <form onSubmit={submit} style={{ display: 'flex', gap: 8, maxWidth: 420, margin: '0 auto' }}>
      <input
        type="email"
        value={email}
        onChange={e => setEmail(e.target.value)}
        placeholder="you@example.com"
        style={{
          flex: 1, padding: '8px 12px', borderRadius: 8,
          border: '1px solid var(--border)', background: 'var(--surface)',
          color: 'var(--text)', fontSize: 13, fontFamily: 'inherit', outline: 'none',
        }}
      />
      <button type="submit" disabled={state === 'sending'} style={{
        padding: '8px 14px', borderRadius: 8, border: '1px solid var(--text)',
        background: 'var(--text)', color: 'var(--surface)', fontSize: 13,
        cursor: 'pointer', fontWeight: 500,
      }}>
        {state === 'sending' ? 'Subscribing…' : 'Subscribe to updates'}
      </button>
      {state === 'err' && err && (
        <div style={{ width: '100%', fontSize: 11, color: '#b91c1c', marginTop: 6 }}>{err}</div>
      )}
    </form>
  );
}

const STYLE_TO_BG = {
  info:    ['#dbeafe', '#1e40af'],
  warning: ['#fef3c7', '#92400e'],
  danger:  ['#fee2e2', '#b91c1c'],
  primary: ['#e0e7ff', '#4338ca'],
  success: ['#d1fae5', '#047857'],
};

function Incident({ incident }) {
  const [bg, fg] = STYLE_TO_BG[incident.style] || STYLE_TO_BG.warning;
  return (
    <div style={{
      padding: '14px 18px', marginBottom: 10,
      borderRadius: 10, background: bg, color: fg,
    }}>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, marginBottom: 4 }}>
        <strong style={{ fontSize: 14 }}>{incident.title}</strong>
        <span style={{ fontSize: 11, opacity: .75 }}>
          {new Date(incident.created_at).toLocaleString()}
        </span>
      </div>
      <div style={{ fontSize: 13, whiteSpace: 'pre-wrap' }}>{incident.content}</div>
      {(incident.updates || []).map((u, i) => (
        <div key={i} style={{
          marginTop: 8, paddingLeft: 10, borderLeft: `2px solid ${fg}`,
          fontSize: 12.5, opacity: .9,
        }}>
          <div style={{ fontSize: 11, opacity: .8 }}>
            {new Date(u.posted_at).toLocaleString()}
          </div>
          <div style={{ whiteSpace: 'pre-wrap' }}>{u.message}</div>
        </div>
      ))}
    </div>
  );
}
