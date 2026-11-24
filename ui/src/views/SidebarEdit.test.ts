// "Edit sidebar" in place (views/SidebarEdit.svelte), mounted: what the person can reach on each
// row, keyboard moves and what is announced, one-click hide and show, renaming where the name is,
// undo, reset asking first, and when the service is written to. Dragging needs real layout, so its
// arithmetic is tested in lib/navEdit.test.ts (`slotAt`, `placeAt`, `place`).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const put = vi.fn(async (_path: string, _body: any) => ({ ok: true, hash: 'abc' }));
vi.mock('../lib/api', async (orig) => ({ ...(await orig<typeof import('../lib/api')>()), put: (p: string, b: any) => put(p, b), api: vi.fn(async () => ({})), post: vi.fn(async () => ({})), on: () => () => {} }));
// jsdom has no layout and no animations: motion is "reduced" here, and the two calls Svelte's
// `animate:` makes are answered with nothing.
window.matchMedia ??= ((q: string) => ({ matches: /reduced-motion/.test(q), media: q, addEventListener() {}, removeEventListener() {} })) as any;
Element.prototype.scrollIntoView ??= () => {};
(Element.prototype as any).getAnimations ??= () => [];
(Element.prototype as any).animate ??= () => ({ cancel() {}, finish() {}, finished: Promise.resolve(), onfinish: null, currentTime: 0, startTime: 0 });

const { mount, unmount, flushSync, tick } = await import('svelte');
const SidebarEdit = (await import('./SidebarEdit.svelte')).default;
const { app } = await import('../lib/store.svelte');
const { dialogs } = await import('../lib/dialog.svelte');

const mk = (id: string) => ({ id, icon: id, base: id[0].toUpperCase() + id.slice(1), label: id });
const items = ['home', 'chats', 'ideas', 'todos', 'docs', 'graph', 'customise'].map(mk);
let host: HTMLElement, comp: any, closed = 0;
const open = (nav: any = null) => { app.nav = nav; comp = mount(SidebarEdit, { target: host, props: { items, close: () => closed++ } }); flushSync(); };
const row = (id: string) => host.querySelector<HTMLElement>(`[data-key="i:${id}"]`)!;
const part = (id: string, p: string) => row(id).querySelector<HTMLElement>(`[data-part="${p}"]`);
/** The rows in order: a hidden one is marked "-", a group heading is in brackets. */
const order = () => [...host.querySelectorAll<HTMLElement>('[data-key]')].map((e) => (e.classList.contains('off') ? '-' : '') + e.dataset.key!.replace(/^i:/, '').replace(/^g:(.*)$/, '[$1]')).join(' ');
const said = () => host.querySelector('[role="status"]')!.textContent;
const key = (el: Element, k: string, init: KeyboardEventInit = {}) => { el.dispatchEvent(new KeyboardEvent('keydown', { key: k, code: k.length === 1 ? 'Key' + k.toUpperCase() : k, bubbles: true, cancelable: true, ...init })); flushSync(); };
const settle = async () => { await tick(); await Promise.resolve(); flushSync(); };

beforeEach(() => { vi.useFakeTimers(); put.mockClear(); closed = 0; host = document.createElement('div'); document.body.appendChild(host); app.settings = { language: 'en' } as any; });
afterEach(() => { if (comp) unmount(comp); comp = null; host.remove(); dialogs.current = null; vi.useRealTimers(); });

describe('the rows', () => {
  it('shows every page with a handle, an icon and a name to click, and an eye', () => {
    open();
    expect(order()).toBe('home chats ideas todos docs graph customise');
    expect(part('chats', 'grip')!.getAttribute('aria-label')).toBe('Move Chats');
    expect(part('chats', 'icon')!.tagName).toBe('BUTTON');
    expect(part('chats', 'name')!.getAttribute('aria-label')).toBe('Rename Chats');
    expect(part('chats', 'eye')!.getAttribute('aria-label')).toBe('Hide Chats');
  });
  it('gives the entries that must stay reachable no way to hide them, and Customise no way to rename it', () => {
    open({ hidden: ['home', 'customise'], order: [], labels: { customise: { en: 'Gone' } } });
    for (const id of ['home', 'customise']) {
      expect(part(id, 'eye')).toBeNull();
      expect(row(id).querySelector('.lock')!.textContent).toContain('Always shown');
      expect(row(id).classList.contains('off')).toBe(false);
    }
    expect(part('customise', 'name')).toBeNull();
    expect(part('customise', 'icon')).toBeNull();
    expect(row('customise').textContent).toContain('Customise');
    expect(part('home', 'name')).not.toBeNull(); // Home can be renamed, as before
  });
});

describe('keyboard', () => {
  it('moves the focused row with Alt and an arrow, says where it is, and keeps the focus on it', async () => {
    open();
    part('chats', 'grip')!.focus();
    key(part('chats', 'grip')!, 'ArrowDown', { altKey: true });
    expect(order()).toBe('home ideas chats todos docs graph customise');
    expect(said()).toBe('Chats, 3 of 7');
    await settle();
    expect(document.activeElement).toBe(part('chats', 'grip'));
    key(part('chats', 'grip')!, 'ArrowUp', { altKey: true });
    key(part('chats', 'grip')!, 'ArrowUp', { altKey: true });
    expect(order()).toBe('chats home ideas todos docs graph customise');
    key(part('chats', 'grip')!, 'ArrowUp', { altKey: true });
    expect(said()).toBe('Chats cannot move further');
  });
  it('walks the rows with plain arrows and moves into a group', async () => {
    open({ hidden: [], order: [], labels: {}, groups: [{ id: 'work', label: { en: 'Work' }, items: ['todos'] }] });
    expect(order()).toBe('home chats ideas docs graph customise [work] todos');
    part('graph', 'grip')!.focus();
    key(part('graph', 'grip')!, 'ArrowDown');
    expect(document.activeElement).toBe(part('customise', 'grip'));
    key(part('customise', 'grip')!, 'ArrowDown', { altKey: true });
    expect(order()).toBe('home chats ideas docs graph [work] customise todos');
    expect(said()).toBe('Customise, 1 of 2 in Work');
  });
  it('undoes with the undo key (⌘Z or Ctrl+Z, whichever this system uses)', () => {
    open();
    part('graph', 'eye')!.click(); flushSync();
    key(part('home', 'grip')!, 'z', { metaKey: true, ctrlKey: false });
    key(part('home', 'grip')!, 'z', { ctrlKey: true });
    expect(order()).toBe('home chats ideas todos docs graph customise');
  });
});

describe('hide, show, rename, undo', () => {
  it('hides with one click into the Hidden list and shows again with one click', () => {
    open();
    part('ideas', 'eye')!.click(); flushSync();
    expect(order()).toBe('home chats todos docs graph customise -ideas');
    expect(host.textContent).toContain('Hidden');
    expect(part('ideas', 'eye')!.getAttribute('aria-label')).toBe('Show Ideas');
    expect(said()).toBe('Ideas is hidden');
    part('ideas', 'eye')!.click(); flushSync();
    expect(order()).toBe('home chats ideas todos docs graph customise');
    expect(host.textContent).not.toContain('Hidden');
  });
  it('renames where the name is: Enter keeps it, Escape does not', async () => {
    open();
    part('ideas', 'name')!.click(); await settle();
    let input = row('ideas').querySelector('input')!;
    expect(input.value).toBe('Ideas');
    input.value = 'Clients'; input.dispatchEvent(new Event('input', { bubbles: true }));
    key(input, 'Enter');
    expect(part('ideas', 'name')!.textContent).toContain('Clients');
    part('ideas', 'name')!.click(); await settle();
    input = row('ideas').querySelector('input')!;
    input.value = 'Nope'; input.dispatchEvent(new Event('input', { bubbles: true }));
    key(input, 'Escape');
    expect(part('ideas', 'name')!.textContent).toContain('Clients');
    expect(closed).toBe(0); // Escape in the field ends the renaming, not the editing
  });
  it('undo takes back the last change, then the one before', () => {
    open();
    const undo = host.querySelector<HTMLButtonElement>('.bar button[aria-label="Undo the last change"]')!;
    expect(undo.disabled).toBe(true);
    part('ideas', 'eye')!.click(); flushSync();
    part('chats', 'grip')!.focus(); key(part('chats', 'grip')!, 'ArrowDown', { altKey: true });
    expect(order()).toBe('home todos chats docs graph customise -ideas');
    undo.click(); flushSync();
    expect(order()).toBe('home chats todos docs graph customise -ideas');
    undo.click(); flushSync();
    expect(order()).toBe('home chats ideas todos docs graph customise');
    expect(undo.disabled).toBe(true);
  });
  it('makes a group, which is a row of its own', async () => {
    open();
    host.querySelector<HTMLButtonElement>('.newgroup')!.click(); await settle();
    expect(order()).toBe('home chats ideas todos docs graph customise [group-1]');
    expect(host.querySelector('.empty')!.textContent).toBe('Drag pages here');
  });
});

describe('saving', () => {
  it('writes a burst of changes once, a moment later, and nothing when nothing changed', async () => {
    open();
    part('ideas', 'eye')!.click(); flushSync();
    part('graph', 'eye')!.click(); flushSync();
    expect(put).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(800);
    expect(put).toHaveBeenCalledTimes(1);
    expect(put).toHaveBeenCalledWith('/layer/nav', { hidden: ['ideas', 'graph'] });
    await vi.advanceTimersByTimeAsync(5000);
    expect(put).toHaveBeenCalledTimes(1);
  });
  it('Done writes what still waits and then closes; with nothing changed it only closes', async () => {
    open();
    part('ideas', 'eye')!.click(); flushSync();
    host.querySelector<HTMLButtonElement>('.done')!.click();
    await vi.advanceTimersByTimeAsync(0); await settle();
    expect(put).toHaveBeenCalledWith('/layer/nav', { hidden: ['ideas'] });
    expect(closed).toBe(1);
    unmount(comp); comp = null; put.mockClear();
    open({ hidden: ['ideas'], order: [], labels: {} });
    host.querySelector<HTMLButtonElement>('.done')!.click();
    await vi.advanceTimersByTimeAsync(0); await settle();
    expect(put).not.toHaveBeenCalled();
    expect(closed).toBe(2);
  });
  it('stays open and says so when the service refuses the change', async () => {
    open();
    put.mockRejectedValueOnce(new Error('`ideas` cannot be hidden today.'));
    part('ideas', 'eye')!.click(); flushSync();
    host.querySelector<HTMLButtonElement>('.done')!.click();
    await vi.advanceTimersByTimeAsync(0); await settle();
    expect(closed).toBe(0);
    expect(host.querySelector('[role="alert"]')!.textContent).toContain('cannot be hidden today');
  });
  it('reset asks first; saying yes saves the usual sidebar and can be undone', async () => {
    const mine = { hidden: ['graph'], order: ['todos', 'home'], labels: { ideas: { en: 'Clients' } } };
    open(mine);
    const reset = [...host.querySelectorAll<HTMLButtonElement>('.foot button')].pop()!;
    reset.click(); await settle();
    expect(dialogs.current?.title).toBe('Reset the sidebar?');
    expect(order()).toBe('todos home chats ideas docs customise -graph'); // nothing yet
    dialogs.current!.resolve(false); dialogs.current = null; await settle();
    expect(order()).toBe('todos home chats ideas docs customise -graph');
    reset.click(); await settle();
    dialogs.current!.resolve(true); dialogs.current = null; await settle();
    expect(order()).toBe('home chats ideas todos docs graph customise');
    await vi.advanceTimersByTimeAsync(800);
    expect(put).toHaveBeenCalledWith('/layer/nav', {});
    host.querySelector<HTMLButtonElement>('.bar button[aria-label="Undo the last change"]')!.click(); flushSync();
    expect(order()).toBe('todos home chats ideas docs customise -graph');
    await vi.advanceTimersByTimeAsync(800);
    expect(put).toHaveBeenLastCalledWith('/layer/nav', mine);
  });
  it('a change still waiting when the editor goes away is written all the same', () => {
    open();
    part('ideas', 'eye')!.click(); flushSync();
    unmount(comp); comp = null;
    expect(put).toHaveBeenCalledWith('/layer/nav', { hidden: ['ideas'] });
  });
});

describe('an arrangement saved by an earlier version', () => {
  const old = { order: ['todos', 'inbox', 'home'], hidden: ['graph', 'inbox'], labels: { ideas: { en: 'Clients', ar: 'العملاء' }, inbox: { en: 'Post' } }, icons: { todos: 'calendar' },
    groups: [{ id: 'work', label: { en: 'Work', ar: 'العمل' }, items: ['ideas', 'inbox'] }], start: 'todos' };
  it('opens as it was and keeps what it cannot show when something else changes', async () => {
    open(structuredClone(old));
    expect(order()).toBe('todos home chats docs customise [work] ideas -graph');
    expect(part('ideas', 'name')!.textContent).toContain('Clients');
    expect(row('todos').querySelector('.start')).not.toBeNull();
    part('chats', 'eye')!.click(); flushSync();
    await vi.advanceTimersByTimeAsync(800);
    expect(put).toHaveBeenCalledWith('/layer/nav', { ...old, hidden: ['graph', 'inbox', 'chats'] });
  });
  it('reads in Arabic too', () => {
    app.settings = { language: 'ar' } as any;
    open(structuredClone(old));
    expect(part('ideas', 'name')!.textContent).toContain('العملاء');
    expect(host.querySelector('[data-key="g:work"]')!.textContent).toContain('العمل');
    expect(part('chats', 'eye')!.getAttribute('aria-label')).toBe('أخفِ المحادثات'.replace('المحادثات', 'Chats'));
  });
});
