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
  },
});

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
  const body = src.replace(/^---\n[\s\S]*?\n---\n?/, '');
  taskMark = secret();
  // Sanitise to a DOM fragment, then add the wrappers with DOM APIs (never by editing the HTML string).
  const frag = DOMPurify.sanitize(marked.parse(body, { async: false }) as string, PURIFY) as DocumentFragment;
  // Each block finds its own direction, so an Arabic paragraph and an English one sit correctly in
  // the same answer; inline code is always left-to-right and isolated from the sentence around it.
  for (const el of Array.from(frag.querySelectorAll('p, li, h1, h2, h3, h4, h5, h6, blockquote, td, th'))) el.setAttribute('dir', 'auto');
  for (const code of Array.from(frag.querySelectorAll('code'))) if (!code.closest('pre')) code.setAttribute('dir', 'ltr');
  for (const box of Array.from(frag.querySelectorAll('span[title]'))) {
    const m = /^([0-9a-f]{24}):([xo])$/.exec(box.getAttribute('title') || '');
    if (!m || m[1] !== taskMark) continue;
    box.removeAttribute('title');
    box.className = 'md-task';
    box.setAttribute('role', 'img');
    box.setAttribute('aria-label', t(m[2] === 'x' ? 'chat3.taskDone' : 'chat3.taskOpen'));
  }
  for (const pre of Array.from(frag.querySelectorAll('pre'))) {
    const wrap = document.createElement('div');
    wrap.className = 'md-code';
    const btn = document.createElement('button');
    btn.type = 'button'; btn.className = 'md-copy'; btn.setAttribute('data-md-copy', COPY_MARK);
    btn.textContent = t('md.copyCode');
    pre.replaceWith(wrap);
    wrap.append(btn, pre);
  }
  for (const table of Array.from(frag.querySelectorAll('table'))) {
    const wrap = document.createElement('div');
    wrap.className = 'md-table'; wrap.tabIndex = 0;
    table.replaceWith(wrap);
    wrap.append(table);
  }
  const host = document.createElement('div');
  host.append(frag);
  return host.innerHTML;
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
  const code = btn.parentElement?.querySelector('pre')?.innerText ?? '';
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
.md-code { position: relative; margin: 0 0 10px; }
.md-code pre { margin: 0; overflow-x: auto; direction: ltr; text-align: left; }
.md-copy { position: absolute; top: 6px; inset-inline-end: 6px; font: inherit; font-size: 12px; font-weight: 500; line-height: 1.4;
  color: var(--muted); background: var(--panel, var(--bg)); border: 1px solid var(--hairline); border-radius: 7px; padding: 2px 8px;
  cursor: pointer; opacity: 0; transition: opacity .15s; }
.md-code:hover .md-copy, .md-copy:focus-visible { opacity: 1; }
.md-copy:hover { color: var(--fg); }
@media (hover: none) { .md-copy { opacity: 1; } }
.md-table { overflow-x: auto; max-width: 100%; margin: 0 0 10px; }
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
