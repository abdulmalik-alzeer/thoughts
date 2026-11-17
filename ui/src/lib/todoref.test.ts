// Which todo an open editor / date picker / move bar belongs to across reloads (todoref.ts).
// Keyed by line number alone (the old behaviour), a reload that puts another todo on that line
// would point the open box — and its save — at that other todo. These tests pin the fix.
import { describe, it, expect } from 'vitest';
import { isAt, reanchor, refOf, type TodoRef } from './todoref';

const F = 'TODO.md';
const item = (line: number, text: string) => ({ line, text });
const list = (file: string, ...texts: (string | null)[]) => ({ file, items: texts.flatMap((t, i) => (t === null ? [] : [item(i, t)])) });

describe('refOf / isAt', () => {
  const open: TodoRef = refOf(F, item(2, 'Pay rent'));

  it('captures file, line and text', () => {
    expect(open).toEqual({ file: F, line: 2, text: 'Pay rent' });
  });
  it('matches the same todo', () => {
    expect(isAt(open, F, item(2, 'Pay rent'))).toBe(true);
  });
  it('does not match another todo that now sits on the same line', () => {
    // A task was added above: line 2 is now a different todo. Line-only keys matched it.
    expect(isAt(open, F, item(2, 'Call the bank'))).toBe(false);
  });
  it('does not match the same line in another list', () => {
    expect(isAt(open, 'apps/x/todo.md', item(2, 'Pay rent'))).toBe(false);
  });
  it('nothing open matches nothing', () => {
    expect(isAt(null, F, item(2, 'Pay rent'))).toBe(false);
    expect(isAt(undefined, F, item(2, 'Pay rent'))).toBe(false);
  });
});

describe('reanchor', () => {
  const open: TodoRef = { file: F, line: 1, text: 'Pay rent' };

  it('keeps the same ref when nothing changed', () => {
    expect(reanchor(open, [list(F, 'Buy milk', 'Pay rent')])).toBe(open);
  });
  it('follows the todo when a task is added above it', () => {
    expect(reanchor(open, [list(F, 'New task', 'Buy milk', 'Pay rent')])).toEqual({ file: F, line: 2, text: 'Pay rent' });
  });
  it('follows the todo when a task above it is removed', () => {
    expect(reanchor(open, [list(F, 'Pay rent', 'Buy milk')])).toEqual({ file: F, line: 0, text: 'Pay rent' });
  });
  it('closes when the todo was changed elsewhere, even though its line still has a todo', () => {
    expect(reanchor(open, [list(F, 'Buy milk', 'Pay rent @due(2026-12-01)')])).toBeNull();
  });
  it('closes when the todo was deleted', () => {
    expect(reanchor(open, [list(F, 'Buy milk')])).toBeNull();
  });
  it('closes when its list is gone', () => {
    expect(reanchor(open, [list('apps/x/todo.md', 'Buy milk', 'Pay rent')])).toBeNull();
  });
  it('closes rather than guess between two todos with the same text', () => {
    expect(reanchor(open, [list(F, 'New task', 'Buy milk', 'Pay rent', 'Pay rent')])).toBeNull();
  });
  it('keeps a todo that has a twin when it is still on its own line', () => {
    expect(reanchor(open, [list(F, 'Buy milk', 'Pay rent', 'Pay rent')])).toBe(open);
  });
  it('looks only in its own list', () => {
    const lists = [list('apps/x/todo.md', 'Pay rent'), list(F, 'A', 'B', 'Pay rent')];
    expect(reanchor(open, lists)).toEqual({ file: F, line: 2, text: 'Pay rent' });
  });
  it('nothing open stays closed', () => {
    expect(reanchor(null, [list(F, 'Pay rent')])).toBeNull();
  });
});
