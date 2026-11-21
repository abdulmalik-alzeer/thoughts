// MADO: where a comment sits in a document. A comment is anchored in the markdown SOURCE by the
// words it is about (`quote`), up to 40 characters either side and the nearest heading above, so it
// can be found again after the document was rewritten around it (docs/mado-spec.md, `Comment`).
//
// The user selects words in the RENDERED document, where the markdown marks are gone: `**bold**`
// reads "bold", a link reads as its label. Both sides are therefore reduced to the same thing, the
// letters and digits in reading order (a "projection"), each one remembering where it came from.
// What was selected is looked up in the projection of the source; when the same words occur more
// than once, the words before and after the selection decide.
//
// No DOM in this file: comments/dom.ts reads the page, this file does the thinking.

export interface TextAnchor { quote: string; prefix: string; suffix: string; heading?: string }
/** A stretch of a text: `start` inclusive, `end` exclusive. */
export interface Span { start: number; end: number }
/** The letters and digits of a text; `map[i]` is where `text[i]` sits in the original. */
export interface Projection { text: string; map: number[] }
/** What was selected in the rendered document, with everything before and after it there. */
export interface Picked { text: string; before?: string; after?: string }

/** How much is kept either side of the quote. */
export const CONTEXT = 40;
/** A quote longer than this is cut: the start of a long selection places the comment well enough. */
export const QUOTE_MAX = 1200;

const WORD = /[\p{L}\p{N}\p{M}]/u;
/** Does this UTF-16 unit count as part of the words (letters, digits, marks; halves of a pair such as an emoji)? */
function kept(code: number, ch: string): boolean {
  if (code < 128) return (code >= 48 && code <= 57) || (code >= 65 && code <= 90) || (code >= 97 && code <= 122);
  if (code >= 0xd800 && code <= 0xdfff) return true;
  return WORD.test(ch);
}

/** The letters and digits of a plain text (rendered words, or a quote): no markdown is read here. */
export function fold(text: string): string {
  let out = '';
  for (let i = 0; i < text.length; i++) if (kept(text.charCodeAt(i), text[i])) out += text[i];
  return out;
}

const FRONTMATTER = /^---\n[\s\S]*?\n---\n?/; // the same rule the renderer uses (lib/md.ts)
const FENCE = /^ {0,3}(`{3,}|~{3,})/;
const LINE_LEAD = /^[ \t]*(?:>[ \t]?)*[ \t]*(?:(?:[-+*]|\d{1,9}[.)])[ \t]+(?:\[[ xX]\][ \t]+)?)?/;
const REF_DEF = /^ {0,3}\[[^\]\n]+\]:\s/;
const ENTITY = /^&(?:#\d{1,7}|#[xX][0-9a-fA-F]{1,6}|[a-zA-Z][a-zA-Z0-9]{1,31});/;
const TAG = /^<\/?[a-zA-Z][^<>\n]*>/;
const AUTOLINK = /^<[a-zA-Z][a-zA-Z0-9+.-]{1,31}:[^\s<>]*>/;

/** The words of a markdown source as they read once rendered: marks, link addresses, list numbers,
 *  the language line of a code block and the note details at the top are left out. */
export function project(source: string): Projection {
  const out: string[] = [];
  const map: number[] = [];
  const keep = (i: number) => { if (kept(source.charCodeAt(i), source[i])) { out.push(source[i]); map.push(i); } };
  let at = FRONTMATTER.exec(source)?.[0].length ?? 0;
  let fence = '';
  while (at < source.length) {
    let eol = source.indexOf('\n', at);
    if (eol < 0) eol = source.length;
    const line = source.slice(at, eol);
    const f = FENCE.exec(line);
    if (fence) {
      // Inside a code block every character is shown as written.
      if (f && f[1][0] === fence[0] && f[1].length >= fence.length && !line.slice(f[0].length).trim()) fence = '';
      else for (let i = at; i < eol; i++) keep(i);
    } else if (f) {
      fence = f[1]; // the opening line (with the language) is not part of the text
    } else if (!REF_DEF.test(line)) {
      inline(source, at + (LINE_LEAD.exec(line)?.[0].length ?? 0), eol, keep);
    }
    at = eol + 1;
  }
  return { text: out.join(''), map };
}

/** One line's worth of inline markdown, from `i` to `end`. */
function inline(source: string, i: number, end: number, keep: (i: number) => void): void {
  while (i < end) {
    const c = source[i];
    if (c === '\\' && i + 1 < end) { keep(i + 1); i += 2; continue; }
    if (c === '`') {
      // A code span: shown as written, up to the same number of backticks.
      let n = 1;
      while (i + n < end && source[i + n] === '`') n++;
      const close = source.indexOf('`'.repeat(n), i + n);
      if (close > 0 && close < end) { for (let k = i + n; k < close; k++) keep(k); i = close + n; continue; }
      i += n; continue;
    }
    if (c === '!' && source[i + 1] === '[') {
      // A picture: neither its description nor its address is text on the page.
      const close = closing(source, i + 1, end, '[', ']');
      if (close > 0) { i = afterTarget(source, close + 1, end); continue; }
    }
    if (c === ']') { const next = afterTarget(source, i + 1, end); if (next > i + 1) { i = next; continue; } }
    if (c === '<') {
      const rest = source.slice(i, end);
      const auto = AUTOLINK.exec(rest);
      if (auto) { for (let k = i + 1; k < i + auto[0].length - 1; k++) keep(k); i += auto[0].length; continue; }
      const tag = TAG.exec(rest);
      if (tag) { i += tag[0].length; continue; }
    }
    if (c === '&') { const ent = ENTITY.exec(source.slice(i, Math.min(end, i + 40))); if (ent) { i += ent[0].length; continue; } }
    keep(i);
    i++;
  }
}

/** The index of the bracket that closes the one at `open`, or -1. */
function closing(source: string, open: number, end: number, a: string, b: string): number {
  let depth = 0;
  for (let i = open; i < end; i++) {
    if (source[i] === '\\') { i++; continue; }
    if (source[i] === a) depth++;
    else if (source[i] === b && --depth === 0) return i;
  }
  return -1;
}

/** After a `]`: skip the `(address)` or `[reference]` of a link, if there is one. */
function afterTarget(source: string, i: number, end: number): number {
  if (source[i] === '(') { const c = closing(source, i, end, '(', ')'); if (c > 0) return c + 1; }
  if (source[i] === '[') { const c = closing(source, i, end, '[', ']'); if (c > 0) return c + 1; }
  return i;
}

function commonEnd(a: string, aEnd: number, b: string, limit = 400): number {
  let n = 0;
  while (n < limit && n < aEnd && n < b.length && a[aEnd - 1 - n] === b[b.length - 1 - n]) n++;
  return n;
}
function commonStart(a: string, aStart: number, b: string, limit = 400): number {
  let n = 0;
  while (n < limit && aStart + n < a.length && n < b.length && a[aStart + n] === b[n]) n++;
  return n;
}
function occurrences(hay: string, needle: string, max = 2000): number[] {
  const at: number[] = [];
  if (!needle) return at;
  for (let i = hay.indexOf(needle); i >= 0 && at.length < max; i = hay.indexOf(needle, i + 1)) at.push(i);
  return at;
}

/** Where `needle` sits in `hay`, when it may occur more than once: the place whose surroundings
 *  agree best with `before` and `after`; of equals, the one nearest to where `before` ends. */
export function bestMatch(hay: string, needle: string, before = '', after = '', near = before.length): number {
  const at = occurrences(hay, needle);
  if (at.length < 2) return at[0] ?? -1;
  let best = at[0], bestScore = -1, bestFar = Infinity;
  for (const pos of at) {
    const score = commonEnd(hay, pos, before) + commonStart(hay, pos + needle.length, after);
    const far = Math.abs(pos - near);
    if (score > bestScore || (score === bestScore && far < bestFar)) { best = pos; bestScore = score; bestFar = far; }
  }
  return best;
}

/** The stretch of the source the selected words came from, or null when they are not in it.
 *  `partial` is set when only the beginning or the end of a long selection could be placed. */
export function locate(source: string | Projection, picked: Picked): (Span & { partial?: boolean }) | null {
  const proj = typeof source === 'string' ? project(source) : source;
  const q = fold(picked.text);
  if (!q) return null;
  const before = fold(picked.before ?? ''), after = fold(picked.after ?? '');
  const span = (ps: number, len: number): Span => ({ start: proj.map[ps], end: proj.map[ps + len - 1] + 1 });
  const whole = bestMatch(proj.text, q, before, after);
  if (whole >= 0) return span(whole, q.length);
  // The page shows something the source does not spell the same way (a drawn figure, a rare mark):
  // place the comment by the longest beginning, or else ending, of the selection that is found.
  for (const share of [0.75, 0.5, 0.25]) {
    const n = Math.floor(q.length * share);
    if (n < 8) break;
    const head = bestMatch(proj.text, q.slice(0, n), before, '');
    if (head >= 0) return { ...span(head, n), partial: true };
    const tail = bestMatch(proj.text, q.slice(-n), '', after);
    if (tail >= 0) return { ...span(tail, n), partial: true };
  }
  return null;
}

/** The text of the nearest heading above `offset`, without its marks. */
export function headingAbove(source: string, offset: number): string | undefined {
  let found: string | undefined;
  let fence = '';
  let at = 0;
  while (at <= offset && at < source.length) {
    let eol = source.indexOf('\n', at);
    if (eol < 0) eol = source.length;
    const line = source.slice(at, eol);
    const f = FENCE.exec(line);
    if (fence) { if (f && f[1][0] === fence[0] && f[1].length >= fence.length) fence = ''; }
    else if (f) fence = f[1];
    else {
      const h = /^ {0,3}#{1,6}[ \t]+(.*?)(?:[ \t]+#+)?[ \t]*$/.exec(line);
      if (h && h[1]) found = h[1].trim();
    }
    at = eol + 1;
  }
  return found;
}

/** The anchor for a stretch of the source. */
export function anchorAt(source: string, span: Span): TextAnchor {
  const start = Math.max(0, Math.min(span.start, source.length));
  const end = Math.max(start, Math.min(span.end, source.length, start + QUOTE_MAX));
  const a: TextAnchor = {
    quote: source.slice(start, end),
    prefix: source.slice(Math.max(0, start - CONTEXT), start),
    suffix: source.slice(end, end + CONTEXT),
  };
  const heading = headingAbove(source, start);
  if (heading) a.heading = heading.slice(0, 200);
  return a;
}

/** The anchor for what the user selected in the rendered document, or null when it cannot be placed. */
export function createAnchor(source: string, picked: Picked): (TextAnchor & { partial?: boolean }) | null {
  const span = locate(source, picked);
  if (!span) return null;
  const a: TextAnchor & { partial?: boolean } = anchorAt(source, span);
  if (!a.quote.trim()) return null;
  if (span.partial) a.partial = true;
  return a;
}

/** Find an anchor again in a (possibly rewritten) source. Null: the words are gone (the comment is "orphaned"). */
export function findAnchor(source: string, anchor: Pick<TextAnchor, 'quote' | 'prefix' | 'suffix' | 'heading'>): Span | null {
  const quote = String(anchor.quote ?? '');
  if (!quote) return null;
  const prefix = String(anchor.prefix ?? ''), suffix = String(anchor.suffix ?? '');
  // 1. The quote as written.
  const exact = occurrences(source, quote);
  if (exact.length === 1) return { start: exact[0], end: exact[0] + quote.length };
  if (exact.length > 1) {
    let best = exact[0], bestScore = -1;
    for (const pos of exact) {
      let score = commonEnd(source, pos, prefix, CONTEXT) + commonStart(source, pos + quote.length, suffix, CONTEXT);
      if (anchor.heading && headingAbove(source, pos) === anchor.heading) score += CONTEXT;
      if (score > bestScore) { best = pos; bestScore = score; }
    }
    return { start: best, end: best + quote.length };
  }
  // 2. The same words with other spacing, line breaks or marks around them.
  const q = fold(quote);
  if (q.length < 3) return null;
  const proj = project(source);
  const pos = bestMatch(proj.text, q, fold(prefix), fold(suffix));
  if (pos < 0) return null;
  return { start: proj.map[pos], end: proj.map[pos + q.length - 1] + 1 };
}

/** A stretch of the source as a stretch of its projection (for marking it on the rendered page). */
export function projected(proj: Projection, span: Span): Span | null {
  const lower = (v: number) => {
    let lo = 0, hi = proj.map.length;
    while (lo < hi) { const mid = (lo + hi) >> 1; if (proj.map[mid] < v) lo = mid + 1; else hi = mid; }
    return lo;
  };
  const start = lower(span.start), end = lower(span.end);
  return end > start ? { start, end } : null;
}

/** The same stretch in the rendered page's own projection: the same place when both read alike,
 *  otherwise wherever the same words with the same surroundings are. */
export function onPage(proj: Projection, pageText: string, span: Span): Span | null {
  const p = projected(proj, span);
  if (!p) return null;
  if (pageText === proj.text) return p;
  const words = proj.text.slice(p.start, p.end);
  const pos = bestMatch(pageText, words, proj.text.slice(Math.max(0, p.start - 400), p.start), proj.text.slice(p.end, p.end + 400), p.start);
  return pos < 0 ? null : { start: pos, end: pos + words.length };
}
