// The design step's own logic: the sealed page, the variables, contrast, undo.
import { describe, it, expect, vi } from 'vitest';

vi.mock('../../store.svelte', () => ({ app: { settings: { language: 'en' } } }));

import type { DesignTokens } from '../types';
import { contrast, fixContrast, fromHsl, hsl, isHex, luminance, readableOn } from './contrast';
import { FONTS, fontStack } from './fonts';
import { History } from './history';
import { DEVICES, MOCK_CSP, looksClean, pinAt, scaleFor, sealedPage } from './srcdoc';
import { COLOR_ROLES, CSS_VARS, FALLBACK, HARMONIES, fixAll, fixed, harmony, inlineVars, lowRoles, reading, rootVars, same, sane, setToken, tokenVars } from './tokens';

const good: DesignTokens = JSON.parse(JSON.stringify(FALLBACK));

describe('the sealed page of a mock-up', () => {
  const parse = (page: string) => new DOMParser().parseFromString(page, 'text/html');

  it('starts with the content policy, then the variables, then the mock-up', () => {
    const page = sealedPage('<style>h1{color:red}</style><body class="x"><h1>Home</h1></body>', good);
    expect(page.startsWith(`<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="${MOCK_CSP}">`)).toBe(true);
    expect(MOCK_CSP).toBe("default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:");
    const doc = parse(page);
    const first = doc.head.firstElementChild!;
    expect([first.tagName, first.getAttribute('http-equiv'), first.getAttribute('content')]).toEqual(['META', 'Content-Security-Policy', MOCK_CSP]);
    const styles = [...doc.querySelectorAll('style')].map((s) => s.textContent ?? '');
    expect(styles[0]).toContain('--mado-color-primary:#3b5bdb');
    expect(styles[1]).toBe('h1{color:red}');
    expect(doc.body.className).toBe('x');
    expect(doc.querySelector('h1')!.textContent).toBe('Home');
    // The policy comes before every byte of the mock-up.
    expect(page.indexOf('Content-Security-Policy')).toBeLessThan(page.indexOf('h1{color:red}'));
  });

  it('keeps a hostile mock-up after the policy, whatever it contains', () => {
    for (const hostile of [
      '</iframe><script>window.parent.hacked = 1</script>',
      '</style></head><meta http-equiv="Content-Security-Policy" content="default-src *; script-src \'unsafe-inline\'"><script>alert(1)</script>',
      '<head><meta http-equiv="Content-Security-Policy" content="script-src *"></head>',
      '</html><html><head><base href="https://evil.example/"><meta http-equiv="refresh" content="0;url=https://evil.example/">',
      '<!--', '<![CDATA[', '<plaintext>', '<template>', '<textarea>', '<title>', '<script>', '<style>', '<svg><style>', '<noscript>', '<xmp>',
      '"></iframe><img src=x onerror=alert(1)>', "'><script>alert(1)</script>", '\u0000</iframe>',
    ]) {
      const page = sealedPage(hostile, good);
      // Our head is whole and closed before the mock-up begins, so nothing in it can swallow or precede the policy.
      const headEnd = page.indexOf('</head>');
      expect(headEnd).toBeGreaterThan(0);
      expect(page.slice(headEnd + '</head>'.length)).toBe(hostile);
      const head = page.slice(0, headEnd);
      expect(head.match(/Content-Security-Policy/g)!.length).toBe(1);
      const doc = parse(page);
      const first = doc.head.firstElementChild!;
      expect([first.getAttribute('http-equiv'), first.getAttribute('content')], hostile).toEqual(['Content-Security-Policy', MOCK_CSP]);
    }
  });

  it('is only ever an attribute value: a frame given it holds it as text', () => {
    const host = document.createElement('div');
    document.body.append(host);
    const hostile = '</iframe><script>window.__broke = true</script><img src=x onerror="window.__broke = true"><p id="escaped">out</p>';
    const frame = document.createElement('iframe');
    frame.setAttribute('sandbox', '');
    frame.setAttribute('referrerpolicy', 'no-referrer');
    frame.setAttribute('srcdoc', sealedPage(hostile, good));
    host.append(frame);
    expect(host.children.length).toBe(1);
    expect(host.querySelector('script, img, #escaped')).toBeNull();
    expect(document.getElementById('escaped')).toBeNull();
    expect((window as any).__broke).toBeUndefined();
    expect(frame.getAttribute('srcdoc')!.endsWith(hostile)).toBe(true);
    expect(frame.getAttribute('sandbox')).toBe('');
    host.remove();
  });

  it('takes what the daemon cleaned and refuses anything that does not look like it', () => {
    for (const ok of [
      '<body></body>',
      '<style>h1{color:red}</style><style>.a{b:c}</style><body class="x" dir="rtl"><h1 title="a > b onclick=1">Home</h1><img src="data:image/png;base64,AAAA"><a href="#top">t</a></body>',
      '<body><p title="&lt;script&gt; onerror=x">words about a script</p><svg viewBox="0 0 1 1"><path d="M0 0" fill="url(#g)"></path></svg></body>',
    ]) expect(looksClean(ok), ok).toBe(true);
    for (const bad of [
      '', null, 42, '<h1>Home</h1>', '<body><script>alert(1)</script></body>', '<body><SCRIPT src=x></SCRIPT></body>', '<body><p onclick="x">a</p></body>',
      '<body><p\nONLOAD = x>a</p></body>', '<body><img src="https://evil.example/x.png"></body>', '<body><img src="data:text/html,x"></body>',
      '<body><a href="javascript:alert(1)">a</a></body>', '<body><iframe srcdoc="x"></iframe></body>', '<body><meta http-equiv="refresh" content="0"></body>',
      '<body><base href="//evil"></body>', '<body><link rel="stylesheet" href="//evil"></body>', '<body><form action="//evil"></form></body>',
      '<body><object data="x"></object></body>', '<body><embed src="x"></body>', '<body><math></math></body>', '<body><svg><foreignObject></foreignObject></svg></body>',
      '<style>a{}</style><p>before the body</p><body></body>', '<body></body><p>after</p>', '<style>a{}<body></body>', '<body><img srcset="x 2x"></body>',
    ]) expect(looksClean(bad), String(bad)).toBe(false);
  });

  it('fits a device into the room there is, and places a pin', () => {
    expect(scaleFor('desktop', 640, null)).toBe(0.5);
    expect(scaleFor('phone', 1000, null)).toBe(1);
    expect(scaleFor('desktop', 0, null)).toBe(1);
    expect(scaleFor('desktop', 640, 1.5)).toBe(1.5);
    expect(scaleFor('desktop', 640, 9)).toBe(2);
    expect(scaleFor('desktop', 640, 0.01)).toBe(0.25);
    expect(DEVICES.phone.w).toBeLessThan(DEVICES.tablet.w);
    const box = { left: 100, top: 50, width: 400, height: 200 };
    expect(pinAt(300, 150, box)).toEqual({ x: 0.5, y: 0.5 });
    expect(pinAt(100, 50, box)).toEqual({ x: 0, y: 0 });
    expect(pinAt(9999, -5, box)).toEqual({ x: 1, y: 0 });
    expect(pinAt(233.3333, 77, box)).toEqual({ x: 0.333, y: 0.135 });
    expect(pinAt(1, 1, { left: 0, top: 0, width: 0, height: 0 })).toBeNull();
  });
});

describe('design settings as CSS variables', () => {
  it('writes the contract\'s variables', () => {
    const vars = Object.fromEntries(tokenVars(good));
    expect(vars['--mado-color-bg']).toBe('#f7f7f5');
    expect(vars['--mado-color-on-primary']).toBe('#ffffff');
    expect(vars['--mado-color-onPrimary']).toBeUndefined();
    expect(tokenVars(good).map(([k]) => k)).toEqual([...CSS_VARS]);
    expect(vars['--mado-font-heading']).toBe(fontStack('System'));
    expect(vars['--mado-font-mono']).toBe(fontStack('Mono'));
    expect(vars['--mado-radius']).toBe('8px');
    expect(vars['--mado-spacing']).toBe('8px');
    expect(vars['--mado-shadow']).toContain('rgba');
    expect(vars['--mado-density']).toBe('1');
    for (const r of COLOR_ROLES) expect(Object.keys(vars).some((k) => k.startsWith('--mado-color-') && vars[k] === (good.color as any)[r])).toBe(true);
    expect(rootVars(good).startsWith(':root{--mado-color-bg:#f7f7f5;')).toBe(true);
    expect(inlineVars(good)).not.toContain('{');
    expect(Object.fromEntries(tokenVars({ ...good, shadow: 'none', density: 'compact' }))['--mado-shadow']).toBe('none');
  });

  it('never writes a value that was not checked', () => {
    const hostile: any = {
      color: { ...good.color, bg: 'red;} body{background:url(//evil.example/x)} :root{--x:', primary: '#12345', text: '#ffffff</style><script>alert(1)</script>', accent: 'url(//evil)', muted: 7 },
      font: { heading: 'x; } * { background: url(//evil.example/f) } :root { --y:', body: '</style><script>alert(1)</script>', mono: 'System' },
      radius: '8px; background: url(//evil)', spacing: Infinity, density: 'compact;position:fixed', shadow: '0 0 0 9999px red',
      extra: '</style>',
    };
    for (const css of [rootVars(hostile), inlineVars(hostile), sealedPage('<body></body>', hostile)]) {
      expect(css).not.toMatch(/evil|script|url\(|position|9999|--x|--y/);
    }
    const s = sane(hostile);
    expect(s.color.bg).toBe(FALLBACK.color.bg);
    expect(s.color.primary).toBe(FALLBACK.color.primary);
    expect(s.font).toEqual({ heading: 'System', body: 'System', mono: 'Mono' });
    expect([s.radius, s.spacing, s.density, s.shadow]).toEqual([8, 8, 'regular', 'soft']);
    expect(Object.keys(s)).toEqual(['color', 'font', 'radius', 'spacing', 'density', 'shadow']);
    // Numbers are clamped; nothing and nonsense give the fallback.
    expect(sane({ ...good, radius: 500, spacing: -3 }).radius).toBe(32);
    expect(sane({ ...good, radius: 500, spacing: -3 }).spacing).toBe(2);
    expect(sane(null)).toEqual(FALLBACK);
    expect(sane('tokens')).toEqual(FALLBACK);
    // Every stack in the list is plain enough to sit in a declaration.
    for (const f of FONTS) expect(f.stack).not.toMatch(/[;{}<>\\]|url\(/);
    expect(fontStack('Comic Sans MS')).toBe(fontStack('System'));
    expect(fontStack('nope', 'mono')).toBe(fontStack('Mono'));
  });

  it('changes one setting by its path and nothing else', () => {
    const next = setToken(good, 'color.primary', '#000000');
    expect(next.color.primary).toBe('#000000');
    expect(good.color.primary).toBe('#3b5bdb');
    expect(setToken(good, 'radius', 3).radius).toBe(3);
    expect(setToken(good, 'font.body', 'Slab').font.body).toBe('Slab');
    expect(same(setToken(good, 'color.__proto__', 'x'), good)).toBe(true);
    expect(same(setToken(good, 'constructor', 'x'), good)).toBe(true);
    expect(same(good, { ...good, color: { ...good.color, bg: '#F7F7F5' } })).toBe(true);
    expect(same(good, null)).toBe(false);
  });
});

describe('contrast', () => {
  it('is the WCAG ratio', () => {
    expect(contrast('#000000', '#ffffff')).toBeCloseTo(21, 6);
    expect(contrast('#ffffff', '#ffffff')).toBe(1);
    expect(contrast('#767676', '#ffffff')).toBeGreaterThanOrEqual(4.5);
    expect(contrast('#777777', '#ffffff')).toBeLessThan(4.5);
    expect(contrast('#123456', '#abcdef')).toBe(contrast('#abcdef', '#123456'));
    expect(luminance('#ff0000')).toBeCloseTo(0.2126, 4);
    expect(isHex('#abcdef') && isHex('#ABCDEF')).toBe(true);
    for (const bad of ['abcdef', '#abc', '#abcdeg', '#abcdef0', ' #abcdef', null, 1]) expect(isHex(bad)).toBe(false);
  });

  it('goes to hue, saturation and lightness and back', () => {
    for (const c of ['#000000', '#ffffff', '#3b5bdb', '#c2410c', '#0b7285', '#808080', '#ff00ff']) {
      const [h, s, l] = hsl(c);
      const back = fromHsl(h, s, l);
      for (const i of [1, 3, 5]) expect(Math.abs(parseInt(back.slice(i, i + 2), 16) - parseInt(c.slice(i, i + 2), 16))).toBeLessThanOrEqual(1);
    }
  });

  it('nudges a colour until it can be read, and only as far as needed', () => {
    const fixedGrey = fixContrast('#aaaaaa', ['#ffffff'], 4.5);
    expect(contrast(fixedGrey, '#ffffff')).toBeGreaterThanOrEqual(4.5);
    expect(contrast(fixedGrey, '#ffffff')).toBeLessThan(5.2);
    // The hue stays.
    const blue = fixContrast('#8fb0ff', ['#ffffff', '#f7f7f5'], 4.5);
    expect(Math.abs(hsl(blue)[0] - hsl('#8fb0ff')[0])).toBeLessThan(4);
    expect(Math.min(contrast(blue, '#ffffff'), contrast(blue, '#f7f7f5'))).toBeGreaterThanOrEqual(4.5);
    // On a dark ground it goes lighter.
    const onDark = fixContrast('#444444', ['#111111'], 4.5);
    expect(luminance(onDark)).toBeGreaterThan(luminance('#444444'));
    expect(contrast(onDark, '#111111')).toBeGreaterThanOrEqual(4.5);
    // Already readable: untouched. Impossible (black and white grounds at 7:1): the best there is, no loop.
    expect(fixContrast('#111111', ['#ffffff'], 4.5)).toBe('#111111');
    expect(isHex(fixContrast('#888888', ['#000000', '#ffffff'], 7))).toBe(true);
    expect(fixContrast('nonsense', ['#ffffff'], 4.5)).toBe('nonsense');
    expect(readableOn('#ffffff')).toBe('#111111');
    expect(readableOn('#101010')).toBe('#ffffff');
  });

  it('reads every role against its ground and fixes what is low', () => {
    expect(lowRoles(good)).toEqual([]);
    expect(reading(good, 'bg')).toBeNull();
    expect(reading(good, 'border')).toBeNull();
    expect(reading(good, 'text')!.level).toBe('AAA');
    const pale = { ...good, color: { ...good.color, text: '#c0c0c0', onPrimary: '#3b5bdb', warning: '#ffe08a' } };
    expect(lowRoles(pale)).toEqual(['text', 'onPrimary', 'warning']);
    expect(reading(pale, 'text')).toMatchObject({ ok: false, target: 4.5, level: 'low' });
    const one = fixed(pale, 'text');
    expect(lowRoles(one)).toEqual(['onPrimary', 'warning']);
    expect(one.color.bg).toBe(pale.color.bg);
    const all = fixAll(pale);
    expect(lowRoles(all)).toEqual([]);
    expect([all.color.bg, all.color.surface, all.color.primary, all.color.border]).toEqual([pale.color.bg, pale.color.surface, pale.color.primary, pale.color.border]);
    // What is shown never rounds a failure up to a pass.
    const edge = { ...good, color: { ...good.color, muted: '#777777', bg: '#ffffff', surface: '#ffffff' } };
    expect(reading(edge, 'muted')).toMatchObject({ ok: false, ratio: 4.4 });
  });

  it('derives harmonies that are always readable and valid', () => {
    const dark = { ...good, color: { ...good.color, bg: '#0d1117', surface: '#161b22', text: '#e6edf3', muted: '#9da7b3', primary: '#3fb950', onPrimary: '#04210c', border: '#30363d' } };
    for (const base of [good, dark, { ...good, color: { ...good.color, primary: '#ffee00' } }, { ...good, color: { ...good.color, primary: '#808080' } }]) {
      for (const h of HARMONIES) {
        const out = harmony(base, h);
        expect(lowRoles(out), `${h} from ${base.color.primary}`).toEqual(lowRoles(base).filter((r) => r === 'primary'));
        expect(same(sane(out), out)).toBe(true);
        expect(out.color.primary).toBe(base.color.primary);
        expect(out.font).toEqual(base.font);
      }
    }
    expect(harmony(good, 'complement').color.accent).not.toBe(good.color.accent);
    expect(harmony(good, 'complement').color.bg).toBe(good.color.bg);
    expect(harmony(good, 'tinted').color.bg).not.toBe(good.color.bg);
    expect(luminance(harmony(dark, 'tinted').color.bg)).toBeLessThan(0.05);
  });
});

describe('undo and redo in the tuner', () => {
  it('steps back and forward, and a new change drops what could be redone', () => {
    const h = new History('a');
    expect([h.canUndo, h.canRedo, h.current]).toEqual([false, false, 'a']);
    h.push('b'); h.push('c');
    expect(h.undo()).toBe('b');
    expect(h.undo()).toBe('a');
    expect(h.undo()).toBe('a');
    expect(h.redo()).toBe('b');
    h.push('d');
    expect([h.canRedo, h.current, h.length]).toEqual([false, 'd', 3]);
    expect(h.redo()).toBe('d');
    expect(h.undo()).toBe('b');
  });

  it('makes one step of a drag: changes with the same key close together', () => {
    let now = 0;
    const h = new History('start', 200, 900, () => now);
    for (const v of ['#100000', '#200000', '#300000']) { now += 50; h.push(v, 'color.primary'); }
    expect(h.length).toBe(2);
    expect(h.undo()).toBe('start');
    expect(h.redo()).toBe('#300000');
    // Another control, a pause, or an undo in between starts a new step.
    now += 50; h.push('r1', 'radius');
    now += 50; h.push('r2', 'radius');
    now += 2000; h.push('r3', 'radius');
    expect(h.length).toBe(4);
    expect(h.undo()).toBe('r2');
    now += 10; h.push('r4', 'radius');
    expect([h.length, h.undo()]).toEqual([4, 'r2']);
    // A change without a key is always its own step; the first step is never overwritten.
    const g = new History('s', 200, 900, () => now);
    g.push('x', 'k'); g.push('y', 'k');
    expect([g.length, g.undo()]).toEqual([2, 's']);
    g.push('p'); g.push('q');
    expect(g.length).toBe(3);
  });

  it('keeps a bounded past and starts again on reset', () => {
    const h = new History(0, 5);
    for (let i = 1; i <= 20; i++) h.push(i);
    expect(h.length).toBe(5);
    let steps = 0;
    while (h.canUndo) { h.undo(); steps++; }
    expect([steps, h.current]).toEqual([4, 16]);
    expect(h.reset(99)).toBe(99);
    expect([h.canUndo, h.canRedo, h.length]).toEqual([false, false, 1]);
  });
});
