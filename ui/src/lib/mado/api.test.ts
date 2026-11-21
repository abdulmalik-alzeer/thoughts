// The MADO client against a fake `fetch`: every route of docs/mado-spec.md goes out with the
// right method, address and body, and nothing odd can be put into an address.
import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest';

vi.mock('../i18n', () => ({ t: (k: string) => k }));

type Call = { method: string; path: string; body: any; auth: string };
const calls: Call[] = [];
let reply: (c: Call) => { status?: number; body?: any } = () => ({ body: {} });

beforeAll(() => {
  history.replaceState(null, '', '/?port=4321&token=test-token');
  vi.stubGlobal('WebSocket', Object.assign(class { readyState = 0; close() {} }, { OPEN: 1, CONNECTING: 0 }));
  vi.stubGlobal('fetch', async (url: string, init: RequestInit = {}) => {
    const u = new URL(url);
    const c: Call = { method: init.method ?? 'GET', path: u.pathname + u.search, body: init.body ? JSON.parse(String(init.body)) : undefined, auth: (init.headers as any)?.Authorization ?? '' };
    calls.push(c);
    expect(u.origin).toBe('http://127.0.0.1:4321');
    const r = reply(c);
    return new Response(JSON.stringify(r.body ?? {}), { status: r.status ?? 200 });
  });
});
beforeEach(() => { calls.length = 0; reply = () => ({ body: {} }); });

const { madoApi, clampParallel } = await import('./api');
const { ApiError } = await import('../api');
const PID = '7b0c8f0e-1111-4222-8333-444455556666';
const m = madoApi(PID);
const B = `/api/mado/${PID}`;
const last = () => calls[calls.length - 1];

describe('madoApi', () => {
  it('reads the overview with the app token', async () => {
    reply = () => ({ body: { project: { id: PID, name: 'Shop', path: '/p' }, exists: false, state: null, files: [] } });
    const o = await m.overview();
    expect(o.exists).toBe(false);
    expect(last()).toEqual({ method: 'GET', path: B, body: undefined, auth: 'Bearer test-token' });
  });

  it('every route: method, address, body', async () => {
    const table: [() => Promise<unknown>, string, string, any][] = [
      [() => m.start(), 'POST', '/start', {}],
      [() => m.doc('spec'), 'GET', '/doc/spec', undefined],
      [() => m.saveDoc('tech', '# T'), 'PUT', '/doc/tech', { text: '# T' }],
      [() => m.graph('schema'), 'GET', '/graph/schema', undefined],
      [() => m.savePositions('flow', { home: { x: 1, y: 2 } }), 'PUT', '/graph/flow/positions', { positions: { home: { x: 1, y: 2 } } }],
      [() => m.plan(), 'GET', '/plan', undefined],
      [() => m.run('spec'), 'POST', '/steps/spec/run', {}],
      [() => m.run('spec', '  small  '), 'POST', '/steps/spec/run', { note: 'small' }],
      [() => m.approve('spec'), 'POST', '/steps/spec/approve', {}],
      [() => m.approve('review', 'abc123'), 'POST', '/steps/review/approve', { planHash: 'abc123' }],
      [() => m.reopen('tech'), 'POST', '/steps/tech/reopen', {}],
      [() => m.addComment({ kind: 'task', taskId: 'auth' }, 'split it'), 'POST', '/comments', { target: { kind: 'task', taskId: 'auth' }, body: 'split it' }],
      [() => m.editComment('c-1', { body: 'x' }), 'PATCH', '/comments/c-1', { body: 'x' }],
      [() => m.editComment('c-1', { status: 'open' }), 'PATCH', '/comments/c-1', { status: 'open' }],
      [() => m.removeComment('c-1'), 'DELETE', '/comments/c-1', undefined],
      [() => m.changes({ step: 'spec', commentIds: ['c1'] }), 'POST', '/changes', { step: 'spec', commentIds: ['c1'] }],
      [() => m.changes({ step: 'spec', commentIds: [], graphOps: [{ file: 'flow.graph.json', ops: [{ op: 'remove-node', id: 'a' }] }], note: ' n ' }), 'POST', '/changes',
        { step: 'spec', commentIds: [], graphOps: [{ file: 'flow.graph.json', ops: [{ op: 'remove-node', id: 'a' }] }], note: 'n' }],
      [() => m.changes({ step: 'spec', commentIds: ['c1'], graphOps: [], note: '  ' }), 'POST', '/changes', { step: 'spec', commentIds: ['c1'] }],
      [() => m.review(), 'GET', '/review', undefined],
      [() => m.styles(), 'GET', '/design/styles', undefined],
      [() => m.pickStyle('calm'), 'POST', '/design/style', { id: 'calm' }],
      [() => m.tokens(), 'GET', '/design/tokens', undefined],
      [() => m.screens(), 'GET', '/design/screens', undefined],
      [() => m.screen('home'), 'GET', '/design/screens/home', undefined],
      [() => m.build(), 'GET', '/build', undefined],
      [() => m.buildStart(4), 'POST', '/build/start', { maxParallel: 4 }],
      [() => m.buildStart(99), 'POST', '/build/start', { maxParallel: 6 }],
      [() => m.buildStart(), 'POST', '/build/start', {}],
      [() => m.buildPause(), 'POST', '/build/pause', {}],
      [() => m.buildResume(), 'POST', '/build/resume', {}],
      [() => m.buildStop(), 'POST', '/build/stop', {}],
      [() => m.retryTask('auth'), 'POST', '/build/tasks/auth/retry', {}],
      [() => m.land(), 'POST', '/build/land', {}],
    ];
    for (const [call, method, path, body] of table) {
      await call();
      expect({ method: last().method, path: last().path, body: last().body }, path).toEqual({ method, path: B + path, body });
    }
  });

  it('approving the review always names a plan, even an empty one (the service then refuses)', async () => {
    await m.approve('review');
    expect(last().body).toEqual({ planHash: '' });
  });

  it('comments: the file shape and a bare list are both read; anything else is no comments', async () => {
    const one = { id: 'c1', target: { kind: 'task', taskId: 't' }, body: 'b', createdAt: '', status: 'open' };
    reply = () => ({ body: { version: 1, comments: [one] } });
    expect(await m.comments()).toEqual([one]);
    reply = () => ({ body: [one] });
    expect(await m.comments()).toEqual([one]);
    reply = () => ({ body: { version: 1 } });
    expect(await m.comments()).toEqual([]);
  });

  it('refuses names and ids that could reach another route, before anything is sent', async () => {
    expect(() => madoApi('../settings')).toThrow();
    expect(() => madoApi('a/b')).toThrow();
    expect(() => madoApi('')).toThrow();
    expect(() => m.doc('../state' as any)).toThrow();
    expect(() => m.doc('plan' as any)).toThrow();
    expect(() => m.graph('flow/../x' as any)).toThrow();
    expect(() => m.run('deploy' as any)).toThrow();
    expect(() => m.removeComment('c1/../../x')).toThrow();
    expect(() => m.editComment('?x=1', { body: 'b' })).toThrow();
    expect(() => m.screen('..')).toThrow();
    expect(() => m.retryTask('a b')).toThrow();
    expect(calls.length).toBe(0);
  });

  it('a refusal comes back as an ApiError with the status and the service\'s words', async () => {
    reply = () => ({ status: 409, body: { error: 'The plan changed', code: 'plan_changed' } });
    const e: any = await m.approve('review', 'old').catch((x) => x);
    expect(e).toBeInstanceOf(ApiError);
    expect(e.status).toBe(409);
    expect(e.code).toBe('plan_changed');
    expect(e.message).toBe('The plan changed');
  });

  it('clampParallel keeps builders between 1 and 6', () => {
    expect([0, 1, 3.4, 6, 7, NaN, -2].map(clampParallel)).toEqual([1, 1, 3, 6, 6, 1, 1]);
  });
});
