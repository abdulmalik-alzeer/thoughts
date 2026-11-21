// Round 0.4.0: answering a build that wants its screens looked at. One answer per request, from one
// window; nothing is sent when nothing here can look.
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../i18n', () => ({ t: (k: string) => k }));
const { forgetLooks, lookAtScreens, looks, onlyOneWindow } = await import('./screenwatch.svelte');
import type { ScreenResult } from './types';

const ask = { url: 'http://localhost:4173', screens: [{ id: 'shop', path: '/' }, { id: 'board', path: '/board' }], nonce: 'n1' };
const seen = (id: string, path: string): ScreenResult => ({ id, path, ok: true, status: 200, errors: [], text: 'Hello', fields: [] });
const alone = (_: string, job: () => Promise<void>) => job();

function service(over: Partial<typeof ask> = {}, waiting = true) {
  const sent: { nonce: string; results: ScreenResult[] }[] = [];
  return {
    sent,
    api: {
      buildScreens: vi.fn(async () => { if (!waiting) throw new Error('409'); return { ...ask, ...over }; }),
      sendScreens: vi.fn(async (nonce: string, results: ScreenResult[]) => { sent.push({ nonce, results }); return {}; }),
    },
  };
}

beforeEach(() => forgetLooks());

describe('looking at the screens', () => {
  it('asks, looks through the shell and sends what it saw with the nonce', async () => {
    const s = service();
    const check = vi.fn(async (_url: string, screens: { id: string; path: string }[]) => screens.map((x) => seen(x.id, x.path)));
    await lookAtScreens('p1', { api: s.api, check, only: alone });
    expect(check).toHaveBeenCalledWith('http://localhost:4173', ask.screens);
    expect(s.sent).toEqual([{ nonce: 'n1', results: [seen('shop', '/'), seen('board', '/board')] }]);
    expect(looks.p1).toBe('sent');
  });
  it('never answers the same request twice, however often it is called', async () => {
    const s = service();
    const check = vi.fn(async () => [seen('shop', '/')]);
    await Promise.all([lookAtScreens('p1', { api: s.api, check, only: alone }), lookAtScreens('p1', { api: s.api, check, only: alone })]);
    await lookAtScreens('p1', { api: s.api, check, only: alone });
    expect(check).toHaveBeenCalledTimes(1);
    expect(s.sent.length).toBe(1);
  });
  it('sends nothing when nothing here can look, and says so', async () => {
    const s = service();
    await lookAtScreens('p1', { api: s.api, check: async () => null, only: alone });
    expect(s.sent).toEqual([]);
    expect(looks.p1).toBe('cannot');
    await lookAtScreens('p1', { api: s.api, check: async () => { throw new Error('boom'); }, only: alone });
    expect(s.sent).toEqual([]);
  });
  it('does nothing while the build is not waiting for a window', async () => {
    const s = service({}, false);
    const check = vi.fn(async () => []);
    await lookAtScreens('p1', { api: s.api, check, only: alone });
    expect(check).not.toHaveBeenCalled();
    expect(looks.p1).toBeUndefined();
  });
  it('does nothing when another window is already at it', async () => {
    const s = service();
    const check = vi.fn(async () => []);
    await lookAtScreens('p1', { api: s.api, check, only: async () => {} });
    expect(s.api.buildScreens).not.toHaveBeenCalled();
    expect(check).not.toHaveBeenCalled();
  });
  it('never opens an address that is not the local product', async () => {
    const s = service({ url: 'https://evil.example' });
    const check = vi.fn(async () => []);
    await lookAtScreens('p1', { api: s.api, check, only: alone });
    expect(check).not.toHaveBeenCalled();
    expect(s.sent).toEqual([]);
    expect(looks.p1).toBe('cannot');
  });
  it('says when what it saw could not be passed on', async () => {
    const s = service();
    s.api.sendScreens.mockRejectedValueOnce(new Error('gone'));
    await lookAtScreens('p1', { api: s.api, check: async () => [seen('shop', '/')], only: alone });
    expect(looks.p1).toBe('failed');
  });
});

describe('one window at a time', () => {
  it('runs the job only when the lock is free', async () => {
    const job = vi.fn(async () => {});
    vi.stubGlobal('navigator', { locks: { request: async (_n: string, _o: unknown, f: (l: unknown) => Promise<void>) => f(null) } });
    await onlyOneWindow('x', job);
    expect(job).not.toHaveBeenCalled();
    vi.stubGlobal('navigator', { locks: { request: async (_n: string, o: any, f: (l: unknown) => Promise<void>) => { expect(o.ifAvailable).toBe(true); return f({}); } } });
    await onlyOneWindow('x', job);
    expect(job).toHaveBeenCalledTimes(1);
    vi.stubGlobal('navigator', {});
    await onlyOneWindow('x', job);
    expect(job).toHaveBeenCalledTimes(2);
    vi.unstubAllGlobals();
  });
  it('tries again later while the shell is looking at another build, without answering', async () => {
    const s = service();
    let busy = true;
    const check = vi.fn(async (_u: string, sc: { id: string; path: string }[]) => { if (busy) throw new Error('screen-check-busy'); return sc.map((x) => seen(x.id, x.path)); });
    await lookAtScreens('p1', { api: s.api, check, only: alone });
    expect(looks.p1).toBe('busy');
    expect(s.sent).toEqual([]);
    busy = false;
    await lookAtScreens('p1', { api: s.api, check, only: alone });
    expect(looks.p1).toBe('sent');
    expect(s.sent.length).toBe(1);
  });
  it('tells the build at once, with nothing seen, when the shell refuses the address or the number of screens', async () => {
    for (const [code, look] of [['screen-check-bad-url', 'badUrl'], ['screen-check-too-many', 'tooMany']] as const) {
      forgetLooks();
      const s = service({ nonce: code });
      await lookAtScreens('p1', { api: s.api, check: async () => { throw new Error(code); }, only: alone });
      expect(looks.p1).toBe(look);
      expect(s.sent).toEqual([{ nonce: code, results: [] }]);
    }
  });
});
