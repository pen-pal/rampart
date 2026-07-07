import React, { useState, useMemo } from 'react';
import {
  Upload, FileText, Loader2, AlertCircle, CheckCircle2,
} from 'lucide-react';
import { api } from '../lib/api.js';
import { canWrite } from '../lib/roles.js';
import { t } from '../lib/i18n.js';
import SubViewHeader from '../components/SubViewHeader.jsx';

// Self-contained styling, mirroring StatusPageBuilder so this lightweight
// view doesn't depend on the dashboard chrome being mounted.
const css = `
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --up:#10b981; --up-soft:#d1fae5;
    --down:#ef4444; --down-soft:#fee2e2;
    --warn:#b45309; --warn-soft:#fef3c7;
    /* Foreground text for the soft status backgrounds (badges + error
       banner). Tuned for the LIGHT soft fills; flipped in the dark block
       below so they don't go dark-on-dark once --up-soft / --down-soft /
       --warn-soft flip. */
    --up-text:#047857; --down-text:#b91c1c; --warn-text:#b45309;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }

  /* Dark-mode overrides. theme.js flips --up-soft / --down-soft but not
     --warn / --warn-soft, so we set those here alongside the dark-tuned
     -text tints. Higher specificity than the base .rampart block. */
  [data-theme="dark"] .rampart {
    --warn-soft:#78350f;
    --up-text:#6ee7b7; --down-text:#fca5a5; --warn-text:#fde68a;
  }
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
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-ghost { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); margin-bottom: 6px; display: block; }
  .textarea {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 12px; color: var(--text); outline: none;
    font-family: 'JetBrains Mono', monospace;
  }
  .textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .banner-err { background: var(--down-soft); color: var(--down-text); border: 1px solid var(--down); padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .badge { display: inline-flex; align-items: center; padding: 2px 8px; border-radius: 999px; font-size: 11px; font-weight: 600; white-space: nowrap; }
  .badge-create { background: var(--up-soft); color: var(--up-text); }
  .badge-skip { background: var(--warn-soft); color: var(--warn-text); }
`;

// ── client-side CSV validation ──────────────────────────────────────
// A best-effort mirror of the server's `parse_csv_and_map` rules
// (rampart-api/src/importers/csv_import.rs). The server stays
// authoritative; this just lets the operator see what will happen.

// Canonical MonitorKind serde strings (snake_case), from
// rampart-core/src/monitor.rs. Anything not in this set → unknown → skip.
const KNOWN_KINDS = new Set([
  'http', 'keyword', 'json_query',
  'tcp', 'ping', 'dns', 'push', 'grpc', 'tls',
  'docker', 'steam', 'mqtt', 'radius', 'kafka',
  'postgres', 'mysql', 'mssql', 'redis', 'mongodb', 'memcached', 'ntp',
  'websocket', 'nats', 'ldap', 'amqp', 'doh', 'rdap', 'snmp',
  'cassandra', 'mdns', 'ssdp', 'domain', 'browser',
  'ssh', 'smtp', 'imap', 'ftp', 'pop3',
]);

// Kinds that probe a full URL (mirror of `Required::Url` arm).
const URL_KINDS = new Set([
  'http', 'keyword', 'json_query', 'tls', 'grpc',
  'websocket', 'doh', 'rdap', 'domain', 'browser',
]);

// Parse one CSV line into fields, honouring simple double-quoted fields
// with embedded commas and "" escapes. Hand-rolled to match the small set
// the server's csv crate accepts for this flat format.
function parseLine(line) {
  const out = [];
  let field = '';
  let inQuotes = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (inQuotes) {
      if (c === '"') {
        if (line[i + 1] === '"') { field += '"'; i++; }
        else { inQuotes = false; }
      } else {
        field += c;
      }
    } else if (c === '"') {
      inQuotes = true;
    } else if (c === ',') {
      out.push(field); field = '';
    } else {
      field += c;
    }
  }
  out.push(field);
  return out.map(f => f.trim());
}

// Parse + validate the whole CSV. Returns { header, rows, error } where each
// row is { line, cells:{...}, status:'create'|'skip', reason }.
function analyze(text) {
  const trimmed = (text || '').trim();
  if (!trimmed) return { rows: [], header: null, error: null };

  const lines = trimmed.split(/\r?\n/).filter(l => l.trim() !== '');
  if (lines.length === 0) return { rows: [], header: null, error: null };

  const header = parseLine(lines[0]).map(h => h.toLowerCase());
  const idx = (name) => header.indexOf(name);
  const ni = idx('name'), ki = idx('kind'), ui = idx('url'),
    hi = idx('hostname'), pi = idx('port');

  if (ni === -1 || ki === -1) {
    return { rows: [], header, error: t('import.err_header') };
  }

  const cell = (cells, i) => (i >= 0 && i < cells.length ? cells[i].trim() : '');

  const rows = lines.slice(1).map((line, di) => {
    const cells = parseLine(line);
    const name = cell(cells, ni);
    const kind = cell(cells, ki);
    const url = cell(cells, ui);
    const hostname = cell(cells, hi);
    const port = cell(cells, pi);

    let status = 'create';
    let reason = '';

    if (!name) {
      status = 'skip'; reason = t('import.skip_missing_name');
    } else if (!kind) {
      status = 'skip'; reason = t('import.skip_unknown_kind', { kind: '' });
    } else if (!KNOWN_KINDS.has(kind.toLowerCase())) {
      status = 'skip'; reason = t('import.skip_unknown_kind', { kind });
    } else {
      const k = kind.toLowerCase();
      if (URL_KINDS.has(k)) {
        if (!url) { status = 'skip'; reason = t('import.skip_missing_field', { field: 'url' }); }
      } else if (k === 'tcp') {
        if (!hostname) { status = 'skip'; reason = t('import.skip_missing_field', { field: 'hostname' }); }
        else if (!port) { status = 'skip'; reason = t('import.skip_missing_field', { field: 'port' }); }
      } else {
        if (!hostname) { status = 'skip'; reason = t('import.skip_missing_field', { field: 'hostname' }); }
      }
    }

    return { line: di + 2, name, kind, url, hostname, port, status, reason };
  });

  return { rows, header, error: null };
}

export default function ImportMonitors({ user } = {}) {
  const writable = canWrite(user);
  const [csv,     setCsv]     = useState('');
  const [busy,    setBusy]    = useState(false);
  const [err,     setErr]     = useState(null);
  const [result,  setResult]  = useState(null);

  const preview = useMemo(() => analyze(csv), [csv]);
  const createCount = preview.rows.filter(r => r.status === 'create').length;
  const skipCount   = preview.rows.filter(r => r.status === 'skip').length;

  const onFilePick = (e) => {
    const file = e.target.files && e.target.files[0];
    e.target.value = ''; // allow re-picking the same file
    if (!file) return;
    const reader = new FileReader();
    reader.onload  = () => { setCsv(String(reader.result || '')); setErr(null); };
    reader.onerror = () => setErr(t('import.err_file_read'));
    reader.readAsText(file);
  };

  const doImport = async () => {
    const text = csv.trim();
    if (!text) { setErr(t('import.err_empty')); return; }
    setBusy(true); setErr(null); setResult(null);
    try {
      const res = await api.monitors.importCsv(text);
      setResult(res);
    } catch (e) {
      setErr(e.message || t('import.err_failed'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 720, margin: '0 auto', padding: '32px 32px 64px' }}>
        <SubViewHeader title={t('import.title')} icon={<Upload/>} />

        <h1 style={{ fontSize: 24, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>
          {t('import.title')}
        </h1>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 22px' }}>
          {t('import.subtitle')}
        </p>

        {!writable ? (
          <div className="banner-err">
            <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>
            {t('import.err_failed')}
          </div>
        ) : (
          <>
            {err && (
              <div className="banner-err">
                <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}
              </div>
            )}

            <div className="card" style={{ padding: 22, marginBottom: 18 }}>
              <div style={{ marginBottom: 16 }}>
                <div className="field-label" style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <FileText size={13}/> {t('import.columns_title')}
                </div>
                <div style={{ fontSize: 12, color: 'var(--text-3)', lineHeight: 1.5 }}>
                  {t('import.columns_hint')}
                </div>
              </div>

              <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 16 }}>
                <label className="btn" style={{ cursor: 'pointer' }}>
                  <Upload size={13}/> {t('import.file_label')}
                  <input type="file" accept=".csv,text/csv,text/plain" onChange={onFilePick} style={{ display: 'none' }}/>
                </label>
                <span style={{ fontSize: 12, color: 'var(--text-3)' }}>{t('import.or')}</span>
              </div>

              <label className="field-label">{t('import.paste_label')}</label>
              <textarea
                className="textarea"
                rows={8}
                value={csv}
                onChange={e => setCsv(e.target.value)}
                placeholder={t('import.paste_placeholder')}
              />

              <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 16 }}>
                <button className="btn btn-accent" onClick={doImport} disabled={busy || createCount === 0}>
                  {busy
                    ? <><Loader2 size={13}/> {t('import.importing')}</>
                    : <><Upload size={13}/> {t('import.import')}</>}
                </button>
              </div>
            </div>

            {preview.error && (
              <div className="banner-err">
                <AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{preview.error}
              </div>
            )}

            {!preview.error && preview.rows.length > 0 && (
              <div className="card" style={{ padding: 22, marginBottom: 18 }}>
                <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', marginBottom: 4, gap: 12, flexWrap: 'wrap' }}>
                  <div style={{ fontSize: 14, fontWeight: 600 }}>{t('import.preview_title')}</div>
                  <div style={{ fontSize: 12.5, color: 'var(--text-2)' }}>
                    {t('import.preview_summary', { create: createCount, skip: skipCount })}
                  </div>
                </div>
                <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginBottom: 12 }}>
                  {t('import.preview_note')}
                </div>

                <div style={{ border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden' }}>
                  <div style={{
                    display: 'grid', gridTemplateColumns: '44px 1.4fr 0.9fr 110px 1.6fr',
                    gap: 12, padding: '8px 12px', fontSize: 11, fontWeight: 600,
                    color: 'var(--text-3)', background: 'var(--surface-2)',
                    textTransform: 'uppercase', letterSpacing: '.03em',
                  }}>
                    <span>{t('import.col_row')}</span>
                    <span>{t('import.col_name')}</span>
                    <span>{t('import.col_kind')}</span>
                    <span>{t('import.col_status')}</span>
                    <span>{t('import.col_detail')}</span>
                  </div>
                  {preview.rows.map((r, i) => (
                    <div key={i} style={{
                      display: 'grid', gridTemplateColumns: '44px 1.4fr 0.9fr 110px 1.6fr',
                      gap: 12, padding: '8px 12px', fontSize: 12, alignItems: 'center',
                      borderTop: '1px solid var(--border)',
                    }}>
                      <span className="mono" style={{ color: 'var(--text-3)' }}>{r.line}</span>
                      <span style={{ color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {r.name || <span style={{ color: 'var(--text-3)' }}>—</span>}
                      </span>
                      <span className="mono" style={{ color: 'var(--text-2)' }}>
                        {r.kind || <span style={{ color: 'var(--text-3)' }}>—</span>}
                      </span>
                      <span>
                        <span className={`badge ${r.status === 'create' ? 'badge-create' : 'badge-skip'}`}>
                          {r.status === 'create' ? t('import.badge_create') : t('import.badge_skip')}
                        </span>
                      </span>
                      <span style={{ color: r.status === 'skip' ? 'var(--warn)' : 'var(--text-3)' }}>
                        {r.reason || ''}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {result && (
              <div className="card" style={{ padding: 22 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 14, fontWeight: 600, marginBottom: 12 }}>
                  <CheckCircle2 size={16} color="var(--up)"/>
                  {t('import.result_created', { n: result.created })}
                </div>
                {(result.skipped && result.skipped.length > 0) ? (
                  <>
                    <div style={{ fontSize: 12.5, color: 'var(--text-2)', marginBottom: 8 }}>
                      {t('import.result_skipped', { n: result.skipped.length })}
                    </div>
                    <div style={{ border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden' }}>
                      {result.skipped.map((s, i) => (
                        <div key={i} style={{
                          display: 'grid', gridTemplateColumns: '1fr 2fr', gap: 12,
                          padding: '8px 12px', fontSize: 12,
                          borderTop: i === 0 ? 'none' : '1px solid var(--border)',
                        }}>
                          <span className="mono" style={{ color: 'var(--text)' }}>{s.row}</span>
                          <span style={{ color: 'var(--down)' }}>{s.reason}</span>
                        </div>
                      ))}
                    </div>
                  </>
                ) : (
                  <div style={{ fontSize: 12.5, color: 'var(--text-3)' }}>{t('import.result_none')}</div>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
