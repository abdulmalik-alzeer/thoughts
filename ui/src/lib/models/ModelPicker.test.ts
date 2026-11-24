// The model picker on the screen (round 0.7.2): one button and one list for every place a model is
// chosen. The button names the choice; the list marks the current one and the default, shows the
// raw id only where it differs from the name, searches as you type, is driven by the keyboard and
// reads properly to a screen reader. The model names here are made up for the tests.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;

const { mount, unmount, flushSync, tick } = await import('svelte');
const ModelPicker = (await import('./ModelPicker.svelte')).default;
const { entriesFrom, recentModels, rememberModel } = await import('./picker');
const { app } = await import('../store.svelte');

const three = entriesFrom({ models: ['alpha', 'beta', 'gamma'], modelNames: { beta: 'Beta Large' }, defaultModel: 'alpha' });
const many = entriesFrom({ models: ['acme', 'bolt', 'crest'].flatMap((m) => Array.from({ length: 40 }, (_, i) => `${m}/model-${i + 1}`)) });

let host: HTMLElement;
let comps: any[] = [];
function put(props: Record<string, unknown>) {
  const c = mount(ModelPicker, { target: host, props: { label: 'Model', onpick: () => {}, ...props } as any });
  comps.push(c); flushSync();
  return c;
}
const btn = () => host.querySelector<HTMLButtonElement>('button.mp-btn')!;
const pop = () => document.querySelector<HTMLElement>('[data-model-picker]');
const input = () => pop()!.querySelector<HTMLInputElement>('input[role="combobox"]')!;
const options = () => [...pop()!.querySelectorAll<HTMLElement>('[role="option"]')];
const text = (el: Element | null | undefined) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
const activeRow = () => document.getElementById(input().getAttribute('aria-activedescendant') ?? '-');
async function open() { btn().click(); await tick(); flushSync(); }
function key(k: string, on: Element = input()) { const e = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }); on.dispatchEvent(e); flushSync(); return e; }
function type(v: string) { input().value = v; input().dispatchEvent(new Event('input', { bubbles: true })); flushSync(); }
const settle = async () => { await Promise.resolve(); await tick(); flushSync(); };

beforeEach(() => { host = document.createElement('div'); document.body.appendChild(host); app.settings = { language: 'en' } as any; localStorage.clear(); });
afterEach(() => { for (const c of comps) unmount(c); comps = []; host.remove(); document.documentElement.dir = ''; });

describe('the button', () => {
  it('names what is chosen, for the eye and for a screen reader, and says it opens a list', () => {
    put({ value: 'beta', models: three });
    expect(text(btn())).toBe('Beta Large');
    expect(btn().getAttribute('aria-label')).toBe('Model: Beta Large');
    expect(btn().getAttribute('aria-haspopup')).toBe('listbox');
    expect(btn().getAttribute('aria-expanded')).toBe('false');
    expect(btn().type).toBe('button');
    expect(pop()).toBeNull();
  });

  it('says Automatic while nothing is chosen, and shows a typed name as it is', () => {
    put({ value: null, models: three });
    expect(text(btn())).toBe('Automatic');
    for (const c of comps) unmount(c); comps = [];
    put({ value: 'my-own-model', models: three });
    expect(text(btn())).toBe('my-own-model');
  });

  it('does not open while it is disabled', async () => {
    put({ value: null, models: three, disabled: true });
    await open();
    expect(pop()).toBeNull();
  });

  it('opens with the arrow keys too', async () => {
    put({ value: null, models: three });
    key('ArrowDown', btn()); await tick(); flushSync();
    expect(pop()).not.toBeNull();
    expect(btn().getAttribute('aria-expanded')).toBe('true');
  });
});

describe('the list', () => {
  it('is a listbox outside every scrolling box, with the keys going to its search field', async () => {
    put({ value: 'beta', models: three });
    await open();
    expect(pop()!.parentElement).toBe(document.body);
    const list = pop()!.querySelector('[role="listbox"]')!;
    expect(list.getAttribute('aria-label')).toBe('Model');
    expect(btn().getAttribute('aria-controls')).toBe(list.id);
    expect(input().getAttribute('aria-controls')).toBe(list.id);
    expect(document.activeElement).toBe(input());
  });

  it('marks the current choice and the default, and shows the raw id only where it differs from the name', async () => {
    put({ value: 'beta', models: three, autoNote: 'The assistant’s own default: alpha.' });
    await open();
    const o = options();
    expect(o.map((x) => text(x.querySelector('.mp-name')))).toEqual(['Automatic', 'alphaDefault', 'Beta Large', 'gamma']);
    expect(o.map((x) => x.getAttribute('aria-selected'))).toEqual(['false', 'false', 'true', 'false']);
    expect(text(o[0].querySelector('.mp-sub'))).toBe('The assistant’s own default: alpha.');
    expect(text(o[2].querySelector('.mp-id'))).toBe('beta');
    expect(o[1].querySelector('.mp-id')).toBeNull();
    expect(o[3].querySelector('.mp-id')).toBeNull();
    expect(text(o[2])).toContain('In use');
    // It starts on the current choice.
    expect(activeRow()).toBe(o[2]);
  });

  it('does not say "Default" twice when the program’s own name says it already', async () => {
    put({ value: null, models: entriesFrom({ models: ['default', 'big'], modelNames: { default: 'Default (recommended)' }, defaultModel: 'default' }) });
    await open();
    expect(pop()!.querySelector('.mp-pill')).toBeNull();
  });

  it('a short list has no search box to look at; a long one has, with its makers as groups', async () => {
    put({ value: null, models: three });
    await open();
    expect(pop()!.querySelector('.mp-search')!.classList.contains('quiet')).toBe(true);
    expect(pop()!.querySelectorAll('[role="group"]').length).toBe(0);
    for (const c of comps) unmount(c); comps = [];
    put({ value: null, models: many });
    await open();
    expect(pop()!.querySelector('.mp-search')!.classList.contains('quiet')).toBe(false);
    const groups = [...pop()!.querySelectorAll('[role="group"]')];
    expect(groups.map((g) => text(document.getElementById(g.getAttribute('aria-labelledby')!)))).toEqual(['acme', 'bolt', 'crest']);
    expect(options().length).toBe(121);
    expect(text(pop()!.querySelector('.mp-count'))).toBe('120 models');
  });

  it('shows the recently used on top of a long list, and the current choice above them', async () => {
    rememberModel('claude', 'bolt/model-3'); rememberModel('claude', 'crest/model-9');
    put({ value: 'acme/model-40', models: many, scope: 'claude' });
    await open();
    const o = options();
    expect(o.slice(0, 4).map((x) => text(x.querySelector('.mp-name')))).toEqual(['Automatic', 'model-40', 'model-9', 'model-3']);
    expect(o[1].getAttribute('aria-selected')).toBe('true');
    expect(text(pop()!.querySelector('[role="group"] .mp-head'))).toBe('Recently used');
    expect(activeRow()).toBe(o[1]);
  });

  it('says so when the program listed no model, and still lets a name be typed', async () => {
    put({ value: null, models: [] });
    await open();
    expect(text(pop()!.querySelector('.mp-none'))).toContain('has not listed its models');
    type('some-model');
    expect(options().map((x) => text(x.querySelector('.mp-name')))).toEqual(['Use “some-model”']);
  });
});

describe('typing and the keys', () => {
  it('filters as you type, shows the search box, and starts on the best match', async () => {
    put({ value: 'gamma', models: three });
    await open();
    type('be');
    expect(pop()!.querySelector('.mp-search')!.classList.contains('quiet')).toBe(false);
    expect(options().map((x) => text(x.querySelector('.mp-name')))).toEqual(['Beta Large', 'Use “be”']);
    expect(activeRow()).toBe(options()[0]);
    expect(text(pop()!.querySelector('.mp-count'))).toBe('1 model found');
  });

  it('Enter picks the row the arrows are on, closes the list and puts the focus back on the button', async () => {
    const onpick = vi.fn();
    put({ value: null, models: three, onpick });
    await open();
    key('ArrowDown'); key('ArrowDown');
    expect(text(activeRow()!.querySelector('.mp-name'))).toBe('Beta Large');
    const e = key('Enter');
    expect(e.defaultPrevented).toBe(true);
    await settle();
    expect(onpick).toHaveBeenCalledWith('beta');
    expect(pop()).toBeNull();
    expect(document.activeElement).toBe(btn());
  });

  it('the arrows go round, and mean the same in a right-to-left language', async () => {
    document.documentElement.dir = 'rtl';
    put({ value: null, models: three });
    await open();
    expect(activeRow()).toBe(options()[0]);
    key('ArrowUp');
    expect(activeRow()).toBe(options()[3]);
    key('ArrowDown');
    expect(activeRow()).toBe(options()[0]);
    key('ArrowLeft'); key('ArrowRight');
    expect(activeRow()).toBe(options()[0]);
  });

  it('picking Automatic sends no model', async () => {
    const onpick = vi.fn();
    put({ value: 'beta', models: three, onpick });
    await open();
    options()[0].click(); await settle();
    expect(onpick).toHaveBeenCalledWith(null);
  });

  it('picking what is already chosen changes nothing', async () => {
    const onpick = vi.fn();
    put({ value: 'beta', models: three, onpick });
    await open();
    key('Enter'); await settle();
    expect(onpick).not.toHaveBeenCalled();
    expect(pop()).toBeNull();
  });

  it('a name that is not listed can be typed and used, but not one that could not be a name', async () => {
    const onpick = vi.fn();
    put({ value: null, models: three, onpick });
    await open();
    type('vendor/new-model:free');
    expect(options().map((x) => text(x.querySelector('.mp-name')))).toEqual(['Use “vendor/new-model:free”']);
    key('Enter'); await settle();
    expect(onpick).toHaveBeenCalledWith('vendor/new-model:free');
    await open();
    type('two words');
    expect(options()).toEqual([]);
    expect(text(pop()!.querySelector('.mp-none'))).toContain('no spaces');
    key('Enter'); await settle();
    expect(onpick).toHaveBeenCalledTimes(1);
  });

  it('typing a name is not offered where only listed models may be chosen', async () => {
    put({ value: null, models: three, typed: false });
    await open();
    type('zzz');
    expect(options()).toEqual([]);
    expect(text(pop()!.querySelector('.mp-none'))).toBe('No model matches.');
  });

  it('Escape first clears what was typed, then closes; the page behind never sees the key', async () => {
    put({ value: null, models: three });
    const seen = vi.fn();
    document.body.addEventListener('keydown', seen);
    await open();
    type('be');
    key('Escape');
    expect(pop()).not.toBeNull();
    expect(input().value).toBe('');
    expect(options().length).toBe(4);
    key('Escape');
    expect(pop()).toBeNull();
    expect(document.activeElement).toBe(btn());
    expect(seen).not.toHaveBeenCalled();
    document.body.removeEventListener('keydown', seen);
  });

  it('a press outside closes it without choosing; a press inside does not', async () => {
    const onpick = vi.fn();
    put({ value: null, models: three, onpick });
    await open();
    pop()!.querySelector('.mp-foot')!.dispatchEvent(new Event('pointerdown', { bubbles: true })); flushSync();
    expect(pop()).not.toBeNull();
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true })); flushSync();
    expect(pop()).toBeNull();
    expect(onpick).not.toHaveBeenCalled();
  });
});

describe('remembering what was used', () => {
  it('remembers a model that was kept, and not one the save refused', async () => {
    put({ value: null, models: three, scope: 'claude', onpick: async (v: string | null) => v !== 'gamma' });
    await open(); options()[2].click(); await settle(); await settle();
    expect(recentModels('claude')).toEqual(['beta']);
    await open(); options()[3].click(); await settle(); await settle();
    expect(recentModels('claude')).toEqual(['beta']);
  });

  it('never remembers Automatic', async () => {
    put({ value: 'beta', models: three, scope: 'claude' });
    await open(); options()[0].click(); await settle();
    expect(recentModels('claude')).toEqual([]);
  });
});

describe('in Arabic', () => {
  it('uses the Arabic words around the models and leaves their names as reported', async () => {
    app.settings = { language: 'ar' } as any;
    document.documentElement.dir = 'rtl';
    put({ value: 'beta', models: three, label: 'النموذج' });
    expect(btn().getAttribute('aria-label')).toBe('النموذج: Beta Large');
    await open();
    expect(options().map((x) => text(x.querySelector('.mp-name')))).toEqual(['تلقائي', 'alphaالافتراضي', 'Beta Large', 'gamma']);
    expect(input().getAttribute('aria-label')).toBe('ابحث في النماذج');
  });
});
