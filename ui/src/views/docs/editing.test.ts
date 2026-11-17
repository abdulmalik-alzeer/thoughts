// The note editor's text changes (editing.ts), as tables. In the cases `‸` is the caret and
// `⟨…⟩` a selection, before and after each change; `null` means "nothing to do, let the text box
// behave as usual". (These were scripts/check-editor.mjs; now they run with `npm test`.)
import { describe, it, expect } from 'vitest';
import * as E from './editing';
import type { Change } from './editing';

/** "ab‸c" or "a⟨b⟩c" → [text, start, end] */
function parse(s: string): [string, number, number] {
  const c = s.indexOf('‸');
  if (c >= 0) return [s.replace('‸', ''), c, c];
  const a = s.indexOf('⟨'), b = s.indexOf('⟩');
  if (!(a >= 0 && b > a)) throw new Error(`no caret in ${JSON.stringify(s)}`);
  return [s.replace('⟨', '').replace('⟩', ''), a, b - 1];
}
function show(r: Change | null): string | null {
  if (!r) return null;
  return r.start === r.end ? r.text.slice(0, r.start) + '‸' + r.text.slice(r.start)
    : r.text.slice(0, r.start) + '⟨' + r.text.slice(r.start, r.end) + '⟩' + r.text.slice(r.end);
}
type Fn = (t: string, s: number, e: number, ...args: any[]) => Change | null;
/** `fn(text, start, end, ...args)` turns `from` into `to` (null: nothing to do). */
function cases(label: string, fn: Fn, list: [string, string | null, ...unknown[]][]) {
  describe(label, () => {
    for (const [from, to, ...args] of list) {
      it(JSON.stringify(from), () => {
        const [t, s, e] = parse(from);
        expect(show(fn(t, s, e, ...args))).toBe(to);
      });
    }
  });
}

// ---------- Enter ----------
cases('enter', E.enter, [
  ['- one‸', '- one\n- ‸'],
  ['* one‸\n* two', '* one\n* ‸\n* two'],
  ['+ one‸', '+ one\n+ ‸'],
  ['- [ ] buy milk‸', '- [ ] buy milk\n- [ ] ‸'],
  ['- [x] done‸', '- [x] done\n- [ ] ‸'],
  ['1. one‸', '1. one\n2. ‸'],
  ['9) nine‸', '9) nine\n10) ‸'],
  ['  - nested‸', '  - nested\n  - ‸'],
  // The text after the caret moves to the new item.
  ['- one ‸two', '- one \n- ‸two'],
  // Counted lists count on, and so do the items after the new one.
  ['1. a‸\n2. b\n3. c\n\ntext', '1. a\n2. ‸\n3. b\n4. c\n\ntext'],
  ['1. a\n2. b‸\n   child\n3. c', '1. a\n2. b\n3. ‸\n   child\n4. c'],
  ['1. a‸\n2. b\n  1. x\n3. c', '1. a\n2. ‸\n3. b\n  1. x\n4. c'],
  // "1. 1. 1." lists keep their number.
  ['1. a\n1. b‸', '1. a\n1. b\n1. ‸'],
  // An empty item ends the list; a nested empty item steps out a level.
  ['- one\n- ‸', '- one\n‸'],
  ['- one\n- ‸\n- three', '- one\n‸\n- three'],
  ['- [ ] a\n- [ ] ‸', '- [ ] a\n‸'],
  ['1. a\n2. ‸', '1. a\n‸'],
  ['- a\n  - ‸', '- a\n- ‸'],
  ['1. a\n   1. ‸', '1. a\n2. ‸'],
  // Not a list, inside the marker, a rule, code, note details, a selection: the usual new line.
  ['plain‸', null],
  ['‸- one', null],
  ['-‸ one', null],
  ['- - -‸', null],
  ['***‸', null],
  ['-not a list‸', null],
  ['```\n- in code‸\n```', null],
  ['~~~md\n- in code‸\n~~~', null],
  ['```\nx\n```\n- after code‸', '```\nx\n```\n- after code\n- ‸'],
  ['---\ntags:\n- a‸\n---\nbody', null],
  ['- ⟨one⟩', null],
  // Arabic text: the markers are at the start of the line however it is shown.
  ['- بند أول‸', '- بند أول\n- ‸'],
  ['1. الأول‸\n2. الثاني', '1. الأول\n2. ‸\n3. الثاني'],
  ['- [ ] مهمة‸', '- [ ] مهمة\n- [ ] ‸'],
  ['- بند\n- ‸', '- بند\n‸'],
]);

// ---------- Tab / Shift+Tab ----------
cases('indent', (t, s, e) => E.indent(t, s, e, 1), [
  ['- a\n- b‸', '- a\n  - b‸'],
  ['1. a\n2. b‸', '1. a\n   1. b‸'],
  ['10. a\n11. b‸', '10. a\n    1. b‸'],
  ['- a\n  - x\n- b‸', '- a\n  - x\n  - b‸'],
  ['1. a\n   1. x\n2. b‸', '1. a\n   1. x\n   2. b‸'],
  ['- [ ] a\n- [ ] ‸', '- [ ] a\n  - [ ] ‸'],
  ['- a\n‸- b', '- a\n‸  - b'],
  ['\t- a\n\t- b‸', '\t- a\n\t\t- b‸'],
  ['- a\n⟨- b\n- c⟩', '- a\n⟨  - b\n  - c⟩'],
  ['- a\n- b\n  more‸', null],
  ['plain‸', null],
  ['```\n- a‸\n```', null],
  ['- بند\n- فرعي‸', '- بند\n  - فرعي‸'],
]);
cases('outdent', (t, s, e) => E.indent(t, s, e, -1), [
  ['- a\n  - b‸', '- a\n- b‸'],
  ['1. a\n   1. b‸', '1. a\n2. b‸'],
  ['- a\n  - x\n    - y‸', '- a\n  - x\n  - y‸'],
  ['- a‸', null],
  ['plain‸', null],
  ['- a\n⟨  - b\n  - c⟩', '- a\n⟨- b\n- c⟩'],
]);

// ---------- lists from the toolbar ----------
cases('bullets', (t, s, e) => E.toggleList(t, s, e, 'bullet'), [
  ['one‸', '- one‸'],
  ['‸', '- ‸'],
  ['- one‸', 'one‸'],
  ['⟨one\n\ntwo⟩', '⟨- one\n\n- two⟩'],
  ['⟨- one\ntwo⟩', '⟨- one\n- two⟩'],
  ['1. one‸', '- one‸'],
  ['## Title‸', '- Title‸'],
  ['  indented‸', '  - indented‸'],
  ['---\ntitle: x‸\n---\n', null],
]);
cases('numbers', (t, s, e) => E.toggleList(t, s, e, 'number'), [
  ['⟨a\nb\nc⟩', '⟨1. a\n2. b\n3. c⟩'],
  ['1. a\n2. b\nc‸', '1. a\n2. b\n3. c‸'],
  ['⟨1. a\n2. b⟩', '⟨a\nb⟩'],
  ['- a‸', '1. a‸'],
]);
cases('checklist', (t, s, e) => E.toggleList(t, s, e, 'task'), [
  ['buy milk‸', '- [ ] buy milk‸'],
  ['- buy milk‸', '- [ ] buy milk‸'],
  ['- [x] done‸', 'done‸'],
  ['⟨- [ ] a\n- [x] b⟩', '⟨a\nb⟩'],
  ['⟨a\n- [x] b⟩', '⟨- [ ] a\n- [x] b⟩'],
  ['مهمة‸', '- [ ] مهمة‸'],
]);

// ---------- headings ----------
cases('heading', (t, s, e, l) => E.setHeading(t, s, e, l), [
  ['Title‸', '# Title‸', 1],
  ['# Title‸', '## Title‸', 2],
  ['### Title‸', 'Title‸', 0],
  ['Ti‸tle', '## Ti‸tle', 2],
  ['- item‸', '## item‸', 2],
  ['‸', '## ‸', 2],
  ['# Title‸', null, 1],
  ['plain‸', null, 0],
  ['---\ntitle: a‸\n---\n', null, 1],
  ['عنوان‸', '# عنوان‸', 1],
]);
it('lineKind', () => {
  expect(E.lineKind('## a\n- [ ] b', 2)).toEqual({ heading: 2, list: null, plain: false });
  expect(E.lineKind('## a\n- [ ] b', 7)).toEqual({ heading: 0, list: 'task', plain: false });
  expect(E.lineKind('1. x', 1)).toEqual({ heading: 0, list: 'number', plain: false });
  expect(E.lineKind('---\na: b\n---\n# x', 5).plain).toBe(true);
});

// ---------- bold and italic ----------
const bold: Fn = (t, s, e) => E.toggleInline(t, s, e, '**');
const ital: Fn = (t, s, e) => E.toggleInline(t, s, e, '*');
cases('bold', bold, [
  ['a ⟨word⟩ b', 'a **⟨word⟩** b'],
  ['a **⟨word⟩** b', 'a ⟨word⟩ b'],
  ['a ⟨**word**⟩ b', 'a ⟨word⟩ b'],
  ['a ***⟨word⟩*** b', 'a *⟨word⟩* b'],
  ['a *⟨word⟩* b', 'a ***⟨word⟩*** b'],
  ['a⟨ two words ⟩b', 'a **⟨two words⟩** b'],
  ['wo‸rd', '**wo‸rd**'],
  ['word‸ next', '**word‸** next'],
  ['**wo‸rd**', 'wo‸rd'],
  ['a ‸ b', 'a **‸** b'],
  ['a **‸** b', 'a ‸ b'],
  ['⟨- one\n- two⟩', '- **⟨one\n- two⟩**'.replace('one\n- two', 'one**\n- **two')],
  ['⟨# Title⟩', '# **⟨Title⟩**'],
  ['كلمة ⟨عربية⟩', 'كلمة **⟨عربية⟩**'],
  ['ع‸ربي', '**ع‸ربي**'],
  ['---\nt: ⟨x⟩\n---\n', null],
]);
cases('italic', ital, [
  ['a ⟨word⟩ b', 'a *⟨word⟩* b'],
  ['a *⟨word⟩* b', 'a ⟨word⟩ b'],
  ['a **⟨word⟩** b', 'a ***⟨word⟩*** b'],
  ['a ***⟨word⟩*** b', 'a **⟨word⟩** b'],
  ['wo‸rd', '*wo‸rd*'],
  ['‸', '*‸*'],
]);

// ---------- links ----------
it('linkTarget', () => {
  const t = E.linkTarget;
  expect(t('https://example.com/a b')).toBe('https://example.com/a%20b');
  expect(t('example.com')).toBe('https://example.com');
  expect(t('www.example.com/x?y=1')).toBe('https://www.example.com/x?y=1');
  expect(t('sara@example.com')).toBe('mailto:sara@example.com');
  expect(t('notes/plan.md')).toBe('notes/plan.md');
  expect(t('plan.md')).toBe('plan.md');
  expect(t('report.pdf')).toBe('report.pdf');
  expect(t('../other/README.md')).toBe('../other/README.md');
  expect(t('https://en.wikipedia.org/wiki/Tax_(law)')).toBe('https://en.wikipedia.org/wiki/Tax_%28law%29');
  expect(t('  ')).toBe('');
});
cases('setLink', (t, s, e, label, addr) => E.setLink(t, s, e, label, addr), [
  ['see ⟨the plan⟩ now', 'see [the plan](https://example.com)‸ now', 'the plan', 'example.com'],
  ['see ‸ now', 'see [example.com](https://example.com)‸ now', '', 'example.com'],
  ['a [old](http://x.y) b‸'.replace(' b‸', '‸ b'), 'a [new](https://z.com)‸ b', 'new', 'https://z.com'],
  ['a [te‸xt](http://x.y) b', 'a text‸ b', 'text', ''],
  ['⟨a [b] c⟩', '[a \\[b\\] c](https://x.com)‸', 'a [b] c', 'x.com'],
]);
it('linkAt', () => {
  const t = 'x [label](http://a.b "t") ![pic](p.png) [x\\]y](q)';
  expect(E.linkAt(t, 5)).toEqual({ start: 2, end: 25, label: 'label', target: 'http://a.b' });
  expect(E.linkAt(t, 30)).toBe(null);
  expect(E.linkAt(t, 44)?.label).toBe('x]y');
  expect(E.linkAt('no link here', 3)).toBe(null);
});
cases('pasteLink', (t, s, e, p) => E.pasteLink(t, s, e, p), [
  ['see ⟨the plan⟩', 'see [the plan](https://example.com/p)‸', 'https://example.com/p'],
  ['see ⟨the plan⟩', 'see [the plan](https://example.com/a%20b)‸', ' https://example.com/a b'.trim().replace(' ', '%20')],
  ['mail ⟨Sara⟩', 'mail [Sara](mailto:sara@example.com)‸', 'mailto:sara@example.com'],
  ['see ‸', null, 'https://example.com'],
  ['see ⟨the plan⟩', null, 'not a link'],
  ['see ⟨two\nlines⟩', null, 'https://example.com'],
  ['⟨https://old.com⟩', null, 'https://example.com'],
  ['[⟨in⟩ a link](x)', null, 'https://example.com'],
  ['```\n⟨code⟩\n```', null, 'https://example.com'],
  ['انظر ⟨الخطة⟩', 'انظر [الخطة](https://example.com)‸', 'https://example.com'],
]);

// ---------- pictures ----------
cases('insertBlocks', (t, s, e, b) => E.insertBlocks(t, s, e, b), [
  ['‸', '![](images/a.png)‸', ['![](images/a.png)']],
  ['text‸', 'text\n![](images/a.png)‸', ['![](images/a.png)']],
  ['te‸xt', 'te\n![](images/a.png)‸\nxt', ['![](images/a.png)']],
  ['a\n‸\nb', 'a\n![](x.png)\n\n![](y.png)‸\nb', ['![](x.png)', '![](y.png)']],
]);
it('pictureMarkdown', () => {
  expect(E.pictureMarkdown('My [receipt]', 'images/r.png')).toBe('![My \\[receipt\\]](images/r.png)');
  expect(E.pictureMarkdown('صورة', 'images/إيصال.png')).toBe('![صورة](images/إيصال.png)');
});

// ---------- the smallest edit ----------
it('diff', () => {
  expect(E.diff('hello world', 'hello brave world')).toEqual({ from: 6, to: 6, insert: 'brave ' });
  expect(E.diff('abc', 'abc')).toEqual({ from: 3, to: 3, insert: '' });
  expect(E.diff('a **b** c', 'a b c')).toEqual({ from: 2, to: 7, insert: 'b' });
  // A changed emoji is replaced whole, never half.
  const d = E.diff('x😀y', 'x😃y');
  expect(d.insert).toBe('😃');
  for (const [a, b] of [['', 'x'], ['x', ''], ['aaa', 'aa'], ['- a\n- b', '- a\n  - b'], ['😀😀', '😀']]) {
    const r = E.diff(a, b);
    expect(a.slice(0, r.from) + r.insert + a.slice(r.to)).toBe(b);
  }
});

// ---------- line endings ----------
it('restoreEol', () => {
  const r = E.restoreEol;
  expect(r('a\nb', 'a\nbc')).toBe('a\nbc');
  expect(r('a\r\nb\r\nc', 'a\nbX\nc')).toBe('a\r\nbX\r\nc');
  expect(r('a\r\nb\r\n', 'a\nb\nnew\n')).toBe('a\r\nb\r\nnew\r\n');
  expect(r('a\r\nb\r\nc', 'a\nnew\nb\nc')).toBe('a\r\nnew\r\nb\r\nc');
  expect(r('a\r\nb\r\nc', 'a\nc')).toBe('a\r\nc');
  // A file with mixed endings: lines nobody touched keep theirs.
  expect(r('x\r\ny\nz\r\nw', 'x\nNEW\ny\nz\nw')).toBe('x\r\nNEW\r\ny\nz\r\nw');
  expect(r('x\r\ny\nz', 'x\nY\nz')).toBe('x\r\nY\nz');
  // Old Mac endings.
  expect(r('a\rb', 'a\nbb')).toBe('a\rbb');
  // Typing and deleting a character gives the file back exactly as it was.
  const orig = '---\r\ntitle: A\r\n---\r\n# A\r\n\r\n- x\r\n';
  const typed = r(orig, E.toLF(orig).replace('- x', '- xy'));
  expect(typed).toBe(orig.replace('- x', '- xy'));
  expect(r(typed, E.toLF(orig))).toBe(orig);
  expect(E.toLF('a\r\nb\rc\nd')).toBe('a\nb\nc\nd');
});

// ---------- note details and code ----------
it('zones', () => {
  const t = '---\ntitle: x\n---\n# A\n```\ncode\n```\nafter';
  expect(E.frontmatterEnd(t)).toBe(17);
  expect(E.plainZone(t, 5)).toBe(true);
  expect(E.plainZone(t, 18)).toBe(false);
  expect(E.inFence(t, t.indexOf('code'))).toBe(true);
  expect(E.inFence(t, t.indexOf('after'))).toBe(false);
  expect(E.inFence('````\n```\nstill\n````\nout', 10)).toBe(true);
  expect(E.inFence('````\n```\nstill\n````\nout', 21)).toBe(false);
  expect(E.frontmatterEnd('no details')).toBe(0);
});

// ---------- where blocks start (preview follows the editor) ----------
it('blockStarts', () => {
  const t = '---\ntitle: x\n---\n# Title\n\nPara one\nstill\n\n[ref]: http://x.com\n\n- a\n- b\n\n```\ncode\n```\n<!-- c -->\n\n| a |\n|---|\n| 1 |\n\n> q\n\n---\n\nend';
  const b = E.blockStarts(t);
  const lines = t.split('\n');
  expect(b.map((x) => x.kind)).toEqual(['heading', 'paragraph', 'list', 'code', 'html', 'table', 'blockquote', 'hr', 'paragraph']);
  expect(b.map((x) => lines[x.line])).toEqual(['# Title', 'Para one', '- a', '```', '<!-- c -->', '| a |', '> q', '---', 'end']);
  expect(E.blockStarts('')).toEqual([]);
});

it('textDir', () => {
  expect(E.textDir('---\ntitle: Notes\n---\n# خطة العمل')).toBe('rtl');
  expect(E.textDir('# Plan\nمرحبا')).toBe('ltr');
  expect(E.textDir('![x](images/a.png) 123 مرحبا')).toBe('rtl');
  expect(E.textDir('123 - !')).toBe('');
});

it('wordAt', () => {
  expect(E.wordAt('one two', 5)).toEqual([4, 7]);
  expect(E.wordAt('one two', 3)).toEqual([0, 3]);
  expect(E.wordAt('a  b', 2)).toBe(null);
  expect(E.wordAt('كتبَ الدرس', 2)).toEqual([0, 4]);
});
