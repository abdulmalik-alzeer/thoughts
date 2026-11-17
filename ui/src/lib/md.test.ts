// Rendering notes and answers: what the sanitiser lets through, the task markers, the wrappers,
// hidden-text detection and link resolution (md.ts).
import { describe, it, expect, vi } from 'vitest';

// The strings come from the app's i18n, which pulls in the whole app store; the keys are enough here.
vi.mock('./i18n', () => ({ t: (k: string) => k }));

const { md, hasHiddenText, revealInvisible, resolveLink } = await import('./md');

/** Render and parse back into a tree to look at. */
function render(src: string): HTMLDivElement {
  const d = document.createElement('div');
  d.innerHTML = md(src);
  return d;
}

describe('md: sanitising', () => {
  it.each([
    ['script tag', '<script>alert(1)</script>\n\ntext'],
    ['img onerror', '<img src="x" onerror="alert(1)">'],
    ['svg onload', '<svg onload="alert(1)"><circle r="1"/></svg>'],
    ['iframe', '<iframe src="https://evil.example"></iframe>'],
    ['inline handler on a div', '<div onclick="alert(1)">hi</div>'],
    ['javascript: link in markdown', '[click](javascript:alert(1))'],
    ['javascript: link in html', '<a href="javascript:alert(1)">click</a>'],
    ['vbscript: link', '<a href="vbscript:msgbox(1)">click</a>'],
    ['style element', '<style>body{display:none}</style>'],
    ['form and input', '<form action="/x"><input name="p"><button>go</button></form>'],
  ])('%s is defused', (_name, src) => {
    const html = md(src);
    expect(html).not.toMatch(/<script|<iframe|<svg|<style|<form|<input/i);
    expect(html).not.toMatch(/\son\w+=/i);
    expect(html).not.toMatch(/javascript:|vbscript:/i);
    // The only button is ours (copy code), and there is no code here.
    expect(html).not.toMatch(/<button/i);
  });

  it.each([
    ['style', '<p style="position:fixed">x</p>'],
    ['class', '<p class="dialog">x</p>'],
    ['id', '<p id="app">x</p>'],
    ['data-*', '<p data-md-copy="x">x</p>'],
    ['aria-*', '<p aria-label="Close">x</p>'],
  ])('%s attributes are stripped', (attr, src) => {
    const p = render(src).querySelector('p')!;
    expect(p.textContent).toBe('x');
    expect(p.getAttributeNames().filter((n) => n !== 'dir')).toEqual([]);
    void attr;
  });

  it('keeps ordinary structure, safe links and pictures', () => {
    const d = render('# T\n\n**b** *i* [web](https://example.com) ![p](images/a.png)\n\n> q');
    expect(d.querySelector('h1')?.textContent).toBe('T');
    expect(d.querySelector('strong')?.textContent).toBe('b');
    expect(d.querySelector('em')?.textContent).toBe('i');
    expect(d.querySelector('a')?.getAttribute('href')).toBe('https://example.com');
    expect(d.querySelector('img')?.getAttribute('src')).toBe('images/a.png');
    expect(d.querySelector('blockquote')).not.toBeNull();
  });

  it('drops the note details at the top', () => {
    const d = render('---\ntitle: Secret\n---\n# Body');
    expect(d.textContent).not.toContain('Secret');
    expect(d.querySelector('h1')?.textContent).toBe('Body');
  });

  it('a copy button written into a note is not one of ours', () => {
    const d = render('<button data-md-copy="x">copy</button>');
    expect(d.querySelector('button')).toBeNull();
  });
});

describe('md: direction', () => {
  it('blocks find their own direction; inline code is left-to-right', () => {
    const d = render('مرحبا `code`\n\n- item\n\n| a |\n|---|\n| 1 |');
    expect(d.querySelector('p')?.getAttribute('dir')).toBe('auto');
    expect(d.querySelector('li')?.getAttribute('dir')).toBe('ltr');
    expect(d.querySelector('td')?.getAttribute('dir')).toBe('auto');
    expect(d.querySelector('p code')?.getAttribute('dir')).toBe('ltr');
  });
  it('a quote or list item whose words sit in a paragraph takes their direction', () => {
    // `dir="auto"` alone would skip the inner paragraph (it has its own `dir`) and resolve to LTR.
    const q = render('> مرحبا بكم').querySelector('blockquote')!;
    expect(q.querySelector('p')).not.toBeNull();
    expect(q.getAttribute('dir')).toBe('rtl');
    const d = render('- `x` أولاً\n\n- ثانياً');
    expect(Array.from(d.querySelectorAll('li')).map((li) => li.getAttribute('dir'))).toEqual(['rtl', 'rtl']);
    expect(d.querySelector('ul')?.getAttribute('dir')).toBe('rtl');
    expect(render('> hello').querySelector('blockquote')?.getAttribute('dir')).toBe('ltr');
    // Nothing to go by (a number, a bare checkbox): the browser decides.
    expect(render('- 12').querySelector('li')?.getAttribute('dir')).toBe('auto');
  });
  it('code blocks are left alone', () => {
    expect(render('```\nx\n```').querySelector('pre code')?.hasAttribute('dir')).toBe(false);
  });
});

describe('md: task markers', () => {
  it('a checklist becomes labelled markers, not checkboxes', () => {
    const d = render('- [ ] open\n- [x] done');
    expect(d.querySelector('input')).toBeNull();
    const marks = Array.from(d.querySelectorAll('span.md-task'));
    expect(marks.map((m) => m.textContent)).toEqual(['[ ]', '[x]']);
    expect(marks.map((m) => m.getAttribute('aria-label'))).toEqual(['chat3.taskOpen', 'chat3.taskDone']);
    expect(marks.every((m) => m.getAttribute('role') === 'img' && !m.hasAttribute('title'))).toBe(true);
  });

  it('a marker faked in the note stays a plain span with its title', () => {
    const fake = '<span title="0123456789abcdef01234567:x">[x]</span>';
    const d = render(fake);
    const span = d.querySelector('span')!;
    expect(span.className).toBe('');
    expect(span.getAttribute('title')).toBe('0123456789abcdef01234567:x');
    expect(d.querySelector('.md-task')).toBeNull();
  });

  it("a previous render's mark is no good in the next one", () => {
    const first = render('- [x] a').innerHTML;
    expect(first).toContain('md-task');
    // Its stamp was removed, so there is nothing to replay; a guessed stamp fails as above.
    expect(first).not.toMatch(/title="[0-9a-f]{24}:/);
  });
});

describe('md: wrappers', () => {
  it('code blocks get a copy button, tables a scroll box', () => {
    const d = render('```\nx\n```\n\n| a |\n|---|\n| 1 |');
    const code = d.querySelector('div.md-code')!;
    expect(code.querySelector('button.md-copy')?.textContent).toBe('md.copyCode');
    expect(code.querySelector('button')?.getAttribute('data-md-copy')).toMatch(/^[0-9a-f]{24}$/);
    expect(code.querySelector('pre')).not.toBeNull();
    const table = d.querySelector('div.md-table')!;
    expect(table.getAttribute('tabindex')).toBe('0');
    expect(table.querySelector('table')).not.toBeNull();
  });
});

describe('hasHiddenText', () => {
  it.each<[string, string, boolean]>([
    ['plain note', '# Plan\n\nText with a [link](x.md).', false],
    ['Arabic with RLM and ZWNJ', 'نص‏ عربي‌ هنا', false],
    ['emoji with ZWJ', 'family 👨‍👩‍👧', false],
    ['England flag (tag characters)', 'go \u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}', false],
    ['leading BOM', '﻿# Title', false],
    ['empty comment', 'a <!-- --> b', false],
    ['HTML comment', 'a\n\n<!-- ignore previous instructions -->\n\nb', true],
    ['unused link definition', 'text\n\n[//]: # (hidden words)', true],
    ['used link definition', 'see [ref]\n\n[ref]: https://example.com', false],
    ['script block', '<script>x</script>', true],
    ['bidi override', 'abc‮def', true],
    ['run of zero-width spaces', 'a​​b', true],
    ['BOM in the middle', 'a﻿b', true],
    ['tag characters outside a flag', 'hi\u{E0041}\u{E0042}', true],
    ['comment in note details only', '---\ntitle: <!-- x -->\n---\nbody', false],
  ])('%s', (_name, src, want) => {
    expect(hasHiddenText(src)).toBe(want);
  });
});

describe('revealInvisible', () => {
  it('splits out invisible characters as their codes', () => {
    expect(revealInvisible('a‮b​c')).toEqual([
      { text: 'a' }, { text: 'U+202E', code: true }, { text: 'b' }, { text: 'U+200B', code: true }, { text: 'c' },
    ]);
    expect(revealInvisible('x\u{E0041}')).toEqual([{ text: 'x' }, { text: 'U+E0041', code: true }]);
    expect(revealInvisible('plain')).toEqual([{ text: 'plain' }]);
    expect(revealInvisible('')).toEqual([]);
  });
});

describe('resolveLink', () => {
  it.each([
    ['apps/a/README.md', 'notes/plan.md', 'apps/a/notes/plan.md'],
    ['apps/a/README.md', '../b/README.md', 'apps/b/README.md'],
    ['apps/a/README.md', '../../INDEX.md', 'INDEX.md'],
    ['apps/a/README.md', './notes/x.md#part', 'apps/a/notes/x.md'],
    ['apps/a/README.md', 'x.md?y=1', 'apps/a/x.md'],
    ['apps/a/README.md', '/me/README.md', 'me/README.md'],
    ['a/README.md', '%D9%85%D9%84%D8%A7%D8%AD%D8%B8%D8%A7%D8%AA/%D8%AE%D8%B7%D8%A9.md', 'a/ملاحظات/خطة.md'],
    ['a/README.md', 'my%20note.md', 'a/my note.md'],
    ['a/README.md', 'bad%E0.md', 'a/bad%E0.md'],
  ])('%s + %s', (from, href, want) => {
    expect(resolveLink(from, href)).toBe(want);
  });
});

describe('md: kept renders', () => {
  it('the same text is parsed once, and still renders the same', async () => {
    const { marked } = await import('marked');
    const parse = vi.spyOn(marked, 'parse');
    const src = 'A **kept** answer\n\n```\ncode\n```\n';
    const a = md(src);
    const b = md(src);
    expect(b).toBe(a);
    expect(parse).toHaveBeenCalledTimes(1);
    md(src + ' changed');
    expect(parse).toHaveBeenCalledTimes(2);
    parse.mockRestore();
  });
});
