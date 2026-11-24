import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { t } from './i18n';

marked.setOptions({ gfm: true, breaks: false });

/** A fresh random token. Markup written into a note cannot know it, so whatever carries it was made by us. */
function secret(): string {
  const b = new Uint8Array(12);
  crypto.getRandomValues(b);
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
}
/** Marks the copy buttons md() adds, so a button-like thing in a note can never pass for one. */
const COPY_MARK = secret();
/** Marks the wrapper of a code block that is a diagram or a chart (`archify`, `chart`): the part of
 *  the app that draws them (figures/hydrate.ts) only touches wrappers carrying it. */
const FIG_MARK = secret();
/** The languages of a code block that the app draws instead of showing. */
export const FIGURE_LANGS = ['archify', 'chart'];
let figuresStarted = false;
/** Marks the wrapper of a `slides` code block (a deck in an answer): slides/hydrate.ts turns only
 *  wrappers carrying it into a card with Present and Save as note. */
const SLIDES_MARK = secret();
let slidesStarted = false;
/** Changes on every md() call; the task-list renderer stamps it on its markers (see below). */
let taskMark = '';

// Task-list items: `<input>` is forbidden in rendered markdown, so a checkbox becomes a plain text
// marker that a screen reader names ("done" / "not done"). The renderer only stamps a placeholder:
// the class and label are put on after sanitising, so markup in a note cannot fake one.
marked.use({
  renderer: {
    checkbox(a: any) {
      const checked = typeof a === 'object' && a ? !!a.checked : !!a;
      return `<span title="${taskMark}:${checked ? 'x' : 'o'}">${checked ? '[x]' : '[ ]'}</span> `;
    },
    // A code block keeps the language it was written with (`class` is stripped by the sanitiser, so
    // it rides on a stamped title, like the task markers): its header names it.
    code(a: any) {
      const text = String(a?.text ?? '').replace(/\n$/, '') + '\n';
      const lang = String(a?.lang ?? '').trim().split(/\s+/)[0] ?? '';
      const stamp = LANG.test(lang) ? ` title="${taskMark}:lang:${lang}"` : '';
      return `<pre${stamp}><code>${a?.escaped ? text : escapeHtml(text)}</code></pre>\n`;
    },
  },
});

/** A language name worth showing over a code block ("ts", "c++", "objective-c", "c#"). */
const LANG = /^[A-Za-z0-9][\w+#.-]{0,23}$/;
const escapeHtml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');

/** What may come out of markdown: plain document structure only. Nothing interactive, no frames, no
 *  SVG or MathML, and no `style`, `class`, `id`, `name`, `data-*` or ARIA attributes, so a note can
 *  neither restyle the app (a fake dialog over the window) nor pose as one of its controls. The
 *  wrappers, copy buttons and task markers are added afterwards, by us, on the sanitised tree.
 *  Image maps (`map`, `area`) stay out too: an `<area>` is a link the shell's link guard would
 *  otherwise have to catch. (The renderer adds no heading ids or highlighting classes, so nothing
 *  here depends on them.) */
const ALLOWED_TAGS = [
  'a', 'abbr', 'b', 'bdi', 'bdo', 'blockquote', 'br', 'caption', 'cite', 'code', 'dd', 'del', 'details', 'dfn', 'div', 'dl', 'dt',
  'em', 'figcaption', 'figure', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'i', 'img', 'ins', 'kbd', 'li', 'mark', 'ol', 'p', 'pre',
  'q', 'rp', 'rt', 'ruby', 's', 'samp', 'small', 'span', 'strong', 'sub', 'summary', 'sup', 'table', 'tbody', 'td', 'tfoot', 'th',
  'thead', 'time', 'tr', 'u', 'ul', 'var', 'wbr',
];
const ALLOWED_ATTR = ['href', 'title', 'alt', 'src', 'width', 'height', 'align', 'start', 'reversed', 'colspan', 'rowspan', 'dir', 'lang', 'open', 'datetime', 'cite'];
const PURIFY = { ALLOWED_TAGS, ALLOWED_ATTR, ALLOW_DATA_ATTR: false, ALLOW_ARIA_ATTR: false, RETURN_DOM_FRAGMENT: true } as const;

/** Render markdown to sanitized HTML (frontmatter stripped).
 *  Code blocks get a copy button and tables a horizontally scrollable wrapper, added on the sanitised
 *  DOM tree. */
export function md(src: string): string {
  // The same text renders the same: a chat shown again after a refresh, or an answer's finished
  // blocks, are not parsed and sanitised again. Keyed by the language too (copy buttons and task
  // markers carry words of it).
  const key = t('md.copyCode') + '\u0000' + src;
  const hit = memo.get(key);
  if (hit !== undefined) { memo.delete(key); memo.set(key, hit); return hit; }
  const html = render(src);
  if (src.length <= MEMO_MAX_CHARS) {
    memo.set(key, html);
    memoChars += key.length + html.length;
    while (memo.size > MEMO_SIZE || memoChars > MEMO_BUDGET) {
      const [k, v] = memo.entries().next().value!;
      memo.delete(k);
      memoChars -= k.length + v.length;
    }
  }
  return html;
}

/** Recently rendered texts, oldest first. */
const memo = new Map<string, string>();
let memoChars = 0;
const MEMO_SIZE = 400;
/** What all kept texts may come to together, in characters (a few megabytes). */
const MEMO_BUDGET = 2_000_000;
/** A long note is rendered each time rather than kept (it is rarely shown twice in a row). */
const MEMO_MAX_CHARS = 100_000;

function render(src: string): string {
  const body = src.replace(/^---\n[\s\S]*?\n---\n?/, '');
  let hasFigure = false;
  let hasSlides = false;
  taskMark = secret();
  // Sanitise to a DOM fragment, then add the wrappers with DOM APIs (never by editing the HTML string).
  const frag = DOMPurify.sanitize(marked.parse(body, { async: false }) as string, PURIFY) as DocumentFragment;
  // Each block finds its own direction, so an Arabic paragraph and an English one sit correctly in
  // the same answer; inline code is always left-to-right and isolated from the sentence around it.
  for (const el of Array.from(frag.querySelectorAll('p, li, h1, h2, h3, h4, h5, h6, blockquote, td, th'))) el.setAttribute('dir', 'auto');
  for (const code of Array.from(frag.querySelectorAll('code'))) if (!code.closest('pre')) code.setAttribute('dir', 'ltr');
  // `dir="auto"` skips children that carry their own `dir`, so a quote or a list item whose words sit
  // in a paragraph inside it would always come out left-to-right (an Arabic quote with its bar on
  // the left, bullets on the wrong side). These take the direction of their own first words instead,
  // and a list follows its first item, so its bullets sit on the same side.
  for (const el of Array.from(frag.querySelectorAll('li, blockquote'))) {
    const d = firstStrongDir(el);
    if (d) el.setAttribute('dir', d);
  }
  // A table runs in the direction of its own words (its columns in the order they were written),
  // not the page's: an English table in the Arabic app is not mirrored.
  for (const table of Array.from(frag.querySelectorAll('table'))) {
    const d = firstStrongDir(table);
    if (d) table.setAttribute('dir', d);
  }
  for (const list of Array.from(frag.querySelectorAll('ul, ol'))) {
    const d = list.querySelector(':scope > li')?.getAttribute('dir');
    if (d === 'rtl' || d === 'ltr') list.setAttribute('dir', d);
  }
  for (const box of Array.from(frag.querySelectorAll('span[title]'))) {
    const m = /^([0-9a-f]{24}):([xo])$/.exec(box.getAttribute('title') || '');
    if (!m || m[1] !== taskMark) continue;
    box.removeAttribute('title');
    box.className = 'md-task';
    box.setAttribute('role', 'img');
    box.setAttribute('aria-label', t(m[2] === 'x' ? 'chat3.taskDone' : 'chat3.taskOpen'));
  }
  // A code block gets a header, as in Claude's own chat: its language and a copy button. The header
  // carries `data-md-copy` too (empty, so it is never taken for the button): an export or a printout
  // that drops the copy buttons drops the whole header with them.
  for (const pre of Array.from(frag.querySelectorAll('pre'))) {
    const m = /^([0-9a-f]{24}):lang:(.+)$/.exec(pre.getAttribute('title') || '');
    if (pre.hasAttribute('title') && m?.[1] === taskMark) pre.removeAttribute('title');
    const lang = m && m[1] === taskMark && LANG.test(m[2]) ? m[2] : '';
    const wrap = document.createElement('div');
    wrap.className = 'md-code';
    const head = document.createElement('div');
    head.className = 'md-codehead'; head.setAttribute('data-md-copy', '');
    const name = document.createElement('span');
    name.className = 'md-lang'; name.dir = 'ltr';
    name.textContent = lang;
    const btn = document.createElement('button');
    btn.type = 'button'; btn.className = 'md-copy'; btn.setAttribute('data-md-copy', COPY_MARK);
    btn.textContent = t('md.copyCode');
    head.append(name, btn);
    pre.replaceWith(wrap);
    wrap.append(head, pre);
    // A diagram or a chart: stamped here, on the sanitised tree, and drawn once it is on the page.
    if (FIGURE_LANGS.includes(lang.toLowerCase())) {
      wrap.classList.add('md-fig');
      wrap.setAttribute('data-md-fig', FIG_MARK);
      wrap.setAttribute('data-fig-kind', lang.toLowerCase());
      hasFigure = true;
    }
    // A slide deck (`slides`): stamped the same way, and shown as a card once it is on the page.
    if (lang.toLowerCase() === 'slides') {
      wrap.classList.add('md-slides');
      wrap.setAttribute('data-md-slides', SLIDES_MARK);
      hasSlides = true;
    }
  }
  for (const table of Array.from(frag.querySelectorAll('table'))) {
    const wrap = document.createElement('div');
    wrap.className = 'md-table'; wrap.tabIndex = 0;
    const d = table.getAttribute('dir');
    if (d) wrap.setAttribute('dir', d); // (and it sits on the side its words start from)
    table.replaceWith(wrap);
    wrap.append(table);
  }
  const host = document.createElement('div');
  host.append(frag);
  // The code that draws figures is only loaded once a text has one.
  if (hasFigure && !figuresStarted) {
    figuresStarted = true;
    void import('./figures/hydrate').then((m) => m.startFigures(FIG_MARK), () => { figuresStarted = false; });
  }
  if (hasSlides && !slidesStarted) {
    slidesStarted = true;
    void import('./slides/hydrate').then((m) => m.startSlides(SLIDES_MARK), () => { slidesStarted = false; });
  }
  return host.innerHTML;
}

/** Letters written right to left (Hebrew, Arabic, Syriac, Thaana, N'Ko and their presentation forms). */
const RTL_LETTER = /[֐-׿؈؋؍؛-ي٭-ٯٱ-ەۥۦۮۯۺ-ܐܒ-ܯݍ-ޥޱ-ߪߴߵߺ-ࠕࡀ-ࡘࡠ-ࣉיִ-﷿ﹰ-﻾]/;
const LETTER = /\p{L}/u;

/** The direction of the first letter in an element's own words, leaving code out (it is always
 *  left-to-right and says nothing about the sentence); null when there is no letter. */
function firstStrongDir(el: Element): 'rtl' | 'ltr' | null {
  const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) => (n.parentElement?.closest('code, pre') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
  });
  for (let n = walk.nextNode(); n; n = walk.nextNode()) {
    for (const ch of n.nodeValue || '') {
      if (RTL_LETTER.test(ch)) return 'rtl';
      if (LETTER.test(ch)) return 'ltr';
    }
  }
  return null;
}

// ---- text a reader cannot see ----
// HTML comments, unused link definitions (the `[//]: # (…)` idiom) and elements whose content is
// dropped all vanish from the rendered note, and some characters are invisible by design, yet the
// assistant reads the file as written. Docs flags such notes and offers the source.
const DROPPED = /<\s*(script|style|template|noscript|title|textarea|xmp|iframe|noembed|noframes|plaintext|select|option|object|embed|svg|math|head)\b/i;
/** Bidi overrides and isolates, invisible operators, tag characters, and runs of zero-width characters.
 *  Single marks that Arabic or emoji use legitimately (LRM, RLM, ZWJ, ZWNJ) are not counted. */
const INVISIBLE = /[‪-‮⁦-⁩⁠-⁤\u{E0000}-\u{E007F}]|[​-‍﻿]{2,}|(?!^)﻿/u;
/** Subdivision flags (England, Scotland, Wales) are spelled with tag characters. */
const TAG_FLAG = /\u{1F3F4}[\u{E0020}-\u{E007E}]+\u{E007F}/gu;
const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** Does this markdown hold text that md() does not show? */
export function hasHiddenText(src: string): boolean {
  const body = src.replace(/^﻿/, '').replace(/^---\n[\s\S]*?\n---\n?/, '');
  if (INVISIBLE.test(body.replace(TAG_FLAG, ''))) return true;
  let hidden = false;
  try {
    const tokens = marked.lexer(body);
    marked.walkTokens(tokens, (tok: any) => {
      if (hidden || tok.type !== 'html') return;
      const html = String(tok.raw ?? tok.text ?? '');
      if (/<!--[\s\S]*?\S[\s\S]*?(-->|$)/.test(html.replace(/<!--\s*-->/g, '')) || DROPPED.test(html)) hidden = true;
    });
    // A link definition that nothing refers to is never shown anywhere.
    for (const label of Object.keys((tokens as any).links ?? {})) {
      if (hidden) break;
      const uses = body.match(new RegExp(`\\[${esc(label).replace(/\s+/g, '\\s+')}\\]`, 'gi'))?.length ?? 0;
      if (uses <= 1) hidden = true;
    }
  } catch { /* unparsable: nothing to add */ }
  return hidden;
}

/** The source split into plain runs and invisible characters (shown as their code, e.g. U+202E). */
export function revealInvisible(src: string): { text: string; code?: boolean }[] {
  const out: { text: string; code?: boolean }[] = [];
  let run = '';
  for (const ch of src) {
    const c = ch.codePointAt(0)!;
    const inv = (c >= 0x200b && c <= 0x200f) || (c >= 0x202a && c <= 0x202e) || (c >= 0x2060 && c <= 0x2064) || (c >= 0x2066 && c <= 0x2069) || c === 0xfeff || (c >= 0xe0000 && c <= 0xe007f);
    if (!inv) { run += ch; continue; }
    if (run) { out.push({ text: run }); run = ''; }
    out.push({ text: 'U+' + c.toString(16).toUpperCase().padStart(4, '0'), code: true });
  }
  if (run) out.push({ text: run });
  return out;
}

/** Delegated click handler for rendered markdown: wire it on the container (`onclick={mdClick}`).
 *  Returns true when the click was a copy button and has been handled. */
export function mdClick(e: Event): boolean {
  const btn = (e.target as HTMLElement | null)?.closest?.('[data-md-copy]') as HTMLButtonElement | null;
  if (!btn || btn.tagName !== 'BUTTON' || btn.getAttribute('data-md-copy') !== COPY_MARK) return false;
  e.preventDefault();
  const code = btn.closest('.md-code')?.querySelector('pre')?.innerText ?? '';
  const done = (ok: boolean) => {
    btn.textContent = t(ok ? 'md.copied' : 'md.copyFailed');
    setTimeout(() => { btn.textContent = t('md.copyCode'); }, 1600);
  };
  navigator.clipboard.writeText(code).then(() => done(true), () => done(false));
  return true;
}

// Styles for the two wrappers live here because rendered markdown is injected HTML (no component scope).
if (typeof document !== 'undefined' && !document.getElementById('md-extras')) {
  const s = document.createElement('style');
  s.id = 'md-extras';
  s.textContent = `
.md-code { position: relative; margin: 0 0 10px; border: var(--border-w, 1px) solid var(--hairline); border-radius: var(--radius); background: var(--code-bg); overflow: hidden; }
.md-code pre { margin: 0; overflow-x: auto; overscroll-behavior-y: auto; direction: ltr; text-align: left; border: 0; border-radius: 0; background: none; padding: 4px 12px 12px; }
.md-codehead { display: flex; align-items: center; justify-content: space-between; gap: 8px; direction: ltr; min-height: 28px;
  padding: 4px 6px 0 12px; font-size: 12px; color: var(--muted); }
.md-lang { font-family: var(--mono); font-size: 11.5px; letter-spacing: .01em; }
.md-copy { font: inherit; font-size: 12px; font-weight: 500; line-height: 1.4;
  color: var(--muted); background: transparent; border: 1px solid transparent; border-radius: 7px; padding: 2px 8px; cursor: pointer; transition: color .15s, background .15s; }
.md-copy:hover { color: var(--fg); background: var(--fill); border-color: var(--hairline); }
@media print { .md-codehead { display: none !important; } }
.md-table { overflow-x: auto; overscroll-behavior-y: auto; max-width: 100%; margin: 0 0 10px; }
.md-table table { min-width: max-content; }
.md :not(pre) > code[dir] { unicode-bidi: isolate; }
.md-task { font-family: var(--mono); font-size: .92em; color: var(--muted); }
.md li:has(> .md-task), .md li:has(> p > .md-task) { list-style: none; }
@media print { .md-copy { display: none !important; } .md-table { overflow: visible; } }
`;
  document.head.appendChild(s);
}

/** Resolve a markdown link against the file it appears in. Link targets arrive percent-encoded
 *  (`ملاحظات/خطة.md` and `<my note.md>` both do), so each part is decoded; a leading `/` starts
 *  from the top of the library. */
export function resolveLink(from: string, href: string): string {
  const target = href.split('#')[0].split('?')[0];
  const base = target.startsWith('/') ? [] : from.split('/').slice(0, -1);
  for (const raw of target.split('/')) {
    let part = raw;
    try { part = decodeURIComponent(raw); } catch { /* keep as written */ }
    if (part === '..') base.pop();
    else if (part && part !== '.') base.push(part);
  }
  return base.join('/');
}
