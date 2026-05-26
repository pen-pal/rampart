import React, { useEffect, useState } from 'react';
import { Activity, LogIn, UserPlus, Loader2 } from 'lucide-react';
import { api, useApi } from '../lib/api.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');

  .rampart-auth {
    --bg:#fafaf9; --surface:#ffffff; --surface-2:#f5f5f4;
    --border:#e7e5e4; --border-2:#d6d3d1;
    --text:#1c1917; --text-2:#57534e; --text-3:#a8a29e;
    --accent:#14b8a6; --accent-2:#0d9488; --accent-soft:#ccfbf1;
    --down:#ef4444; --down-soft:#fee2e2;

    background: var(--bg); color: var(--text);
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
    min-height: 100vh;
    display: flex; align-items: center; justify-content: center;
  }
  .rampart-auth * { box-sizing: border-box; }

  .auth-card {
    width: 100%; max-width: 400px;
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 14px; padding: 36px 32px;
    box-shadow: 0 8px 32px rgba(0,0,0,.04);
  }

  .auth-brand {
    display: flex; align-items: center; gap: 10px;
    margin-bottom: 24px;
  }
  .auth-brand-mark {
    width: 32px; height: 32px; border-radius: 8px;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    display: flex; align-items: center; justify-content: center;
    color: white; box-shadow: 0 2px 8px rgba(20,184,166,.35);
  }

  .field { margin-bottom: 16px; }
  .field-label {
    font-size: 12px; font-weight: 500; color: var(--text-2);
    margin-bottom: 6px; display: block;
  }
  .input {
    width: 100%; padding: 10px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 14px; color: var(--text); outline: none;
    font-family: inherit;
    transition: border-color .12s, box-shadow .12s;
  }
  .input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }

  .btn-primary {
    width: 100%;
    padding: 10px 16px; border-radius: 8px;
    background: var(--accent); color: white; border: 1px solid var(--accent);
    font-size: 14px; font-weight: 500; cursor: pointer;
    font-family: inherit; line-height: 1;
    display: inline-flex; align-items: center; justify-content: center; gap: 8px;
    transition: background .12s;
  }
  .btn-primary:hover { background: var(--accent-2); }
  .btn-primary:disabled { opacity: .55; cursor: not-allowed; }

  .banner-err {
    padding: 10px 14px; margin-bottom: 14px;
    background: var(--down-soft); color: #b91c1c;
    border: 1px solid #fecaca; border-radius: 8px;
    font-size: 13px;
  }

  .hint {
    font-size: 12px; color: var(--text-3); margin-top: 12px; line-height: 1.5;
  }
`;

export default function Login() {
  const meState = useApi(() => api.auth.me(), []);
  const [email,    setEmail]    = useState('');
  const [name,     setName]     = useState('');
  const [password, setPassword] = useState('');
  const [busy,     setBusy]     = useState(false);
  const [err,      setErr]      = useState(null);

  // Determine mode from /v1/auth/me. While loading, show a tiny spinner.
  const needsSetup = meState.data?.needs_setup === true;
  const alreadyLoggedIn = meState.data?.user;

  // If we land here already authenticated, bounce home.
  useEffect(() => {
    if (alreadyLoggedIn && window.location.hash.startsWith('#/login')) {
      window.location.hash = '#/';
    }
  }, [alreadyLoggedIn]);

  const submit = async (e) => {
    e?.preventDefault?.();
    setErr(null);
    if (!email.trim() || !password) {
      setErr('Email and password are required.');
      return;
    }
    setBusy(true);
    try {
      if (needsSetup) {
        if (password.length < 10) {
          setErr('Pick a password at least 10 characters long.');
          setBusy(false);
          return;
        }
        await api.auth.register(email.trim(), name.trim() || null, password);
      } else {
        await api.auth.login(email.trim(), password);
      }
      window.location.hash = '#/';
    } catch (e2) {
      setErr(
        e2.status === 401 ? 'Wrong email or password.'
        : e2.status === 409 ? 'Registration is closed — a user already exists. Try logging in.'
        : (e2.message || 'Something went wrong.')
      );
      setBusy(false);
    }
  };

  return (
    <div className="rampart-auth">
      <style>{css}</style>
      <div className="auth-card">
        <div className="auth-brand">
          <div className="auth-brand-mark">
            <Activity size={17} strokeWidth={2.4}/>
          </div>
          <div>
            <div style={{ fontSize: 17, fontWeight: 600, letterSpacing: '-.01em' }}>Rampart</div>
            <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 2 }}>
              {meState.loading ? 'Loading…' : needsSetup ? 'Create your admin account' : 'Sign in'}
            </div>
          </div>
        </div>

        {err && <div className="banner-err">{err}</div>}

        <form onSubmit={submit}>
          <div className="field">
            <label className="field-label">Email</label>
            <input className="input" type="email" autoComplete="email"
              value={email} onChange={e => setEmail(e.target.value)}
              placeholder="you@example.com" autoFocus/>
          </div>

          {needsSetup && (
            <div className="field">
              <label className="field-label">Name <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span></label>
              <input className="input" type="text" autoComplete="name"
                value={name} onChange={e => setName(e.target.value)}
                placeholder="Your name"/>
            </div>
          )}

          <div className="field">
            <label className="field-label">Password</label>
            <input className="input" type="password"
              autoComplete={needsSetup ? 'new-password' : 'current-password'}
              value={password} onChange={e => setPassword(e.target.value)}
              placeholder={needsSetup ? 'At least 10 characters' : ''}/>
          </div>

          <button className="btn-primary" type="submit" disabled={busy || meState.loading}>
            {busy ? <><Loader2 size={14} className="spin"/> Working…</>
              : needsSetup ? <><UserPlus size={14}/> Create admin account</>
              : <><LogIn size={14}/> Sign in</>}
          </button>
        </form>

        <div className="hint">
          {needsSetup
            ? 'The first user becomes the admin. Subsequent users can be added later — registration locks itself after this signup.'
            : 'Sessions last 14 days. Use any modern browser; cookies are required.'}
        </div>
      </div>

      <style>{`@keyframes spin { to { transform: rotate(360deg); } } .spin { animation: spin 1s linear infinite; }`}</style>
    </div>
  );
}
