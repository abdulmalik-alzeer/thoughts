// A deck saved as one HTML file: nothing of the note can run or load anything.
import { describe, it, expect } from 'vitest';

window.matchMedia ??= ((q: string) => ({ matches: false, media: q, addEventListener() {}, removeEventListener() {} })) as any;
const { toHtml, CSP } = await import('./standalone');
const { parseDeck } = await import('./deck');
const { md } = await import('../md');

const sectionsOf = (src: string) => parseDeck(src).slides.map((s) => { const el = document.createElement('section'); el.innerHTML = md(s.body); return el; });
const parse = (html: string) => new DOMParser().parseFromString(html, 'text/html');

describe('a saved deck', () => {
  const src = `---\nslides: true\ntitle: Evil <b>deck</b>\n---\n# One\n\n<script>alert(1)</script>\n\n<img src=x onerror="alert(2)">\n\n![remote](https://example.com/a.png)\n\n![local](data:image/png;base64,iVBORw0KGgo=)\n\n[bad](javascript:alert(3)) [good](https://example.com)\n\nNote: <script>alert(4)</script> say *this*\n\n---\n\n## Two\n\n<iframe src="https://example.com"></iframe>\n\n\`\`\`js\nconsole.log(1)\n\`\`\``;
  const deck = parseDeck(src);
  const html = toHtml(deck, sectionsOf(src), { '--bg': '#fff', '--fg': '#111', '--accent': 'red; } body { x' });
  const doc = parse(html);

  it('runs only reveal.js and the fixed start-up call', () => {
    const scripts = Array.from(doc.querySelectorAll('script'));
    expect(scripts).toHaveLength(2);
    expect(scripts[0].textContent).toContain('reveal.js');
    expect(scripts[1].textContent).toMatch(/^Reveal\.initialize\(\{[^}]*\}\);$/);
    expect(html).not.toContain('alert(');
  });

  it('keeps nothing that runs or loads: no handlers, javascript: links, frames or remote pictures', () => {
    for (const el of Array.from(doc.body.querySelectorAll('*'))) for (const a of Array.from(el.attributes)) expect(a.name.startsWith('on'), `${el.tagName} ${a.name}`).toBe(false);
    expect(doc.querySelector('iframe')).toBeNull();
    expect(doc.querySelector('a[href^="javascript"]')).toBeNull();
    expect(doc.querySelector('a[href="https://example.com"]')).not.toBeNull();
    const imgs = Array.from(doc.querySelectorAll('img')).map((i) => i.getAttribute('src'));
    expect(imgs).toEqual(['data:image/png;base64,iVBORw0KGgo=']);
    expect(doc.querySelector('button, .md-codehead')).toBeNull();
  });

  it('says in a CSP that nothing may be loaded', () => {
    const csp = doc.querySelector('meta[http-equiv="Content-Security-Policy"]');
    expect(csp?.getAttribute('content')).toBe(CSP);
    expect(CSP).toContain("default-src 'none'");
    expect(doc.querySelector('link')).toBeNull();
  });

  it('carries the slides, the notes as text and the colours written out', () => {
    expect(doc.querySelectorAll('.reveal .slides > section')).toHaveLength(2);
    const notes = doc.querySelector('aside.notes');
    expect(notes?.textContent).toContain('say');
    expect(doc.title).toBe('Evil <b>deck</b>');
    expect(html).toContain('--bg: #fff;');
    expect(html).not.toContain('body { x');
  });

  it('runs right to left for an Arabic deck', () => {
    const ar = '---\nslides: true\n---\n# مراجعة\n\n---\n\n## التالي';
    const d = parse(toHtml(parseDeck(ar), sectionsOf(ar)));
    expect(d.documentElement.getAttribute('dir')).toBe('rtl');
    expect(d.documentElement.getAttribute('lang')).toBe('ar');
    expect(d.querySelector('.reveal')?.getAttribute('dir')).toBe('rtl');
    expect(d.querySelectorAll('script')[1].textContent).toContain('rtl: true');
  });
});
