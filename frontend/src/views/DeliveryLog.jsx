import React, { useState } from 'react';
import {
  ChevronLeft, ChevronRight, Send, AlertCircle, Loader2, CheckCircle2, XCircle, RotateCw, Download,
} from 'lucide-react';
import { api, useApi, ApiError, formatRelative, formatClock, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { isAdmin } from '../lib/roles.js';

const css = `
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', monospace; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 12px; border-radius: 8px; cursor: pointer;
    font-size: 13px; font-weight: 500; line-height: 1;
    background: var(--surface); border: 1px solid var(--border); color: var(--text-2);
    font-family: inherit;
  }
  .btn:hover { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .btn:disabled { opacity: .55; cursor: not-allowed; }
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th {
    text-align: left; font-size: 11px; font-weight: 600; color: var(--text-3);
    text-transform: uppercase; letter-spacing: .04em;
    padding: 10px 14px; border-bottom: 1px solid var(--border);
  }
  td { padding: 11px 14px; border-top: 1px solid var(--border); vertical-align: top; }
  tbody tr:first-child td { border-top: none; }
  .kind {
    font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500;
    background: var(--surface-2); color: var(--text-2); text-transform: uppercase;
  }
  .pill {
    display: inline-flex; align-items: center; gap: 4px;
    font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500;
  }
  .pill-ok   { background: var(--up-soft);   color: #047857; }
  .pill-fail { background: var(--down-soft); color: #b91c1c; }
`;

const PAGE_SIZE = 50;
const tsToDate = (ts) => (Array.isArray(ts) ? offsetDateTimeArrayToDate(ts) : new Date(ts));

export default function DeliveryLog({ user }) {
  const admin = isAdmin(user);
  // Keyset pagination: `cursors` is the stack of `before` values for the
  // pages already visited. Empty → first (newest) page (before=null). The
  // backend has no offset, so each forward step carries the last row's
  // sent_at as the next page's `before`.
  const [cursors, setCursors] = useState([]);
  const [reloadKey, setReloadKey] = useState(0);
  // Transient per-row retry feedback keyed by delivery id:
  // { [id]: { state: 'sending' | 'ok' | 'gone' | 'err', msg? } }.
  const [retryState, setRetryState] = useState({});

  const retry = async (id) => {
    if (!id) return;
    setRetryState(s => ({ ...s, [id]: { state: 'sending' } }));
    try {
      await api.deliveryLog.retry(id);
      setRetryState(s => ({ ...s, [id]: { state: 'ok' } }));
      // Refetch the page so the new delivery row shows up.
      setTimeout(() => setReloadKey(k => k + 1), 900);
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) {
        setRetryState(s => ({ ...s, [id]: { state: 'gone' } }));
      } else {
        setRetryState(s => ({ ...s, [id]: { state: 'err', msg: e.message } }));
      }
    }
  };
  const before = cursors.length ? cursors[cursors.length - 1] : undefined;
  // Fetch one extra row to learn whether a next page exists without a count.
  const state = useApi(
    () => api.deliveryLog.list({ limit: PAGE_SIZE + 1, before }),
    [before, reloadKey],
    { pollMs: 30_000 },
  );

  // Resolve monitor_id → name (fetched once) so the table shows a readable,
  // clickable monitor instead of a raw UUID.
  const monitorsState = useApi(() => api.monitors.list(), []);
  const monitorName = (id) => (monitorsState.data || []).find((m) => m.id === id)?.name || null;

  // The endpoint may return either a bare array or { items, ... }. Normalise.
  const raw = state.data;
  const allRows = Array.isArray(raw) ? raw : (raw?.items || raw?.rows || []);
  const hasNext = allRows.length > PAGE_SIZE;
  const rows = hasNext ? allRows.slice(0, PAGE_SIZE) : allRows;
  const page = cursors.length;

  // Forward cursor = sent_at of the last visible row, as RFC3339 for the
  // `before` query param the backend expects.
  const goNext = () => {
    const last = rows[rows.length - 1];
    if (!last) return;
    setCursors(c => [...c, tsToDate(last.sent_at).toISOString()]);
  };
  const goPrev = () => setCursors(c => c.slice(0, -1));

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 1080, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('delivery.back')}
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('delivery.title')}</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('delivery.subtitle')}</p>
          </div>
          {/* Admin-only full-window CSV dump. Plain anchor with the `download`
              attribute so the browser streams it straight to disk — matches the
              audit-log export affordance. */}
          {admin && (
            <a className="btn" download href="/v1/delivery-log/export.csv" title={t('delivery.export_csv')}>
              <Download size={13}/> {t('delivery.export_csv')}
            </a>
          )}
        </div>

        {state.error && (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>
            {t('delivery.error')}{' '}
            <button className="btn btn-ghost" style={{ marginLeft: 8 }} onClick={() => setReloadKey(k => k + 1)}>
              {t('delivery.retry')}
            </button>
          </div>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {state.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}>
              <Loader2 size={16}/> {t('delivery.loading')}
            </div>
          ) : rows.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Send size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('delivery.empty.title')}</div>
              <div style={{ fontSize: 12.5 }}>{t('delivery.empty.cta')}</div>
            </div>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>{t('delivery.col.time')}</th>
                  <th>{t('delivery.col.channel')}</th>
                  <th>{t('delivery.col.event')}</th>
                  <th>{t('delivery.col.monitor')}</th>
                  <th>{t('delivery.col.result')}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={r.id || `${r.sent_at}-${i}`}>
                    <td style={{ whiteSpace: 'nowrap', color: 'var(--text-2)' }}>
                      <div>{formatRelative(tsToDate(r.sent_at))}</div>
                      <div className="mono" style={{ fontSize: 10.5, color: 'var(--text-3)' }}>{formatClock(tsToDate(r.sent_at))}</div>
                    </td>
                    <td><span className="kind">{r.channel_kind}</span></td>
                    <td><span className="kind">{r.event_kind}</span></td>
                    <td className="mono" style={{ fontSize: 11, color: 'var(--text-2)', wordBreak: 'break-all', maxWidth: 220 }}>
                      {r.monitor_id ? (
                        <a href={`#/monitor/${r.monitor_id}`} style={{ color: 'var(--accent-2)', textDecoration: 'none' }}>
                          {monitorName(r.monitor_id) || `${r.monitor_id.slice(0, 8)}…`}
                        </a>
                      ) : '—'}
                    </td>
                    <td>
                      {r.ok ? (
                        <span className="pill pill-ok"><CheckCircle2 size={12}/> {t('delivery.ok')}</span>
                      ) : (
                        <span title={r.error || ''} style={{ display: 'inline-flex', flexDirection: 'column', gap: 3 }}>
                          <span className="pill pill-fail"><XCircle size={12}/> {t('delivery.fail')}</span>
                          {r.error && <span style={{ fontSize: 11, color: 'var(--down)', maxWidth: 280, wordBreak: 'break-word' }}>{r.error}</span>}
                          {admin && r.id && (() => {
                            const rs = retryState[r.id];
                            return (
                              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, marginTop: 2 }}>
                                <button className="btn btn-ghost" style={{ padding: '3px 8px', fontSize: 11.5 }}
                                  disabled={rs?.state === 'sending' || rs?.state === 'ok'}
                                  onClick={() => retry(r.id)}>
                                  {rs?.state === 'sending'
                                    ? <><Loader2 size={11}/> {t('delivery.retrying')}</>
                                    : rs?.state === 'ok'
                                    ? <><CheckCircle2 size={11}/> {t('delivery.retry_sent')}</>
                                    : <><RotateCw size={11}/> {t('delivery.retry_send')}</>}
                                </button>
                                {rs?.state === 'gone' && <span style={{ fontSize: 11, color: 'var(--down)' }}>{t('delivery.retry_gone')}</span>}
                                {rs?.state === 'err' && <span style={{ fontSize: 11, color: 'var(--down)' }}>{rs.msg || t('delivery.retry_failed')}</span>}
                              </span>
                            );
                          })()}
                        </span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        {(page > 0 || hasNext) && (
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: 16 }}>
            <button className="btn" disabled={page === 0 || state.loading} onClick={goPrev}>
              <ChevronLeft size={13}/> {t('delivery.prev')}
            </button>
            <span style={{ fontSize: 12, color: 'var(--text-3)' }}>{t('delivery.page', { n: page + 1 })}</span>
            <button className="btn" disabled={!hasNext || state.loading} onClick={goNext}>
              {t('delivery.next')} <ChevronRight size={13}/>
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
