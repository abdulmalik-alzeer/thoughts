// Checks the note editor's text changes (ui/src/views/docs/editing.ts) without a browser.
// Run from the repository root:  node --experimental-strip-types scripts/check-editor.mjs
// (Node 22.6 or later; Node 23.6+ needs no flag.)
//
// In the cases below `‸` is the caret and `⟨…⟩` a selection, before and after each change.
import assert from 'node:assert/strict';
import * as E from '../ui/src/views/docs/editing.ts';

let passed = 0;
const failures = [];
function check(name, fn) {
  try { fn(); passed++; } catch (e) { failures.push(`✗ ${name}\n${e.message}`); }
}

/** "ab‸c" or "a⟨b⟩c" → [text, start, end] */
function parse(s) {
  const c = s.indexOf('‸');
  if (c >= 0) return [s.replace('‸', ''), c, c];
  const a = s.indexOf('⟨'), b = s.indexOf('⟩');
  assert.ok(a >= 0 && b > a, `no caret in ${JSON.stringify(s)}`);
  return [s.replace('⟨', '').replace('⟩', ''), a, b - 1];
}
function show(r) {
  if (!r) return null;
  return r.start === r.end ? r.text.slice(0, r.start) + '‸' + r.text.slice(r.start)
    : r.text.slice(0, r.start) + '⟨' + r.text.slice(r.start, r.end) + '⟩' + r.text.slice(r.end);
}
/** `fn(text, start, end, ...args)` turns `from` into `to` (null: nothing to do). */
function cases(label, fn, list) {
  for (const [from, to, ...args] of list) {
    check(`${label}: ${JSON.stringify(from)}`, () => {
      const [t, s, e] = parse(from);
      assert.equal(show(fn(t, s, e, ...args)), to);
    });
  }
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
check('lineKind', () => {
  assert.deepEqual(E.lineKind('## a\n- [ ] b', 2), { heading: 2, list: null, plain: false });
  assert.deepEqual(E.lineKind('## a\n- [ ] b', 7), { heading: 0, list: 'task', plain: false });
  assert.deepEqual(E.lineKind('1. x', 1), { heading: 0, list: 'number', plain: false });
  assert.equal(E.lineKind('---\na: b\n---\n# x', 5).plain, true);
});

// ---------- bold and italic ----------
const bold = (t, s, e) => E.toggleInline(t, s, e, '**');
const ital = (t, s, e) => E.toggleInline(t, s, e, '*');
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
check('linkTarget', () => {
  const t = E.linkTarget;
  assert.equal(t('https://example.com/a b'), 'https://example.com/a%20b');
  assert.equal(t('example.com'), 'https://example.com');
  assert.equal(t('www.example.com/x?y=1'), 'https://www.example.com/x?y=1');
  assert.equal(t('sara@example.com'), 'mailto:sara@example.com');
  assert.equal(t('notes/plan.md'), 'notes/plan.md');
  assert.equal(t('plan.md'), 'plan.md');
  assert.equal(t('report.pdf'), 'report.pdf');
  assert.equal(t('../other/README.md'), '../other/README.md');
  assert.equal(t('https://en.wikipedia.org/wiki/Tax_(law)'), 'https://en.wikipedia.org/wiki/Tax_%28law%29');
  assert.equal(t('  '), '');
});
cases('setLink', (t, s, e, label, addr) => E.setLink(t, s, e, label, addr), [
  ['see ⟨the plan⟩ now', 'see [the plan](https://example.com)‸ now', 'the plan', 'example.com'],
  ['see ‸ now', 'see [example.com](https://example.com)‸ now', '', 'example.com'],
  ['a [old](http://x.y) b‸'.replace(' b‸', '‸ b'), 'a [new](https://z.com)‸ b', 'new', 'https://z.com'],
  ['a [te‸xt](http://x.y) b', 'a text‸ b', 'text', ''],
  ['⟨a [b] c⟩', '[a \\[b\\] c](https://x.com)‸', 'a [b] c', 'x.com'],
]);
check('linkAt', () => {
  const t = 'x [label](http://a.b "t") ![pic](p.png) [x\\]y](q)';
  assert.deepEqual(E.linkAt(t, 5), { start: 2, end: 25, label: 'label', target: 'http://a.b' });
  assert.equal(E.linkAt(t, 30), null);
  assert.equal(E.linkAt(t, 44)?.label, 'x]y');
  assert.equal(E.linkAt('no link here', 3), null);
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
check('pictureMarkdown', () => {
  assert.equal(E.pictureMarkdown('My [receipt]', 'images/r.png'), '![My \\[receipt\\]](images/r.png)');
  assert.equal(E.pictureMarkdown('صورة', 'images/إيصال.png'), '![صورة](images/إيصال.png)');
});

// ---------- the smallest edit ----------
check('diff', () => {
  assert.deepEqual(E.diff('hello world', 'hello brave world'), { from: 6, to: 6, insert: 'brave ' });
  assert.deepEqual(E.diff('abc', 'abc'), { from: 3, to: 3, insert: '' });
  assert.deepEqual(E.diff('a **b** c', 'a b c'), { from: 2, to: 7, insert: 'b' });
  // A changed emoji is replaced whole, never half.
  const d = E.diff('x😀y', 'x😃y');
  assert.equal(d.insert, '😃');
  for (const [a, b] of [['', 'x'], ['x', ''], ['aaa', 'aa'], ['- a\n- b', '- a\n  - b'], ['😀😀', '😀']]) {
    const r = E.diff(a, b);
    assert.equal(a.slice(0, r.from) + r.insert + a.slice(r.to), b);
  }
});

// ---------- line endings ----------
check('restoreEol', () => {
  const r = E.restoreEol;
  assert.equal(r('a\nb', 'a\nbc'), 'a\nbc');
  assert.equal(r('a\r\nb\r\nc', 'a\nbX\nc'), 'a\r\nbX\r\nc');
  assert.equal(r('a\r\nb\r\n', 'a\nb\nnew\n'), 'a\r\nb\r\nnew\r\n');
  assert.equal(r('a\r\nb\r\nc', 'a\nnew\nb\nc'), 'a\r\nnew\r\nb\r\nc');
  assert.equal(r('a\r\nb\r\nc', 'a\nc'), 'a\r\nc');
  // A file with mixed endings: lines nobody touched keep theirs.
  assert.equal(r('x\r\ny\nz\r\nw', 'x\nNEW\ny\nz\nw'), 'x\r\nNEW\r\ny\nz\r\nw');
  assert.equal(r('x\r\ny\nz', 'x\nY\nz'), 'x\r\nY\nz');
  // Old Mac endings.
  assert.equal(r('a\rb', 'a\nbb'), 'a\rbb');
  // Typing and deleting a character gives the file back exactly as it was.
  const orig = '---\r\ntitle: A\r\n---\r\n# A\r\n\r\n- x\r\n';
  const typed = r(orig, E.toLF(orig).replace('- x', '- xy'));
  assert.equal(typed, orig.replace('- x', '- xy'));
  assert.equal(r(typed, E.toLF(orig)), orig);
  assert.equal(E.toLF('a\r\nb\rc\nd'), 'a\nb\nc\nd');
});

// ---------- note details and code ----------
check('zones', () => {
  const t = '---\ntitle: x\n---\n# A\n```\ncode\n```\nafter';
  assert.equal(E.frontmatterEnd(t), 17);
  assert.equal(E.plainZone(t, 5), true);
  assert.equal(E.plainZone(t, 18), false);
  assert.equal(E.inFence(t, t.indexOf('code')), true);
  assert.equal(E.inFence(t, t.indexOf('after')), false);
  assert.equal(E.inFence('````\n```\nstill\n````\nout', 10), true);
  assert.equal(E.inFence('````\n```\nstill\n````\nout', 21), false);
  assert.equal(E.frontmatterEnd('no details'), 0);
});

// ---------- where blocks start (preview follows the editor) ----------
check('blockStarts', () => {
  const t = '---\ntitle: x\n---\n# Title\n\nPara one\nstill\n\n[ref]: http://x.com\n\n- a\n- b\n\n```\ncode\n```\n<!-- c -->\n\n| a |\n|---|\n| 1 |\n\n> q\n\n---\n\nend';
  const b = E.blockStarts(t);
  const lines = t.split('\n');
  assert.deepEqual(b.map((x) => x.kind), ['heading', 'paragraph', 'list', 'code', 'html', 'table', 'blockquote', 'hr', 'paragraph']);
  assert.deepEqual(b.map((x) => lines[x.line]), ['# Title', 'Para one', '- a', '```', '<!-- c -->', '| a |', '> q', '---', 'end']);
  assert.deepEqual(E.blockStarts(''), []);
});

check('textDir', () => {
  assert.equal(E.textDir('---\ntitle: Notes\n---\n# خطة العمل'), 'rtl');
  assert.equal(E.textDir('# Plan\nمرحبا'), 'ltr');
  assert.equal(E.textDir('![x](images/a.png) 123 مرحبا'), 'rtl');
  assert.equal(E.textDir('123 - !'), '');
});

check('wordAt', () => {
  assert.deepEqual(E.wordAt('one two', 5), [4, 7]);
  assert.deepEqual(E.wordAt('one two', 3), [0, 3]);
  assert.equal(E.wordAt('a  b', 2), null);
  assert.deepEqual(E.wordAt('كتبَ الدرس', 2), [0, 4]);
});

if (failures.length) {
  console.error(failures.join('\n\n'));
  console.error(`\n${failures.length} failed, ${passed} passed`);
  process.exit(1);
}
console.log(`editor text changes: all ${passed} checks passed`);
