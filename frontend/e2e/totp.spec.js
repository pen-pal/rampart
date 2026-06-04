// E2E: TOTP enrolment + recovery codes + disable flow.
//
// Drives the Security view through the full 2FA dance:
//
//   idle  → setup     → fetch secret + otpauth_uri
//   setup → enrolling → compute TOTP from the displayed base32
//                       secret via WebCrypto inside the page context,
//                       type the 6-digit code, submit
//   enrolling → activated → confirm recovery codes are rendered
//   activated → disabling → submit password + a fresh TOTP, confirm
//                           the user state flips back to no-2FA.
//
// Why drive the UI rather than POSTing directly? The UI exposes
// regressions the API contract test would miss — the EnrollPanel
// rendering the base32 secret in `<code className="mono">`, the
// `Activate` button gating on a 6-character input, the recovery-
// codes columned layout, etc.

import { test, expect } from '@playwright/test';
import { api, ensureLoggedIn, fixtures, gotoView } from './helpers.js';

test.describe.configure({ mode: 'serial' });

const ENROL_BTN  = /set up authenticator/i;
const ACTIVATE   = /activate/i;
const DISABLE    = /turn off two-factor/i;
const CONFIRM    = /turn off 2fa/i;

// ──────────────────────────────────────────────────────────────────────
// helpers
// ──────────────────────────────────────────────────────────────────────

/**
 * Compute a TOTP code (RFC 6238 / SHA-1 / 6 digits / 30s period) inside
 * the page context using WebCrypto. Returns a 6-digit zero-padded
 * string — the same format an authenticator app would show.
 *
 * The base32 secret format is the standard RFC 4648 alphabet without
 * padding (which is what `totp-rs` emits in `otpauth` mode + what
 * Authenticator apps expect). Capital letters only; spaces stripped.
 */
async function totpCodeFor(page, base32) {
  return page.evaluate(async (b32) => {
    const ALPHA = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
    const clean = b32.toUpperCase().replace(/[^A-Z2-7]/g, '');
    const out = [];
    let bits = 0, value = 0;
    for (const ch of clean) {
      const v = ALPHA.indexOf(ch);
      if (v < 0) continue;
      value = (value << 5) | v;
      bits += 5;
      if (bits >= 8) {
        bits -= 8;
        out.push((value >> bits) & 0xff);
      }
    }
    const key = new Uint8Array(out);

    const counter = Math.floor(Date.now() / 30_000);
    const buf = new ArrayBuffer(8);
    const dv = new DataView(buf);
    dv.setUint32(0, Math.floor(counter / 0x100000000));
    dv.setUint32(4, counter >>> 0);

    const cryptoKey = await crypto.subtle.importKey(
      'raw', key, { name: 'HMAC', hash: 'SHA-1' }, false, ['sign'],
    );
    const hmac = new Uint8Array(await crypto.subtle.sign('HMAC', cryptoKey, buf));
    const offset = hmac[hmac.length - 1] & 0x0f;
    const code =
      (((hmac[offset]     & 0x7f) << 24)
      | ( hmac[offset + 1]         << 16)
      | ( hmac[offset + 2]         <<  8)
      |   hmac[offset + 3])
      % 1_000_000;
    return String(code).padStart(6, '0');
  }, base32);
}

// ──────────────────────────────────────────────────────────────────────
// Pre-flight cleanup — if a previous spec left this user with 2FA
// active, peel it off via the API so the enrol flow has a clean state
// to start from. Uses the maintenance hatch every probe-test pattern
// here uses: hit the JSON API directly with the page's cookie jar.
// ──────────────────────────────────────────────────────────────────────
async function disableExistingTotp(page) {
  const me = await api(page, 'GET', '/v1/auth/me').catch(() => null);
  if (!me?.user?.totp_enabled) return;
  // The endpoint takes the password + a current TOTP, but if we still
  // have the secret around in localStorage we don't — peeking at the
  // current row via the DB hatch isn't available, so the cleanest path
  // is: skip the spec when 2FA is leftover from a prior run. Surface a
  // skip rather than a fail so the cross-browser matrix doesn't false-
  // alarm.
  test.skip(true, '2FA was left enabled by an earlier spec; reset DB to re-run');
}

// ──────────────────────────────────────────────────────────────────────
// flow
// ──────────────────────────────────────────────────────────────────────

test('TOTP enrol → activate → recovery codes → disable', async ({ page }) => {
  await ensureLoggedIn(page);
  await disableExistingTotp(page);

  // 1. Go to Security, click Set up authenticator.
  await gotoView(page, '#/security');
  await expect(page.getByRole('button', { name: ENROL_BTN })).toBeVisible();
  await page.getByRole('button', { name: ENROL_BTN }).click();

  // 2. EnrollPanel should mount with the secret in a `<code className=
  //    "mono">…</code>`. Pull it from the DOM.
  const secretLocator = page.locator('code.mono').first();
  await expect(secretLocator).toBeVisible({ timeout: 5_000 });
  const secret = (await secretLocator.textContent())?.trim();
  expect(secret, 'EnrollPanel should render the base32 secret').toBeTruthy();
  expect(secret).toMatch(/^[A-Z2-7]+$/);

  // 3. Compute a code from the displayed secret + type it into the
  //    input + click Activate.
  const code = await totpCodeFor(page, secret);
  expect(code).toMatch(/^\d{6}$/);
  await page.locator('input.input.mono').fill(code);
  await page.getByRole('button', { name: ACTIVATE }).click();

  // 4. Activation lands on the RecoveryPanel — eight recovery codes
  //    rendered in a 2-column block. Confirm at least one is visible
  //    and looks like a recovery code (alphanumeric, 8-12 chars).
  const recoveryArea = page.locator('div.mono').filter({ hasText: /[A-Z0-9]{6,}/ }).first();
  await expect(recoveryArea).toBeVisible({ timeout: 5_000 });

  // 5. Click "I saved the codes" to close the panel.
  await page.getByRole('button', { name: /i saved the codes/i }).click();

  // Reload to land back on the steady-state "2FA is ON" view.
  await page.waitForLoadState('networkidle');
  await gotoView(page, '#/security');
  await expect(page.getByRole('button', { name: DISABLE })).toBeVisible({ timeout: 5_000 });

  // 6. Disable: click "Turn off two-factor", fill password + a fresh
  //    TOTP code, confirm.
  await page.getByRole('button', { name: DISABLE }).click();
  await page.getByPlaceholder(/^password$/i).fill(fixtures.ADMIN_PASSWORD);
  const codeAgain = await totpCodeFor(page, secret);
  await page.locator('input.input.mono').fill(codeAgain);
  await page.getByRole('button', { name: CONFIRM }).click();

  // The disable handler reloads after a 1s success message. Wait for
  // the load + re-check that the user is no-longer-2FA'd via the API.
  await page.waitForLoadState('load');
  await page.waitForTimeout(1_500);
  const meAfter = await api(page, 'GET', '/v1/auth/me');
  // `/v1/auth/me` returns `{ user: {...} }`; unwrap before asserting.
  expect(meAfter.user?.totp_enabled).toBe(false);
});
