// Public, token-based subscription-management page.
// Rendered at #/manage/:token. No login chrome, no auth gate in App.jsx
// (mirrors StatusPageView). A subscriber lands here from the "manage your
// subscriptions" link in any notification email.
//
// Backend (all public, token in the path):
//   GET  /v1/public/subscribers/manage/:token
//        → { email, subscriptions: [{status_page_slug, status_page_title, subscribed_at}] }
//   POST /v1/public/subscribers/manage/:token/unsubscribe-all
//   POST /v1/public/subscribers/manage/:token/unsubscribe/:slug

import React, { useEffect, useState } from 'react';
import { CheckCircle2, AlertCircle, Loader2, Bell } from 'lucide-react';
import { offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { confirmDialog } from '../lib/notify.js';

function toDate(v) {
  return Array.isArray(v) ? offsetDateTimeArrayToDate(v) : new Date(v);
}

const css = `
  .public {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh; font-feature-settings: 'cv11','ss01';
  }
  .public.dark {
    --bg:#0c0a09; --surface:#1c1917; --surface-2:#292524;
    --border:#292524; --border-2:#44403c;
    --text:#fafaf9; --text-2:#d6d3d1; --text-3:#78716c;
    --accent:#2dd4bf; --accent-2:#14b8a6; --accent-soft:#134e4a;
    --up:#34d399; --up-soft:#064e3b; --down:#f87171; --down-soft:#7f1d1d;
  }
  .public * { box-sizing: border-box; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 14px; }
  .sub-row {
    display: flex; align-items: center; justify-content: space-between; gap: 14px;
    padding: 16px 20px; border-top: 1px solid var(--border);
  }
  .sub-row:first-child { border-top: none; }
  .btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 8px 14px; border-radius: 9px; cursor: pointer;
    font-size: 13px; font-weight: 500; font-family: inherit;
    background: var(--surface); border: 1px solid var(--border); color: var(--text-2);
    transition: all .12s;
  }
  .btn:hover:not([disabled]) { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .btn[disabled] { opacity: .55; cursor: not-allowed; }
  .btn-danger { color: var(--down); border-color: var(--down); }
  .btn-danger:hover:not([disabled]) { background: var(--down-soft); }
`;

export default function ManageSubscription({ token }) {
  const [state, setState] = useState('loading'); // loading | ok | invalid | done
  const [data,  setData]  = useState(null);
  const [busy,  setBusy]  = useState(null);      // slug being unsubscribed, or 'all'
  const [err,   setErr]   = useState(null);

  const base = `/v1/public/subscribers/manage/${encodeURIComponent(token)}`;

  const load = async () => {
    try {
      const r = await fetch(base);
      if (!r.ok) { setState('invalid'); return; }
      setData(await r.json());
      setState('ok');
    } catch {
      setState('invalid');
    }
  };

  useEffect(() => { load(); /* eslint-disable-next-line */ }, [token]);

  const unsubscribePage = async (slug) => {
    setBusy(slug); setErr(null);
    try {
      const r = await fetch(`${base}/unsubscribe/${encodeURIComponent(slug)}`, { method: 'POST' });
      if (!r.ok) throw new Error();
      await load();
    } catch { setErr(t('subscribe.manage.err_failed')); }
    finally { setBusy(null); }
  };

  const unsubscribeAll = async () => {
    if (!(await confirmDialog({ message: t('subscribe.manage.confirm_all') }))) return;
    setBusy('all'); setErr(null);
    try {
      const r = await fetch(`${base}/unsubscribe-all`, { method: 'POST' });
      if (!r.ok) throw new Error();
      setState('done');
    } catch { setErr(t('subscribe.manage.err_failed')); setBusy(null); }
  };

  const shell = (inner) => (
    <div className="public">
      <style>{css}</style>
      <div style={{ maxWidth: 560, margin: '0 auto', padding: '64px 24px' }}>{inner}</div>
    </div>
  );

  if (state === 'loading') {
    return shell(
      <div style={{ textAlign: 'center', color: 'var(--text-3)' }}>
        <Loader2 size={22} style={{ animation: 'spin 1s linear infinite' }}/>
        <div style={{ marginTop: 10, fontSize: 13 }}>{t('subscribe.manage.loading')}</div>
      </div>,
    );
  }

  if (state === 'invalid') {
    return shell(
      <div style={{ textAlign: 'center' }}>
        <AlertCircle size={32} color="var(--text-3)" style={{ marginBottom: 12 }}/>
        <p style={{ fontSize: 14, color: 'var(--text-2)' }}>{t('subscribe.manage.invalid')}</p>
      </div>,
    );
  }

  if (state === 'done') {
    return shell(
      <div style={{ textAlign: 'center' }}>
        <CheckCircle2 size={32} color="var(--up)" style={{ marginBottom: 12 }}/>
        <p style={{ fontSize: 14, color: 'var(--text-2)' }}>{t('subscribe.manage.done')}</p>
      </div>,
    );
  }

  const subs = data?.subscriptions || [];
  return shell(
    <>
      <h1 style={{ fontSize: 24, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em', display: 'flex', alignItems: 'center', gap: 10 }}>
        <Bell size={20}/> {t('subscribe.manage.title')}
      </h1>
      {data?.email && (
        <p style={{ fontSize: 13, color: 'var(--text-3)', margin: '0 0 24px' }}>
          {t('subscribe.manage.email_label')} <strong style={{ color: 'var(--text-2)' }}>{data.email}</strong>
        </p>
      )}

      {err && (
        <div style={{ background: 'var(--down-soft)', color: 'var(--down)', padding: '10px 14px', borderRadius: 8, fontSize: 13, marginBottom: 16 }}>
          {err}
        </div>
      )}

      {subs.length === 0 ? (
        <div className="card" style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>
          {t('subscribe.manage.none')}
        </div>
      ) : (
        <>
          <div className="card" style={{ overflow: 'hidden', marginBottom: 18 }}>
            {subs.map((sapt) => (
              <div className="sub-row" key={sapt.status_page_slug}>
                <div style={{ minWidth: 0 }}>
                  <div style={{ fontSize: 14, fontWeight: 500 }}>{sapt.status_page_title}</div>
                  <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 2 }}>
                    {t('subscribe.manage.subscribed_on', { date: toDate(sapt.subscribed_at).toLocaleDateString() })}
                  </div>
                </div>
                <button className="btn" disabled={!!busy}
                  onClick={() => unsubscribePage(sapt.status_page_slug)}>
                  {busy === sapt.status_page_slug ? t('subscribe.manage.unsubscribing') : t('subscribe.manage.unsubscribe')}
                </button>
              </div>
            ))}
          </div>
          <button className="btn btn-danger" disabled={!!busy} onClick={unsubscribeAll}>
            {busy === 'all' ? t('subscribe.manage.unsubscribing') : t('subscribe.manage.unsubscribe_all')}
          </button>
        </>
      )}
    </>,
  );
}
