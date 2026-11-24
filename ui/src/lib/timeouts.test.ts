// The request time limit (timeouts.ts) and how api() reports a request that never got an answer.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_TIMEOUT_MS, deadline, timeoutFor } from './timeouts';
import words from './i18n/ui-core';

// The real word list pulls in the whole app; the key is enough to see which sentence was chosen.
vi.mock('./i18n', () => ({ t: (k: string) => `t:${k}` }));

describe('timeoutFor', () => {
  it.each<[string, string]>([
    ['GET', '/status'],
    ['GET', '/chats'],
    ['PUT', '/settings?confirm=bypassPermissions'],
    ['POST', '/todos/toggle'],
    ['GET', '/wrap'],
    ['GET', '/wrap/abc/changes'],
    ['GET', '/git'],
    ['POST', '/chats/abc/stop'],
  ])('%s %s has the default limit', (m, p) => {
    expect(timeoutFor(m, p)).toBe(DEFAULT_TIMEOUT_MS);
  });

  it.each<[string, string]>([
    ['POST', '/chats/abc-123/send'],
    ['POST', '/git/sync'],
    ['POST', '/git/join'],
    ['POST', '/wrap'],
    ['POST', '/wrap/w1/apply'],
    ['POST', '/wrap/w1/save'],
    ['POST', '/reorganize'],
    ['POST', '/reorganize/r1/keep'],
    ['POST', '/library/import'],
    ['POST', '/library/import-folder'],
    ['POST', '/library/item/transcript'],
    ['POST', '/move/export'],
    ['POST', '/move/import'],
    ['GET', '/layer/export'],
    ['POST', '/layer/import'],
    ['POST', '/layer/pending/accept'],
    ['POST', '/updates/resolve'],
    ['POST', '/ai/claude/provider/test'],
    ['POST', '/ai/codex/provider/test'],
    ['PUT', '/ai/codex/provider'],
    ['post', '/library/history/repair?x=1'],
  ])('%s %s has no limit', (m, p) => {
    expect(timeoutFor(m, p)).toBe(0);
  });

  it('honours an explicit limit, 0 included', () => {
    expect(timeoutFor('GET', '/status', 5000)).toBe(5000);
    expect(timeoutFor('GET', '/status', 0)).toBe(0);
    expect(timeoutFor('POST', '/git/sync', 90_000)).toBe(90_000);
  });
});

describe('deadline', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('aborts after the limit and says so', () => {
    const d = deadline(1000);
    vi.advanceTimersByTime(999);
    expect(d.signal!.aborted).toBe(false);
    vi.advanceTimersByTime(1);
    expect(d.signal!.aborted).toBe(true);
    expect(d.timedOut()).toBe(true);
  });

  it('never fires once cleared, and 0 means no signal at all', () => {
    const d = deadline(1000);
    d.clear();
    vi.advanceTimersByTime(5000);
    expect(d.signal!.aborted).toBe(false);
    expect(d.timedOut()).toBe(false);
    expect(deadline(0).signal).toBeUndefined();
  });
});

describe('api() without an answer', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    // Browser-preview connection details; a socket that never opens.
    history.replaceState(null, '', '/?port=1&token=test');
    vi.stubGlobal('WebSocket', Object.assign(class { readyState = 0; close() {} }, { OPEN: 1, CONNECTING: 0 }));
    // A service that takes the request and never answers: only the abort ends it.
    vi.stubGlobal('fetch', (_url: string, init: RequestInit) => new Promise((_ok, fail) => {
      init.signal?.addEventListener('abort', () => fail(new DOMException('aborted', 'AbortError')));
    }));
  });
  afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

  it('gives up after the default limit with a status-0 TIMEOUT error', async () => {
    const { api } = await import('./api');
    const p = api('/status');
    const seen = p.catch((e) => e);
    await vi.advanceTimersByTimeAsync(DEFAULT_TIMEOUT_MS);
    const e = await seen;
    expect(e.status).toBe(0);
    expect(e.code).toBe('TIMEOUT');
    expect(e.message).toBe('t:uicore.timeout');
    expect(words.en['uicore.timeout']).toBeTruthy();
    expect(words.ar['uicore.timeout']).toBeTruthy();
  });

  it('keeps waiting on long work', async () => {
    const { api } = await import('./api');
    let settled = false;
    api('/git/sync', { method: 'POST', body: {} }).catch(() => {}).finally(() => { settled = true; });
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(settled).toBe(false);
  });
});
