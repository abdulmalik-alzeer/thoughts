// Editing a sent message and branching a chat (branch.ts).
import { describe, it, expect } from 'vitest';
import { branchInfo, branchesOf, canBranch, editKind, editNotes, fileName, jumpLater, madeKey, origin, takeJump } from './branch';

const m = (id: string, role: string, extra: any = {}) => ({ id, role, text: id, ts: '2026-11-10T10:00:00Z', ...extra });
const chat = (messages: any[], extra: any = {}) => ({ kind: 'chat', provider: 'claude', messages, ...extra });

describe('editKind', () => {
  const c = chat([m('u1', 'user'), m('a1', 'assistant'), m('u2', 'user'), m('t', 'tool'), m('a2', 'assistant'), m('n', 'system')]);
  it('the last message of the user is edited in place, an earlier one in a branch', () => {
    expect(editKind(c, 'u2')).toBe('inplace');
    expect(editKind(c, 'u1')).toBe('branch');
  });
  it('nothing else is edited', () => {
    expect(editKind(c, 'a1')).toBeNull();
    expect(editKind(c, 'gone')).toBeNull();
    expect(editKind({ ...c, kind: 'customize' }, 'u2')).toBeNull();
    expect(editKind(null, 'u2')).toBeNull();
  });
});

describe('canBranch', () => {
  it('from the user’s messages and the answers of an ordinary chat', () => {
    const c = chat([]);
    expect(canBranch(c, m('u', 'user'))).toBe(true);
    expect(canBranch(c, m('a', 'assistant'))).toBe(true);
    expect(canBranch(c, m('t', 'tool'))).toBe(false);
    expect(canBranch({ ...c, kind: 'wrap' }, m('a', 'assistant'))).toBe(false);
  });
});

describe('editNotes', () => {
  it('says when the message was saved already and when its answer changed the library', () => {
    const c = chat([m('u1', 'user'), m('a1', 'assistant'), m('c1', 'changes', { tool: { total: 2 } })], { wrappedAt: '2026-11-11T00:00:00Z' });
    expect(editNotes(c, 'u1')).toEqual({ saved: true, changes: true });
    expect(editNotes({ ...c, wrappedAt: '2026-11-09T00:00:00Z' }, 'u1').saved).toBe(false);
    expect(editNotes({ ...c, wrappedAt: null }, 'u1').saved).toBe(false);
  });
  it('changes already undone, or none, are not mentioned', () => {
    expect(editNotes(chat([m('u1', 'user'), m('c1', 'changes', { tool: { total: 2, undone: true } })]), 'u1').changes).toBe(false);
    expect(editNotes(chat([m('u1', 'user'), m('c1', 'changes', { tool: { total: 0 } })]), 'u1').changes).toBe(false);
    expect(editNotes(chat([m('c0', 'changes', { tool: { total: 1 } }), m('u1', 'user')]), 'u1').changes).toBe(false);
  });
});

describe('a branch and its original', () => {
  const meta = { branch: { from: 'o', fromTitle: 'Shop', at: 'a1', atRole: 'assistant', atText: 'Candles.', before: false, how: 'fork' } };
  it('reads where a branch came from', () => {
    expect(branchInfo(meta)?.from).toBe('o');
    expect(branchInfo({})).toBeNull();
    expect(branchInfo(null)).toBeNull();
    expect(branchInfo({ branch: { from: 3 } })).toBeNull();
  });
  it('names the original by its title now, or says it is gone', () => {
    const info = branchInfo(meta)!;
    expect(origin(info, [{ id: 'o', title: 'Shop plans' }])).toEqual({ exists: true, title: 'Shop plans' });
    expect(origin(info, [])).toEqual({ exists: false, title: 'Shop' });
  });
  it('lists the branches that still exist, newest first, once each', () => {
    const list = { branches: [{ id: 'b1' }, { id: 'b2' }, { id: 'gone' }, { id: 'b1' }] };
    expect(branchesOf(list, [{ id: 'b1', title: 'One' }, { id: 'b2', title: 'Two' }])).toEqual([{ id: 'b1', title: 'One' }, { id: 'b2', title: 'Two' }]);
    expect(branchesOf({}, [])).toEqual([]);
  });
  it('says how the conversation reaches the assistant', () => {
    expect(madeKey('fork', 'claude')).toBe('chatfBranch.made.fork');
    expect(madeKey('replay', 'codex')).toBe('chatfBranch.made.replayCodex');
    expect(madeKey('replay', 'claude')).toBe('chatfBranch.made.replay');
    expect(madeKey('summary', 'codex')).toBe('chatfBranch.made.summary');
    expect(madeKey('none', 'claude')).toBe('chatfBranch.made.none');
  });
  it('remembers a message to show once, for the chat it is in', () => {
    jumpLater('o', 'a1');
    expect(takeJump('x')).toBeUndefined();
    expect(takeJump('o')).toBe('a1');
    expect(takeJump('o')).toBeUndefined();
  });
  it('names a file by its last part', () => {
    expect(fileName('/x/attachments/c/prices (2).txt')).toBe('prices (2).txt');
    expect(fileName('C:\\x\\a.png')).toBe('a.png');
  });
});
