// The built-in pages' guards yield to full access (lib/fullAccess.ts) and stay as before while it is off.
import { describe, it, expect, beforeEach } from 'vitest';

window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;

const { setPages, sectionPlan, pageSwap } = await import('./pages.svelte');
const { layoutNav } = await import('./navLayout');
const { app } = await import('./store.svelte');

const view = (id: string) => ({ id, title: id, plugin: 'x.js', render: () => () => {} });
const settingsSections = ['general', 'accessibility', 'ai', 'usage', 'git', 'privacy', 'modules', 'daemon', 'appLayer', 'maintenance'];
const items = ['home', 'chats', 'customise'].map((id) => ({ id, icon: id }));

describe.each([false, true])('full access %s', (full) => {
  beforeEach(() => {
    app.safeMode = false; app.homeWidgets = []; app.pluginViews = [view('x.js:mine') as any];
    app.settings = { fullAccess: { on: full } };
  });

  it('locked Settings sections, and replacing Settings or an out-of-reach page', () => {
    setPages({ settings: { sections: { hidden: ['privacy'] }, replace: 'x.js:mine' }, customise: { replace: 'x.js:mine' } });
    const keys = sectionPlan('settings', settingsSections).map((e) => e.key);
    expect(keys.includes('privacy')).toBe(!full);
    expect(pageSwap('settings')?.id).toBe(full ? 'x.js:mine' : undefined);
    expect(pageSwap('customise')?.id).toBe(full ? 'x.js:mine' : undefined);
  });

  it('the sidebar: never-hidden entries and Customise\'s own look', () => {
    const r = layoutNav(items, { hidden: ['home', 'customise'], order: [], labels: {}, icons: { customise: 'folder' }, groups: [] }, full);
    expect(r.flat.map((x) => x.id)).toEqual(full ? ['chats'] : ['home', 'chats', 'customise']);
    if (!full) expect(r.flat.find((x) => x.id === 'customise')?.icon).toBe('customise');
  });
});

describe('route context of a replacement page', () => {
  it('is filtered by permission while off and whole with full access', async () => {
    const { pageCtx } = await import('./layer');
    const p = { perms: new Set<string>(), read: [] as string[] };
    const ctx = { replaces: 'chat', route: { kind: 'chat', id: 'abc', path: 'ideas/x.md' } };
    app.settings = { fullAccess: { on: false } };
    expect(pageCtx(p, ctx).route).toEqual({ kind: 'chat', id: null, path: null });
    app.settings = { fullAccess: { on: true } };
    expect(pageCtx(p, ctx).route).toEqual({ kind: 'chat', id: 'abc', path: 'ideas/x.md' });
  });
});
