// The text changes behind the note editor's writing aids: bold, italic, headings, links, lists,
// checklists, indenting, list continuation, pictures, and keeping a file's own line endings.
//
// Pure functions of (text, selection): no DOM, no app state, so `editing.test.ts` can run
// them under Node. Each returns the whole new text with the new selection, or null for "nothing to
// do here, let the text box behave as usual". The editor turns a result into ONE native edit (so
// one action is one undo step) and never touches text outside the lines an action is about.
// Positions are offsets into the text as the text box holds it: line breaks are always "\n".
import { marked } from 'marked';

export type Change = { text: string; start: number; end: number };

// ---------- lines ----------

export const lineStart = (text: string, pos: number) => text.lastIndexOf('\n', pos - 1) + 1;
export function lineEnd(text: string, pos: number): number { const i = text.indexOf('\n', pos); return i < 0 ? text.length : i; }

/** Lines touched by a selection; one ending at the very start of a line leaves that line out. */
function coveredLines(text: string, s: number, e: number) {
  const lines = text.split('\n');
  const starts: number[] = [];
  for (let i = 0, at = 0; i < lines.length; i++) { starts.push(at); at += lines[i].length + 1; }
  const at = (p: number) => { let lo = 0, hi = starts.length - 1; while (lo < hi) { const m = (lo + hi + 1) >> 1; if (starts[m] <= p) lo = m; else hi = m - 1; } return lo; };
  const first = at(s);
  let last = at(e);
  if (e > s && last > first && starts[last] === e) last--;
  return { lines, starts, first, last, at };
}

/** Columns a run of spaces and tabs takes (a tab reaches the next multiple of 4). */
export function widthOf(ws: string): number {
  let w = 0;
  for (const c of ws) w = c === '\t' ? w + 4 - (w % 4) : w + 1;
  return w;
}

/** Note details at the top (`---` … `---`): where they end, exactly as the formatted view finds them. */
export function frontmatterEnd(text: string): number {
  const m = /^---\n[\s\S]*?\n---\n?/.exec(text);
  return m ? m[0].length : 0;
}

/** Is `pos` inside a fenced code block (``` or ~~~)? */
export function inFence(text: string, pos: number): boolean {
  const upTo = text.slice(frontmatterEnd(text), lineStart(text, pos));
  let open: { ch: string; n: number } | null = null;
  for (const line of upTo.split('\n')) {
    const m = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line);
    if (!m) continue;
    if (!open) { if (m[1][0] === '~' || !m[2].includes('`')) open = { ch: m[1][0], n: m[1].length }; }
    else if (m[1][0] === open.ch && m[1].length >= open.n && !m[2].trim()) open = null;
  }
  return !!open;
}

/** Where the formatting aids stay out: the note details and code. */
export const plainZone = (text: string, pos: number) => pos < frontmatterEnd(text) || inFence(text, pos);

// ---------- list items ----------

export type Item = {
  indent: string;
  /** `-`, `*`, `+`, or the number with its `.` / `)`. */
  marker: string;
  num: number | null;
  delim: string;
  space: string;
  /** `[ ] ` / `[x] ` (with what follows it), or ''. */
  task: string;
  checked: boolean;
  /** Length of indent + marker + space + task: where the item's own text starts. */
  prefix: number;
  content: string;
};

const HR = /^ {0,3}([-*_])[ \t]*(?:\1[ \t]*){2,}$/;
const ITEM = /^([ \t]*)(?:([-*+])|(\d{1,9})([.)]))([ \t]+)(\[([ xX])\](?:[ \t]+|$))?/;

export function parseItem(line: string): Item | null {
  if (HR.test(line)) return null;
  const m = ITEM.exec(line);
  if (!m) return null;
  const num = m[3] !== undefined ? Number(m[3]) : null;
  const task = m[6] ?? '';
  return {
    indent: m[1], marker: m[2] ?? `${m[3]}${m[4]}`, num, delim: m[4] ?? '', space: m[5], task,
    checked: !!m[7] && m[7] !== ' ', prefix: m[0].length, content: line.slice(m[0].length),
  };
}

/** Width from the line's start to where an item's text starts (where its children line up). */
const contentCol = (it: Item) => widthOf(it.indent) + it.marker.length + widthOf(it.space);

/** The item above line `i` at the same depth, skipping deeper lines and blank lines. */
function prevSibling(lines: string[], i: number, width: number): Item | null {
  for (let k = i - 1; k >= 0; k--) {
    const l = lines[k];
    if (!l.trim()) continue;
    const it = parseItem(l);
    const w = widthOf((/^[ \t]*/.exec(l) ?? [''])[0]);
    if (it && w === width) return it;
    if (w > width) continue;
    return null;
  }
  return null;
}

/** The nearest item above line `i` that is less deep: the one it belongs to. */
function parentOf(lines: string[], i: number, width: number): Item | null {
  for (let k = i - 1; k >= 0; k--) {
    const l = lines[k];
    if (!l.trim()) continue;
    const it = parseItem(l);
    const w = widthOf((/^[ \t]*/.exec(l) ?? [''])[0]);
    if (w >= width) continue;
    return it;
  }
  return null;
}

/** Spaces and tabs `ws` made `cols` columns narrower (never below nothing). */
function narrower(ws: string, cols: number): string {
  const target = Math.max(0, widthOf(ws) - cols);
  let out = ws;
  while (out && widthOf(out) > target) out = out.slice(0, -1);
  return out + ' '.repeat(target - widthOf(out));
}

// ---------- applying line edits ----------

type LineEdit = { line: number; text: string; oldPrefix: number; newPrefix: number };

/** Rebuild the text with some lines replaced; the selection follows the text it was on. A position
 *  inside a line's replaced lead (its marker or indentation) moves to the end of the new lead. */
function withLines(text: string, s: number, e: number, edits: LineEdit[]): Change | null {
  const c = coveredLines(text, s, e);
  const byLine = new Map(edits.map((x) => [x.line, x]));
  if (![...byLine.values()].some((x) => x.text !== c.lines[x.line])) return null;
  const out = c.lines.map((l, i) => byLine.get(i)?.text ?? l);
  const newStarts: number[] = [];
  for (let i = 0, at = 0; i < out.length; i++) { newStarts.push(at); at += out[i].length + 1; }
  const map = (p: number, wholeLine: boolean) => {
    const k = c.at(p), col = p - c.starts[k], x = byLine.get(k);
    if (!x || (wholeLine && col === 0)) return newStarts[k] + col;
    const nc = col >= x.oldPrefix ? col - x.oldPrefix + x.newPrefix : Math.min(col, x.newPrefix);
    return newStarts[k] + Math.min(Math.max(0, nc), out[k].length);
  };
  // A selection that starts at the start of a line keeps the whole line selected.
  const ns = map(s, s !== e);
  return { text: out.join('\n'), start: ns, end: s === e ? ns : map(e, false) };
}

// ---------- Enter: lists continue, an empty item ends the list ----------

export function enter(text: string, s: number, e: number): Change | null {
  if (s !== e || plainZone(text, s)) return null;
  const ls = lineStart(text, s), le = lineEnd(text, s);
  const line = text.slice(ls, le);
  const it = parseItem(line);
  const col = s - ls;
  if (!it || col < it.prefix) return null;
  const lines = text.split('\n');
  const i = text.slice(0, ls).split('\n').length - 1;
  const width = widthOf(it.indent);
  if (!line.slice(it.prefix).trim()) {
    // An empty item: a nested one steps out a level, a top one ends the list.
    if (width > 0) return indent(text, s, s, -1);
    return { text: text.slice(0, ls) + text.slice(le), start: ls, end: ls };
  }
  let marker = it.marker;
  let renumber = false;
  if (it.num !== null) {
    // "1. 1. 1." lists keep their one number; counted lists count on, and so do the items after.
    const prev = prevSibling(lines, i, width);
    const lazy = prev?.num === it.num && prev.delim === it.delim;
    marker = `${lazy ? it.num : it.num + 1}${it.delim}`;
    renumber = !lazy;
  }
  const lead = it.indent + marker + it.space + (it.task ? '[ ] ' : '');
  const rest = line.slice(col).replace(/^[ \t]+/, '');
  lines.splice(i, 1, line.slice(0, col), lead + rest);
  if (renumber && it.num !== null) {
    let old = it.num + 1, next = it.num + 2;
    for (let k = i + 2; k < lines.length; k++) {
      const l = lines[k];
      if (!l.trim()) continue;
      const w = widthOf((/^[ \t]*/.exec(l) ?? [''])[0]);
      if (w > width) continue;
      const sib = parseItem(l);
      if (!sib || w !== width || sib.num !== old || sib.delim !== it.delim) break;
      lines[k] = sib.indent + `${next}${it.delim}` + l.slice(sib.indent.length + sib.marker.length);
      old++; next++;
    }
  }
  const caret = s + 1 + lead.length;
  return { text: lines.join('\n'), start: caret, end: caret };
}

// ---------- Tab / Shift+Tab: list items in and out ----------

/** Indent (`dir` 1) or outdent (-1) the list items a selection touches. Null when the selection
 *  touches no list item, or nothing can move (so Tab keeps moving the focus). */
export function indent(text: string, s: number, e: number, dir: 1 | -1): Change | null {
  if (plainZone(text, s) || plainZone(text, e)) return null;
  const c = coveredLines(text, s, e);
  const lines = c.lines.slice();
  if (!lines.slice(c.first, c.last + 1).some((l) => parseItem(l))) return null;
  const edits: LineEdit[] = [];
  let lastCols = 2, lastUnit = '  ';
  for (let i = c.first; i <= c.last; i++) {
    const l = lines[i];
    if (!l.trim()) continue;
    const it = parseItem(l);
    const ws = (/^[ \t]*/.exec(l) ?? [''])[0];
    const width = widthOf(ws);
    let newWs = ws;
    if (it) {
      if (dir === 1) {
        const prev = prevSibling(lines, i, width);
        const cols = prev ? contentCol(prev) - width : it.marker.length + widthOf(it.space);
        const unit = ws.includes('\t') ? '\t' : ' '.repeat(Math.max(1, cols));
        newWs = ws + unit; lastUnit = unit; lastCols = widthOf(newWs) - width;
      } else {
        if (!width) continue;
        const parent = parentOf(lines, i, width);
        const to = parent ? widthOf(parent.indent) : Math.max(0, width - (it.marker.length + widthOf(it.space)));
        newWs = narrower(ws, width - to); lastCols = width - to;
      }
    } else {
      newWs = dir === 1 ? ws + lastUnit : narrower(ws, Math.min(lastCols, width));
    }
    let body = l.slice(ws.length);
    if (it && it.num !== null) {
      // A numbered item takes the next number at its new depth (1 when it starts a new list there).
      const sib = prevSibling([...lines.slice(0, i), newWs + body], i, widthOf(newWs));
      const n = sib?.num != null && sib.delim === it.delim ? sib.num + 1 : dir === 1 ? 1 : it.num;
      body = `${n}${it.delim}` + body.slice(it.marker.length);
    }
    const nl = newWs + body;
    if (nl === l) continue;
    lines[i] = nl;
    const oldLead = it ? it.prefix : ws.length;
    edits.push({ line: i, text: nl, oldPrefix: oldLead, newPrefix: it ? (parseItem(nl)?.prefix ?? newWs.length) : newWs.length });
  }
  return withLines(text, s, e, edits);
}

// ---------- toolbar: lists, checklists, headings ----------

export type ListKind = 'bullet' | 'number' | 'task';
const isKind = (it: Item | null, k: ListKind) => !!it && (k === 'task' ? !!it.task : !it.task && (k === 'number' ? it.num !== null : it.num === null));
const HEADING = /^ {0,3}(#{1,6})(?:[ \t]+|$)/;

/** What the caret's line is, for the toolbar: its heading level (0 = normal text) and list kind. */
export function lineKind(text: string, pos: number): { heading: number; list: ListKind | null; plain: boolean } {
  const line = text.slice(lineStart(text, pos), lineEnd(text, pos));
  const it = parseItem(line);
  return {
    heading: Number(HEADING.exec(line)?.[1].length ?? 0),
    list: it ? (it.task ? 'task' : it.num !== null ? 'number' : 'bullet') : null,
    plain: plainZone(text, pos),
  };
}

/** Make the touched lines bulleted, numbered or a checklist, or plain text again when they all
 *  already are that. Blank lines between are left alone. */
export function toggleList(text: string, s: number, e: number, kind: ListKind): Change | null {
  if (plainZone(text, s)) return null;
  const c = coveredLines(text, s, e);
  let idx = [];
  for (let i = c.first; i <= c.last; i++) if (c.lines[i].trim()) idx.push(i);
  if (!idx.length) idx = [c.first];
  const off = idx.every((i) => isKind(parseItem(c.lines[i]), kind));
  const counters = new Map<number, number>();
  const edits: LineEdit[] = [];
  for (const i of idx) {
    const l = c.lines[i];
    const it = parseItem(l);
    const ws = it ? it.indent : (/^[ \t]*/.exec(l) ?? [''])[0];
    const rest = it ? it.content : l.slice(ws.length).replace(HEADING, '');
    const oldPrefix = it ? it.prefix : l.length - l.slice(ws.length).replace(HEADING, '').length;
    let lead = '';
    if (!off) {
      if (kind === 'bullet') lead = (it && it.num === null ? it.marker : '-') + ' ';
      else if (kind === 'task') lead = (it && it.num === null ? it.marker : '-') + ` [${it?.checked ? 'x' : ' '}] `;
      else {
        const w = widthOf(ws);
        let n = counters.get(w);
        if (n === undefined) {
          const sib = prevSibling(c.lines, i, w);
          n = sib?.num != null ? sib.num + 1 : 1;
        }
        counters.set(w, n + 1);
        lead = `${n}${it?.delim || '.'} `;
      }
    }
    edits.push({ line: i, text: ws + lead + rest, oldPrefix, newPrefix: ws.length + lead.length });
  }
  return withLines(text, s, e, edits);
}

/** Set the touched lines to heading `level` (1–3), or to normal text (0). */
export function setHeading(text: string, s: number, e: number, level: number): Change | null {
  if (plainZone(text, s)) return null;
  const c = coveredLines(text, s, e);
  const edits: LineEdit[] = [];
  for (let i = c.first; i <= c.last; i++) {
    const l = c.lines[i];
    if (!l.trim() && i !== c.first) continue;
    let rest = l.replace(HEADING, '');
    if (level > 0) { const it = parseItem(rest); if (it) rest = it.content; rest = rest.replace(/^[ \t]+/, ''); }
    const lead = level > 0 ? '#'.repeat(level) + ' ' : '';
    edits.push({ line: i, text: lead + rest, oldPrefix: l.length - rest.length, newPrefix: lead.length });
  }
  return withLines(text, s, e, edits);
}

// ---------- bold and italic ----------

const WORD = /[\p{L}\p{N}\p{M}_\u200c\u200d]/u;
const stars = (text: string, i: number, step: 1 | -1) => { let n = 0; for (let k = step > 0 ? i : i - 1; k >= 0 && k < text.length && text[k] === '*'; k += step) n++; return n; };

/** The word around the caret, if it is in one. */
export function wordAt(text: string, pos: number): [number, number] | null {
  const ls = lineStart(text, pos), le = lineEnd(text, pos);
  const before = /[\p{L}\p{N}\p{M}_\u200c\u200d]+$/u.exec(text.slice(ls, pos))?.[0].length ?? 0;
  const after = /^[\p{L}\p{N}\p{M}_\u200c\u200d]+/u.exec(text.slice(pos, le))?.[0].length ?? 0;
  return before || after ? [pos - before, pos + after] : null;
}

/** Is [a, b) wrapped in `mark` (`**` bold, `*` italic), counting the stars on both sides? */
function wrapped(text: string, a: number, b: number, mark: string): boolean {
  const m = Math.min(stars(text, a, -1), stars(text, b, 1));
  return mark === '**' ? m >= 2 : m === 1 || m >= 3;
}

/** Bold or italic on or off for the selection (each line on its own), the word at the caret, or —
 *  with neither — a pair of marks with the caret between them. */
export function toggleInline(text: string, s: number, e: number, mark: '**' | '*'): Change | null {
  if (plainZone(text, s)) return null;
  const L = mark.length;
  if (s === e) {
    const w = wordAt(text, s);
    if (w) {
      const r = toggleInline(text, w[0], w[1], mark)!;
      const shift = r.text.length > text.length ? L : -L;
      return { text: r.text, start: s + shift, end: s + shift };
    }
    if (stars(text, s, -1) >= L && stars(text, s, 1) >= L && wrapped(text, s, s, mark)) {
      return { text: text.slice(0, s - L) + text.slice(s + L), start: s - L, end: s - L };
    }
    return { text: text.slice(0, s) + mark + mark + text.slice(s), start: s + L, end: s + L };
  }
  // The parts to change: on each line, the selected text without its outer spaces and without
  // a list marker or heading marks in front.
  const c = coveredLines(text, s, e);
  const parts: [number, number][] = [];
  for (let i = c.first; i <= c.last; i++) {
    const l = c.lines[i], st = c.starts[i];
    const lead = Math.max(parseItem(l)?.prefix ?? 0, HEADING.exec(l)?.[0].length ?? 0);
    let a = Math.max(s, st + lead), b = Math.min(e, st + l.length);
    while (a < b && /\s/.test(text[a])) a++;
    while (b > a && /\s/.test(text[b - 1])) b--;
    // Marks that were selected along with the words count as being around them.
    const inA = stars(text, a, 1), inB = stars(text, b, -1);
    if (inA && inB && a + inA < b - inB) { a += inA; b -= inB; }
    if (a < b) parts.push([a, b]);
  }
  if (!parts.length) return null;
  const off = parts.every(([a, b]) => wrapped(text, a, b, mark));
  let out = text;
  for (const [a, b] of parts.slice().reverse()) {
    if (off) out = out.slice(0, a - L) + out.slice(a, b) + out.slice(b + L);
    else if (!wrapped(text, a, b, mark)) out = out.slice(0, a) + mark + out.slice(a, b) + mark + out.slice(b);
  }
  // Keep the same words selected.
  const delta = (p: number) => {
    let d = 0;
    for (const [a, b] of parts) {
      if (!off && wrapped(text, a, b, mark)) continue;
      const sign = off ? -1 : 1;
      if (p > b) d += 2 * L * sign;
      else if (p >= a) d += L * sign;
    }
    return d;
  };
  const fa = parts[0][0], lb = parts[parts.length - 1][1];
  return { text: out, start: fa + delta(fa), end: lb + delta(lb) };
}

// ---------- links ----------

const LINK = /(!?)\[((?:\\.|[^\\\]\n])*)\]\((<[^>\n]*>|[^)\s]*)(?:[ \t]+"[^"\n]*")?\)/g;

export type LinkSpan = { start: number; end: number; label: string; target: string };

/** The link (not a picture) the caret or selection is in, if any. */
export function linkAt(text: string, s: number, e = s): LinkSpan | null {
  const ls = lineStart(text, s), line = text.slice(ls, lineEnd(text, s));
  for (const m of line.matchAll(LINK)) {
    const a = ls + m.index!, b = a + m[0].length;
    if (m[1] || s < a || e > b) continue;
    return { start: a, end: b, label: m[2].replace(/\\([\\[\]])/g, '$1'), target: m[3].replace(/^<|>$/g, '') };
  }
  return null;
}

const escLabel = (s: string) => s.replace(/([\\[\]])/g, '\\$1');

const FILE_EXT = /\.(md|txt|csv|json|pdf|png|jpe?g|gif|webp|docx?|xlsx?|pptx?|zip)$/i;
/** What the user typed as a link address, ready for Markdown: `example.com` becomes a web address,
 *  `name@example.com` an email address, and spaces and brackets are escaped. */
export function linkTarget(raw: string): string {
  let u = raw.trim();
  if (!u) return '';
  if (/^[a-z][a-z0-9+.-]*:/i.test(u) || u.startsWith('#') || u.startsWith('/') || u.startsWith('.')) { /* as typed */ }
  else if (/^[^\s@/]+@[^\s@/]+\.[^\s@/]+$/.test(u)) u = 'mailto:' + u;
  else if (/^www\./i.test(u) || (/^[^\s/]+\.[a-z]{2,}(?:[/?#:]|$)/i.test(u) && !FILE_EXT.test(u.split(/[?#]/)[0]))) u = 'https://' + u;
  return u.replace(/[ ()<>]/g, (ch) => ({ ' ': '%20', '(': '%28', ')': '%29', '<': '%3C', '>': '%3E' })[ch]!);
}

export const isUrl = (s: string) => /^(https?:\/\/[^\s/$.?#][^\s]*|mailto:[^\s@]+@[^\s@]+)$/i.test(s.trim());

/** Insert or change a link. An empty address over an existing link removes the link and keeps its text. */
export function setLink(text: string, s: number, e: number, label: string, address: string): Change {
  const old = linkAt(text, s, e);
  const [a, b] = old ? [old.start, old.end] : [s, e];
  const target = linkTarget(address);
  const shown = label.trim() || (old ? old.label : '') || address.trim();
  const md = target ? `[${escLabel(shown)}](${target})` : shown;
  return { text: text.slice(0, a) + md + text.slice(b), start: a + md.length, end: a + md.length };
}

/** A web address pasted over selected words makes them a link. Null: paste as usual. */
export function pasteLink(text: string, s: number, e: number, pasted: string): Change | null {
  if (s === e || plainZone(text, s)) return null;
  const sel = text.slice(s, e);
  if (sel.includes('\n') || !sel.trim() || isUrl(sel) || !isUrl(pasted) || linkAt(text, s, e)) return null;
  const md = `[${escLabel(sel)}](${linkTarget(pasted)})`;
  return { text: text.slice(0, s) + md + text.slice(e), start: s + md.length, end: s + md.length };
}

// ---------- pictures ----------

export const pictureMarkdown = (alt: string, link: string) => `![${escLabel(alt.replace(/\s+/g, ' ').trim())}](${linkTarget(link)})`;

/** Put blocks (pictures) in place of the selection, each on a line of its own. */
export function insertBlocks(text: string, s: number, e: number, blocks: string[]): Change {
  const before = s > 0 && text[s - 1] !== '\n' ? '\n' : '';
  const after = e < text.length && text[e] !== '\n' ? '\n' : '';
  const ins = before + blocks.join('\n\n') + after;
  const caret = s + before.length + blocks.join('\n\n').length;
  return { text: text.slice(0, s) + ins + text.slice(e), start: caret, end: caret };
}

/** The smallest replacement that turns `a` into `b`: [from, to) in `a` gets `insert`. */
export function diff(a: string, b: string): { from: number; to: number; insert: string } {
  let p = 0;
  const n = Math.min(a.length, b.length);
  while (p < n && a.charCodeAt(p) === b.charCodeAt(p)) p++;
  let q = 0;
  while (q < n - p && a.charCodeAt(a.length - 1 - q) === b.charCodeAt(b.length - 1 - q)) q++;
  // Never split a surrogate pair.
  if (p > 0 && /[\ud800-\udbff]/.test(a[p - 1])) p--;
  if (q > 0 && /[\udc00-\udfff]/.test(a[a.length - q])) q--;
  return { from: p, to: a.length - q, insert: b.slice(p, b.length - q) };
}

// ---------- line endings ----------

export const toLF = (s: string) => s.replace(/\r\n?/g, '\n');

/** The text box only knows "\n". Give `lf` the line endings of `prev` (the same text before the
 *  last change): every untouched line keeps its own ending, new lines get the one the file mostly uses. */
export function restoreEol(prev: string, lf: string): string {
  if (!prev.includes('\r')) return lf;
  const parts = prev.split(/(\r\n|\r|\n)/);
  const pt: string[] = [], ps: string[] = [];
  parts.forEach((x, i) => (i % 2 ? ps : pt).push(x));
  const count: Record<string, number> = {};
  for (const x of ps) count[x] = (count[x] ?? 0) + 1;
  const usual = Object.keys(count).sort((x, y) => count[y] - count[x])[0] ?? '\n';
  const nt = lf.split('\n');
  let p = 0;
  while (p < pt.length && p < nt.length && pt[p] === nt[p]) p++;
  let q = 0;
  while (q < pt.length - p && q < nt.length - p && pt[pt.length - 1 - q] === nt[nt.length - 1 - q]) q++;
  const same = pt.length === nt.length; // lines edited in place keep their endings too
  let out = nt[0];
  for (let k = 0; k < nt.length - 1; k++) {
    let end: string | undefined;
    if (k < p || same) end = ps[k];
    else if (k >= nt.length - q) end = ps[k + pt.length - nt.length];
    out += (end ?? usual) + nt[k + 1];
  }
  return out;
}

// ---------- the formatted view beside the editor ----------

export type Block = { line: number; kind: string };

/** Where each block of the formatted view starts in the text (line numbers, note details counted),
 *  in order, with its kind (heading, paragraph, list, code, table, blockquote, hr, html). */
export function blockStarts(text: string): Block[] {
  const fm = frontmatterEnd(text);
  const body = text.slice(fm);
  let tokens: any[] = [];
  try { tokens = marked.lexer(body) as any[]; } catch { return []; }
  const out: Block[] = [];
  let at = 0, line = text.slice(0, fm).split('\n').length - 1, counted = 0;
  for (const t of tokens) {
    if (t.type === 'space' || !t.raw) continue;
    const i = body.indexOf(t.raw, at);
    if (i < 0) continue;
    for (let k = counted; k < i; k++) if (body.charCodeAt(k) === 10) line++;
    counted = i;
    out.push({ line, kind: t.type });
    at = i + t.raw.length;
  }
  return out;
}

/** The note's own direction, from its first letter (note details left out); '' when it has none. */
export function textDir(text: string): 'rtl' | 'ltr' | '' {
  const fm = frontmatterEnd(text);
  const m = /[\p{L}]/u.exec(text.slice(fm, fm + 4000).replace(/!?\[[^\]]*\]\([^)]*\)|https?:\/\/\S+|`[^`]*`/g, ''));
  if (!m) return '';
  return /[\u0590-\u08ff\ufb1d-\ufdff\ufe70-\ufeff]/.test(m[0]) ? 'rtl' : 'ltr';
}
