// MADO: reading a selection out of a rendered document and marking commented passages in it.
// The page side of anchor.ts: the same "letters and digits in reading order", each remembering the
// text node it sits in. Parts the renderer adds itself (the header of a code block with its copy
// button, the box of a task) are not the document's words and are skipped on both sides.
import { fold, type Picked, type Span } from '../anchor';

const SKIP = '.md-codehead, .md-task, [data-mado-skip]';
const MARK = 'mado-hl';

export interface PageText {
  /** The letters and digits of the page. */
  text: string;
  nodes: Text[];
  /** For each character of `text`: its text node (an index into `nodes`) and its offset there. */
  nodeAt: number[];
  offAt: number[];
  /** For each node: the index in `text` of its first kept character (text.length when it has none after it). */
  starts: number[];
}

/** The words of a rendered document. */
export function pageText(root: Node): PageText {
  const nodes: Text[] = [], nodeAt: number[] = [], offAt: number[] = [], starts: number[] = [];
  let text = '';
  const doc = root.ownerDocument ?? document;
  const walker = doc.createTreeWalker(root, 4 /* NodeFilter.SHOW_TEXT */);
  for (let n = walker.nextNode() as Text | null; n; n = walker.nextNode() as Text | null) {
    if (n.parentElement?.closest(SKIP)) continue;
    const k = nodes.push(n) - 1;
    starts.push(text.length);
    const data = n.data;
    for (let i = 0; i < data.length; i++) {
      if (fold(data[i])) { text += data[i]; nodeAt.push(k); offAt.push(i); }
    }
  }
  return { text, nodes, nodeAt, offAt, starts };
}

/** The part of the page a range covers, as a stretch of `page.text` (null: no words in it). */
export function rangeSpan(page: PageText, range: Range): Span | null {
  let start = -1, end = -1;
  for (let k = 0; k < page.nodes.length; k++) {
    const node = page.nodes[k];
    if (!range.intersectsNode(node)) { if (start >= 0) break; continue; }
    const lo = node === range.startContainer ? range.startOffset : 0;
    const hi = node === range.endContainer ? range.endOffset : node.length;
    const last = k + 1 < page.starts.length ? page.starts[k + 1] : page.text.length;
    for (let i = page.starts[k]; i < last; i++) {
      const off = page.offAt[i];
      if (off < lo || off >= hi) continue;
      if (start < 0) start = i;
      end = i + 1;
    }
  }
  return start >= 0 ? { start, end } : null;
}

export interface PagePick extends Picked { /** The words as the reader sees them, for showing. */ label: string; rect: DOMRect | null }

/** What is selected inside `root` right now, ready for `createAnchor`; null when nothing useful is. */
export function pickedIn(root: HTMLElement, selection: Selection | null = typeof window !== 'undefined' ? window.getSelection() : null): PagePick | null {
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) return null;
  const range = selection.getRangeAt(0);
  if (!root.contains(range.commonAncestorContainer) && !range.intersectsNode(root)) return null;
  const page = pageText(root);
  const span = rangeSpan(page, range);
  if (!span) return null;
  let rect: DOMRect | null = null;
  try { rect = range.getBoundingClientRect?.() ?? null; } catch { /* no layout (tests) */ }
  return {
    text: page.text.slice(span.start, span.end),
    before: page.text.slice(0, span.start),
    after: page.text.slice(span.end),
    label: range.toString().replace(/\s+/g, ' ').trim().slice(0, 240),
    rect,
  };
}

export interface Highlight { id: string; span: Span; /** Extra class names: the comment's state. */ cls?: string }

/** Take every mark out again, leaving the document as it was rendered. */
export function unpaint(root: HTMLElement): void {
  const marks = Array.from(root.querySelectorAll(`mark.${MARK}`));
  for (const m of marks) m.replaceWith(...Array.from(m.childNodes));
  if (marks.length) root.normalize();
}

/** Mark commented passages (stretches of the page's own text, see `onPage` in anchor.ts). */
export function paint(root: HTMLElement, marks: Highlight[], label: (id: string) => string = () => ''): void {
  unpaint(root);
  if (!marks.length) return;
  const page = pageText(root);
  // One passage at a time, earlier first; where two overlap, the later one starts after the earlier.
  const sorted = [...marks].filter((m) => m.span.end > m.span.start).sort((a, b) => a.span.start - b.span.start);
  const perNode = new Map<number, { from: number; to: number; m: Highlight }[]>();
  let floor = 0;
  for (const m of sorted) {
    const start = Math.max(m.span.start, floor), end = Math.min(m.span.end, page.text.length);
    if (end <= start) continue;
    floor = end;
    for (let i = start; i < end;) {
      const k = page.nodeAt[i];
      let j = i;
      while (j + 1 < end && page.nodeAt[j + 1] === k) j++;
      const list = perNode.get(k) ?? [];
      // A passage that runs on into the next node takes the spaces at the joint with it.
      list.push({ from: i === start ? page.offAt[i] : 0, to: j + 1 === end ? page.offAt[j] + 1 : page.nodes[k].length, m });
      perNode.set(k, list);
      i = j + 1;
    }
  }
  const doc = root.ownerDocument;
  for (const [k, list] of perNode) {
    const node = page.nodes[k];
    // From the end of the node backwards, so the offsets of what is still to do stay true.
    for (const seg of list.sort((a, b) => b.from - a.from)) {
      if (seg.to < node.length) node.splitText(seg.to);
      const mid = seg.from > 0 ? node.splitText(seg.from) : node;
      const mark = doc.createElement('mark');
      mark.className = `${MARK}${seg.m.cls ? ' ' + seg.m.cls : ''}`;
      mark.dataset.madoComment = seg.m.id;
      mark.tabIndex = 0;
      const say = label(seg.m.id);
      if (say) mark.title = say;
      mid.replaceWith(mark);
      mark.appendChild(mid);
    }
  }
}

/** The comment a click or key press inside a rendered document landed on. */
export function commentAt(e: Event): string | null {
  const el = (e.target as HTMLElement | null)?.closest?.(`mark.${MARK}`) as HTMLElement | null;
  return el?.dataset.madoComment ?? null;
}

/** A click inside a rendered document: the comment whose mark was clicked, or null. It never
 *  opens, stops or otherwise handles a link — a click on one (also one inside a mark) is left
 *  untouched for the app's own link guard, which confirms where a link really goes. */
export function docClick(e: Event): string | null {
  if ((e.target as Element | null)?.closest?.('a, area')) return null;
  return commentAt(e);
}
