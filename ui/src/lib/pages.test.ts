// The sections of the app's own pages (`pages.json` `<page>.sections`) and a page replaced as a
// whole: what goes where, what falls back, and what is said when something is not there.
import { describe, it, expect, beforeEach } from 'vitest';

window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;

const { setPages, sectionPlan, pageWarnings, pageSwap, swapState, showOriginal, useSwap, keptBuiltin, REFERENCE } = await import('./pages.svelte');
const { app } = await import('./store.svelte');

const w = (id: string) => ({ id, title: id, plugin: 'hours.js', render: () => () => {} });
const keys = (e: { key: string; size: string; instead?: string }[]) => e.map((x) => x.key + (x.size !== 'full' ? '@' + x.size : '') + (x.instead ? '<' + x.instead : ''));

describe('sections', () => {
  beforeEach(() => { app.safeMode = false; app.homeWidgets = []; app.pluginViews = []; setPages(null); });

  it('without a customisation a page is drawn as it always was', () => {
    expect(keys(sectionPlan('todos', ['tools', 'quickAdd', 'lists']))).toEqual(['tools', 'quickAdd', 'lists']);
  });

  it('reorders, hides and resizes; what is not listed keeps its usual place after', () => {
    setPages({ todos: { sections: { order: ['lists'], hidden: ['quickAdd'], size: { lists: 'half', tools: 'half' } } } });
    expect(keys(sectionPlan('todos', ['tools', 'quickAdd', 'lists']))).toEqual(['lists@half', 'tools@half']);
  });

  it('replaces one section with an add-on box, and gives the section back when the box is missing', () => {
    setPages({ todos: { sections: { replace: { quickAdd: 'widget:add' } } } });
    expect(keys(sectionPlan('todos', ['tools', 'quickAdd', 'lists']))).toEqual(['tools', 'quickAdd', 'lists']);
    app.homeWidgets = [w('add')];
    expect(keys(sectionPlan('todos', ['tools', 'quickAdd', 'lists']))).toEqual(['tools', 'quickAdd<quickAdd', 'lists']);
    expect(sectionPlan('todos', ['tools', 'quickAdd', 'lists'])[1].box?.id).toBe('add');
  });

  it('inserts boxes before or after a section and places boxes named in the order', () => {
    app.homeWidgets = [w('a'), w('b'), w('c')];
    setPages({ todos: { sections: { order: ['widget:c', 'tools'], insert: [{ box: 'widget:a', before: 'lists' }, { box: 'widget:b', after: 'tools' }], size: { 'widget:c': 'third' } } } });
    expect(keys(sectionPlan('todos', ['tools', 'quickAdd', 'lists']))).toEqual(['widget:c@third', 'tools', 'widget:b', 'quickAdd', 'widget:a', 'lists']);
  });

  it('ignores names this version does not have, and says so (an update can never silently break a page)', () => {
    setPages({ todos: { sections: { order: ['gone', 'lists'], hidden: ['vanished'] }, hidden: ['oldPart'] } });
    expect(keys(sectionPlan('todos', ['tools', 'quickAdd', 'lists']))).toEqual(['lists', 'tools', 'quickAdd']);
    expect(pageWarnings('todos')).toEqual(['oldPart', 'gone', 'vanished']);
  });

  it('never hides or replaces a section the reference keeps', () => {
    for (const id of REFERENCE.pages.settings.locked ?? []) {
      setPages({ settings: { sections: { hidden: [id], replace: { [id]: 'widget:x' } } } });
      app.homeWidgets = [w('x')];
      expect(sectionPlan('settings', REFERENCE.pages.settings.sections).find((e) => e.id === id)?.box).toBeUndefined();
    }
  });

  it('a renamed id keeps working', () => {
    REFERENCE.renamed.oldLists = 'lists';
    try {
      setPages({ todos: { sections: { order: ['oldLists'] } } });
      expect(keys(sectionPlan('todos', ['tools', 'quickAdd', 'lists']))[0]).toBe('lists');
      expect(pageWarnings('todos')).toEqual([]);
    } finally { delete REFERENCE.renamed.oldLists; }
  });
});

describe('a page replaced as a whole', () => {
  const view = { id: 'board.js:board', short: 'board', title: 'Board', plugin: 'board.js', render: () => () => {} };
  beforeEach(() => { app.safeMode = false; app.pluginViews = []; (app as any).addonFailures = {}; (app as any).layerOff = []; (app as any).layerItems = []; setPages(null); });

  it('shows the add-on page while it is there, and the app’s own page (saying why) when it is not', () => {
    setPages({ todos: { replace: 'board.js:board' } });
    // Its add-on is not in the layer at all: the app's page, and why.
    expect(swapState('todos')?.state).toBe('failed');
    expect(swapState('todos')?.reason).toContain('board.js');
    // The add-on is there but never added that page: missing.
    (app as any).layerItems = [{ name: 'board.js', kind: 'plugin' }];
    expect(swapState('todos')?.state).toBe('missing');
    app.pluginViews = [view];
    expect(pageSwap('todos')?.id).toBe('board.js:board');
    expect(swapState('todos')).toBeNull();
    // It failed to draw: the app's page, with the reason.
    showOriginal('todos', 'boom');
    expect(pageSwap('todos')).toBeUndefined();
    expect(swapState('todos')).toMatchObject({ state: 'failed', reason: 'boom' });
    useSwap('todos');
    expect(pageSwap('todos')?.id).toBe('board.js:board');
  });

  it('the add-on crashed: its error is the reason', () => {
    (app as any).layerItems = [{ name: 'board.js', kind: 'plugin' }];
    (app as any).addonFailures = { 'board.js': 'TypeError: x is undefined' };
    setPages({ todos: { replace: 'board.js:board' } });
    expect(swapState('todos')).toMatchObject({ state: 'failed', reason: 'TypeError: x is undefined' });
  });

  it('kept as the app made it, per page, and never in safe mode', () => {
    app.pluginViews = [view];
    setPages({ todos: { replace: 'board.js:board' }, ideas: { replace: 'board.js:board' } }, ['todos']);
    expect(keptBuiltin('todos')).toBe(true);
    expect(pageSwap('todos')).toBeUndefined();
    expect(swapState('todos')?.state).toBe('builtin');
    expect(pageSwap('ideas')?.id).toBe('board.js:board');
    app.safeMode = true;
    expect(pageSwap('ideas')).toBeUndefined();
  });

  it('Settings is never replaced, whatever the file says', () => {
    app.pluginViews = [view];
    setPages({ settings: { replace: 'board.js:board' } });
    expect(pageSwap('settings')).toBeUndefined();
  });
});
