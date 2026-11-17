import { describe, it, expect, vi, afterEach } from 'vitest';
import { HEARTBEAT, TTL, claim, heldByOther, lockKey, makePresence, readLock, release, type Store } from './editLock';

function memStore(): Store & { map: Map<string, string> } {
  const map = new Map<string, string>();
  return { map, getItem: (k) => map.get(k) ?? null, setItem: (k, v) => void map.set(k, v), removeItem: (k) => void map.delete(k) };
}
/** A tiny event target standing in for `window` (storage, pagehide…). */
function fakeWin() {
  const ls = new Map<string, Set<(e: any) => void>>();
  return {
    addEventListener: (t: string, f: any) => { (ls.get(t) ?? ls.set(t, new Set()).get(t)!).add(f); },
    removeEventListener: (t: string, f: any) => { ls.get(t)?.delete(f); },
    fire: (t: string, e: any = {}) => { for (const f of ls.get(t) ?? []) f(e); },
  };
}

const K = lockKey('/lib', 'notes/a.md');

describe('edit marks', () => {
  it('one window holds a note; another sees it and cannot take it', () => {
    const s = memStore();
    expect(claim(s, K, 'A', 1000)).toBe(true);
    expect(heldByOther(s, K, 'B', 2000)).toBe(true);
    expect(heldByOther(s, K, 'A', 2000)).toBe(false);
    expect(claim(s, K, 'B', 2000)).toBe(false);
    expect(readLock(s, K, 2000)?.win).toBe('A');
  });

  it('a mark of a window that stopped renewing (crashed) goes stale and can be taken', () => {
    const s = memStore();
    claim(s, K, 'A', 1000);
    expect(heldByOther(s, K, 'B', 1000 + TTL)).toBe(true);
    expect(heldByOther(s, K, 'B', 1000 + TTL + 1)).toBe(false);
    expect(claim(s, K, 'B', 1000 + TTL + 1)).toBe(true);
    expect(readLock(s, K, 1000 + TTL + 1)?.win).toBe('B');
  });

  it('release only drops the window’s own mark', () => {
    const s = memStore();
    claim(s, K, 'A', 1000);
    release(s, K, 'B');
    expect(readLock(s, K, 1000)?.win).toBe('A');
    release(s, K, 'A');
    expect(readLock(s, K, 1000)).toBeNull();
  });

  it('broken or foreign values count as no mark', () => {
    const s = memStore();
    s.setItem(K, '{not json');
    expect(readLock(s, K, 0)).toBeNull();
    s.setItem(K, JSON.stringify({ win: 3, at: 'x' }));
    expect(claim(s, K, 'B', 0)).toBe(true);
  });

  it('notes and libraries are told apart', () => {
    const s = memStore();
    claim(s, K, 'A', 0);
    expect(heldByOther(s, lockKey('/lib', 'notes/b.md'), 'B', 0)).toBe(false);
    expect(heldByOther(s, lockKey('/other', 'notes/a.md'), 'B', 0)).toBe(false);
  });
});

describe('presence', () => {
  afterEach(() => vi.useRealTimers());

  it('holds while editing, renews, and lets go when done', () => {
    vi.useFakeTimers();
    let t = 0;
    const s = memStore(), w = fakeWin();
    const a = makePresence({ store: s, me: 'A', now: () => t, win: w });
    const b = makePresence({ store: s, me: 'B', now: () => t, win: w });
    const stop = a.hold('/lib', 'notes/a.md');
    expect(b.elsewhere('/lib', 'notes/a.md')).toBe(true);
    expect(a.elsewhere('/lib', 'notes/a.md')).toBe(false);
    // Renewed on every heartbeat, so it never goes stale while the window lives.
    for (let i = 0; i < 40; i++) { t += HEARTBEAT; vi.advanceTimersByTime(HEARTBEAT); }
    expect(t).toBeGreaterThan(TTL);
    expect(b.elsewhere('/lib', 'notes/a.md')).toBe(true);
    stop();
    expect(b.elsewhere('/lib', 'notes/a.md')).toBe(false);
    stop(); // twice is harmless
  });

  it('a second window waits; it is told when it gets the note, and when it loses it', () => {
    vi.useFakeTimers();
    let t = 0;
    const s = memStore(), w = fakeWin();
    const a = makePresence({ store: s, me: 'A', now: () => t, win: w });
    const b = makePresence({ store: s, me: 'B', now: () => t, win: w });
    const stopA = a.hold('/lib', 'n.md');
    const seen: boolean[] = [];
    const stopB = b.hold('/lib', 'n.md', (lost) => seen.push(lost));
    expect(seen).toEqual([true]);
    expect(readLock(s, lockKey('/lib', 'n.md'), t)?.win).toBe('A'); // B did not overwrite A's mark
    stopA();
    t += HEARTBEAT; vi.advanceTimersByTime(HEARTBEAT);
    expect(seen.at(-1)).toBe(false);
    expect(a.elsewhere('/lib', 'n.md')).toBe(true);
    stopB();
  });

  it('closing or reloading the window takes its marks away', () => {
    const s = memStore(), w = fakeWin();
    const a = makePresence({ store: s, me: 'A', now: () => 0, win: w });
    a.hold('/lib', 'n.md');
    w.fire('pagehide');
    expect(readLock(s, lockKey('/lib', 'n.md'), 0)).toBeNull();
  });

  it('another window’s change reaches subscribers at once', () => {
    const s = memStore(), w = fakeWin();
    const a = makePresence({ store: s, me: 'A', now: () => 0, win: w });
    const fn = vi.fn();
    const off = a.subscribe(fn);
    w.fire('storage', { key: 'thoughts.docs.open' });
    expect(fn).not.toHaveBeenCalled();
    w.fire('storage', { key: lockKey('/lib', 'n.md') });
    expect(fn).toHaveBeenCalledTimes(1);
    off();
    w.fire('storage', { key: lockKey('/lib', 'n.md') });
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('the same note held twice in one window stays held until both let go', () => {
    const s = memStore();
    const a = makePresence({ store: s, me: 'A', now: () => 0, win: null });
    const one = a.hold('/lib', 'n.md'), two = a.hold('/lib', 'n.md');
    one();
    expect(heldByOther(s, lockKey('/lib', 'n.md'), 'B', 0)).toBe(true);
    two();
    expect(heldByOther(s, lockKey('/lib', 'n.md'), 'B', 0)).toBe(false);
  });
});
