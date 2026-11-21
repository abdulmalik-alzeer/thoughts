// Where a comment sits (anchor.ts): from words selected on the rendered page to a quote in the
// markdown source, and finding that quote again after the document was rewritten around it.
import { describe, it, expect } from 'vitest';
import { CONTEXT, anchorAt, bestMatch, createAnchor, findAnchor, fold, headingAbove, locate, onPage, project, projected } from './anchor';

const DOC = `---
title: Spec
---
# Shop

Intro with **bold words** and a [link label](https://example.com/some/path) here.

## Features

### F1 — Sign in

People sign in with an email address. The email address is checked.

1. First the email address is typed.
2. Then a \`code\` arrives.

### F2 — Basket

- [ ] The basket shows a total.
- [x] The total includes *tax*.

\`\`\`js
const total = price * qty;
\`\`\`

> A quote about the basket &amp; the total.

| Name | Type |
|------|------|
| price | number |

المستخدم يضيف **منتجاً** إلى السلة ثم يدفع.
`;

describe('fold and project', () => {
  it('keeps letters, digits and marks only', () => {
    expect(fold('Hello, **wor-ld** 42!')).toBe('Helloworld42');
    expect(fold('السَّلة، ثم')).toBe('السَّلةثم');
  });

  it('reads the source as the page reads once rendered', () => {
    const p = project(DOC);
    expect(p.text.startsWith('ShopIntrowithboldwordsandalinklabelhere')).toBe(true);
    expect(p.text).not.toContain('title'); // the note details are not text
    expect(p.text).not.toContain('example'); // nor a link's address
    expect(p.text).not.toContain('amp'); // nor an entity's name
    expect(p.text).toContain('consttotalpriceqty'); // code is, as written
    expect(p.text).not.toContain('jsconst'); // but not the language line
    expect(p.text).toContain('FirsttheemailaddressistypedThenacodearrives'); // list numbers are not text
    expect(p.text).toContain('ThebasketshowsatotalThetotalincludestax'); // task boxes neither
  });

  it('remembers where every character came from', () => {
    const p = project(DOC);
    for (let i = 0; i < p.text.length; i += 7) expect(DOC[p.map[i]]).toBe(p.text[i]);
    expect(p.map.length).toBe(p.text.length);
  });

  it('skips pictures, tags and reference links', () => {
    expect(project('a ![alt words](pic.png) b').text).toBe('ab');
    expect(project('a <span class="x">kept</span> b').text).toBe('akeptb');
    expect(project('see [the label][ref1] now\n\n[ref1]: https://example.com "t"').text).toBe('seethelabelnow');
    expect(project('visit <https://a.example/x> now').text).toBe('visithttpsaexamplexnow');
    expect(project('1\\. not a list').text).toBe('1notalist');
  });
});

describe('createAnchor', () => {
  it('anchors plain words with context and the heading above', () => {
    const a = createAnchor(DOC, { text: 'The basket shows a total', before: '', after: '' })!;
    expect(a.quote).toBe('The basket shows a total');
    expect(a.heading).toBe('F2 — Basket');
    expect(a.prefix.length).toBeLessThanOrEqual(CONTEXT);
    expect(a.suffix.length).toBeLessThanOrEqual(CONTEXT);
    expect(DOC).toContain(a.prefix + a.quote + a.suffix);
  });

  it('maps a selection across bold and a link back to the source, marks included', () => {
    const a = createAnchor(DOC, { text: 'with bold words and a link label here' })!;
    expect(a.quote).toBe('with **bold words** and a [link label](https://example.com/some/path) here');
    expect(a.heading).toBe('Shop');
  });

  it('tells two identical passages apart by what comes before and after', () => {
    const page = project(DOC).text; // the rendered page reads the same
    const second = page.indexOf('emailaddress', page.indexOf('emailaddress') + 1);
    const third = page.indexOf('emailaddress', second + 1);
    const pick = (at: number) => ({ text: 'email address', before: page.slice(0, at), after: page.slice(at + 'emailaddress'.length) });
    const a1 = createAnchor(DOC, pick(page.indexOf('emailaddress')))!;
    const a2 = createAnchor(DOC, pick(second))!;
    const a3 = createAnchor(DOC, pick(third))!;
    expect(a1.suffix.startsWith('. The email')).toBe(true);
    expect(a2.suffix.startsWith(' is checked')).toBe(true);
    expect(a3.suffix.startsWith(' is typed')).toBe(true);
    // And each is found again exactly where it was made.
    const s1 = findAnchor(DOC, a1)!, s2 = findAnchor(DOC, a2)!, s3 = findAnchor(DOC, a3)!;
    expect(s1.start).toBeLessThan(s2.start);
    expect(s2.start).toBeLessThan(s3.start);
    expect(new Set([s1.start, s2.start, s3.start]).size).toBe(3);
  });

  it('uses nearby words even when the page reads a little differently from the source', () => {
    // The page has extra words the source does not spell (a drawn figure's caption, say).
    const a = createAnchor(DOC, { text: 'email address', before: 'SOMETHINGELSE Firstthe', after: 'istyped' })!;
    expect(a.suffix.startsWith(' is typed')).toBe(true);
  });

  it('anchors Arabic words, across bold', () => {
    const a = createAnchor(DOC, { text: 'يضيف منتجاً إلى السلة' })!;
    expect(a.quote).toBe('يضيف **منتجاً** إلى السلة');
    expect(findAnchor(DOC, a)).toEqual({ start: DOC.indexOf(a.quote), end: DOC.indexOf(a.quote) + a.quote.length });
  });

  it('anchors inside code, a table and a quote', () => {
    expect(createAnchor(DOC, { text: 'total = price * qty' })!.quote).toBe('total = price * qty');
    expect(createAnchor(DOC, { text: 'price number' })!.quote).toBe('price | number');
    expect(createAnchor(DOC, { text: 'the basket & the total' })!.quote).toBe('the basket &amp; the total');
  });

  it('gives nothing for a selection with no words, or words that are not in the document', () => {
    expect(createAnchor(DOC, { text: ' — … ' })).toBeNull();
    expect(createAnchor(DOC, { text: 'nothing like this anywhere' })).toBeNull();
  });

  it('places a long selection by its beginning when the rest does not match', () => {
    const a = createAnchor(DOC, { text: 'People sign in with an email address. PLUS WORDS THE SOURCE NEVER HAD' })!;
    expect(a.partial).toBe(true);
    expect(a.quote.startsWith('People sign in')).toBe(true);
  });

  it('never keeps more than 40 characters either side', () => {
    const a = anchorAt('x'.repeat(100) + 'QUOTE' + 'y'.repeat(100), { start: 100, end: 105 });
    expect(a).toEqual({ quote: 'QUOTE', prefix: 'x'.repeat(40), suffix: 'y'.repeat(40) });
  });
});

describe('findAnchor', () => {
  const a = createAnchor(DOC, { text: 'The basket shows a total' })!;

  it('finds the quote where it was', () => {
    const s = findAnchor(DOC, a)!;
    expect(DOC.slice(s.start, s.end)).toBe(a.quote);
  });

  it('finds it after text was added above and below', () => {
    const edited = DOC.replace('## Features', '## Features\n\nA new opening paragraph.\n').replace('| price | number |', '| price | number |\n| qty | number |');
    const s = findAnchor(edited, a)!;
    expect(edited.slice(s.start, s.end)).toBe('The basket shows a total');
  });

  it('finds it after the lines were wrapped differently and emphasis was added', () => {
    const edited = DOC.replace('The basket shows a total', 'The **basket**\nshows   a total');
    const s = findAnchor(edited, a)!;
    expect(edited.slice(s.start, s.end)).toBe('The **basket**\nshows   a total');
  });

  it('prefers the copy with the same surroundings when the quote now occurs twice', () => {
    const edited = DOC.replace('# Shop', '# Shop\n\nThe basket shows a total, said the summary.');
    const s = findAnchor(edited, a)!;
    expect(s.start).toBe(edited.lastIndexOf('The basket shows a total'));
  });

  it('uses the heading when the surroundings no longer help', () => {
    const src = '# One\n\nsame words\n\n# Two\n\nsame words\n';
    const s = findAnchor(src, { quote: 'same words', prefix: 'zzz', suffix: 'zzz', heading: 'Two' })!;
    expect(s.start).toBe(src.lastIndexOf('same words'));
  });

  it('is lost when the words were rewritten', () => {
    expect(findAnchor(DOC.replace('The basket shows a total', 'A sum is displayed'), a)).toBeNull();
    expect(findAnchor('', a)).toBeNull();
    expect(findAnchor(DOC, { quote: '', prefix: '', suffix: '' })).toBeNull();
  });
});

describe('helpers', () => {
  it('headingAbove ignores # lines inside code', () => {
    const src = '# Real\n\n```\n# not a heading\n```\n\ntext';
    expect(headingAbove(src, src.indexOf('text'))).toBe('Real');
    expect(headingAbove('no headings here', 5)).toBeUndefined();
    expect(headingAbove('## Closed ##\nx', 13)).toBe('Closed');
  });

  it('bestMatch: single, none, and nearest of equals', () => {
    expect(bestMatch('abcabc', 'x')).toBe(-1);
    expect(bestMatch('abcxabc', 'x')).toBe(3);
    expect(bestMatch('abab', 'ab', 'ab', '')).toBe(2);
    expect(bestMatch('abab', 'ab', '', 'ab')).toBe(0);
  });

  it('locate returns source offsets', () => {
    const s = locate(DOC, { text: 'bold words' })!;
    expect(DOC.slice(s.start, s.end)).toBe('bold words');
  });

  it('projected and onPage give the same stretch on a page that reads alike', () => {
    const p = project(DOC);
    const s = locate(p, { text: 'The email address is checked' })!;
    const on = onPage(p, p.text, s)!;
    expect(on).toEqual(projected(p, s));
    expect(p.text.slice(on.start, on.end)).toBe('Theemailaddressischecked');
  });

  it('onPage finds the stretch on a page that reads differently, by its surroundings', () => {
    const p = project(DOC);
    const s = locate(p, { text: 'email address', before: 'Firstthe', after: 'istyped' })!;
    const page = 'EXTRA' + p.text;
    const on = onPage(p, page, s)!;
    expect(page.slice(on.start, on.end)).toBe('emailaddress');
    expect(page.slice(on.end, on.end + 7)).toBe('istyped');
  });
});
