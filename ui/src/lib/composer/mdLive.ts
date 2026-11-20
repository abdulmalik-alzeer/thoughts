// Markdown formatted as it is typed, in the message box (Composer.svelte). The text stays exactly
// what was typed — marks and all, which are only dimmed — but **bold** looks bold, `code` looks like
// code, a heading is larger, a quote has its bar and a fenced block its panel, as in the message box
// of Claude's own chat. The text is read with @lezer/markdown (GitHub's flavour) and read again only
// from where it changed.
import { Decoration, EditorView, ViewPlugin, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import type { Range } from '@codemirror/state';
import { parser as baseParser, GFM } from '@lezer/markdown';
import { TreeFragment, type Tree } from '@lezer/common';
import { highlightTree, tagHighlighter, tags } from '@lezer/highlight';

const mdParser = baseParser.configure(GFM);

/** Classes for the inline parts (several may apply to one stretch: bold inside a link). */
const inline = tagHighlighter([
  { tag: tags.strong, class: 'cmd-strong' },
  { tag: tags.emphasis, class: 'cmd-em' },
  { tag: tags.strikethrough, class: 'cmd-strike' },
  { tag: tags.monospace, class: 'cmd-code' },
  { tag: [tags.link, tags.url], class: 'cmd-link' },
  { tag: tags.processingInstruction, class: 'cmd-mark' },
  { tag: tags.labelName, class: 'cmd-label' },
  { tag: tags.contentSeparator, class: 'cmd-mark' },
]);

const HEADING = /^(?:ATXHeading|SetextHeading)([1-6])$/;
/** Each line also finds its own direction, so an Arabic line and an English one sit correctly. */
const autoDir = Decoration.line({ attributes: { dir: 'auto' } });
const lineDeco = new Map<string, Decoration>();
const lineClass = (cls: string) => {
  let d = lineDeco.get(cls);
  // (Code is always written left to right.)
  if (!d) lineDeco.set(cls, (d = Decoration.line({ class: cls, attributes: { dir: cls.includes('cmd-block') ? 'ltr' : 'auto' } })));
  return d;
};
const markDeco = new Map<string, Decoration>();
const markClass = (cls: string) => {
  let d = markDeco.get(cls);
  if (!d) markDeco.set(cls, (d = Decoration.mark({ class: cls })));
  return d;
};

/** The decorations for a document and its tree (also used by the tests). */
export function decorate(doc: { lines: number; line(n: number): { from: number; to: number }; lineAt(pos: number): { number: number } }, tree: Tree): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  // Line kinds first: headings, quotes, code blocks, by line number.
  const kinds = new Map<number, string[]>();
  const add = (n: number, cls: string) => { const k = kinds.get(n); if (k) k.push(cls); else kinds.set(n, [cls]); };
  tree.iterate({
    enter: (node) => {
      const name = node.name;
      const h = HEADING.exec(name);
      if (h) { for (let n = doc.lineAt(node.from).number, e = doc.lineAt(node.to).number; n <= e; n++) add(n, 'cmd-h cmd-h' + h[1]); return false; }
      if (name === 'Blockquote') { for (let n = doc.lineAt(node.from).number, e = doc.lineAt(node.to).number; n <= e; n++) add(n, 'cmd-quote'); }
      if (name === 'FencedCode' || name === 'CodeBlock') {
        const a = doc.lineAt(node.from).number, b = doc.lineAt(node.to).number;
        for (let n = a; n <= b; n++) add(n, 'cmd-block' + (n === a ? ' cmd-block-first' : '') + (n === b ? ' cmd-block-last' : ''));
        return false;
      }
      return undefined;
    },
  });
  for (let n = 1; n <= doc.lines; n++) {
    const l = doc.line(n);
    const k = kinds.get(n);
    ranges.push((k ? lineClass([...new Set(k.join(' ').split(' '))].join(' ')) : autoDir).range(l.from));
  }
  highlightTree(tree, inline, (from, to, cls) => { if (to > from) ranges.push(markClass(cls).range(from, to)); });
  // Inside a fenced block only its fences are dimmed; the code itself is plain monospace.
  tree.iterate({
    enter: (node) => {
      if (node.name === 'CodeInfo') ranges.push(markClass('cmd-info').range(node.from, node.to));
      return undefined;
    },
  });
  return Decoration.set(ranges, true);
}

class Live {
  decorations: DecorationSet;
  private tree: Tree;
  private fragments: readonly TreeFragment[] = [];
  constructor(view: EditorView) {
    this.tree = mdParser.parse(view.state.doc.toString());
    this.decorations = decorate(view.state.doc, this.tree);
  }
  update(u: ViewUpdate) {
    if (!u.docChanged) return;
    const changed: { fromA: number; toA: number; fromB: number; toB: number }[] = [];
    u.changes.iterChangedRanges((fromA, toA, fromB, toB) => changed.push({ fromA, toA, fromB, toB }));
    this.fragments = TreeFragment.applyChanges(TreeFragment.addTree(this.tree, this.fragments), changed);
    this.tree = mdParser.parse(u.state.doc.toString(), this.fragments);
    this.decorations = decorate(u.state.doc, this.tree);
  }
}

export const markdownLive = [
  ViewPlugin.fromClass(Live, { decorations: (v) => v.decorations }),
  EditorView.perLineTextDirection.of(true),
];
