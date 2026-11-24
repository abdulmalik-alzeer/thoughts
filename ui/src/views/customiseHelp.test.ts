// Customise in 0.7.0: explanations sit behind "?" help icons (lib/HelpTip.svelte) beside what they
// explain, while warnings, costs, privacy and state stay in view. The help icon itself: a real
// button named after what it explains, opened by click or keyboard, closed by Escape, a click
// outside or the focus leaving, read out by screen readers, and kept inside the window.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('../lib/api', async (orig) => ({
  ...(await orig<typeof import('../lib/api')>()),
  api: vi.fn(async () => ({})), post: vi.fn(async () => ({})), on: () => () => {},
}));
window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;

const { mount, unmount, flushSync, createRawSnippet } = await import('svelte');
const HelpTip = (await import('../lib/HelpTip.svelte')).default;
const Customise = (await import('./Customise.svelte')).default;
const CustomiseStart = (await import('./CustomiseStart.svelte')).default;
const { app } = await import('../lib/store.svelte');
const { t } = await import('../lib/i18n');
const dict = (await import('../lib/i18n/custhelp')).default;

let host: HTMLElement;
let comps: any[] = [];
const put = (C: any, props: any = {}, target: HTMLElement = host) => { const c = mount(C, { target, props }); comps.push(c); flushSync(); return c; };
const text = (el: Element) => (el.textContent ?? '').replace(/\s+/g, ' ');
const tipBtn = (name: string) => [...host.querySelectorAll<HTMLButtonElement>('button.helptip-btn')].find((b) => b.getAttribute('aria-label') === name);

beforeEach(() => { host = document.createElement('div'); document.body.appendChild(host); app.settings = { language: 'en' } as any; });
afterEach(() => { for (const c of comps) unmount(c); comps = []; host.remove(); document.documentElement.dir = ''; });

describe('the help icon', () => {
  it('is a button named after what it explains, and shows its text only when opened', () => {
    put(HelpTip, { text: 'How it works.', label: 'About plugins' });
    const b = tipBtn('About plugins')!;
    expect(b.tagName).toBe('BUTTON');
    expect(b.type).toBe('button');
    expect(b.getAttribute('aria-expanded')).toBe('false');
    expect(text(host)).not.toContain('How it works.');
    b.click(); flushSync();
    expect(b.getAttribute('aria-expanded')).toBe('true');
    const pop = document.getElementById(b.getAttribute('aria-controls')!)!;
    expect(pop.getAttribute('role')).toBe('note');
    expect(text(pop)).toContain('How it works.');
    // Inside a live region that is always there, so the text is read out as it appears.
    expect(pop.closest('[aria-live="polite"]')).not.toBeNull();
    b.click(); flushSync();
    expect(text(host)).not.toContain('How it works.');
  });

  it('is named after the heading it sits in when no name is given', () => {
    const h = document.createElement('h3'); h.textContent = 'Automatic tasks '; host.appendChild(h);
    put(HelpTip, { text: 'x' }, h);
    expect(h.querySelector('button')!.getAttribute('aria-label')).toBe('About Automatic tasks');
    app.settings = { language: 'ar' } as any; flushSync();
    expect(h.querySelector('button')!.getAttribute('aria-label')).toBe('عن Automatic tasks');
  });

  it('closes on Escape, gives the focus back, and keeps the key from closing a dialog around it', () => {
    const dialog = document.createElement('div'); host.appendChild(dialog);
    const outer = vi.fn();
    dialog.addEventListener('keydown', outer);
    put(HelpTip, { text: 'Tip', label: 'About it' }, dialog);
    const b = tipBtn('About it')!;
    b.focus(); b.click(); flushSync();
    b.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    flushSync();
    expect(b.getAttribute('aria-expanded')).toBe('false');
    expect(document.activeElement).toBe(b);
    expect(outer).not.toHaveBeenCalled();
  });

  it('closes on a click outside and when the focus moves elsewhere', () => {
    const other = document.createElement('button'); host.appendChild(other);
    put(HelpTip, { text: 'Tip', label: 'About it' });
    const b = tipBtn('About it')!;
    b.click(); flushSync();
    other.dispatchEvent(new Event('pointerdown', { bubbles: true })); flushSync();
    expect(b.getAttribute('aria-expanded')).toBe('false');
    b.click(); flushSync();
    b.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: other })); flushSync();
    expect(b.getAttribute('aria-expanded')).toBe('false');
  });

  it('stays inside the window at its edge, also right to left', () => {
    const rect = (l: number, t0: number, w: number, h: number) => ({ left: l, right: l + w, top: t0, bottom: t0 + h, width: w, height: h, x: l, y: t0, toJSON() {} });
    const orig = HTMLElement.prototype.getBoundingClientRect;
    const ow = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetWidth');
    const oh = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight');
    Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, get() { return this.classList?.contains('helptip-pop') ? 300 : 0; } });
    Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, get() { return this.classList?.contains('helptip-pop') ? 120 : 0; } });
    HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
      if (this.classList.contains('helptip-btn')) return rect(Number(this.dataset.x), 700, 18, 18) as DOMRect;
      if (this.classList.contains('helptip-pop')) return rect(parseFloat(this.style.left), parseFloat(this.style.top), 300, 120) as DOMRect;
      return orig.call(this);
    };
    try {
      for (const [dir, x] of [['ltr', window.innerWidth - 20], ['rtl', 4]] as const) {
        document.documentElement.dir = dir; host.dir = dir;
        put(HelpTip, { text: 'Tip', label: 'Edge ' + dir });
        const b = tipBtn('Edge ' + dir)!;
        b.dataset.x = String(x);
        b.click(); flushSync();
        const pop = document.getElementById(b.getAttribute('aria-controls')!)!;
        const left = parseFloat(pop.style.left), top = parseFloat(pop.style.top);
        expect(left).toBeGreaterThanOrEqual(8);
        expect(left + 300).toBeLessThanOrEqual(window.innerWidth - 8);
        // Near the bottom of the window it opens above the button.
        expect(top + 120).toBeLessThanOrEqual(700);
        b.click(); flushSync();
      }
      // Away from the edge it starts at the button's start edge: its right edge in Arabic.
      put(HelpTip, { text: 'Tip', label: 'Middle rtl' });
      const b = tipBtn('Middle rtl')!;
      b.dataset.x = '500';
      b.style.direction = 'rtl'; // (the test DOM does not inherit `direction` from the page)
      b.click(); flushSync();
      const pop = document.getElementById(b.getAttribute('aria-controls')!)!;
      expect(parseFloat(pop.style.left) + 300).toBe(500 + 18 + 6);
    } finally {
      HTMLElement.prototype.getBoundingClientRect = orig;
      if (ow) Object.defineProperty(HTMLElement.prototype, 'offsetWidth', ow);
      if (oh) Object.defineProperty(HTMLElement.prototype, 'offsetHeight', oh);
    }
  });

  it('still takes its content as children', () => {
    const children = createRawSnippet(() => ({ render: () => '<span>Child words</span>' }));
    put(HelpTip, { children, label: 'Kids' });
    tipBtn('Kids')!.click(); flushSync();
    expect(text(host)).toContain('Child words');
  });
});

describe('Customise › Ask', () => {
  it('shows a heading, New customisation and two compact suggestions, each with its own "?"', () => {
    put(Customise);
    const page = text(host);
    // None of the explanations are on the page until asked for.
    for (const k of ['ldc.intro', 'r10-cust-start.lead', 'r10-cust-start.step1', 'r10-cust-start.step2', 'r10-cust-start.step3', 'ldc.ask.historyHint', 'ldc.ask.ideas', 'auto-ui.askCard']) {
      expect(page, k).not.toContain(t(k));
    }
    expect(host.querySelector('#cz-ask-h')!.textContent).toContain(t('czh.ask.title'));
    for (const [button, tip] of [['r10-cust-start.new', 'czh.ask.how'], ['ldc.ask.seeReady', 'czh.ask.ready'], ['auto-ui.askCardGo', 'czh.ask.suggest']]) {
      const b = [...host.querySelectorAll('#cz-panel button:not(.helptip-btn)')].find((x) => text(x).includes(t(button)));
      expect(b, button).toBeTruthy();
      // The "?" sits right beside the button it explains.
      expect(b!.parentElement!.querySelector('button.helptip-btn')!.getAttribute('aria-label')).toBe(t(tip));
    }
    expect(tipBtn(t('czh.page'))!.closest('h1')).not.toBeNull();
    expect(tipBtn(t('czh.ask.about'))!.closest('h2')).not.toBeNull();
    // No full-width sentence cards any more: one card on the Ask tab.
    expect(host.querySelectorAll('#cz-panel .card')).toHaveLength(1);
  });

  it('the "?" beside New customisation holds the paragraph and the three steps', () => {
    put(Customise);
    tipBtn(t('czh.ask.how'))!.click(); flushSync();
    const pop = host.querySelector('.helptip-pop')!;
    expect(text(pop)).toContain(t('r10-cust-start.lead'));
    const steps = [...pop.querySelectorAll('ol > li')].map(text);
    expect(steps).toEqual([t('r10-cust-start.step1'), t('r10-cust-start.step2'), t('r10-cust-start.step3')]);
  });

  it('the suggestions explain themselves behind their "?"', () => {
    put(Customise);
    tipBtn(t('czh.ask.ready'))!.click(); flushSync();
    expect(text(host)).toContain(t('ldc.ask.ideas'));
    const other = tipBtn(t('czh.ask.suggest'))!;
    other.dispatchEvent(new Event('pointerdown', { bubbles: true })); other.click(); flushSync();
    expect(text(host)).toContain(t('auto-ui.askCard'));
    expect(text(host)).not.toContain(t('ldc.ask.ideas')); // opening one closed the other (a click outside)
  });

  it('works in Arabic', () => {
    app.settings = { language: 'ar' } as any;
    document.documentElement.dir = 'rtl';
    put(Customise);
    expect(tipBtn('كيف يعمل «تخصيص جديد»')).toBeTruthy();
    expect(text(host)).toContain('اطلب تغييراً');
  });
});

describe('a new customisation chat', () => {
  it('asks its question with the explanation behind a "?" and keeps the examples', () => {
    put(CustomiseStart, { onpick: () => {} });
    expect(text(host)).toContain(t('r10-cust-start.emptyTitle'));
    expect(text(host)).not.toContain(t('r10-cust-start.emptyBody'));
    expect(text(host)).toContain(t('r10-cust-start.ex1'));
    tipBtn(t('czh.start'))!.click(); flushSync();
    expect(text(host)).toContain(t('r10-cust-start.emptyBody'));
  });
});

// The tabs that load from the service are checked on their source: what is moved is only inside a
// HelpTip, and what must never be hidden is outside every HelpTip.
const raw = import.meta.glob('./*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const outsideTips = (src: string) => src.replace(/<HelpTip\b[^>]*\/>/g, '').replace(/<HelpTip\b[\s\S]*?<\/HelpTip>/g, '');
const keyIn = (src: string, k: string) => src.includes(`'${k}'`);

describe('what moved behind a "?" and what stays in view', () => {
  const moved: Record<string, string[]> = {
    'Customise.svelte': ['ldc.intro', 'r10-cust-start.lead', 'r10-cust-start.step1', 'ldc.ask.historyHint', 'ldc.ask.ideas', 'auto-ui.askCard'],
    'Plugins.svelte': ['plg.intro', 'plg.offHint'],
    'ReadyPacks.svelte': ['ldc.ready.intro'],
    'SharePacks.svelte': ['plg.share.hint', 'layer.shareHint'],
    'Automations.svelte': ['auto-ui.before.lead', 'auto-ui.run.hint', 'auto-ui.list.hint', 'auto-ui.done.hint'],
    'UpdateChoices.svelte': ['ldb.upd.intro', 'layer.upd.conflictNote'],
    'CustomiseStart.svelte': ['r10-cust-start.emptyBody'],
  };
  const kept: Record<string, string[]> = {
    // Privacy, cost and what is sent to the assistant.
    'Automations.svelte': ['auto-ui.before.sent', 'auto-ui.before.notSent', 'auto-ui.before.cost', 'auto-ui.before.seeHint', 'auto-ui.loadFailed', 'auto-ui.list.lastFailed'],
    'SharePacks.svelte': ['ldc.share.privacy', 'ldc.share.fpHint', 'ldc.share.needName'],
    'Plugins.svelte': ['plg.exp.fpHint', 'plg.empty', 'plg.mayDo', 'plg.mayNothing', 'plg.loadFailed'],
    'ReadyPacks.svelte': ['czh.ready.safe', 'ldc.ready.mayDo', 'ldc.ready.nothing', 'ldc.ready.from'],
    'AutomaticTasks.svelte': ['auto.noBudget', 'layer.wf.empty', 'auto.limitsInvalid'],
    'Routines.svelte': ['auto.rt.draftCost', 'auto.rt.empty'],
    'Rollback.svelte': ['shell.rb.empty', 'shell.rb.safeMode', 'ldc.hist.hidden'],
    'UpdateChoices.svelte': ['ldb.upd.newBrings', 'ldb.upd.keptIntro'],
  };
  it('moves the explanations into tips', () => {
    for (const [file, keys] of Object.entries(moved)) {
      const src = raw['./' + file];
      for (const k of keys) {
        expect(keyIn(src, k), `${file} uses ${k}`).toBe(true);
        expect(keyIn(outsideTips(src), k), `${file}: ${k} is only in a tip`).toBe(false);
      }
    }
  });
  it('never hides warnings, costs, privacy, empty states or state', () => {
    for (const [file, keys] of Object.entries(kept)) {
      const src = outsideTips(raw['./' + file]);
      for (const k of keys) expect(keyIn(src, k), `${file} shows ${k}`).toBe(true);
    }
  });
  it('History on Customise has its introduction behind a "?", the History page and dialog keep it', () => {
    const src = raw['./Rollback.svelte'];
    expect(src).toMatch(/\{#if compact\}[\s\S]*<HelpTip[\s\S]*shell\.rb\.intro[\s\S]*\{:else\}\s*<p class="muted" id="rb-desc">\{t\('shell\.rb\.intro'\)\}<\/p>/);
    expect(raw['./Customise.svelte']).toContain('<Rollback embedded compact />');
    expect(raw['./History.svelte']).toContain('<Rollback embedded />');
  });
});

describe('help icon names', () => {
  it('every name exists in English and Arabic', () => {
    expect(Object.keys(dict.ar).sort()).toEqual(Object.keys(dict.en).sort());
    for (const v of Object.values(dict.ar)) expect(v).toMatch(/[؀-ۿ]/);
  });
});
