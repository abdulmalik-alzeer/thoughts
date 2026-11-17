// Ordering overlapping requests (serial.ts) and the settings saves built on it (store.svelte.ts).
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { newestOnly, oneAtATime } from './serial';

/** A promise resolved (or refused) by hand. */
function later<T = any>() {
  let ok!: (v: T) => void, fail!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => { ok = a; fail = b; });
  return { promise, ok, fail };
}
const flush = () => new Promise((r) => setTimeout(r, 0));

describe('newestOnly', () => {
  it('drops an older answer that arrives after a newer one', async () => {
    const guard = newestOnly();
    const shown: string[] = [];
    const a = later<string>(), b = later<string>();
    const first = guard(() => a.promise, (v) => shown.push(v));
    const second = guard(() => b.promise, (v) => shown.push(v));
    b.ok('new');
    a.ok('old');
    expect(await second).toBe(true);
    expect(await first).toBe(false);
    expect(shown).toEqual(['new']);
  });

  it('shows answers that arrive in order, one after the other', async () => {
    const guard = newestOnly();
    const shown: string[] = [];
    const a = later<string>(), b = later<string>();
    const first = guard(() => a.promise, (v) => shown.push(v));
    const second = guard(() => b.promise, (v) => shown.push(v));
    a.ok('old');
    await first;
    b.ok('new');
    await second;
    expect(shown).toEqual(['old', 'new']);
  });

  it('a failed newer request does not hide a later answer', async () => {
    const guard = newestOnly();
    const shown: number[] = [];
    await expect(guard(() => Promise.reject(new Error('x')), (v: number) => shown.push(v))).rejects.toThrow('x');
    await guard(() => Promise.resolve(3), (v) => shown.push(v));
    expect(shown).toEqual([3]);
  });
});

describe('oneAtATime', () => {
  it('starts a job only when the one before it has finished, even when it failed', async () => {
    const run = oneAtATime();
    const log: string[] = [];
    const a = later(), b = later();
    const one = run(async () => { log.push('a start'); await a.promise; log.push('a end'); });
    const two = run(async () => { log.push('b start'); await b.promise; log.push('b end'); });
    const three = run(async () => { log.push('c'); return 7; });
    await flush();
    expect(log).toEqual(['a start']);
    a.fail(new Error('refused'));
    await expect(one).rejects.toThrow('refused');
    await flush();
    expect(log).toEqual(['a start', 'b start']);
    b.ok(undefined);
    await two;
    expect(await three).toBe(7);
    expect(log).toEqual(['a start', 'b start', 'b end', 'c']);
  });
});

// The store's settings saves: the service takes the whole object, so each save must be built from
// what the one before it saved.
const puts: { path: string; body: any; answer: ReturnType<typeof later> }[] = [];
vi.mock('./api', () => ({
  api: vi.fn(async () => ({})),
  post: vi.fn(async () => ({})),
  inTauri: () => false,
  put: (path: string, body: any) => { const answer = later(); puts.push({ path, body, answer }); return answer.promise; },
}));
vi.mock('./logo', () => ({ syncDockIcon: async () => {} }));

describe('saveSettings', () => {
  beforeEach(() => {
    puts.length = 0;
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} }));
  });

  it('a theme pick during another save keeps that save’s change', async () => {
    const { app, saveSettings } = await import('./store.svelte');
    app.settings = { theme: 'saudi', mode: 'light', notifications: true };
    const one = saveSettings({ notifications: false });
    const two = saveSettings({ theme: 'night' });
    await flush();
    expect(puts.length).toBe(1);
    puts[0].answer.ok({ ...puts[0].body });
    await one;
    await flush();
    expect(puts.length).toBe(2);
    expect(puts[1].body).toEqual({ theme: 'night', mode: 'light', notifications: false });
    puts[1].answer.ok({ ...puts[1].body });
    await two;
    expect(app.settings).toEqual({ theme: 'night', mode: 'light', notifications: false });
  });

  it('a whole-object builder runs when its turn comes', async () => {
    const { app, saveSettings } = await import('./store.svelte');
    app.settings = { theme: 'saudi', mode: 'light' };
    const one = saveSettings({ mode: 'dark' });
    const two = saveSettings((now) => ({ ...now, theme: 'night' }));
    await flush();
    puts[0].answer.ok({ ...puts[0].body });
    await one;
    await flush();
    expect(puts[1].body).toEqual({ theme: 'night', mode: 'dark' });
    puts[1].answer.ok({ ...puts[1].body });
    await two;
  });

  it('zoom steps pressed quickly build on each other', async () => {
    const { app, zoomStep } = await import('./store.svelte');
    app.settings = { theme: 'saudi', accessibility: { zoom: 100, reduceMotion: true } };
    const steps = [zoomStep(1), zoomStep(1), zoomStep(1)];
    for (let i = 0; i < 3; i++) {
      await flush();
      puts[i].answer.ok({ ...puts[i].body });
    }
    expect(await Promise.all(steps)).toEqual([110, 125, 150]);
    expect(puts.map((p) => p.body.accessibility.zoom)).toEqual([110, 125, 150]);
    // What else was saved stays as it was.
    expect(puts.every((p) => p.body.accessibility.reduceMotion === true)).toBe(true);
    expect(app.settings.accessibility.zoom).toBe(150);
  });
});
