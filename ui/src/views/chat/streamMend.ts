// The block of an answer that is still being written, made presentable for one frame. Markdown read
// half-way through shows its workings: "**bold" prints its asterisks until the closing pair arrives,
// a table's header row is a line of pipes until the row under it comes, "[link](https://ex" shows
// as brackets. Claude's own chat never shows those, so neither does this one: what is still open is
// closed (or left out) in the copy that is rendered, never in the text itself. The next piece of the
// answer is rendered from the real text again, so nothing here outlives a frame.

/** Block kinds whose inline text can be mended (code and raw HTML are shown as they are). */
const INLINE = new Set(['paragraph', 'heading', 'list', 'blockquote', 'table', 'text']);

/**
 * The still-open block `raw` (a top-level token of type `type`), ready to render.
 * Returns '' when nothing of it should be shown yet.
 */
export function mendTail(raw: string, type: string): string {
  // A code block's closing fence half-typed ("``") would show as a line of code for a moment.
  if (type === 'code') return /^ {0,3}(`{3,}|~{3,})/.test(raw) ? raw.replace(/\n {0,3}(?:`+|~+)[ \t]*$/, '\n') : raw;
  if (!INLINE.has(type)) return raw;
  let s = raw;
  // A last line that may yet become something else: "#" (a heading), "-" or "=" under a paragraph
  // (which marked takes for a heading underline), "*" / "---" (a rule or a list). Held for a moment.
  s = s.replace(/(^|\n)[ \t]*(?:#{1,6}|[-=*_+]+|\d{1,9}[.)]?)[ \t]*$/, '$1');
  // A table whose header row has arrived but not yet the row of dashes under it: held back until
  // marked can lay it out as a table (otherwise a line of pipes jumps into a grid).
  if (type === 'paragraph') s = s.replace(/(^|\n)(?:[ \t]*\|[^\n]*(?:\n|$))+$/, '$1');
  if (!s.trim()) return '';
  return closeInline(dropOpenLink(s));
}

/** "[text](https://half" and "![alt](half" at the end: the text alone (an image: nothing yet). */
function dropOpenLink(s: string): string {
  // (Not a bracket inside inline code that is still open.)
  const inCode = (at: number) => ((s.slice(0, at).match(/`/g)?.length ?? 0) % 2) === 1;
  const m = /(!?)\[([^\]\n]*)\]\([^)\s]*(?:\s+"[^"\n]*)?$/.exec(s);
  if (m && !inCode(m.index)) return s.slice(0, m.index) + (m[1] ? '' : m[2]);
  // "[text" with nothing after it yet, at the very end: the bracket waits.
  const b = /(^|[^\\!\]])\[([^\]\n]*)$/.exec(s);
  if (b && !inCode(b.index)) return s.slice(0, b.index) + b[1] + b[2];
  return s;
}

/** Close emphasis, strikethrough and inline code left open in the last line(s). */
function closeInline(s: string): string {
  const stack: { mark: string; at: number }[] = [];
  let code = 0;          // length of the backtick run that opened the code span we are in (0: none)
  let codeAt = -1;
  const isSpace = (c: string | undefined) => c === undefined || /\s/.test(c);
  const isWord = (c: string | undefined) => !!c && /[\p{L}\p{N}]/u.test(c);
  for (let i = 0; i < s.length; ) {
    const c = s[i];
    if (c === '\\' && !code) { i += 2; continue; }
    if (c === '`') {
      let n = 1; while (s[i + n] === '`') n++;
      if (!code) { code = n; codeAt = i; } else if (n === code) { code = 0; codeAt = -1; }
      i += n; continue;
    }
    if (code) { i++; continue; }
    // A list marker at the start of a line is not emphasis.
    if ((c === '*' || c === '-' || c === '+') && /(^|\n)[ \t]*$/.test(s.slice(Math.max(0, i - 12), i)) && isSpace(s[i + 1])) { i++; continue; }
    let mark = '';
    if (c === '*' || c === '_' || c === '~') {
      let n = 1; while (s[i + n] === c) n++;
      if (c === '~') { if (n < 2) { i++; continue; } mark = '~~'; }
      else mark = c.repeat(Math.min(n, 3));
      const before = s[i - 1], after = s[i + n];
      // Inside a word an underscore is just a letter (snake_case).
      if (c === '_' && isWord(before) && isWord(after)) { i += n; continue; }
      const opens = !isSpace(after) || i + n === s.length, closes = !isSpace(before);
      if (mark === '***') {
        // Both at once: treat as strong around emphasis.
        for (const part of ['**', c]) toggle(part, i, opens, closes);
      } else toggle(mark, i, opens, closes);
      i += n; continue;
    }
    i++;
  }
  function toggle(mark: string, at: number, opens: boolean, closes: boolean) {
    const k = stack.map((x) => x.mark).lastIndexOf(mark);
    if (k >= 0 && closes) { stack.splice(k); return; }
    if (opens) stack.push({ mark, at });
  }
  let out = s;
  if (code) {
    // An opener with nothing after it yet is left out; otherwise the span is closed.
    if (!out.slice(codeAt + code).trim()) return closeMarks(out.slice(0, codeAt), stack.filter((x) => x.at < codeAt));
    out += '`'.repeat(code);
  }
  return closeMarks(out, stack);
}

function closeMarks(s: string, stack: { mark: string; at: number }[]): string {
  let out = s;
  // From the innermost out. An opener with nothing after it yet is dropped rather than closed.
  for (let k = stack.length - 1; k >= 0; k--) {
    const { mark, at } = stack[k];
    const after = out.slice(at + mark.length);
    if (!after.trim()) { out = out.slice(0, at) + after; continue; }
    // A closing mark must touch the text: it goes before any trailing spaces.
    const ws = /\s*$/.exec(out)![0];
    out = out.slice(0, out.length - ws.length) + mark + ws;
  }
  return out;
}
