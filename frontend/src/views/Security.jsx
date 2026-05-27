// Account-security view. v1 only covers TOTP enrolment + recovery codes;
// password change / session management land here later.

import React, { useEffect, useState } from 'react';
import {
  ChevronLeft, Shield, ShieldCheck, Copy, Check, AlertCircle, Loader2, X,
} from 'lucide-react';
import { api, useApi } from '../lib/api.js';

const css = `
  @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap');
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
  .btn-accent { background: var(--accent); color: white; border-color: var(--accent); }
  .btn-accent:hover { background: var(--accent-2); }
  .btn-danger { color: var(--down); }
  .btn-danger:hover { background: var(--down-soft); border-color: var(--down); }
  .btn-ghost  { background: transparent; border-color: transparent; }
  .btn-ghost:hover { background: var(--surface-2); }
  .input {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    background: var(--surface); border: 1px solid var(--border);
    font-size: 13px; color: var(--text); outline: none; font-family: inherit;
  }
  .input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .banner-err { background: var(--down-soft); color: #b91c1c; border: 1px solid #fecaca; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
  .banner-ok  { background: var(--up-soft);   color: #047857; border: 1px solid #a7f3d0; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; }
`;

export default function Security() {
  const meState = useApi(() => api.auth.me(), []);
  const me = meState.data?.user;

  return (
    <div className="rampart">
      <style>{css}</style>
      <div style={{ maxWidth: 720, margin: '0 auto', padding: '32px 32px 64px' }}>
        <a href="#/" className="btn btn-ghost" style={{ marginBottom: 18 }}>
          <ChevronLeft size={14}/> Dashboard
        </a>
        <h1 style={{ fontSize: 28, fontWeight: 600, margin: '0 0 4px', letterSpacing: '-.02em' }}>
          Security
        </h1>
        <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 22px' }}>
          Account-level protections. Two-factor auth gates the password login with a one-time code from your authenticator app.
        </p>

        {!me ? (
          <div style={{ padding: 24, textAlign: 'center', color: 'var(--text-3)' }}><Loader2 size={16}/></div>
        ) : (
          <Totp user={me}/>
        )}
      </div>
    </div>
  );
}

function Totp({ user }) {
  const [phase,     setPhase]     = useState('idle'); // idle | enrolling | activated | disabling
  const [secret,    setSecret]    = useState(null);
  const [otpauth,   setOtpauth]   = useState(null);
  const [code,      setCode]      = useState('');
  const [recovery,  setRecovery]  = useState(null); // array shown once
  const [pw,        setPw]        = useState('');
  const [busy,      setBusy]      = useState(false);
  const [err,       setErr]       = useState(null);
  const [ok,        setOk]        = useState(null);

  const isEnabled = user.totp_enabled;

  const startEnroll = async () => {
    setErr(null); setBusy(true);
    try {
      const r = await api.auth.totpSetup();
      setSecret(r.secret); setOtpauth(r.otpauth_uri); setPhase('enrolling');
    } catch (e) { setErr(e.message); }
    finally { setBusy(false); }
  };

  const confirmEnroll = async () => {
    setErr(null); setBusy(true);
    try {
      const r = await api.auth.totpEnable(code.trim());
      setRecovery(r.recovery_codes);
      setPhase('activated');
      setOk('Two-factor auth is now active.');
    } catch (e) { setErr(e.message || 'Could not enable.'); }
    finally { setBusy(false); }
  };

  const startDisable = () => { setPhase('disabling'); setErr(null); setOk(null); };
  const confirmDisable = async () => {
    setErr(null); setBusy(true);
    try {
      await api.auth.totpDisable(pw, code.trim());
      setOk('Two-factor auth turned off.');
      setTimeout(() => window.location.reload(), 1000);
    } catch (e) { setErr(e.message || 'Could not disable.'); }
    finally { setBusy(false); }
  };

  return (
    <div className="card" style={{ padding: 22 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14 }}>
        {isEnabled
          ? <ShieldCheck size={20} color="var(--accent)"/>
          : <Shield      size={20} color="var(--text-3)"/>}
        <h2 style={{ fontSize: 16, fontWeight: 600, margin: 0 }}>
          Two-factor authentication {isEnabled && <span style={{ fontSize: 11, color: 'var(--accent-2)', background: 'var(--accent-soft)', padding: '2px 8px', borderRadius: 999, marginLeft: 4 }}>ON</span>}
        </h2>
      </div>

      {err && <div className="banner-err">{err}</div>}
      {ok  && <div className="banner-ok">{ok}</div>}

      {phase === 'idle' && !isEnabled && (
        <>
          <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 14px' }}>
            Add a second factor (Google Authenticator, 1Password, Authy…) on top of your password.
          </p>
          <button className="btn btn-accent" onClick={startEnroll} disabled={busy}>
            {busy ? <><Loader2 size={13}/> Generating…</> : 'Set up authenticator'}
          </button>
        </>
      )}

      {phase === 'idle' && isEnabled && (
        <>
          <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 14px' }}>
            Two-factor is active. You'll be asked for a code on every sign-in.
          </p>
          <button className="btn btn-danger" onClick={startDisable} disabled={busy}>
            Turn off two-factor
          </button>
        </>
      )}

      {phase === 'enrolling' && (
        <EnrollPanel
          secret={secret} otpauth={otpauth}
          code={code} setCode={setCode}
          busy={busy} onConfirm={confirmEnroll}
          onCancel={() => { setPhase('idle'); setSecret(null); }}
        />
      )}

      {phase === 'activated' && recovery && (
        <RecoveryPanel codes={recovery} onClose={() => window.location.reload()}/>
      )}

      {phase === 'disabling' && (
        <DisablePanel
          pw={pw} setPw={setPw}
          code={code} setCode={setCode}
          busy={busy} onConfirm={confirmDisable}
          onCancel={() => setPhase('idle')}
        />
      )}
    </div>
  );
}

function EnrollPanel({ secret, otpauth, code, setCode, busy, onConfirm, onCancel }) {
  // QR rendered via the Google Charts shim — keeps the bundle small,
  // works without any qrcode npm dep. For offline / air-gapped deploys
  // the printed `secret` is the fallback (most apps accept manual entry).
  const qrUrl = `https://api.qrserver.com/v1/create-qr-code/?size=200x200&data=${encodeURIComponent(otpauth)}`;

  return (
    <>
      <ol style={{ paddingLeft: 18, fontSize: 13, color: 'var(--text-2)', lineHeight: 1.7, margin: '0 0 14px' }}>
        <li>Scan this QR code with your authenticator app:
          <div style={{ marginTop: 8, display: 'inline-block', padding: 6, background: '#fff', border: '1px solid var(--border)', borderRadius: 8 }}>
            <img src={qrUrl} width="180" height="180" alt="otpauth QR" style={{ display: 'block' }}/>
          </div>
        </li>
        <li>Or paste the secret manually: <code className="mono" style={{ padding: '2px 6px', background: 'var(--surface-2)', borderRadius: 4 }}>{secret}</code></li>
        <li>Enter the 6-digit code your app shows now:</li>
      </ol>
      <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
        <input className="input mono" maxLength={6} value={code} onChange={e => setCode(e.target.value)}
          placeholder="123456" autoFocus style={{ maxWidth: 160 }}/>
        <button className="btn btn-accent" onClick={onConfirm} disabled={busy || code.length < 6}>
          {busy ? <><Loader2 size={13}/> Activating…</> : 'Activate'}
        </button>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>Cancel</button>
      </div>
    </>
  );
}

function RecoveryPanel({ codes, onClose }) {
  const [copied, setCopied] = useState(false);
  const text = codes.join('\n');
  const copy = async () => {
    try { await navigator.clipboard.writeText(text); setCopied(true); setTimeout(() => setCopied(false), 1500); }
    catch { /* ignore */ }
  };

  return (
    <div>
      <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 12px' }}>
        <strong>Save these recovery codes.</strong> Each one works once, and we won't show them again.
        If you lose access to your authenticator, paste one of these into the code prompt at login.
      </p>
      <div className="mono" style={{
        padding: 14, background: 'var(--surface-2)', border: '1px solid var(--border)',
        borderRadius: 8, fontSize: 13, columnCount: 2, columnGap: 24, lineHeight: 1.6,
      }}>
        {codes.map((c, i) => <div key={i}>{c}</div>)}
      </div>
      <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
        <button className="btn btn-ghost" onClick={copy}>
          {copied ? <><Check size={13}/> Copied</> : <><Copy size={13}/> Copy all</>}
        </button>
        <button className="btn btn-accent" onClick={onClose} style={{ marginLeft: 'auto' }}>
          I saved the codes
        </button>
      </div>
    </div>
  );
}

function DisablePanel({ pw, setPw, code, setCode, busy, onConfirm, onCancel }) {
  return (
    <>
      <p style={{ fontSize: 13, color: 'var(--text-2)', margin: '0 0 14px' }}>
        Confirm your password and a current 6-digit code (or a recovery code) to turn off 2FA.
      </p>
      <div style={{ marginBottom: 10 }}>
        <input className="input" type="password" value={pw} onChange={e => setPw(e.target.value)} placeholder="Password" autoFocus/>
      </div>
      <div style={{ marginBottom: 12 }}>
        <input className="input mono" value={code} onChange={e => setCode(e.target.value)} placeholder="6-digit code or recovery"/>
      </div>
      <div style={{ display: 'flex', gap: 8 }}>
        <button className="btn btn-danger" onClick={onConfirm} disabled={busy || !pw || !code}>
          {busy ? <><Loader2 size={13}/> Turning off…</> : 'Turn off 2FA'}
        </button>
        <button className="btn btn-ghost" onClick={onCancel} disabled={busy}>Cancel</button>
      </div>
    </>
  );
}
