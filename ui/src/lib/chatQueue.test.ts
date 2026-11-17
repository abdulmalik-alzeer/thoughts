// The message waiting for an answer to finish (chatQueue.svelte.ts): when it goes.
import { describe, it, expect, vi, beforeEach } from 'vitest';

const listeners: ((e: any) => void)[] = [];
const chats: Record<string, any> = {};
const post = vi.fn(async (_path: string, _body?: any) => ({}));
vi.mock('./api', () => ({
  ApiError: class extends Error {},
  on: (fn: (e: any) => void) => { listeners.push(fn); return () => {}; },
  post: (path: string, body?: any) => post(path, body),
  api: async (path: string) => { const c = chats[path.split('/').pop()!]; if (!c) throw new Error('gone'); return c; },
}));
vi.mock('./errors', () => ({ failed: () => {} }));
vi.mock('./i18n', () => ({ t: (k: string) => k }));

const { enqueue, waiting } = await import('./chatQueue.svelte');
const emit = (e: any) => listeners.forEach((l) => l(e));
const settle = () => new Promise((r) => setTimeout(r, 0));
const q = (text: string) => ({ text, files: [], mentions: [] });
const at = (role: string) => ({ id: role, role, text: role, ts: '' });

describe('chatQueue after a reconnect', () => {
  beforeEach(() => { post.mockClear(); for (const k of Object.keys(waiting)) delete waiting[k]; });

  it('a message goes when its answer finished while the connection was down', async () => {
    chats.a = { running: false, interrupted: false, messages: [at('user'), at('assistant')] };
    enqueue('a', q('next'));
    emit({ type: 'resync' });
    await settle();
    expect(post).toHaveBeenCalledWith('/chats/a/send', expect.objectContaining({ text: 'next' }));
    expect(waiting.a).toBeUndefined();
  });

  it('it keeps waiting while the answer is still being written, or when it was stopped', async () => {
    chats.b = { running: true, interrupted: false, messages: [at('user')] };
    chats.c = { running: false, interrupted: false, messages: [at('user'), at('system')] };
    enqueue('b', q('one')); enqueue('c', q('two'));
    emit({ type: 'resync' });
    await settle();
    expect(post).not.toHaveBeenCalled();
    expect([waiting.b?.text, waiting.c?.text]).toEqual(['one', 'two']);
  });

  it('a chat that cannot be reached keeps its message', async () => {
    enqueue('missing', q('kept'));
    emit({ type: 'resync' });
    await settle();
    expect(waiting.missing?.text).toBe('kept');
  });
});
