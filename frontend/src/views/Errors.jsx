import React, { useState, useEffect } from 'react';
import {
  ChevronLeft, Plus, Trash2, AlertCircle, Loader2, X, Check, Bug, Copy,
} from 'lucide-react';
import { api, useApi, formatRelative } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { canWrite } from '../lib/roles.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --down:#ef4444; --down-soft:#fee2e2;
    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif; min-height: 100vh;
  }
  .rampart * { box-sizing: border-box; }
  .mono { font-family: 'JetBrains Mono', monospace; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
  .btn {
    display: inline-flex; align-items: center; gap: 6px; padding: 7px 12px;
    border-radius: 8px; cursor: pointer; font-size: 13px; font-weight: 500; line-height: 1;
    background: var(--surface); border: 1px solid var(--border); color: var(--text-2); font-family: inherit;
  }
  .btn:hover { background: var(--surface-2); color: var(--text); border-color: var(--border-2); }
  .btn:disabled { opacity: .55; cursor: not-allowed; }
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-danger { color: var(--down); }
  .btn-danger:hover { background: var(--down-soft); border-color: var(--down); }
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .input, .select {
    width: 100%; padding: 9px 12px; border-radius: 8px; background: var(--surface);
    border: 1px solid var(--border); font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input.mono { font-family: 'JetBrains Mono', monospace; font-size: 12.5px; }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .field-label { font-size: 12px; font-weight: 500; color: var(--text-2); display: block; margin-bottom: 6px; }
  .field-hint { font-size: 11.5px; color: var(--text-3); margin-top: 6px; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .row { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 14px; padding: 14px 18px; border-top: 1px solid var(--border); }
  .row:first-child { border-top: none; }
  .pill { display: inline-flex; align-items: center; font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500; background: var(--accent-soft); color: var(--accent-2); }
  .pill-down { background: var(--down-soft); color: #b91c1c; }
  .pill-muted { background: var(--surface-2); color: var(--text-3); }
  .codeblock { background: #1c1917; color: #e7e5e4; border-radius: 8px; padding: 12px 14px; font-family: 'JetBrains Mono', monospace; font-size: 12px; overflow-x: auto; white-space: pre; }
`;

const STATUSES = ['unresolved', 'resolved', 'ignored'];

function dsnFor(project) {
  const loc = window.location;
  return `${loc.protocol}//${project.public_key}@${loc.host}/${project.id}`;
}

export default function Errors({ user, openIssueId }) {
  const [project, setProject] = useState(null); // selected project object
  // `openIssueId` (from a #/errors/<id> deep link, e.g. trace → errors) opens
  // the issue directly; IssueDetail loads it by id without needing the project.
  const [issueId, setIssueId] = useState(openIssueId || null); // selected issue id

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 940, margin: '0 auto', padding: '32px 32px 64px' }}>
        {issueId ? (
          <IssueDetail issueId={issueId} user={user} onBack={() => setIssueId(null)} />
        ) : project ? (
          <ProjectIssues project={project} onBack={() => setProject(null)} onOpenIssue={setIssueId} />
        ) : (
          <ProjectsList user={user} onOpen={setProject} />
        )}
      </div>
    </div>
  );
}

function ProjectsList({ user, onOpen }) {
  const writable = canWrite(user);
  const [reloadKey, setReloadKey] = useState(0);
  const projectsState = useApi(() => api.errorProjects.list(), [reloadKey]);
  const channelsState = useApi(() => api.notifications.list(), []);
  const [creating, setCreating] = useState(false);
  const [err, setErr] = useState(null);
  const projects = projectsState.data || [];
  const channels = channelsState.data || [];

  const reload = () => { setCreating(false); setReloadKey(k => k + 1); };
  const remove = async (id) => {
    if (!confirm(t('errors.delete_confirm'))) return;
    setErr(null);
    try { await api.errorProjects.remove(id); reload(); }
    catch (e) { setErr(e.message); }
  };

  return (
    <>
      <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}><ChevronLeft size={14}/> {t('errors.back')}</a>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
        <div>
          <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('errors.title')}</h1>
          <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>{t('errors.subtitle')}</p>
        </div>
        {writable && (
          <button className="btn btn-accent" onClick={() => setCreating(true)}><Plus size={14}/> {t('errors.new_project')}</button>
        )}
      </div>

      {(err || projectsState.error) && (
        <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err || t('errors.load_error')}</div>
      )}

      {creating && <ProjectForm channels={channels} onCancel={() => setCreating(false)} onSaved={reload}/>}

      <div className="card" style={{ overflow: 'hidden' }}>
        {projectsState.loading ? (
          <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('errors.loading')}</div>
        ) : projects.length === 0 ? (
          <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
            <Bug size={28} style={{ marginBottom: 10, opacity: .5 }}/>
            <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('errors.empty.title')}</div>
            <div style={{ fontSize: 12.5 }}>{t('errors.empty.cta')}</div>
          </div>
        ) : projects.map(p => (
          <div className="row" key={p.id}>
            <div style={{ minWidth: 0, cursor: 'pointer' }} onClick={() => onOpen(p)}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
                <span style={{ fontSize: 14, fontWeight: 600 }}>{p.name}</span>
                {p.platform && <span className="pill pill-muted">{p.platform}</span>}
              </div>
              <div className="mono" style={{ fontSize: 11, color: 'var(--text-3)' }}>{p.slug}</div>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <button className="btn btn-ghost" onClick={() => onOpen(p)}>{t('errors.open')}</button>
              {writable && (
                <button className="btn btn-ghost btn-danger" onClick={() => remove(p.id)}><Trash2 size={13}/></button>
              )}
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

function ProjectForm({ channels, onCancel, onSaved }) {
  const [name, setName] = useState('');
  const [platform, setPlatform] = useState('');
  const [channelIds, setChannelIds] = useState([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(null);

  const toggle = (id) => setChannelIds(ids => ids.includes(id) ? ids.filter(x => x !== id) : [...ids, id]);

  const submit = async () => {
    setErr(null);
    if (!name.trim()) { setErr(t('errors.err_name')); return; }
    setBusy(true);
    try {
      await api.errorProjects.create({ name: name.trim(), platform: platform.trim() || undefined, alert_channel_ids: channelIds });
      onSaved();
    } catch (e) { setErr(e.message || t('errors.err_save')); setBusy(false); }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>{t('errors.new_project')}</h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}><X size={14}/></button>
      </div>
      {err && <div className="banner-err">{err}</div>}
      <div style={{ display: 'flex', gap: 12, marginBottom: 14, flexWrap: 'wrap' }}>
        <div style={{ flex: '1 1 220px' }}>
          <label className="field-label">{t('errors.field.name')}</label>
          <input className="input" value={name} onChange={e => setName(e.target.value)} placeholder={t('errors.field.name_ph')}/>
        </div>
        <div style={{ flex: '1 1 160px' }}>
          <label className="field-label">{t('errors.field.platform')}</label>
          <input className="input" value={platform} onChange={e => setPlatform(e.target.value)} placeholder="python, javascript…"/>
        </div>
      </div>
      <label className="field-label">{t('errors.field.alert_channels')}</label>
      {channels.length === 0 ? (
        <div className="field-hint" style={{ marginTop: 0 }}>{t('errors.no_channels')}</div>
      ) : (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px 16px' }}>
          {channels.map(c => (
            <label key={c.id} style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 13, cursor: 'pointer' }}>
              <input type="checkbox" checked={channelIds.includes(c.id)} onChange={() => toggle(c.id)}/> {c.name}
            </label>
          ))}
        </div>
      )}
      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 16 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>
          {busy ? <><Loader2 size={13}/> {t('common.saving')}</> : <><Check size={13}/> {t('errors.create_project')}</>}
        </button>
      </div>
    </div>
  );
}

function ProjectIssues({ project, onBack, onOpenIssue }) {
  const [status, setStatus] = useState('unresolved');
  const issuesState = useApi(() => api.errorProjects.issues(project.id, status || undefined), [status]);
  const histState = useApi(() => api.errorProjects.histogram(project.id, 168), []);
  const [copied, setCopied] = useState(false);
  const issues = issuesState.data || [];
  const hist = histState.data || [];
  const dsn = dsnFor(project);

  const copyDsn = async () => {
    try { await navigator.clipboard.writeText(dsn); setCopied(true); setTimeout(() => setCopied(false), 1500); }
    catch { /* clipboard blocked — the DSN is visible to copy manually */ }
  };

  return (
    <>
      <button className="btn btn-ghost" style={{ marginBottom: 18 }} onClick={onBack}><ChevronLeft size={14}/> {t('errors.all_projects')}</button>
      <h1 style={{ fontSize: 24, fontWeight: 600, margin: '0 0 4px' }}>{project.name}</h1>
      <p style={{ fontSize: 12.5, color: 'var(--text-2)', margin: '0 0 16px' }}>{t('errors.dsn_hint')}</p>

      <div className="card" style={{ padding: '12px 14px', marginBottom: 18, display: 'flex', alignItems: 'center', gap: 10 }}>
        <span className="field-label" style={{ margin: 0 }}>DSN</span>
        <code className="mono" style={{ flex: 1, fontSize: 11.5, color: 'var(--text-2)', wordBreak: 'break-all' }}>{dsn}</code>
        <button className="btn btn-ghost" onClick={copyDsn}>{copied ? <Check size={13}/> : <Copy size={13}/>}</button>
      </div>

      <SourceMaps projectId={project.id} />

      {hist.length > 1 && (() => {
        const max = Math.max(1, ...hist.map(b => b.count));
        return (
          <div title={t('errors.volume_hint')} style={{ display: 'flex', alignItems: 'flex-end', gap: 1, height: 44, marginBottom: 14 }}>
            {hist.map((b, i) => (
              <div key={i} title={`${b.count}`} style={{ flex: 1, height: `${Math.max((b.count / max) * 100, b.count ? 4 : 0)}%`, minHeight: b.count ? 2 : 0, background: 'var(--down)', opacity: .7, borderRadius: 1 }}/>
            ))}
          </div>
        );
      })()}

      <div style={{ display: 'flex', gap: 8, marginBottom: 14 }}>
        {STATUSES.map(s => (
          <button key={s} className={`btn ${status === s ? 'btn-accent' : ''}`} onClick={() => setStatus(s)}>{t(`errors.status.${s}`)}</button>
        ))}
        <button className={`btn ${status === '' ? 'btn-accent' : ''}`} onClick={() => setStatus('')}>{t('errors.status.all')}</button>
      </div>

      <div className="card" style={{ overflow: 'hidden' }}>
        {issuesState.loading ? (
          <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('errors.loading')}</div>
        ) : issues.length === 0 ? (
          <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-3)', fontSize: 13 }}>{t('errors.no_issues')}</div>
        ) : issues.map(i => (
          <div className="row" key={i.id} style={{ cursor: 'pointer' }} onClick={() => onOpenIssue(i.id)}>
            <div style={{ minWidth: 0 }}>
              <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 3, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{i.title}</div>
              <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                {i.culprit ? `${i.culprit} · ` : ''}{t('errors.last_seen', { when: formatRelative(i.last_seen) })}
              </div>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span className={`pill ${i.status === 'unresolved' ? 'pill-down' : 'pill-muted'}`}>{t(`errors.status.${i.status}`)}</span>
              <span className="pill pill-muted">{t('errors.times_seen', { n: i.times_seen })}</span>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

// Source-map management for a project: list uploaded maps + upload (paste JSON)
// + delete. Feeds the server-side symbolication of minified stack frames.
function SourceMaps({ projectId }) {
  const [maps, setMaps] = useState([]);
  const [open, setOpen] = useState(false);
  const [release, setRelease] = useState('');
  const [filename, setFilename] = useState('');
  const [mapText, setMapText] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(null);

  const load = async () => { try { setMaps(await api.sourcemaps.list(projectId)); } catch { /* none yet */ } };
  useEffect(() => { load(); /* eslint-disable-next-line */ }, [projectId]);

  const upload = async () => {
    setErr(null); setBusy(true);
    try {
      const map = JSON.parse(mapText);
      await api.sourcemaps.upload(projectId, { release: release.trim(), filename: filename.trim(), map });
      setRelease(''); setFilename(''); setMapText(''); setOpen(false); await load();
    } catch (e) { setErr(e.message || t('errors.sm_err')); }
    finally { setBusy(false); }
  };
  const remove = async (id) => { try { await api.sourcemaps.remove(projectId, id); await load(); } catch { /* ignore */ } };

  return (
    <div className="card" style={{ padding: '12px 14px', marginBottom: 18 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <span className="field-label" style={{ margin: 0 }}>{t('errors.sourcemaps')}</span>
        <button className="btn btn-ghost" onClick={() => setOpen(o => !o)}><Plus size={13}/> {t('errors.sm_upload')}</button>
      </div>
      {maps.length > 0 && (
        <div style={{ marginTop: 8 }}>
          {maps.map(m => (
            <div key={m.id} style={{ display: 'flex', gap: 10, fontSize: 12, padding: '3px 0', alignItems: 'center' }}>
              <span className="pill pill-muted">{m.release}</span>
              <span className="mono" style={{ color: 'var(--text-2)' }}>{m.filename}</span>
              <button className="btn btn-ghost" style={{ marginLeft: 'auto' }} onClick={() => remove(m.id)} title={t('common.delete')}><Trash2 size={12}/></button>
            </div>
          ))}
        </div>
      )}
      {open && (
        <div style={{ marginTop: 10, display: 'flex', flexDirection: 'column', gap: 6 }}>
          <input className="input" placeholder={t('errors.sm_release')} value={release} onChange={e => setRelease(e.target.value)}/>
          <input className="input" placeholder={t('errors.sm_filename')} value={filename} onChange={e => setFilename(e.target.value)}/>
          <textarea className="input" rows={4} placeholder={t('errors.sm_json')} value={mapText} onChange={e => setMapText(e.target.value)} style={{ fontFamily: 'monospace', fontSize: 11 }}/>
          {err && <span style={{ color: 'var(--down)', fontSize: 12 }}>{err}</span>}
          <button className="btn btn-accent" disabled={busy || !release.trim() || !filename.trim() || !mapText.trim()} onClick={upload} style={{ alignSelf: 'flex-start' }}>
            {busy ? <Loader2 size={13}/> : <Check size={13}/>} {t('errors.sm_save')}
          </button>
        </div>
      )}
    </div>
  );
}

function IssueDetail({ issueId, user, onBack }) {
  const writable = canWrite(user);
  const [reloadKey, setReloadKey] = useState(0);
  const issueState = useApi(() => api.errorIssues.get(issueId), [issueId, reloadKey]);
  const eventsState = useApi(() => api.errorIssues.events(issueId), [issueId]);
  const statsState = useApi(() => api.errorIssues.stats(issueId), [issueId]);
  const usersState = useApi(() => api.errorIssues.affectedUsers(issueId), [issueId]);
  const affectedUsers = usersState.data || [];
  const [busy, setBusy] = useState(false);
  const issue = issueState.data;
  const events = eventsState.data || [];
  const stats = statsState.data;

  const act = async (fn) => {
    setBusy(true);
    try { await fn(issueId); setReloadKey(k => k + 1); }
    catch { /* surfaced by the reloaded state */ }
    finally { setBusy(false); }
  };

  // Assignee picker — editor-visible directory (id/name/email only).
  const [assignable, setAssignable] = useState([]);
  useEffect(() => {
    if (!writable) return;
    api.errorIssues.assignableUsers()
      .then(u => setAssignable(Array.isArray(u) ? u : []))
      .catch(() => {});
  }, [writable]);
  const assign = async (uid) => {
    setBusy(true);
    try { await api.errorIssues.assign(issueId, uid || null); setReloadKey(k => k + 1); }
    catch { /* surfaced by reload */ }
    finally { setBusy(false); }
  };

  if (issueState.loading) {
    return <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/> {t('errors.loading')}</div>;
  }
  if (!issue) {
    return <><button className="btn btn-ghost" onClick={onBack}><ChevronLeft size={14}/> {t('errors.back_issues')}</button><div className="banner-err" style={{ marginTop: 16 }}>{t('errors.load_error')}</div></>;
  }

  const latest = events[0];
  const frames = latest && Array.isArray(latest.stacktrace) ? latest.stacktrace : [];
  // Cross-tier: Sentry events carry a trace context, stored verbatim in the
  // event's `context`. If present, link to the trace + its logs.
  const traceId = latest?.context?.contexts?.trace?.trace_id || null;
  // Breadcrumbs: the SDK trail leading up to the error, stored verbatim in the
  // event `context`. Sentry sends either `breadcrumbs.values[]` (current) or a
  // bare `breadcrumbs[]` (older SDKs) — accept both.
  const crumbsRaw = latest?.context?.breadcrumbs;
  const crumbs = Array.isArray(crumbsRaw)
    ? crumbsRaw
    : Array.isArray(crumbsRaw?.values) ? crumbsRaw.values : [];
  const crumbPill = (lvl) => {
    if (lvl === 'error' || lvl === 'fatal' || lvl === 'critical') return 'pill-fail';
    if (lvl === 'warning' || lvl === 'warn') return 'pill-warn';
    return 'pill-muted';
  };
  const crumbTime = (ts) => {
    if (ts == null) return '';
    const d = typeof ts === 'number' ? new Date(ts * 1000) : new Date(ts);
    return Number.isNaN(d.getTime()) ? '' : d.toLocaleTimeString();
  };

  return (
    <>
      <button className="btn btn-ghost" style={{ marginBottom: 18 }} onClick={onBack}><ChevronLeft size={14}/> {t('errors.back_issues')}</button>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 14, marginBottom: 16 }}>
        <div style={{ minWidth: 0 }}>
          <h1 style={{ fontSize: 20, fontWeight: 600, margin: '0 0 6px', wordBreak: 'break-word' }}>{issue.title}</h1>
          <div style={{ fontSize: 12, color: 'var(--text-3)' }}>
            {issue.culprit ? `${issue.culprit} · ` : ''}
            {t('errors.times_seen', { n: issue.times_seen })} · {t('errors.first_seen', { when: formatRelative(issue.first_seen) })}
          </div>
        </div>
        {writable && (
          <div style={{ display: 'flex', gap: 8, flexShrink: 0, alignItems: 'center' }}>
            <select className="select" style={{ width: 'auto' }} value={issue.assignee || ''} disabled={busy}
              onChange={e => assign(e.target.value)} title={t('errors.assignee')}>
              <option value="">{t('errors.unassigned')}</option>
              {assignable.map(u => <option key={u.id} value={u.id}>{u.name || u.email}</option>)}
            </select>
            {issue.status !== 'resolved' && <button className="btn" disabled={busy} onClick={() => act(api.errorIssues.resolve)}><Check size={13}/> {t('errors.resolve')}</button>}
            {issue.status !== 'ignored' && <button className="btn" disabled={busy} onClick={() => act(api.errorIssues.ignore)}>{t('errors.ignore')}</button>}
            {issue.status !== 'unresolved' && <button className="btn" disabled={busy} onClick={() => act(api.errorIssues.unresolve)}>{t('errors.unresolve')}</button>}
          </div>
        )}
      </div>

      {latest && (
        <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap', marginBottom: 16, fontSize: 12, color: 'var(--text-2)' }}>
          {latest.level && <span>{t('errors.level')}: <b>{latest.level}</b></span>}
          {latest.environment && <span>{t('errors.environment')}: <b>{latest.environment}</b></span>}
          {latest.release && <span>{t('errors.release')}: <b>{latest.release}</b></span>}
          {latest.server_name && <span>{t('errors.server')}: <b>{latest.server_name}</b></span>}
        </div>
      )}

      {stats && (
        <div className="card" style={{ padding: 14, marginBottom: 16, display: 'flex', gap: 24, flexWrap: 'wrap', alignItems: 'flex-start' }}>
          <div>
            <div style={{ fontSize: 22, fontWeight: 600 }}>{stats.users_affected}</div>
            <div style={{ fontSize: 11, color: 'var(--text-3)' }}>{t('errors.users_affected')}</div>
          </div>
          {stats.by_release && stats.by_release.length > 0 && (
            <div style={{ minWidth: 160 }}>
              <div style={{ fontSize: 11, color: 'var(--text-3)', marginBottom: 4 }}>{t('errors.by_release')}</div>
              {stats.by_release.slice(0, 5).map(([k, n]) => (
                <div key={k} style={{ display: 'flex', justifyContent: 'space-between', gap: 12, fontSize: 12 }}>
                  <span className="mono" style={{ color: 'var(--text-2)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{k}</span><span>{n}</span>
                </div>
              ))}
            </div>
          )}
          {stats.by_environment && stats.by_environment.length > 0 && (
            <div style={{ minWidth: 140 }}>
              <div style={{ fontSize: 11, color: 'var(--text-3)', marginBottom: 4 }}>{t('errors.by_environment')}</div>
              {stats.by_environment.slice(0, 5).map(([k, n]) => (
                <div key={k} style={{ display: 'flex', justifyContent: 'space-between', gap: 12, fontSize: 12 }}>
                  <span style={{ color: 'var(--text-2)' }}>{k}</span><span>{n}</span>
                </div>
              ))}
            </div>
          )}
          {affectedUsers.length > 0 && (
            <div style={{ minWidth: 200, flex: 1 }}>
              <div style={{ fontSize: 11, color: 'var(--text-3)', marginBottom: 4 }}>{t('errors.affected_users')}</div>
              {affectedUsers.slice(0, 6).map((u, i) => (
                <div key={i} style={{ display: 'flex', justifyContent: 'space-between', gap: 12, fontSize: 12 }}>
                  <span className="mono" style={{ color: 'var(--text-2)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={u.ident}>{u.ident}</span>
                  <span style={{ color: 'var(--text-3)' }}>×{u.events}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {traceId && (
        <div style={{ display: 'flex', gap: 12, alignItems: 'center', marginBottom: 16, padding: '8px 12px', background: 'var(--accent-soft)', borderRadius: 8, fontSize: 12.5 }}>
          <span style={{ color: 'var(--accent-2)' }}>{t('errors.trace_context')}</span>
          <a href={`#/traces/${traceId}`} style={{ color: 'var(--accent-2)', fontWeight: 500 }}>{t('errors.view_trace')}</a>
          <a href={`#/logs/trace/${traceId}`} style={{ color: 'var(--accent-2)', fontWeight: 500 }}>{t('errors.view_logs')}</a>
          <span className="mono" style={{ color: 'var(--text-3)', marginLeft: 'auto', overflow: 'hidden', textOverflow: 'ellipsis' }}>{traceId}</span>
        </div>
      )}

      {frames.length > 0 && (
        <>
          <div className="field-label">{t('errors.stacktrace')}</div>
          <div className="codeblock" style={{ marginBottom: 16 }}>
            {frames.slice().reverse().map(f => {
              const r = f.resolved;
              const fn = (r && r.function) || f.function || '?';
              const loc = r && r.filename
                ? `${r.filename}${r.lineno ? ':' + r.lineno : ''}`
                : `${f.module || f.filename || '?'}${f.lineno ? ':' + f.lineno : ''}`;
              return `  at ${fn} (${loc})${r ? '   ← ' + t('errors.symbolicated') : ''}`;
            }).join('\n')}
          </div>
        </>
      )}

      {crumbs.length > 0 && (
        <>
          <div className="field-label">{t('errors.breadcrumbs')}</div>
          <div className="card" style={{ overflow: 'hidden', marginBottom: 16 }}>
            {crumbs.slice(-25).map((c, i) => (
              <div className="row" key={i} style={{ display: 'flex', gap: 10, alignItems: 'baseline' }}>
                <span className={`pill ${crumbPill(c.level)}`} style={{ flexShrink: 0 }}>{c.category || c.type || c.level || 'info'}</span>
                <span style={{ minWidth: 0, fontSize: 12.5, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {c.message || (c.data && typeof c.data === 'object' ? JSON.stringify(c.data) : c.data) || ''}
                </span>
                <span className="mono" style={{ marginLeft: 'auto', color: 'var(--text-3)', fontSize: 11, flexShrink: 0 }}>{crumbTime(c.timestamp)}</span>
              </div>
            ))}
          </div>
        </>
      )}

      <div className="field-label">{t('errors.recent_events', { n: events.length })}</div>
      <div className="card" style={{ overflow: 'hidden' }}>
        {events.length === 0 ? (
          <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)', fontSize: 12.5 }}>{t('errors.no_events')}</div>
        ) : events.map(e => (
          <div className="row" key={e.id}>
            <div style={{ minWidth: 0, fontSize: 12.5 }}>
              <span className="mono" style={{ color: 'var(--text-3)' }}>{formatRelative(e.ts)}</span>
              {e.message ? ` — ${e.message}` : ''}
            </div>
            {e.environment && <span className="pill pill-muted">{e.environment}</span>}
          </div>
        ))}
      </div>
    </>
  );
}
