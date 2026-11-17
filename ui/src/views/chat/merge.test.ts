// The chat view's merge rules (merge.ts): events, fetched copies and the order replies come back in.
import { describe, it, expect } from 'vitest';
import { Fetches, applyEvent, applyPartial, emptyStream, readyForQueued, type MergeContext, type Stream } from './merge';
import type { Chat, ChatMessage, DaemonEvent } from '../../lib/types';

const live: MergeContext = { again: false, loading: false, sending: false };
const replay: MergeContext = { again: true, loading: true, sending: false };

const msg = (id: string, role: string, text = id, extra: Partial<ChatMessage> = {}): ChatMessage => ({ id, role, text, ts: '2026-11-17T09:00:00Z', ...extra });

function chatOf(over: Partial<Chat> = {}): Chat {
  return {
    schemaVersion: 1, id: 'c1', title: 'T', pinned: false, archived: false, projectId: null, kind: 'chat', provider: 'claude',
    sessionId: null, created: '', updated: '', messages: [], options: { model: null, effort: null, permissionMode: 'acceptEdits', web: false },
    miniWrap: null, miniWrapAt: null, wrappedAt: null, dirty: false, running: true, interrupted: false, costUsd: 0, meta: null, ...over,
  };
}
const stream = (over: Partial<Stream> = {}): Stream => ({ ...emptyStream(), ...over });
const delta = (text: string, seq?: number): DaemonEvent => ({ type: 'chat.delta', chatId: 'c1', text, seq } as DaemonEvent);
const think = (text: string, seq?: number): DaemonEvent => ({ type: 'chat.thinking', chatId: 'c1', text, seq });
const done = (ok = true): DaemonEvent => ({ type: 'chat.done', chatId: 'c1', ok, title: 'T', chat: {} as any });

describe('applyEvent: text pieces', () => {
  it.each([
    ['the next piece is added', stream({ text: 'Hel', seq: 1 }), delta('lo', 2), live, 'Hello', 2, false],
    ['a piece already shown is not added twice', stream({ text: 'Hello', seq: 2 }), delta('lo', 2), replay, 'Hello', 2, false],
    ['a gap asks for the service copy', stream({ text: 'Hel', seq: 1 }), delta('xx', 3), live, 'Hel', 1, true],
    ['a gap while a fetch is on its way asks nothing more', stream({ text: 'Hel', seq: 1 }), delta('xx', 3), { ...live, loading: true }, 'Hel', 1, false],
    ['a gap gone through again asks nothing', stream({ text: 'Hel', seq: 1 }), delta('xx', 3), replay, 'Hel', 1, false],
    ['an unnumbered piece (older service) is added', stream({ text: 'a', seq: 4 }), delta('b'), live, 'ab', 5, false],
  ])('%s', (_n, s, e, ctx, text, seq, reload) => {
    const fx = applyEvent(chatOf(), s, e, ctx);
    expect([s.text, s.seq, !!fx.reload]).toEqual([text, seq, reload]);
  });
});

describe('applyEvent: messages', () => {
  it('a completed answer replaces the text in progress and ends the thinking line', () => {
    const c = chatOf(); const s = stream({ text: 'Hi', seq: 1, thinking: 'hmm' });
    const fx = applyEvent(c, s, { type: 'chat.message', chatId: 'c1', message: msg('a1', 'assistant', 'Hi') }, live);
    expect([c.messages.length, s.text, s.thinking, s.seq, fx.scroll]).toEqual([1, '', '', 1, 'follow']);
  });
  it("the user's own message scrolls down whatever, and clears the suggestion", () => {
    const c = chatOf({ suggestion: 'next?' }); const s = stream({ thinking: 'hmm' });
    const fx = applyEvent(c, s, { type: 'chat.message', chatId: 'c1', message: msg('u1', 'user') }, live);
    expect([fx.scroll, c.suggestion, s.thinking]).toEqual(['force', undefined, 'hmm']);
  });
  it('a message the fetched copy has is not added again', () => {
    const c = chatOf({ messages: [msg('a1', 'assistant', 'Hi')] }); const s = stream();
    const fx = applyEvent(c, s, { type: 'chat.message', chatId: 'c1', message: msg('a1', 'assistant', 'Hi') }, replay);
    expect([c.messages.length, fx.scroll]).toEqual([1, undefined]);
  });
  it('a recovered failure is never put back over its quiet note', () => {
    const c = chatOf({ messages: [msg('e1', 'system', 'recovered')] });
    applyEvent(c, stream(), { type: 'chat.message', chatId: 'c1', message: msg('e1', 'error', 'lost', { tool: { code: 'sessionLost' } }) }, replay);
    expect(c.messages[0].text).toBe('recovered');
  });
  it('a removed and a changed message', () => {
    const c = chatOf({ messages: [msg('u1', 'user'), msg('a1', 'assistant', 'old')] });
    applyEvent(c, stream(), { type: 'chat.message.changed', chatId: 'c1', message: msg('a1', 'assistant', 'new') }, live);
    applyEvent(c, stream(), { type: 'chat.message.removed', chatId: 'c1', messageId: 'u1' }, live);
    expect(c.messages.map((m) => m.text)).toEqual(['new']);
  });
});

describe('applyEvent: end and updates', () => {
  it('a live end clears the stream, fetches again and is announced', () => {
    const s = stream({ text: 'x', seq: 3, thinking: 't', thinkSeq: 2 });
    expect(applyEvent(chatOf(), s, done(), live)).toEqual({ reload: true, ended: true });
    expect(s).toEqual(emptyStream());
  });
  it('an end gone through again over a copy from after it does nothing', () => {
    const s = stream({ text: 'x', seq: 3 });
    expect(applyEvent(chatOf({ running: false }), s, done(), replay)).toEqual({});
    expect(s.text).toBe('x');
  });
  it('an end gone through again over a copy from before it fetches again, unannounced', () => {
    expect(applyEvent(chatOf({ running: true }), stream(), done(), replay)).toEqual({ reload: true, ended: false });
  });
  it('an update keeps the chat working while a send is on its way', () => {
    const c = chatOf({ running: true, meta: { phase: 'preview' } });
    applyEvent(c, stream(), { type: 'chat.updated', chat: { running: false, title: 'New', phase: 'saving' } as any }, { ...live, sending: true });
    expect([c.running, c.title, c.meta?.phase]).toEqual([true, 'New', 'saving']);
    applyEvent(c, stream(), { type: 'chat.updated', chat: { running: false, title: 'Old', phase: null } as any }, replay);
    expect(c.title).toBe('New');
  });
});

describe('applyPartial', () => {
  it('a chat at rest has nothing in progress', () => {
    const s = stream({ text: 'x', seq: 2 });
    applyPartial({ running: false }, s, { text: 'y', seq: 9, thinking: '' });
    expect(s).toEqual(emptyStream());
  });
  it('a running chat takes the copy, and keeps the screen when an older service sends none', () => {
    const s = stream({ text: 'x', seq: 2 });
    applyPartial({ running: true }, s, undefined);
    expect(s.text).toBe('x');
    applyPartial({ running: true }, s, { text: 'abc', seq: 3, thinking: 'hm', thinkingSeq: 4 });
    expect(s).toEqual({ text: 'abc', seq: 3, thinking: 'hm', thinkSeq: 4 });
  });
});

describe('Fetches: overlapping fetches', () => {
  it('a reply older than the copy on screen is dropped', () => {
    const f = new Fetches();
    const a = f.begin(); // on a gap
    const b = f.begin(); // on the end of the answer
    expect(f.accept(b)).toBe(true);
    expect(f.accept(a)).toBe(false);
    f.end(); f.end();
    expect(f.loading).toBe(false);
  });
  it('replies in order are both used', () => {
    const f = new Fetches();
    const a = f.begin(); const b = f.begin();
    expect([f.accept(a), f.accept(b)]).toEqual([true, true]);
  });
  it('events are held until no fetch is on its way, and every copy goes through them', () => {
    const f = new Fetches();
    f.hold(done()); // nothing on its way: not held
    const a = f.begin(); const b = f.begin();
    f.hold(delta('x', 1));
    f.accept(a); expect(f.replay().length).toBe(1); f.end();
    f.hold(delta('y', 2));
    f.accept(b); expect(f.replay().length).toBe(2); f.end();
    expect(f.replay()).toEqual([]);
    expect(f.newest).toBe(b);
  });
  it('a finished chat is not left "running" by a stale copy (the whole sequence)', () => {
    // Fetch A starts on a gap; the answer ends (B starts); B's copy comes back first, then A's.
    const f = new Fetches(); const s = stream({ text: 'Hel', seq: 1 });
    let chat = chatOf({ running: true });
    const a = f.begin();
    const fx = applyEvent(chat, s, done(), { ...live, loading: f.loading }); f.hold(done());
    expect(fx.reload).toBe(true);
    const b = f.begin();
    const copyB = chatOf({ running: false, messages: [msg('a1', 'assistant', 'Hello')] });
    const copyA = chatOf({ running: true, partial: { text: 'Hel', seq: 1, thinking: '' } });
    for (const [t, c] of [[b, copyB], [a, copyA]] as const) {
      if (f.accept(t)) { chat = c; applyPartial(c, s, c.partial); for (const e of f.replay()) applyEvent(chat, s, e, { ...replay }); }
      f.end();
    }
    expect([chat.running, s.text, chat.messages.length]).toEqual([false, '', 1]);
  });
});

describe('readyForQueued: after a reconnect', () => {
  it.each([
    ['still running', { running: true }, false, false],
    ['answered', { messages: [msg('u', 'user'), msg('t', 'tool'), msg('a', 'assistant')] }, false, true],
    ['answered, then a tool result', { messages: [msg('a', 'assistant'), msg('r', 'tool_result')] }, false, true],
    ['stopped', { messages: [msg('a', 'assistant'), msg('s', 'system')] }, false, false],
    ['stopped, but "send now" was asked', { messages: [msg('s', 'system')] }, true, true],
    ['failed', { messages: [msg('e', 'error')] }, false, false],
    ['no answer yet', { messages: [msg('a0', 'assistant'), msg('u', 'user')] }, false, false],
    ['cut off by a restart', { interrupted: true, messages: [msg('a', 'assistant')] }, false, false],
  ])('%s', (_n, over, asked, want) => {
    expect(readyForQueued(chatOf({ running: false, ...over }), asked)).toBe(want);
  });
});
