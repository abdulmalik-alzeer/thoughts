// Print and Save as PDF (printPdf.ts): the note's title, the file name made from it, and the copy
// that is laid out on paper.
import { describe, it, expect, vi } from 'vitest';

vi.mock('./i18n', () => ({ t: (k: string) => k }));
vi.mock('./store.svelte', () => ({ toast: vi.fn() }));

const { titleOf, pdfFileName, buildPrintRoot } = await import('./printPdf');
const { md } = await import('./md');

describe('titleOf', () => {
  it('takes the frontmatter title first', () => {
    expect(titleOf('---\ntitle: "Weekly plan"\nstatus: active\n---\n# Something else\n', 'x')).toBe('Weekly plan');
    expect(titleOf("---\ntitle: 'خطة الأسبوع'\n---\nbody", 'x')).toBe('خطة الأسبوع');
  });
  it('then the first heading, without its inline markup', () => {
    expect(titleOf('Intro line\n\n## The **real** `title` [here](x.md)\n# Later', 'x')).toBe('The real title here');
    expect(titleOf('---\nstatus: seed\n---\n# ملاحظات الاجتماع #\n', 'x')).toBe('ملاحظات الاجتماع');
  });
  it('ignores headings inside code blocks', () => {
    expect(titleOf('```\n# not a title\n```\nno heading', 'readme')).toBe('readme');
  });
  it('falls back to the given name', () => {
    expect(titleOf('', 'my note')).toBe('my note');
    expect(titleOf('#hashtag only', 'n')).toBe('n');
  });
});

describe('pdfFileName', () => {
  it.each([
    ['Weekly plan', 'Weekly plan.pdf'],
    ['خطة الأسبوع', 'خطة الأسبوع.pdf'],
    ['a/b\\c:d*e?f"g<h>i|j', 'a b c d e f g h i j.pdf'],
    ['../../secret', 'secret.pdf'],
    ['  .hidden.  ', 'hidden.pdf'],
    ['report.PDF', 'report.pdf'],
    ['line\nbreak\ttab', 'line break tab.pdf'],
    ['‮evil‏', 'evil.pdf'],
    ['CON', 'CON note.pdf'],
    ['', 'document.pdf'],
    ['///', 'document.pdf'],
  ])('%j → %j', (title, name) => expect(pdfFileName(title)).toBe(name));

  it('keeps names short', () => {
    expect(Array.from(pdfFileName('ب'.repeat(300))).length).toBe(104);
  });
});

describe('buildPrintRoot', () => {
  function rendered(src: string): HTMLElement {
    const el = document.createElement('article');
    el.innerHTML = md(src);
    return el;
  }

  it('copies the rendered note without the app controls', () => {
    const el = rendered('# Title\n\n```js\nconst a = 1;\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] open\n');
    const root = buildPrintRoot({ content: el, title: 'Title' });
    expect(root.id).toBe('print-root');
    expect(root.querySelector('[data-md-copy], button')).toBeNull();
    expect(root.querySelector('[tabindex]')).toBeNull();
    expect(root.querySelector('pre code')?.textContent).toContain('const a = 1;');
    expect(root.querySelectorAll('td').length).toBe(2);
    expect(root.querySelectorAll('.md-task').length).toBe(2);
    // The view's own copy is untouched.
    expect(el.querySelector('[data-md-copy]')).not.toBeNull();
  });

  it('runs the way most of the note reads, each block finding its own direction', () => {
    const ar = buildPrintRoot({ content: rendered('# ملاحظة\n\nهذه فقرة عربية فيها كلمة English واحدة.\n\nAnd one English line.'), title: 't' });
    expect(ar.dir).toBe('rtl');
    expect(Array.from(ar.querySelectorAll('p')).every((p) => p.getAttribute('dir') === 'auto')).toBe(true);
    const en = buildPrintRoot({ content: rendered('# Note\n\nAn English paragraph with عربي in it.'), title: 't' });
    expect(en.dir).toBe('ltr');
  });

  it('puts a heading on top when asked', () => {
    const root = buildPrintRoot({ content: rendered('body'), title: 'Plan', heading: 'Plan — log' });
    const h = root.firstElementChild as HTMLElement;
    expect(h.tagName).toBe('H1');
    expect(h.textContent).toBe('Plan — log');
    expect(h.getAttribute('dir')).toBe('auto');
  });

  it('keeps links, and marks external ones whose address is not already shown', () => {
    const root = buildPrintRoot({ content: rendered('[site](https://example.com/page) <https://example.com/x> [note](notes/a.md) [mail](mailto:a@b.c)'), title: 't' });
    const links = Array.from(root.querySelectorAll('a'));
    expect(links.map((a) => a.getAttribute('href'))).toEqual(['https://example.com/page', 'https://example.com/x', 'notes/a.md', 'mailto:a@b.c']);
    expect(links.map((a) => a.classList.contains('print-url'))).toEqual([true, false, false, true]);
  });

  it('loads images at once and unfolds folded sections', () => {
    const el = document.createElement('div');
    el.innerHTML = '<p><img src="blob:x" loading="lazy" data-zoom="1" tabindex="0" role="button"></p><details><summary>More</summary><p>hidden</p></details>';
    const root = buildPrintRoot({ content: el, title: 't' });
    const img = root.querySelector('img')!;
    expect(img.getAttribute('src')).toBe('blob:x');
    expect(img.getAttribute('loading')).toBe('eager');
    expect(img.hasAttribute('role')).toBe(false);
    expect(root.querySelector('details')!.hasAttribute('open')).toBe(true);
  });

  it('takes several nodes in order', () => {
    const a = document.createElement('p'); a.textContent = 'one';
    const b = document.createElement('p'); b.textContent = 'two';
    const root = buildPrintRoot({ content: [a, b], title: 't' });
    expect(Array.from(root.querySelectorAll('p')).map((p) => p.textContent)).toEqual(['one', 'two']);
    expect(a.parentNode).toBeNull();
  });
});
