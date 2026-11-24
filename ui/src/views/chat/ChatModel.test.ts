// The composer's model chip (round 0.7.2): the model has its own control on the composer, with the
// effort beside it under the names the program gives its levels. The names here are made up.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;

const { mount, unmount, flushSync, tick } = await import('svelte');
const ChatModel = (await import('./ChatModel.svelte')).default;
const ChatOptions = (await import('./ChatOptions.svelte')).default;
const { entriesFrom } = await import('../../lib/models/picker');
const { app } = await import('../../lib/store.svelte');

const models = entriesFrom({ models: ['alpha', 'beta'], modelNames: { beta: 'Beta Large' } });
const names: Record<string, string> = { low: 'Quick', high: 'Thorough' };
let host: HTMLElement;
let comps: any[] = [];
const put = (C: any, props: Record<string, unknown>) => { const c = mount(C, { target: host, props: props as any }); comps.push(c); flushSync(); return c; };
const chip = (over: Record<string, unknown> = {}) => put(ChatModel, { model: null, effort: null, models, efforts: ['low', 'high'], effortName: (v: string) => names[v] ?? v, onmodel: () => {}, oneffort: () => {}, ...over });
const btn = () => host.querySelector<HTMLButtonElement>('button.mp-btn')!;
const eff = () => host.querySelector<HTMLSelectElement>('select.eff');
const text = (el: Element | null | undefined) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();

beforeEach(() => { host = document.createElement('div'); document.body.appendChild(host); app.settings = { language: 'en' } as any; localStorage.clear(); });
afterEach(() => { for (const c of comps) unmount(c); comps = []; host.remove(); });

describe('the model chip on the composer', () => {
  it('names the model in force, or says the model is automatic', () => {
    chip();
    expect(btn().classList.contains('chip')).toBe(true);
    expect(text(btn())).toBe('Model: auto');
    expect(btn().getAttribute('aria-label')).toBe('Model: Automatic');
    for (const c of comps) unmount(c); comps = [];
    chip({ model: 'beta' });
    expect(text(btn())).toBe('Beta Large');
    expect(btn().getAttribute('aria-label')).toBe('Model: Beta Large');
    expect(btn().title).toBe('Beta Large'); // the whole name when the chip is cut short
  });

  it('opens the model list directly and passes the choice on', async () => {
    const onmodel = vi.fn();
    chip({ onmodel, note: 'The program could not list its models.' });
    btn().click(); await tick(); flushSync();
    const pop = document.querySelector<HTMLElement>('[data-model-picker]')!;
    expect(text(pop.querySelector('.mp-note'))).toBe('The program could not list its models.');
    [...pop.querySelectorAll<HTMLElement>('[role="option"]')].find((o) => text(o).startsWith('Beta Large'))!.click();
    await Promise.resolve(); await tick(); flushSync();
    expect(onmodel).toHaveBeenCalledWith('beta');
  });

  it('offers the effort levels beside it under the program’s own names', () => {
    const oneffort = vi.fn();
    chip({ effort: 'high', oneffort });
    expect(eff()!.getAttribute('aria-label')).toBe('Effort');
    expect([...eff()!.options].map((o) => [o.value, o.text])).toEqual([['', 'Effort: auto'], ['low', 'Quick'], ['high', 'Thorough']]);
    expect(eff()!.value).toBe('high');
    eff()!.value = ''; eff()!.dispatchEvent(new Event('change', { bubbles: true }));
    expect(oneffort).toHaveBeenCalledWith(null);
    eff()!.value = 'low'; eff()!.dispatchEvent(new Event('change', { bubbles: true }));
    expect(oneffort).toHaveBeenCalledWith('low');
  });

  it('shows no effort menu when the program lists no levels, but keeps a level already set', () => {
    chip({ efforts: [] });
    expect(eff()).toBeNull();
    for (const c of comps) unmount(c); comps = [];
    chip({ efforts: [], effort: 'odd' });
    expect([...eff()!.options].map((o) => o.value)).toEqual(['', 'odd']);
  });
});

describe('the options menu beside it', () => {
  const label = (_p: string, v: string) => v;
  it('a chat’s menu has the modes only: no second model or effort control', () => {
    put(ChatOptions, { options: { permissionMode: 'plan' }, modes: ['acceptEdits', 'plan'], label });
    host.querySelector<HTMLButtonElement>('button.cobtn')!.click(); flushSync();
    const pop = host.querySelector('.pop')!;
    expect(pop.querySelectorAll('input[type="radio"]').length).toBe(2);
    expect(pop.querySelector('.mp-btn')).toBeNull();
    expect(pop.querySelector('select')).toBeNull();
  });

  it('a plan’s menu still chooses the model and the effort', () => {
    put(ChatOptions, { plain: true, options: { model: 'beta', effort: 'high' }, models, efforts: ['low', 'high'], modes: [], label });
    expect(text(host.querySelector('button.cobtn'))).toContain('Beta Large · high');
    host.querySelector<HTMLButtonElement>('button.cobtn')!.click(); flushSync();
    const pop = host.querySelector('.pop')!;
    expect(text(pop.querySelector('.mp-btn'))).toBe('Beta Large');
    expect(pop.querySelector('select')!.value).toBe('high');
  });
});
