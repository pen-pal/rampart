import React, { useState } from 'react';
import { ChevronLeft, Loader2, AlertCircle, Gauge, Copy, Check } from 'lucide-react';
import { api, useApi, formatRelative } from '../lib/api.js';
import { t } from '../lib/i18n.js';

// Coarse browser family from a UA string (mirrors the server-side bucketing).
function browserFromUa(ua) {
  if (!ua) return '—';
  const s = ua.toLowerCase();
  if (s.includes('edg/') || s.includes('edge')) return 'Edge';
  if (s.includes('opr/') || s.includes('opera')) return 'Opera';
  if (s.includes('chrome') || s.includes('crios')) return 'Chrome';
  if (s.includes('firefox') || s.includes('fxios')) return 'Firefox';
  if (s.includes('safari')) return 'Safari';
  return 'Other';
}

// Per-URL drill-down: recent individual page-loads — who, browser, vitals, trace.
function PageDetail({ app, url, hours }) {
  const st = useApi(() => api.rum.page(app, url, hours), [app, url, hours]);
  const rows = st.data || [];
  const cols = '120px 1fr 80px 64px 64px 100px';
  if (st.loading) return <div style={{ padding: '8px 16px', fontSize: 12, color: 'var(--text-3)' }}><Loader2 size={13}/></div>;
  if (!rows.length) return <div style={{ padding: '8px 16px', fontSize: 12, color: 'var(--text-3)' }}>{t('rum.no_samples')}</div>;
  return (
    <div style={{ padding: '4px 16px 12px', background: 'var(--bg)' }}>
      <div className="row" style={{ gridTemplateColumns: cols, fontSize: 10.5, color: 'var(--text-3)', fontWeight: 600 }}>
        <span>{t('rum.when')}</span><span>{t('rum.who')}</span><span>{t('rum.browser')}</span><span>LCP</span><span>INP</span><span>{t('rum.trace')}</span>
      </div>
      {rows.map((r, i) => (
        <div className="row" key={i} style={{ gridTemplateColumns: cols, fontSize: 11.5 }}>
          <span>{formatRelative(r.ts)}</span>
          <span title={r.user_id || r.session || ''} style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {r.user_id || (r.session ? `${t('rum.anon')} ${String(r.session).slice(0, 8)}` : '—')}
          </span>
          <span>{browserFromUa(r.ua)}</span>
          <span>{r.lcp_ms == null ? '—' : `${Math.round(r.lcp_ms)}ms`}</span>
          <span>{r.inp_ms == null ? '—' : `${Math.round(r.inp_ms)}ms`}</span>
          <span>{r.trace_id ? <a href={`#/traces/${encodeURIComponent(r.trace_id)}`} className="mono" style={{ color: 'var(--accent)' }}>{String(r.trace_id).slice(0, 8)}…</a> : '—'}</span>
        </div>
      ))}
    </div>
  );
}

const css = `
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4; --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e; --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --good:#16a34a; --good-soft:#dcfce7; --amber:#b45309; --amber-soft:#fef3c7; --poor:#ef4444; --poor-soft:#fee2e2;
    background: var(--bg); color: var(--text); font-family: Inter, ui-sans-serif, system-ui, sans-serif; min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', monospace; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn { display: inline-flex; align-items: center; gap: 6px; padding: 7px 12px; border-radius: 8px; cursor: pointer; font-size: 13px; font-weight: 500; line-height: 1; background: var(--surface); border: 1px solid var(--border); color: var(--text-2); font-family: inherit; }
  .btn:hover { background: var(--surface-2); color: var(--text); }
  .btn-ghost { background: transparent; border-color: transparent; }
  .select { padding: 8px 10px; border-radius: 8px; background: var(--surface); border: 1px solid var(--border); font-size: 13px; color: var(--text); outline: none; font-family: inherit; }
  .banner-err { background: var(--poor-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .vital { border-radius: 12px; padding: 16px 18px; border: 1px solid var(--border); }
  .vital .lbl { font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: .04em; color: var(--text-3); }
  .vital .val { font-size: 26px; font-weight: 700; margin-top: 4px; }
  .v-good { background: var(--good-soft); border-color: #bbf7d0; } .v-good .val { color: var(--good); }
  .v-amber { background: var(--amber-soft); border-color: #fde68a; } .v-amber .val { color: var(--amber); }
  .v-poor { background: var(--poor-soft); border-color: #fecaca; } .v-poor .val { color: var(--poor); }
  .v-none { background: var(--surface-2); } .v-none .val { color: var(--text-3); }
  .row { display: grid; grid-template-columns: 1fr 70px 90px 90px 80px; gap: 10px; align-items: center; padding: 11px 16px; border-top: 1px solid var(--border); font-size: 12.5px; }
  .row:first-child { border-top: none; }
  .codeblock { background:#1c1917; color:#e7e5e4; border-radius:8px; padding:12px 14px; font-family:'JetBrains Mono',monospace; font-size:12px; overflow-x:auto; display:flex; justify-content:space-between; gap:10px; align-items:center; }
`;

// p75 thresholds: [good <=, poor >]. ms except cls.
const THRESH = { lcp: [2500, 4000], inp: [200, 500], cls: [0.1, 0.25], fcp: [1800, 3000], ttfb: [800, 1800] };

function rating(metric, v) {
  if (v == null) return 'none';
  const th = THRESH[metric];
  if (!th) return 'none';
  if (v <= th[0]) return 'good';
  if (v > th[1]) return 'poor';
  return 'amber';
}
function fmtVital(metric, v) {
  if (v == null) return '—';
  if (metric === 'cls') return v.toFixed(2);
  return v < 1000 ? `${Math.round(v)}ms` : `${(v / 1000).toFixed(2)}s`;
}

export default function Rum() {
  const [app, setApp] = useState('');
  const [hours, setHours] = useState(24);
  const [copied, setCopied] = useState(false);
  const [openPage, setOpenPage] = useState(null);
  const appsState = useApi(() => api.rum.apps(), []);
  const sumState = useApi(() => api.rum.summary(app, hours), [app, hours]);
  const pagesState = useApi(() => api.rum.pages(app, hours), [app, hours]);
  const tracedState = useApi(() => api.rum.traced(app, hours), [app, hours]);
  const browsersState = useApi(() => api.rum.browsers(app, hours), [app, hours]);
  const usersState = useApi(() => api.rum.users(app, hours), [app, hours]);
  const apps = appsState.data || [];
  const sum = sumState.data;
  const pages = pagesState.data || [];
  const traced = tracedState.data || [];
  const browsers = browsersState.data || [];
  const users = usersState.data || [];

  const snippet = `<script src="${window.location.origin}/rum/snippet.js" data-app="web"></script>`;
  const copy = async () => {
    try { await navigator.clipboard.writeText(snippet); setCopied(true); setTimeout(() => setCopied(false), 1500); } catch { /* visible to copy manually */ }
  };

  const VITALS = [['lcp', sum?.lcp_p75], ['inp', sum?.inp_p75], ['cls', sum?.cls_p75], ['fcp', sum?.fcp_p75], ['ttfb', sum?.ttfb_p75]];

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 1000, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}><ChevronLeft size={14}/> {t('rum.back')}</a>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', gap: 14, marginBottom: 18, flexWrap: 'wrap' }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('rum.title')}</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('rum.subtitle')}</p>
          </div>
          <div style={{ display: 'flex', gap: 8 }}>
            <select className="select" value={app} onChange={e => setApp(e.target.value)}>
              <option value="">{t('rum.all_apps')}</option>
              {apps.map(a => <option key={a} value={a}>{a}</option>)}
            </select>
            <select className="select" value={hours} onChange={e => setHours(Number(e.target.value))}>
              <option value={24}>{t('rum.last_24h')}</option>
              <option value={168}>{t('rum.last_7d')}</option>
            </select>
          </div>
        </div>

        {sumState.error && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{t('rum.load_error')}</div>}

        {sumState.loading ? (
          <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('rum.loading')}</div>
        ) : !sum || sum.views === 0 ? (
          <div className="card" style={{ padding: 40, textAlign: 'center', color: 'var(--text-3)', marginBottom: 18 }}>
            <Gauge size={28} style={{ marginBottom: 10, opacity: .5 }}/>
            <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('rum.empty.title')}</div>
            <div style={{ fontSize: 12.5 }}>{t('rum.empty.cta')}</div>
          </div>
        ) : (
          <>
            <div style={{ fontSize: 12.5, color: 'var(--text-3)', marginBottom: 10 }}>{t('rum.pageviews', { n: sum.views })}</div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))', gap: 12, marginBottom: 24 }}>
              {VITALS.map(([metric, v]) => (
                <div className={`vital v-${rating(metric, v)}`} key={metric}>
                  <div className="lbl">{metric.toUpperCase()}</div>
                  <div className="val">{fmtVital(metric, v)}</div>
                </div>
              ))}
            </div>

            <div className="field-label" style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-2)', marginBottom: 8 }}>{t('rum.pages')}</div>
            <div className="card" style={{ overflow: 'hidden', marginBottom: 24 }}>
              <div className="row" style={{ fontWeight: 600, color: 'var(--text-3)', fontSize: 11 }}>
                <span>{t('rum.page')}</span><span>{t('rum.views')}</span><span>LCP</span><span>INP</span><span>CLS</span>
              </div>
              {pages.map(p => {
                // Rows are per-(app, url): the same path on two sites is two
                // rows, so identity (key / open-state / drill-down) must include
                // the app, not just the url. The drill-down also uses the row's
                // OWN app (p.app), not the filter (which is empty for "all apps").
                const pageKey = `${p.app} ${p.url}`;
                const showApp = !app; // app filter empty ⇒ "all apps" ⇒ label each row
                return (
                <React.Fragment key={pageKey}>
                  <div className="row" style={{ cursor: 'pointer' }} title={t('rum.drill_hint')}
                    onClick={() => setOpenPage(openPage === pageKey ? null : pageKey)}>
                    <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={showApp ? `${p.app} · ${p.url}` : p.url}>
                      <span style={{ color: 'var(--text-3)', fontSize: 9, marginRight: 5 }}>{openPage === pageKey ? '▾' : '▸'}</span>
                      {showApp && <span style={{ display: 'inline-block', background: 'var(--surface-2)', color: 'var(--text-2)', fontSize: 10, fontWeight: 600, padding: '1px 6px', borderRadius: 4, marginRight: 6 }}>{p.app}</span>}
                      {p.url}
                    </span>
                    <span>{p.views}</span>
                    <span style={{ color: `var(--${rating('lcp', p.lcp_p75) === 'none' ? 'text-3' : rating('lcp', p.lcp_p75)})` }}>{fmtVital('lcp', p.lcp_p75)}</span>
                    <span style={{ color: `var(--${rating('inp', p.inp_p75) === 'none' ? 'text-3' : rating('inp', p.inp_p75)})` }}>{fmtVital('inp', p.inp_p75)}</span>
                    <span style={{ color: `var(--${rating('cls', p.cls_p75) === 'none' ? 'text-3' : rating('cls', p.cls_p75)})` }}>{fmtVital('cls', p.cls_p75)}</span>
                  </div>
                  {openPage === pageKey && <PageDetail app={p.app} url={p.url} hours={hours} />}
                </React.Fragment>
                );
              })}
            </div>

            {users.length > 0 && (
              <>
                <div className="field-label" style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-2)', marginBottom: 8 }}>{t('rum.users')}</div>
                <div className="card" style={{ overflow: 'hidden', marginBottom: 24 }}>
                  <div className="row" style={{ gridTemplateColumns: '1fr 90px 110px', fontWeight: 600, color: 'var(--text-3)', fontSize: 11 }}>
                    <span>{t('rum.who')}</span><span>{t('rum.views')}</span><span>LCP p75</span>
                  </div>
                  {users.map((u, i) => (
                    <div className="row" key={i} style={{ gridTemplateColumns: '1fr 90px 110px' }}>
                      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={u.user_id}>{u.user_id}</span>
                      <span>{u.views}</span>
                      <span style={{ color: `var(--${rating('lcp', u.lcp_p75) === 'none' ? 'text-3' : rating('lcp', u.lcp_p75)})` }}>{fmtVital('lcp', u.lcp_p75)}</span>
                    </div>
                  ))}
                </div>
              </>
            )}

            {browsers.length > 0 && (
              <>
                <div className="field-label" style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-2)', marginBottom: 8 }}>{t('rum.browsers')}</div>
                <div className="card" style={{ overflow: 'hidden', marginBottom: 24 }}>
                  <div className="row" style={{ gridTemplateColumns: '1fr 90px 110px', fontWeight: 600, color: 'var(--text-3)', fontSize: 11 }}>
                    <span>{t('rum.browser')}</span><span>{t('rum.views')}</span><span>LCP p75</span>
                  </div>
                  {browsers.map((b, i) => (
                    <div className="row" key={i} style={{ gridTemplateColumns: '1fr 90px 110px' }}>
                      <span>{b.browser}</span>
                      <span>{b.views}</span>
                      <span style={{ color: `var(--${rating('lcp', b.lcp_p75) === 'none' ? 'text-3' : rating('lcp', b.lcp_p75)})` }}>{fmtVital('lcp', b.lcp_p75)}</span>
                    </div>
                  ))}
                </div>
              </>
            )}

            {traced.length > 0 && (
              <>
                <div className="field-label" style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-2)', marginBottom: 8 }}>{t('rum.traced')}</div>
                <div className="card" style={{ overflow: 'hidden', marginBottom: 24 }}>
                  <div className="row" style={{ gridTemplateColumns: '1fr 90px 170px', fontWeight: 600, color: 'var(--text-3)', fontSize: 11 }}>
                    <span>{t('rum.page')}</span><span>{t('rum.load')}</span><span>{t('rum.trace')}</span>
                  </div>
                  {traced.map((r, i) => (
                    <div className="row" key={i} style={{ gridTemplateColumns: '1fr 90px 170px' }}>
                      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={r.url}>{r.url}</span>
                      <span>{r.load_ms == null ? '—' : `${Math.round(r.load_ms)} ms`}</span>
                      <a href={`#/traces/${encodeURIComponent(r.trace_id)}`} style={{ color: 'var(--accent)', fontFamily: 'monospace', fontSize: 12 }}>
                        {r.trace_id.slice(0, 12)}… →
                      </a>
                    </div>
                  ))}
                </div>
              </>
            )}
          </>
        )}

        <div className="field-label" style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-2)', marginBottom: 8 }}>{t('rum.install')}</div>
        <div className="codeblock">
          <code style={{ wordBreak: 'break-all' }}>{snippet}</code>
          <button className="btn btn-ghost" style={{ color: '#e7e5e4', flexShrink: 0 }} onClick={copy}>{copied ? <Check size={14}/> : <Copy size={14}/>}</button>
        </div>
        <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 6 }}>{t('rum.install_hint')}</div>
      </div>
    </div>
  );
}
