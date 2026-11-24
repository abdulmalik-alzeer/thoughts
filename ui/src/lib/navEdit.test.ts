// Editing the sidebar in place (lib/navEdit.ts): order, groups, hide and show, the protected
// entries, keyboard steps, undo, reset, what is saved, and arrangements saved by earlier versions.
import { describe, it, expect } from 'vitest';
import type { NavConfig } from './store.svelte';
import { layoutNav } from './navLayout';
import * as E from './navEdit';

const mk = (id: string, alias?: string): E.EditItem => ({ id, alias, icon: id, base: id[0].toUpperCase() + id.slice(1) });
const items = ['home', 'chats', 'ideas', 'todos', 'docs', 'graph', 'customise'].map((id) => mk(id));
const nav = (n: Partial<NavConfig> = {}): NavConfig => ({ ...E.emptyNav(), ...n });
/** The sidebar as the app would show it: ids, with a group's name in brackets before its entries. */
const shown = (n: NavConfig, list = items) => layoutNav(list, n).groups.flatMap((g) => [...(g.id ? [`[${g.id}]`] : []), ...g.items.map((i) => i.id)]).join(' ');
const rows = (n: NavConfig, list = items) => E.editRows(list, n).rows.map((r) => (r.kind === 'head' ? `[${r.group}]` : r.item.id)).join(' ');
const work = nav({ groups: [{ id: 'work', label: { en: 'Work' }, items: ['todos', 'docs'] }] });

describe('ordering', () => {
  it('puts an entry at a place among the shown ones', () => {
    const n = E.place(items, nav(), 'graph', { group: null, index: 1 });
    expect(shown(n)).toBe('home graph chats ideas todos docs customise');
    expect(E.cleanNav(n)).toEqual({ order: ['home', 'graph', 'chats', 'ideas', 'todos', 'docs', 'customise'] });
  });
  it('writes no order at all when the result is the app’s own order', () => {
    const there = E.place(items, nav(), 'graph', { group: null, index: 0 });
    expect(E.cleanNav(E.place(items, there, 'graph', { group: null, index: 5 }))).toEqual({});
  });
  it('moves into a group, inside it, and out again', () => {
    let n = E.place(items, work, 'ideas', { group: 'work', index: 1 });
    expect(shown(n)).toBe('home chats graph customise [work] todos ideas docs');
    expect(n.groups![0].items).toEqual(['todos', 'ideas', 'docs']);
    n = E.place(items, n, 'docs', { group: 'work', index: 0 });
    expect(shown(n)).toBe('home chats graph customise [work] docs todos ideas');
    n = E.place(items, n, 'todos', { group: null, index: 0 });
    expect(shown(n)).toBe('todos home chats graph customise [work] docs ideas');
    expect(n.groups![0].items).toEqual(['docs', 'ideas']);
  });
  it('past the end means the end; an unknown entry or group changes nothing harmful', () => {
    expect(shown(E.place(items, work, 'home', { group: 'work', index: 99 }))).toBe('chats ideas graph customise [work] todos docs home');
    expect(E.place(items, work, 'nope', { group: null, index: 0 })).toBe(work);
    expect(shown(E.place(items, work, 'todos', { group: 'gone', index: 0 }))).toBe('todos home chats ideas graph customise [work] docs');
  });
  it('a hidden entry keeps its place while others move around it', () => {
    const n = E.place(items, nav({ hidden: ['ideas'] }), 'graph', { group: null, index: 2 }); // shown: home chats | todos docs graph customise
    expect(shown(n)).toBe('home chats graph todos docs customise');
    expect(shown(E.setHidden(n, mk('ideas'), false))).toBe('home chats ideas graph todos docs customise');
  });
  it('a gap between rows is a place: under a heading is the top of its group, above it the end of what comes before', () => {
    const r = E.editRows(items, work).rows; // home chats ideas graph customise [work] todos docs
    expect(E.placeAt(r, 0)).toEqual({ group: null, index: 0 });
    expect(E.placeAt(r, 5)).toEqual({ group: null, index: 5 });
    expect(E.placeAt(r, 6)).toEqual({ group: 'work', index: 0 });
    expect(E.placeAt(r, 8)).toEqual({ group: 'work', index: 2 });
    expect(E.placeAt(r, 99)).toEqual({ group: 'work', index: 2 });
    expect(E.slotAt([15, 45, 75], 0)).toBe(0);
    expect(E.slotAt([15, 45, 75], 50)).toBe(2);
    expect(E.slotAt([15, 45, 75], 500)).toBe(3);
  });
});

describe('keyboard moves', () => {
  const walk = (n: NavConfig, id: string, d: -1 | 1, times: number) => { const seen: string[] = []; for (let i = 0; i < times; i++) { const s = E.step(items, n, id, d); if (!s) { seen.push('stop'); break; } n = s.nav; seen.push(rows(n)); } return seen; };
  it('steps down through the list, into a group, and stops at the end', () => {
    expect(walk(work, 'graph', 1, 5)).toEqual([
      'home chats ideas customise graph [work] todos docs',
      'home chats ideas customise [work] graph todos docs',
      'home chats ideas customise [work] todos graph docs',
      'home chats ideas customise [work] todos docs graph',
      'stop',
    ]);
  });
  it('steps up out of a group to the end of what is above it, and stops at the top', () => {
    expect(walk(work, 'todos', -1, 2)).toEqual(['home chats ideas graph customise todos [work] docs', 'home chats ideas graph todos customise [work] docs']);
    expect(E.step(items, work, 'home', -1)).toBeNull();
    expect(E.step(items, work, 'nope', 1)).toBeNull();
  });
  it('steps over hidden entries and through an empty group', () => {
    const n = nav({ hidden: ['chats'], groups: [{ id: 'a', label: { en: 'A' }, items: [] }, { id: 'b', label: { en: 'B' }, items: ['docs'] }] });
    expect(rows(E.step(items, n, 'home', 1)!.nav)).toBe('ideas home todos graph customise [a] [b] docs');
    const inA = E.step(items, E.place(items, n, 'graph', { group: null, index: 99 }), 'graph', 1)!;
    expect(inA.place).toEqual({ group: 'a', index: 0 });
    expect(rows(E.step(items, inA.nav, 'graph', 1)!.nav)).toBe('home ideas todos customise [a] [b] graph docs');
  });
  it('says where the entry went', () => {
    expect(E.step(items, work, 'customise', 1)!.place).toEqual({ group: 'work', index: 0 });
  });
});

describe('hide and show', () => {
  it('hides with one call, lists the hidden ones, and shows them again where they were', () => {
    const n = E.setHidden(nav(), mk('ideas'), true);
    expect(n.hidden).toEqual(['ideas']);
    expect(rows(n)).toBe('home chats todos docs graph customise');
    expect(E.editRows(items, n).hidden.map((i) => i.id)).toEqual(['ideas']);
    expect(E.cleanNav(E.setHidden(n, mk('ideas'), false))).toEqual({});
  });
  it('an entry hidden inside a group comes back inside it', () => {
    const n = E.setHidden(work, mk('todos'), true);
    expect(E.editRows(items, n).rows.find((r) => r.kind === 'head')).toMatchObject({ group: 'work', count: 1 });
    expect(shown(E.setHidden(n, mk('todos'), false))).toBe('home chats ideas graph customise [work] todos docs');
  });
  it('knows an add-on’s page by its short name too', () => {
    const board = mk('hours.js:board', 'board'), list = [...items, board];
    const old = nav({ hidden: ['board'] });
    expect(E.isHidden(old, board)).toBe(true);
    expect(E.setHidden(old, board, false).hidden).toEqual([]);
    expect(E.setHidden(nav(), board, true).hidden).toEqual(['hours.js:board']);
    expect(E.editRows(list, old).hidden.map((i) => i.id)).toEqual(['hours.js:board']);
  });
  it('hiding the page the app opens on puts the start back on Home', () => {
    expect(E.setHidden(nav({ start: 'todos' }), mk('todos'), true).start).toBeUndefined();
    expect(E.setStart(nav({ hidden: ['todos'] }), mk('todos')).start).toBeUndefined();
    expect(E.setStart(nav(), mk('todos')).start).toBe('todos');
    expect(E.setStart(nav({ start: 'todos' }), mk('home')).start).toBeUndefined();
    expect(E.cleanNav(nav({ start: 'todos', hidden: ['todos'] }))).toEqual({ hidden: ['todos'] });
  });
});

describe('protected entries', () => {
  it('Home and Customise can never be hidden, by any route', () => {
    for (const id of ['home', 'customise']) {
      expect(E.canHide(mk(id))).toBe(false);
      expect(E.setHidden(nav(), mk(id), true)).toEqual(nav());
      expect(E.isHidden(nav({ hidden: [id] }), mk(id))).toBe(false);
      expect(E.editRows(items, nav({ hidden: [id] })).hidden).toEqual([]);
    }
    // Never written either, even if an old file or a draft carries them (the footer's Settings and Help too).
    expect(E.cleanNav(nav({ hidden: ['home', 'customise', 'settings', 'help', 'ideas'] }))).toEqual({ hidden: ['ideas'] });
  });
  it('Customise keeps the app’s own name and icon', () => {
    const c = mk('customise');
    expect(E.canRestyle(c)).toBe(false);
    expect(E.rename(nav(), c, 'en', 'Mine')).toEqual(nav());
    expect(E.setIcon(nav(), c, 'eye')).toEqual(nav());
    expect(E.labelOf(nav({ labels: { customise: { en: 'Mine' } } }), c, 'en')).toBe('Customise');
    expect(E.cleanNav(nav({ labels: { customise: { en: 'Mine' } }, icons: { customise: 'eye' } }))).toEqual({});
  });
  it('they can still be moved, and reset keeps them', () => {
    expect(shown(E.place(items, nav(), 'customise', { group: null, index: 0 }))).toBe('customise home chats ideas todos docs graph');
    expect(shown(E.emptyNav())).toBe('home chats ideas todos docs graph customise');
  });
  it('with full access on nothing is locked (as in the rest of the app)', () => {
    expect(E.setHidden(nav(), mk('customise'), true, true).hidden).toEqual(['customise']);
    expect(E.rename(nav(), mk('customise'), 'en', 'Mine', true).labels).toEqual({ customise: { en: 'Mine' } });
    expect(E.cleanNav(nav({ hidden: ['home'] }), true)).toEqual({ hidden: ['home'] });
  });
  it('an icon must be one of the reference’s', () => {
    expect(E.setIcon(nav(), mk('todos'), 'shield')).toEqual(nav());
    expect(E.setIcon(nav(), mk('todos'), 'calendar').icons).toEqual({ todos: 'calendar' });
    expect(E.setIcon(nav({ icons: { todos: 'calendar' } }), mk('todos'), '').icons).toEqual({});
  });
});

describe('names', () => {
  it('names in one language and keeps the other', () => {
    let n = E.rename(nav(), mk('ideas'), 'en', '  Clients  ');
    expect(n.labels).toEqual({ ideas: { en: 'Clients' } });
    n = E.rename(n, mk('ideas'), 'ar', 'العملاء');
    expect(n.labels).toEqual({ ideas: { en: 'Clients', ar: 'العملاء' } });
    expect(E.labelOf(n, mk('ideas'), 'ar')).toBe('العملاء');
    n = E.rename(n, mk('ideas'), 'en', '');
    expect(n.labels).toEqual({ ideas: { ar: 'العملاء' } });
    expect(E.labelOf(n, mk('ideas'), 'en')).toBe('العملاء'); // the same fallback the sidebar uses
    expect(E.rename(n, mk('ideas'), 'ar', ' ').labels).toEqual({});
  });
  it('cuts a name at 40 characters and goes back to the usual look', () => {
    expect(E.rename(nav(), mk('ideas'), 'en', 'x'.repeat(60)).labels.ideas.en).toHaveLength(40);
    const n = nav({ labels: { ideas: { en: 'Clients' } }, icons: { ideas: 'book' } });
    expect(E.restyled(n, mk('ideas'))).toBe(true);
    expect(E.cleanNav(E.usualLook(n, mk('ideas')))).toEqual({});
  });
});

describe('groups', () => {
  it('adds up to the limit, with ids that are free', () => {
    let n = nav({ groups: [{ id: 'group-2', label: { en: 'B' }, items: ['docs'] }] });
    const a = E.addGroup(n, 'ar', 'مجموعة جديدة')!;
    expect(a.id).toBe('group-3');
    expect(a.nav.groups![1]).toEqual({ id: 'group-3', label: { ar: 'مجموعة جديدة' }, items: [] });
    n = nav();
    for (let i = 0; i < E.guards().maxGroups; i++) n = E.addGroup(n, 'en', 'G')!.nav;
    expect(E.addGroup(n, 'en', 'G')).toBeNull();
  });
  it('shows an empty group while editing but does not save it', () => {
    const n = E.addGroup(work, 'en', 'Later')!.nav;
    expect(rows(n)).toBe('home chats ideas graph customise [work] todos docs [group-2]');
    expect(E.cleanNav(n).groups).toEqual([{ id: 'work', label: { en: 'Work' }, items: ['todos', 'docs'] }]);
    expect(E.sameNav(n, work)).toBe(true);
  });
  it('renames (never to nothing), removes (its pages stay) and reorders', () => {
    expect(E.renameGroup(work, 'work', 'ar', 'العمل').groups![0].label).toEqual({ en: 'Work', ar: 'العمل' });
    expect(E.renameGroup(work, 'work', 'en', '  ')).toBe(work);
    const gone = E.removeGroup(items, work, 'work');
    expect(gone.groups).toEqual([]);
    expect(shown(gone)).toBe('home chats ideas graph customise todos docs');
    const two = nav({ groups: [{ id: 'a', label: { en: 'A' }, items: ['chats'] }, { id: 'b', label: { en: 'B' }, items: ['docs', 'todos'] }] });
    const swapped = E.moveGroup(items, two, 'b', 0);
    expect(shown(swapped)).toBe('home ideas graph customise [b] todos docs [a] chats');
    expect(E.moveGroup(items, two, 'a', 0)).toBe(two);
    expect(E.moveGroup(items, two, 'nope', 0)).toBe(two);
  });
});

describe('undo and reset', () => {
  it('takes changes back one at a time, newest first', () => {
    const u = new E.Undo<NavConfig>();
    let n = nav();
    for (const change of [(x: NavConfig) => E.setHidden(x, mk('graph'), true), (x: NavConfig) => E.rename(x, mk('ideas'), 'en', 'Clients'), (x: NavConfig) => E.place(items, x, 'docs', { group: null, index: 0 })]) { u.push(n); n = change(n); }
    expect(shown(n)).toBe('docs home chats ideas todos customise');
    n = u.pop()!; expect(shown(n)).toBe('home chats ideas todos docs customise');
    n = u.pop()!; expect(n.labels).toEqual({});
    n = u.pop()!; expect(E.cleanNav(n)).toEqual({});
    expect(u.pop()).toBeUndefined();
    expect(u.size).toBe(0);
  });
  it('keeps a bounded number of steps', () => {
    const u = new E.Undo<number>(3);
    for (let i = 0; i < 10; i++) u.push(i);
    expect([u.size, u.pop()]).toEqual([3, 9]);
  });
  it('reset is the empty arrangement: saved as {}, and itself a step that undo takes back', () => {
    const u = new E.Undo<NavConfig>();
    const before = E.setHidden(work, mk('graph'), true);
    u.push(before);
    const after = E.emptyNav();
    expect(E.cleanNav(after)).toEqual({});
    expect(shown(after)).toBe('home chats ideas todos docs graph customise');
    expect(u.pop()).toBe(before);
  });
});

describe('arrangements saved before this editor', () => {
  // A file as the old form (or the assistant) wrote it: short names for an add-on's page, a page
  // that is not in the sidebar right now (Inbox while empty), every key in use.
  const board = mk('hours.js:board', 'board');
  const list = [...items, board];
  const old: NavConfig = {
    order: ['todos', 'board', 'inbox', 'home'], hidden: ['graph', 'inbox'],
    labels: { ideas: { en: 'Clients', ar: 'العملاء' }, inbox: { en: 'Post' }, board: { en: 'Hours' } },
    icons: { todos: 'calendar', inbox: 'bell' },
    groups: [{ id: 'work', label: { en: 'Work', ar: 'العمل' }, items: ['ideas', 'board', 'inbox'] }],
    start: 'todos',
  };
  it('opens as the sidebar shows it', () => {
    expect(shown(old, list)).toBe('todos home chats docs customise [work] hours.js:board ideas');
    expect(rows(E.draftOf(old), list)).toBe('todos home chats docs customise [work] hours.js:board ideas');
    expect(E.editRows(list, E.draftOf(old)).hidden.map((i) => i.id)).toEqual(['graph']);
    expect(E.labelOf(old, board, 'en')).toBe('Hours');
    expect(E.isStart(old, mk('todos'))).toBe(true);
  });
  it('is saved back unchanged when nothing is changed', () => {
    expect(E.cleanNav(E.draftOf(old))).toEqual(old);
    expect(E.sameNav(E.draftOf(old), old)).toBe(true);
  });
  it('keeps what it cannot show through every kind of change', () => {
    let n = E.draftOf(old);
    n = E.place(list, n, 'docs', { group: 'work', index: 0 });
    n = E.setHidden(n, mk('chats'), true);
    n = E.rename(n, mk('docs'), 'en', 'Files');
    n = E.moveGroup(list, E.addGroup(n, 'en', 'More')!.nav, 'group-2', 0);
    const out = E.cleanNav(n);
    // Inbox is not in the sidebar now: still ordered, hidden, named, re-iconed and in its group.
    expect(out.order).toContain('inbox');
    expect(out.hidden).toEqual(['graph', 'inbox', 'chats']);
    expect(out.labels.inbox).toEqual({ en: 'Post' });
    expect(out.icons).toEqual({ todos: 'calendar', inbox: 'bell' });
    expect(out.groups).toEqual([{ id: 'work', label: { en: 'Work', ar: 'العمل' }, items: ['docs', 'hours.js:board', 'ideas', 'inbox'] }]);
    expect(out.start).toBe('todos');
    // And when Inbox has something again it is back in its group, in place.
    expect(shown(n, [...list, mk('inbox')]).endsWith('[work] docs hours.js:board ideas')).toBe(true);
    expect(E.editRows([...list, mk('inbox')], n).hidden.map((i) => i.id)).toContain('inbox');
  });
  it('renaming a page known by its short name leaves one name, under its full id', () => {
    expect(E.rename(E.draftOf(old), board, 'ar', 'الساعات').labels).toMatchObject({ 'hours.js:board': { en: 'Hours', ar: 'الساعات' } });
    expect(E.rename(E.draftOf(old), board, 'ar', 'الساعات').labels.board).toBeUndefined();
  });
  it('tells two groups with one id apart', () => {
    const twice = nav({ groups: [{ id: 'g', label: { en: 'A' }, items: ['chats'] }, { id: 'g', label: { en: 'B' }, items: ['docs'] }] });
    expect(E.draftOf(twice).groups!.map((g) => g.id)).toEqual(['g', 'g-2']);
  });
  it('null, and a file with nothing in it, are the usual sidebar', () => {
    expect(E.cleanNav(E.draftOf(null))).toEqual({});
    expect(rows(E.draftOf({} as NavConfig))).toBe('home chats ideas todos docs graph customise');
    expect(E.cleanNav(null)).toEqual({});
  });
});
