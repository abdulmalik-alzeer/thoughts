// From a selection on the rendered page to an anchor in the source and back to a mark on the page
// (comments/dom.ts with anchor.ts), through the app's real markdown renderer.
import { describe, it, expect, vi } from 'vitest';

vi.mock('../../i18n', () => ({ t: (k: string) => k }));
const { md } = await import('../../md');
import { createAnchor, findAnchor, onPage, project } from '../anchor';
import { commentAt, pageText, paint, pickedIn, rangeSpan, unpaint } from './dom';

const DOC = `# Shop

Intro with **bold words** and a [link label](https://example.com/p) here.

## F1 — Sign in

People sign in with an email address. The email address is checked.

1. First the email address is typed.
2. Then a \`code\` arrives.

- [ ] The basket shows a total.

\`\`\`js
const total = price * qty;
\`\`\`

| Name | Type |
|------|------|
| price | number |

المستخدم يضيف **منتجاً** إلى السلة ثم يدفع.
`;

function page(src = DOC): HTMLElement {
  const root = document.createElement('article');
  root.innerHTML = md(src);
  document.body.replaceChildren(root);
  return root;
}

/** Select from the nth occurrence of `from` to the end of the following `to`, as a user would drag. */
function select(root: HTMLElement, from: string, to = from, nth = 0): Selection {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const range = document.createRange();
  let seen = 0, started = false;
  for (let n = walker.nextNode() as Text | null; n; n = walker.nextNode() as Text | null) {
    if (n.parentElement?.closest('.md-codehead')) continue;
    if (!started) {
      let at = -1;
      for (let i = n.data.indexOf(from); i >= 0; i = n.data.indexOf(from, i + 1)) { if (seen++ === nth) { at = i; break; } }
      if (at < 0) continue;
      range.setStart(n, at); started = true;
      const end = n.data.indexOf(to, at);
      if (end >= 0) { range.setEnd(n, end + to.length); break; }
      continue;
    }
    const end = n.data.indexOf(to);
    if (end >= 0) { range.setEnd(n, end + to.length); break; }
  }
  const sel = window.getSelection()!;
  sel.removeAllRanges();
  sel.addRange(range);
  return sel;
}

describe('the page reads like the source', () => {
  it('for a document with every usual kind of block', () => {
    expect(pageText(page()).text).toBe(project(DOC).text);
  });
  it('for a document with note details at the top', () => {
    const src = '---\ntitle: x\n---\n# A\n\nbody text';
    expect(pageText(page(src)).text).toBe(project(src).text);
  });
});

describe('selection to anchor', () => {
  it('a selection inside one paragraph', () => {
    const root = page();
    const pick = pickedIn(root, select(root, 'People sign in', 'email address'))!;
    expect(pick.label).toBe('People sign in with an email address');
    expect(createAnchor(DOC, pick)!.quote).toBe('People sign in with an email address');
  });

  it('a selection that starts in plain text and ends inside a link, across bold', () => {
    const root = page();
    const pick = pickedIn(root, select(root, 'Intro', 'link'))!;
    const a = createAnchor(DOC, pick)!;
    expect(a.quote).toBe('Intro with **bold words** and a [link');
    expect(a.heading).toBe('Shop');
  });

  it('the second and the third of three identical passages', () => {
    const root = page();
    const a2 = createAnchor(DOC, pickedIn(root, select(root, 'email address', 'email address', 1))!)!;
    const a3 = createAnchor(DOC, pickedIn(root, select(root, 'email address', 'email address', 2))!)!;
    expect(a2.suffix.startsWith(' is checked')).toBe(true);
    expect(a3.suffix.startsWith(' is typed')).toBe(true);
    expect(a3.heading).toBe('F1 — Sign in');
  });

  it('across two list items', () => {
    const root = page();
    const a = createAnchor(DOC, pickedIn(root, select(root, 'is typed', 'Then a'))!)!;
    expect(a.quote).toBe('is typed.\n2. Then a');
  });

  it('inside a code block: the header the renderer adds is not part of it', () => {
    const root = page();
    expect(root.querySelector('.md-codehead')).not.toBeNull();
    const a = createAnchor(DOC, pickedIn(root, select(root, 'const total', 'qty'))!)!;
    expect(a.quote).toBe('const total = price * qty');
  });

  it('Arabic words', () => {
    const root = page();
    const a = createAnchor(DOC, pickedIn(root, select(root, 'يضيف', 'السلة'))!)!;
    expect(a.quote).toBe('يضيف **منتجاً** إلى السلة');
  });

  it('nothing when nothing is selected, or the selection is elsewhere', () => {
    const root = page();
    window.getSelection()!.removeAllRanges();
    expect(pickedIn(root)).toBeNull();
    const other = document.createElement('p');
    other.textContent = 'outside words';
    document.body.append(other);
    const r = document.createRange();
    r.selectNodeContents(other);
    window.getSelection()!.addRange(r);
    expect(pickedIn(root)).toBeNull();
  });
});

describe('marks on the page', () => {
  function markFor(root: HTMLElement, src: string, from: string, to = from, nth = 0) {
    const a = createAnchor(src, pickedIn(root, select(root, from, to, nth))!)!;
    window.getSelection()!.removeAllRanges();
    const p = project(src);
    return { a, span: onPage(p, pageText(root).text, findAnchor(src, a)!)! };
  }

  it('marks exactly the commented words, also across elements, and can be taken out again', () => {
    const root = page();
    const before = root.innerHTML;
    const one = markFor(root, DOC, 'Intro', 'link');
    const two = markFor(root, DOC, 'email address', 'email address', 2);
    paint(root, [{ id: 'c1', span: one.span }, { id: 'c2', span: two.span, cls: 'sent' }], (id) => 'about ' + id);
    const text = (id: string) => Array.from(root.querySelectorAll(`mark[data-mado-comment="${id}"]`)).map((m) => m.textContent).join('');
    expect(text('c1')).toBe('Intro with bold words and a link');
    expect(text('c2')).toBe('email address');
    const m2 = root.querySelector<HTMLElement>('mark[data-mado-comment="c2"]')!;
    expect(m2.classList.contains('sent')).toBe(true);
    expect(m2.title).toBe('about c2');
    expect(m2.closest('li')?.textContent).toContain('First the');
    // The words of the page are unchanged by the marks.
    expect(pageText(root).text).toBe(project(DOC).text);
    const click = new MouseEvent('click', { bubbles: true });
    let hit: string | null = null;
    root.addEventListener('click', (e) => { hit = commentAt(e); });
    m2.dispatchEvent(click);
    expect(hit).toBe('c2');
    unpaint(root);
    expect(root.innerHTML).toBe(before);
  });

  it('painting twice gives the same page as painting once', () => {
    const root = page();
    const one = markFor(root, DOC, 'People sign in', 'email address');
    paint(root, [{ id: 'c1', span: one.span }]);
    const once = root.innerHTML;
    paint(root, [{ id: 'c1', span: one.span }]);
    expect(root.innerHTML).toBe(once);
  });

  it('two comments that overlap do not break the page', () => {
    const root = page();
    const a = markFor(root, DOC, 'People sign in', 'email address');
    const b = markFor(root, DOC, 'sign in with', 'The email');
    paint(root, [{ id: 'a', span: a.span }, { id: 'b', span: b.span }]);
    expect(root.querySelectorAll('mark mark').length).toBe(0);
    expect(pageText(root).text).toBe(project(DOC).text);
  });

  it('a mark follows its words after the document was rewritten around them', () => {
    const root = page();
    const { a } = markFor(root, DOC, 'The basket shows', 'a total');
    const edited = DOC.replace('## F1', 'A new paragraph about the basket.\n\n## F1').replace('The basket shows a total', 'The *basket* shows a total');
    const root2 = page(edited);
    const span = onPage(project(edited), pageText(root2).text, findAnchor(edited, a)!)!;
    paint(root2, [{ id: 'c', span }]);
    expect(Array.from(root2.querySelectorAll('mark')).map((m) => m.textContent).join('')).toBe('The basket shows a total');
  });

  it('rangeSpan of a range with no words is null', () => {
    const root = page('a\n\n---\n\nb');
    const r = document.createRange();
    r.selectNode(root.querySelector('hr')!);
    expect(rangeSpan(pageText(root), r)).toBeNull();
  });
});

describe('clicks in a document', () => {
  it('a click on a mark names its comment; a click on a link is left alone for the app\'s link guard', async () => {
    const { docClick } = await import('./dom');
    const src = 'Read [the guide](https://example.com/guide) before the basket total.';
    const root = page(src);
    const p = project(src);
    const whole = onPage(p, pageText(root).text, { start: 0, end: src.length })!;
    paint(root, [{ id: 'c1', span: whole }]);
    const opened = vi.fn();
    vi.stubGlobal('open', opened);
    const seen: { id: string | null; prevented: boolean }[] = [];
    root.addEventListener('click', (e) => { const id = docClick(e); seen.push({ id, prevented: e.defaultPrevented }); });
    const guard = (e: Event) => e.preventDefault(); // the app's own link guard, further up, has the last word
    document.body.addEventListener('click', guard);
    const link = root.querySelector('a')!;
    const inLink = link.querySelector('mark')!; // the link's words are part of a commented passage
    expect(inLink).not.toBeNull();
    inLink.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    Array.from(root.querySelectorAll('mark')).find((m) => !m.closest('a'))!.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    expect(seen).toEqual([{ id: null, prevented: false }, { id: 'c1', prevented: false }]);
    expect(opened).not.toHaveBeenCalled();
    document.body.removeEventListener('click', guard);
    vi.unstubAllGlobals();
  });

  it('the document pane opens no link itself', async () => {
    const files = import.meta.glob('../../../views/mado/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    expect(Object.keys(files).length).toBeGreaterThan(5);
    for (const [name, text] of Object.entries(files)) {
      expect(text, name).not.toMatch(/openUrl|plugin-opener|window\.open|location\.(href|assign|replace)/);
    }
  });
});
