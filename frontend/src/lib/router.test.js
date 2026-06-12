// Unit tests for the hash-router parser.

import { describe, it, expect } from 'vitest';
import { parseRoute } from './router.js';

describe('parseRoute', () => {
  it('defaults to dashboard for empty hash', () => {
    expect(parseRoute('')).toEqual({ view: 'dashboard', id: null });
    expect(parseRoute(null)).toEqual({ view: 'dashboard', id: null });
    expect(parseRoute('#/')).toEqual({ view: 'dashboard', id: null });
  });

  it('routes #/login to login', () => {
    expect(parseRoute('#/login')).toEqual({ view: 'login', id: null });
  });

  it('routes #/monitor/<id> to monitor with id', () => {
    expect(parseRoute('#/monitor/019e6512-abc'))
      .toEqual({ view: 'monitor', id: '019e6512-abc' });
  });

  it('routes bare #/monitor to monitor with null id', () => {
    expect(parseRoute('#/monitor')).toEqual({ view: 'monitor', id: null });
  });

  it('routes named views without ids', () => {
    expect(parseRoute('#/new-monitor'))   .toEqual({ view: 'new-monitor',   id: null });
    expect(parseRoute('#/status-page'))   .toEqual({ view: 'status-page',   id: null });
    expect(parseRoute('#/notifications')) .toEqual({ view: 'notifications', id: null });
    expect(parseRoute('#/escalations'))   .toEqual({ view: 'escalations',   id: null });
    expect(parseRoute('#/traces'))        .toEqual({ view: 'traces',        id: null });
    expect(parseRoute('#/logs'))          .toEqual({ view: 'logs',          id: null });
    expect(parseRoute('#/errors'))        .toEqual({ view: 'errors',        id: null });
    expect(parseRoute('#/maintenance'))   .toEqual({ view: 'maintenance',   id: null });
    expect(parseRoute('#/s/acme'))        .toEqual({ view: 'public-status', id: 'acme' });
    expect(parseRoute('#/s/acme-corp'))   .toEqual({ view: 'public-status', id: 'acme-corp' });
    expect(parseRoute('#/api-keys'))      .toEqual({ view: 'api-keys',      id: null });
    expect(parseRoute('#/proxies'))       .toEqual({ view: 'proxies',       id: null });
    expect(parseRoute('#/security'))      .toEqual({ view: 'security',      id: null });
    expect(parseRoute('#/users'))         .toEqual({ view: 'users',         id: null });
    expect(parseRoute('#/settings/smtp')) .toEqual({ view: 'smtp-settings', id: null });
    expect(parseRoute('#/audit'))         .toEqual({ view: 'audit',         id: null });
  });

  it('treats unknown hashes as dashboard (graceful fallback)', () => {
    expect(parseRoute('#/totally-unknown')).toEqual({ view: 'dashboard', id: null });
  });

  it('login route accepts trailing slashes/segments', () => {
    expect(parseRoute('#/login/')).toEqual({ view: 'login', id: null });
    expect(parseRoute('#/login?x=1')).toEqual({ view: 'login', id: null });
  });
});
