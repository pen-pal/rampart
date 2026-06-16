import React, { useState } from 'react';
import {
  ChevronLeft, Plus, Trash2, Server, AlertCircle, Loader2, X, Pause, Play,
} from 'lucide-react';
import { api, useApi, formatRelative, offsetDateTimeArrayToDate } from '../lib/api.js';
import { t } from '../lib/i18n.js';
import { confirmDialog } from '../lib/notify.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
  .rampart {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
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
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-danger { color: var(--down); }
  .btn-danger:hover { background: var(--down-soft); border-color: var(--down); }
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .input, .select {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input:focus, .select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .row {
    display: grid; grid-template-columns: 1fr auto auto auto;
    align-items: center; gap: 14px;
    padding: 14px 18px; border-top: 1px solid var(--border);
  }
  .row:first-child { border-top: none; }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .pill {
    display: inline-flex; align-items: center;
    font-size: 10.5px; padding: 2px 8px; border-radius: 999px; font-weight: 500;
  }
  .pill-active   { background: var(--accent-soft); color: var(--accent-2); }
  .pill-paused   { background: var(--surface-2);   color: var(--text-2); }
`;

const tsToDate = (ts) => (Array.isArray(ts) ? offsetDateTimeArrayToDate(ts) : new Date(ts));

export default function Proxies() {
  const proxiesState = useApi(() => api.proxies.list(), [], { pollMs: 30_000 });
  const [creating, setCreating] = useState(false);
  const [busy,     setBusy]     = useState(null);
  const [err,      setErr]      = useState(null);

  const reload = () => window.location.reload();

  const remove = async (id) => {
    if (!(await confirmDialog({ message: t('proxies.delete_confirm') }))) return;
    setBusy(id); setErr(null);
    try { await api.proxies.remove(id); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const togglePause = async (p) => {
    setBusy(p.id); setErr(null);
    try { await api.proxies.setActive(p.id, !p.active); reload(); }
    catch (e) { setErr(e.message); }
    finally { setBusy(null); }
  };

  const proxies = proxiesState.data || [];

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 920, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> {t('common.dashboard')}
        </a>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 22 }}>
          <div>
            <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>{t('proxies.title')}</h1>
            <p style={{ fontSize: 13, color: 'var(--text-2)', margin: 0 }}>
              {t('proxies.subtitle')}
            </p>
          </div>
          <button className="btn btn-accent" onClick={() => setCreating(true)}>
            <Plus size={14}/> {t('proxies.new')}
          </button>
        </div>

        {err && <div className="banner-err"><AlertCircle size={14} style={{ verticalAlign: '-2px', marginRight: 6 }}/>{err}</div>}

        {creating && (
          <CreateForm onCancel={() => setCreating(false)} onCreated={reload}/>
        )}

        <div className="card" style={{ overflow: 'hidden' }}>
          {proxiesState.loading ? (
            <div style={{ padding: 32, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
          ) : proxies.length === 0 ? (
            <div style={{ padding: 48, textAlign: 'center', color: 'var(--text-3)' }}>
              <Server size={28} style={{ marginBottom: 10, opacity: .5 }}/>
              <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-2)', marginBottom: 4 }}>{t('proxies.empty.title')}</div>
              <div style={{ fontSize: 12.5 }}>{t('proxies.empty.cta')}</div>
            </div>
          ) : proxies.map(p => (
            <div className="row" key={p.id}>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 3 }}>
                  <span className="mono" style={{ fontSize: 13, fontWeight: 600 }}>
                    {p.protocol}://{p.host}:{p.port}
                  </span>
                  <span className={`pill pill-${p.active ? 'active' : 'paused'}`}>{p.active ? t('proxies.state.active') : t('proxies.state.paused')}</span>
                </div>
                <div style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
                  {p.auth ? t('proxies.auth_as', { user: p.username || t('proxies.no_username') }) : t('proxies.no_auth')}
                  {' · '}{t('proxies.created', { when: formatRelative(tsToDate(p.created_at)) })}
                </div>
              </div>
              <button className="btn btn-ghost" onClick={() => togglePause(p)} disabled={busy === p.id}>
                {p.active ? <><Pause size={13}/> {t('common.pause')}</> : <><Play size={13}/> {t('common.resume')}</>}
              </button>
              <button className="btn btn-ghost btn-danger" onClick={() => remove(p.id)} disabled={busy === p.id}>
                <Trash2 size={13}/>
              </button>
              <span/>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function CreateForm({ onCancel, onCreated }) {
  const [protocol, setProtocol] = useState('http');
  const [host,     setHost]     = useState('');
  const [port,     setPort]     = useState('8080');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [busy,     setBusy]     = useState(false);
  const [err,      setErr]      = useState(null);

  const submit = async () => {
    setErr(null);
    if (!host.trim()) { setErr(t('proxies.err_host')); return; }
    const portNum = parseInt(port, 10);
    if (!Number.isFinite(portNum) || portNum < 1 || portNum > 65535) {
      setErr(t('proxies.err_port'));
      return;
    }
    setBusy(true);
    try {
      await api.proxies.create({
        protocol,
        host: host.trim(),
        port: portNum,
        username: username.trim() || null,
        password: password || null,
        active: true,
      });
      onCreated();
    } catch (e) { setErr(e.message || t('proxies.err_create')); setBusy(false); }
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
        <h3 style={{ fontSize: 15, fontWeight: 600, margin: 0 }}>{t('proxies.new')}</h3>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy} aria-label={t('common.cancel')}><X size={14}/></button>
      </div>
      {err && <div className="banner-err">{err}</div>}

      <div style={{ display: 'grid', gridTemplateColumns: '120px 1fr 100px', gap: 12, marginBottom: 12 }}>
        <div>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('proxies.protocol')}</label>
          <select className="select" value={protocol} onChange={e => setProtocol(e.target.value)}>
            <option value="http">http</option>
            <option value="https">https</option>
            <option value="socks5">socks5</option>
            <option value="socks4">socks4</option>
            <option value="socks">socks</option>
          </select>
        </div>
        <div>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('proxies.host')}</label>
          <input className="input mono" value={host} onChange={e => setHost(e.target.value)} placeholder="proxy.internal"/>
        </div>
        <div>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('proxies.port')}</label>
          <input className="input mono" value={port} onChange={e => setPort(e.target.value)}/>
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12, marginBottom: 16 }}>
        <div>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('proxies.username_optional')}</label>
          <input className="input" value={username} onChange={e => setUsername(e.target.value)}/>
        </div>
        <div>
          <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-2)', display: 'block', marginBottom: 6 }}>{t('proxies.password_optional')}</label>
          <input className="input" type="password" value={password} onChange={e => setPassword(e.target.value)}/>
        </div>
      </div>

      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>{t('common.cancel')}</button>
        <button className="btn btn-accent" onClick={submit} disabled={busy}>
          {busy ? <><Loader2 size={13}/> {t('proxies.creating')}</> : <><Plus size={13}/> {t('proxies.create')}</>}
        </button>
      </div>
    </div>
  );
}
