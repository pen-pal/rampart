// Unit tests for the API client helpers + the request() wrapper's
// 401-redirect + parsing behaviour.

import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  api, ApiError,
  formatRelative, formatClock,
  offsetDateTimeArrayToDate, statusToClass,
} from './api.js';

describe('ApiError', () => {
  it('captures status + code + message', () => {
    const e = new ApiError(500, 'internal', 'kaboom');
    expect(e).toBeInstanceOf(Error);
    expect(e.status).toBe(500);
    expect(e.code).toBe('internal');
    expect(e.message).toBe('kaboom');
    expect(e.payload).toEqual({ status: 500, code: 'internal', message: 'kaboom' });
  });

  it('falls back to code or status string when message is empty', () => {
    expect(new ApiError(0, 'network').message).toBe('network');
    expect(new ApiError(418).message).toBe('HTTP 418');
  });
});

describe('statusToClass', () => {
  it('maps each known status to a css class', () => {
    expect(statusToClass('up')).toBe('up');
    expect(statusToClass('down')).toBe('down');
    expect(statusToClass('warn')).toBe('warn');
    expect(statusToClass('paused')).toBe('paused');
    expect(statusToClass('maintenance')).toBe('maint');   // shortened on purpose
    expect(statusToClass('pending')).toBe('paused');      // visually muted
  });

  it('falls back for unknown values', () => {
    expect(statusToClass('???')).toBe('paused');
    expect(statusToClass(undefined)).toBe('paused');
  });
});

describe('offsetDateTimeArrayToDate', () => {
  it('decodes the time crate tuple format ([year, ordinal_day, h, m, s, ns])', () => {
    // Jan 1, 2026, 00:00:00 UTC → ordinal 1.
    const d = offsetDateTimeArrayToDate([2026, 1, 0, 0, 0, 0, 0, 0, 0]);
    expect(d.toISOString()).toBe('2026-01-01T00:00:00.000Z');
  });

  it('handles non-leap-year ordinal-day arithmetic', () => {
    // Feb 1, 2026 = ordinal 32.
    const d = offsetDateTimeArrayToDate([2026, 32, 12, 30, 45, 0, 0, 0, 0]);
    expect(d.toISOString()).toBe('2026-02-01T12:30:45.000Z');
  });

  it('rounds nanoseconds to milliseconds', () => {
    const d = offsetDateTimeArrayToDate([2026, 1, 0, 0, 0, 999_999_999, 0, 0, 0]);
    // 1000 ms rounds up — clamp test is "close to second boundary".
    expect(d.getUTCSeconds() + d.getUTCMilliseconds() / 1000).toBeCloseTo(1.0, 1);
  });

  it('returns invalid Date for nonsense input', () => {
    expect(Number.isNaN(offsetDateTimeArrayToDate(null).getTime())).toBe(true);
    expect(Number.isNaN(offsetDateTimeArrayToDate([]).getTime())).toBe(true);
  });
});

describe('formatRelative', () => {
  it('produces "Xs ago" for sub-minute deltas', () => {
    const date = new Date(Date.now() - 5_000);
    expect(formatRelative(date.toISOString())).toBe('5s ago');
  });

  it('falls back to "—" when given null/undefined', () => {
    expect(formatRelative(null)).toBe('—');
    expect(formatRelative(undefined)).toBe('—');
  });

  it('produces "Xm ago" for minute deltas', () => {
    const date = new Date(Date.now() - 3 * 60_000);
    expect(formatRelative(date.toISOString())).toBe('3m ago');
  });

  it('produces "Xh ago" for hour deltas', () => {
    const date = new Date(Date.now() - 2 * 3_600_000);
    expect(formatRelative(date.toISOString())).toBe('2h ago');
  });

  it('produces "Xd ago" for day deltas', () => {
    const date = new Date(Date.now() - 4 * 86_400_000);
    expect(formatRelative(date.toISOString())).toBe('4d ago');
  });

  it('also accepts the time crate tuple format', () => {
    const out = formatRelative([2026, 1, 0, 0, 0, 0, 0, 0, 0]);
    expect(typeof out).toBe('string');
    expect(out.endsWith(' ago')).toBe(true);
  });
});

describe('formatClock', () => {
  it('formats HH:MM:SS', () => {
    const s = formatClock(new Date('2026-05-27T14:30:45Z').toISOString());
    // Output depends on local TZ; check it has hh:mm:ss shape.
    expect(s).toMatch(/^\d{2}:\d{2}:\d{2}$/);
  });

  it('returns — for null input', () => {
    expect(formatClock(null)).toBe('—');
  });
});

// ─── request() behavior — exercised via api.monitors.* with a mocked fetch
describe('request() wrapper', () => {
  beforeEach(() => {
    // Default to a 200 OK with an empty list. Tests override per-case.
    global.fetch = vi.fn().mockResolvedValue(jsonResponse([], 200));
    // Reset hash so 401-redirect logic has a stable baseline.
    window.location.hash = '#/';
  });

  function jsonResponse(body, status = 200) {
    return {
      ok:     status >= 200 && status < 300,
      status,
      text:   () => Promise.resolve(typeof body === 'string' ? body : JSON.stringify(body)),
    };
  }

  it('sends credentials same-origin so the session cookie travels', async () => {
    await api.monitors.list();
    const opts = global.fetch.mock.calls[0][1];
    expect(opts.credentials).toBe('same-origin');
  });

  it('decodes JSON on 200', async () => {
    global.fetch = vi.fn().mockResolvedValue(jsonResponse([{ id: 'x' }]));
    const r = await api.monitors.list();
    expect(r).toEqual([{ id: 'x' }]);
  });

  it('returns null on 204 (no content) responses', async () => {
    global.fetch = vi.fn().mockResolvedValue({
      ok: true, status: 204, text: () => Promise.resolve(''),
    });
    const r = await api.monitors.pause('x');
    expect(r).toBeNull();
  });

  it('throws ApiError carrying status + message from {error: {code, message}}', async () => {
    global.fetch = vi.fn().mockResolvedValue(jsonResponse({
      error: { code: 'conflict', message: 'already exists' },
    }, 409));
    await expect(api.notifications.create('webhook', 'x', {}))
      .rejects.toMatchObject({ status: 409, code: 'conflict', message: 'already exists' });
  });

  it('redirects to #/login on 401 for protected paths', async () => {
    global.fetch = vi.fn().mockResolvedValue(jsonResponse(
      { error: { code: 'unauthorized', message: 'unauthorized' } }, 401));
    await expect(api.monitors.list()).rejects.toBeInstanceOf(ApiError);
    expect(window.location.hash).toBe('#/login');
  });

  it('does NOT redirect for 401s on /v1/auth/* (lets Login form show "wrong password")', async () => {
    global.fetch = vi.fn().mockResolvedValue(jsonResponse(
      { error: { code: 'unauthorized', message: 'unauthorized' } }, 401));
    window.location.hash = '#/login';
    await expect(api.auth.login('a@b.c', 'wrong'))
      .rejects.toMatchObject({ status: 401 });
    expect(window.location.hash).toBe('#/login');  // unchanged
  });

  it('wraps fetch network errors in ApiError(status=0, code=network)', async () => {
    global.fetch = vi.fn().mockRejectedValue(new TypeError('connection refused'));
    try {
      await api.monitors.list();
      throw new Error('expected to throw');
    } catch (e) {
      expect(e).toBeInstanceOf(ApiError);
      expect(e.status).toBe(0);
      expect(e.code).toBe('network');
    }
  });
});

describe('api helpers', () => {
  beforeEach(() => {
    global.fetch = vi.fn().mockResolvedValue({
      ok: true, status: 200, text: () => Promise.resolve('null'),
    });
  });

  it('builds the right URLs', async () => {
    await api.monitors.list();
    expect(global.fetch.mock.calls[0][0]).toBe('/v1/monitors');

    await api.monitors.heartbeats('abc-id', 50);
    expect(global.fetch.mock.calls[1][0]).toBe('/v1/monitors/abc-id/heartbeats?limit=50');

    await api.monitors.summary(3600);
    expect(global.fetch.mock.calls[2][0]).toBe('/v1/monitors/summary?window=3600');

    await api.notifications.attach('m', 'n');
    expect(global.fetch.mock.calls[3][0]).toBe('/v1/monitors/m/notifications/n');

    await api.traces.list(100);
    expect(global.fetch.mock.calls[4][0]).toBe('/v1/traces?limit=100');

    await api.traces.serviceMap(24);
    expect(global.fetch.mock.calls[5][0]).toBe('/v1/traces/service-map?hours=24');

    await api.logs.services();
    expect(global.fetch.mock.calls[6][0]).toBe('/v1/logs/services');

    await api.logs.query({ service: 'api', level: 'warn' });
    expect(global.fetch.mock.calls[7][0]).toBe('/v1/logs?service=api&level=warn');
    await api.errorProjects.list();
    expect(global.fetch.mock.calls[4][0]).toBe('/v1/error-projects');

    await api.errorProjects.issues('p1', 'unresolved');
    expect(global.fetch.mock.calls[5][0]).toBe('/v1/error-projects/p1/issues?status=unresolved');

    await api.errorIssues.resolve('i1');
    expect(global.fetch.mock.calls[6][0]).toBe('/v1/error-issues/i1/resolve');
    expect(global.fetch.mock.calls[6][1].method).toBe('POST');
  });

  it('serialises POST body and sets Content-Type', async () => {
    await api.notifications.create('webhook', 'test', { url: 'https://x' });
    const [, opts] = global.fetch.mock.calls[0];
    expect(opts.method).toBe('POST');
    expect(opts.headers['Content-Type']).toBe('application/json');
    const body = JSON.parse(opts.body);
    expect(body.kind).toBe('webhook');
    expect(body.config).toEqual({ url: 'https://x' });
    expect(body.active).toBe(true);
  });
});
