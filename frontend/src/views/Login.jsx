import React, { useEffect, useState } from 'react';
import { LogIn, UserPlus, Loader2 } from 'lucide-react';
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
  /* Brand mark — inline copy of docs/assets/logo.svg. Slate shield with
     an orange ECG-pulse cut across the centre. Matches the README hero +
     favicon + GitHub social-preview mark exactly so the auth screen sets
     the same brand frame the dashboard / marketing surfaces use. */
  .auth-brand-mark { width: 32px; height: 32px; }

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
  // Two-step login: when the backend answers POST /login with
  // { totp_required, challenge_token }, we swap the form for a code prompt.
  const [challengeToken, setChallengeToken] = useState(null);
  const [totpCode,       setTotpCode]       = useState('');

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
        window.location.hash = '#/';
      } else {
        const r = await api.auth.login(email.trim(), password);
        if (r && r.totp_required) {
          // Stop here, prompt for code. Password stays in state so the
          // form can reset on Back without re-entry.
          setChallengeToken(r.challenge_token);
          setBusy(false);
          return;
        }
        window.location.hash = '#/';
      }
    } catch (e2) {
      setErr(
        e2.status === 401 ? 'Wrong email or password.'
        : e2.status === 409 ? 'Registration is closed — a user already exists. Try logging in.'
        : (e2.message || 'Something went wrong.')
      );
      setBusy(false);
    }
  };

  const submitTotp = async (e) => {
    e?.preventDefault?.();
    setErr(null);
    if (!totpCode.trim()) { setErr('Enter the 6-digit code or a recovery code.'); return; }
    setBusy(true);
    try {
      await api.auth.totpVerify(challengeToken, totpCode.trim());
      window.location.hash = '#/';
    } catch (e2) {
      // 401 on bad code: parse the new challenge token if the server
      // returned one so the user can retry without re-entering password.
      if (e2.status === 401 && e2.message) {
        try {
          const parsed = JSON.parse(e2.message);
          if (parsed?.challenge_token) setChallengeToken(parsed.challenge_token);
        } catch { /* fall through */ }
        setErr('Invalid code. Try again or use a recovery code.');
      } else {
        setErr(e2.message || 'Something went wrong.');
      }
      setBusy(false);
    }
  };

  const cancelTotp = () => {
    setChallengeToken(null);
    setTotpCode('');
    setErr(null);
  };

  return (
    <div className="rampart-auth">
      <style>{css}</style>
      <div className="auth-card">
        <div className="auth-brand">
          <svg className="auth-brand-mark" viewBox="0 0 24 24" role="img" aria-label="Rampart">
            <path fill="#3b414c" d="M12 2 L20 4 V12 C20 17 17 21 12 22 C7 21 4 17 4 12 V4 Z"/>
            <path fill="none" stroke="#d27a3c" strokeWidth="1.6"
                  strokeLinecap="round" strokeLinejoin="round"
                  d="M6 13 H9 L11 9 L13 16 L15 7 L17 13 H18"/>
          </svg>
          <div>
            <div style={{ fontSize: 17, fontWeight: 600, letterSpacing: '-.01em' }}>Rampart</div>
            <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 2 }}>
              {meState.loading ? 'Loading…'
                : challengeToken ? 'Two-factor verification'
                : needsSetup ? 'Create your admin account'
                : 'Sign in'}
            </div>
          </div>
        </div>

        {err && <div className="banner-err">{err}</div>}

        {challengeToken ? (
          <form onSubmit={submitTotp}>
            <div className="field">
              <label className="field-label" htmlFor="auth-totp">Code</label>
              <input id="auth-totp" className="input" type="text" inputMode="numeric"
                autoComplete="one-time-code" autoFocus maxLength={11}
                value={totpCode} onChange={e => setTotpCode(e.target.value)}
                placeholder="6-digit code or recovery code"/>
              <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 6 }}>
                Open your authenticator app and enter the current 6-digit code, or use one of the recovery codes saved during setup.
              </div>
            </div>
            <button className="btn-primary" type="submit" disabled={busy}>
              {busy ? <><Loader2 size={14} className="spin"/> Verifying…</> : <><LogIn size={14}/> Verify</>}
            </button>
            <button type="button" onClick={cancelTotp} style={{
              marginTop: 10, background: 'transparent', border: 'none',
              color: 'var(--text-3)', fontSize: 12, cursor: 'pointer',
            }}>
              ← Back to email + password
            </button>
          </form>
        ) : (
        <form onSubmit={submit}>
          <div className="field">
            <label className="field-label" htmlFor="auth-email">Email</label>
            <input id="auth-email" className="input" type="email" autoComplete="email"
              value={email} onChange={e => setEmail(e.target.value)}
              placeholder="you@example.com" autoFocus/>
          </div>

          {needsSetup && (
            <div className="field">
              <label className="field-label" htmlFor="auth-name">
                Name <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>· optional</span>
              </label>
              <input id="auth-name" className="input" type="text" autoComplete="name"
                value={name} onChange={e => setName(e.target.value)}
                placeholder="Your name"/>
            </div>
          )}

          <div className="field">
            <label className="field-label" htmlFor="auth-password">Password</label>
            <input id="auth-password" className="input" type="password"
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
        )}

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
