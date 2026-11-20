// What a page standing in for a built-in page is told about the route (lib/layer.ts `pageCtx`):
// never more than its permissions let it read.
import { describe, it, expect } from 'vitest';

window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;
const { pageCtx } = await import('./layer');
const { app } = await import('./store.svelte');

const plugin = (perms: string[], read: string[] = []) => ({ perms: new Set(perms), read });

describe('pageCtx', () => {
  it('gives the kind only, without permissions', () => {
    expect(pageCtx(plugin([]), { replaces: 'docs', route: { kind: 'docs', id: null, path: 'clients/acme.md' } })).toEqual({ replaces: 'docs', route: { kind: 'docs', id: null, path: null } });
    expect(pageCtx(plugin(['todos:read']), { replaces: 'chats', route: { kind: 'chat', id: 'abc', path: null } }).route.id).toBeNull();
  });
  it('gives a file with library:read, never a private one, never outside the read folders', () => {
    app.settings = { ...(app.settings ?? {}), privatePaths: ['me'] } as any;
    expect(pageCtx(plugin(['library:read']), { route: { kind: 'docs', id: null, path: 'clients/acme.md' } }).route.path).toBe('clients/acme.md');
    expect(pageCtx(plugin(['library:read']), { route: { kind: 'docs', id: null, path: 'me/pay.md' } }).route.path).toBeNull();
    expect(pageCtx(plugin(['library:read'], ['journal/']), { route: { kind: 'docs', id: null, path: 'clients/acme.md' } }).route.path).toBeNull();
    expect(pageCtx(plugin(['library:read']), { route: { kind: 'docs', id: null, path: '../etc/passwd' } }).route.path).toBeNull();
  });
  it('a chat id only with chats:read; a mode word always', () => {
    expect(pageCtx(plugin(['chats:read']), { route: { kind: 'chat', id: 'abc', path: null } }).route.id).toBe('abc');
    expect(pageCtx(plugin([]), { route: { kind: 'todos', id: 'calendar', path: null } }).route.id).toBe('calendar');
    expect(pageCtx(plugin([]), { route: { kind: 'item', id: 'clients/acme/README.md', path: null } }).route.id).toBeNull();
  });
});
