// Routes of second app windows (windows.ts): what a window may be opened at, and its address.
import { describe, it, expect } from 'vitest';
import { initialRoute, isViewLabel, mainRoute, windowQuery, windowRoute, windowTitle } from './windows';

describe('isViewLabel', () => {
  it('knows second windows by their own labels, never the main window’s', () => {
    for (const yes of ['view-1', 'view-42']) expect(isViewLabel(yes)).toBe(true);
    for (const no of ['main', 'main-2', 'capture', 'view', 'view-', 'view-x', 'views-1', 'view-1234567', '']) expect(isViewLabel(no)).toBe(false);
  });
});

describe('windowRoute', () => {
  it('opens a note or a chat', () => {
    expect(windowRoute({ kind: 'docs', path: 'apps/plan/README.md' })).toEqual({ kind: 'docs', path: 'apps/plan/README.md' });
    expect(windowRoute({ kind: 'chat', id: '0b6f3a52-6c0e-4c55-9d4c-2f1e0e5b7a10' })).toEqual({ kind: 'chat', id: '0b6f3a52-6c0e-4c55-9d4c-2f1e0e5b7a10' });
  });
  it('refuses anything else, as the desktop app does', () => {
    for (const bad of [
      { kind: 'docs' }, { kind: 'docs', path: '' }, { kind: 'docs', path: '/etc/passwd' }, { kind: 'docs', path: 'a/../../b.md' },
      { kind: 'docs', path: '?some words' }, { kind: 'docs', path: 'a\nb.md' }, { kind: 'docs', path: 'x'.repeat(1025) },
      { kind: 'chat' }, { kind: 'chat', id: '../x' }, { kind: 'chat', id: 'a_b' }, { kind: 'chat', id: 'abc', path: 'x.md' },
      { kind: 'settings' }, { kind: 'home' }, { kind: 'plugin', id: 'x' },
    ]) expect(windowRoute(bad)).toBeNull();
  });
});

describe('mainRoute', () => {
  it('may also send the main window to Settings or Customise', () => {
    expect(mainRoute({ kind: 'settings' })).toEqual({ kind: 'settings' });
    expect(mainRoute({ kind: 'settings', id: 'accessibility' })).toEqual({ kind: 'settings', id: 'accessibility' });
    expect(mainRoute({ kind: 'customise', id: 'automate' })).toEqual({ kind: 'customise', id: 'automate' });
    expect(mainRoute({ kind: 'chat', id: 'abc' })).toEqual({ kind: 'chat', id: 'abc' });
    expect(mainRoute({ kind: 'settings', id: 'a b' })).toBeNull();
    expect(mainRoute({ kind: 'rollback' })).toBeNull();
  });
});

describe('initialRoute and windowQuery', () => {
  it('read back what the window was opened at, in any language', () => {
    const note = { kind: 'docs', path: 'apps/خطة العمل/README & notes #1?.md' };
    expect(initialRoute(windowQuery(note))).toEqual(note);
    expect(initialRoute(windowQuery(note, true))).toEqual(note);
    expect(new URLSearchParams(windowQuery(note, true)).get('safe')).toBe('1');
    expect(initialRoute(windowQuery({ kind: 'chat', id: 'abc-1' }))).toEqual({ kind: 'chat', id: 'abc-1' });
  });
  it('the shell’s address reads the same', () => {
    // As src-tauri/src/windows.rs builds it (its test has the same path).
    expect(initialRoute('?open=docs&path=apps%2F%D8%AE%D8%B7%D8%A9%20%D8%A7%D9%84%D8%B9%D9%85%D9%84%2FREADME.md&safe=1')).toEqual({ kind: 'docs', path: 'apps/خطة العمل/README.md' });
  });
  it('is nothing for the main window or a bad address', () => {
    expect(initialRoute('')).toBeNull();
    expect(initialRoute('?safe=1')).toBeNull();
    expect(initialRoute('?open=settings')).toBeNull();
    expect(initialRoute('?open=docs&path=..%2F..%2Fx')).toBeNull();
  });
});

describe('windowTitle', () => {
  it('names the window after what it shows', () => {
    expect(windowTitle('plan.md')).toBe('plan.md — Thoughts');
    expect(windowTitle('  a\n b ')).toBe('a b — Thoughts');
    expect(windowTitle('')).toBe('Thoughts');
    expect(windowTitle('x'.repeat(200)).length).toBe(80 + ' — Thoughts'.length);
  });
});
