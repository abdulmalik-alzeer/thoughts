// OpenDocument text (.odt) as a readable page, and the shared pieces for reading OpenDocument text
// (styles, lists, paragraphs) that slides.ts uses for presentations (.odp). Everything comes from the
// file's content.xml, parsed with DOMParser and only ever read: the page is built with DOM calls and
// text nodes, so nothing in the file can run or style the app, and pictures are only ever the
// file's own (anything pointing outside it is left out).
import { OfficeError, OfficeZip, Pictures, parseXml, resolvePart, imgType } from './safezip';

export const NS = {
  office: 'urn:oasis:names:tc:opendocument:xmlns:office:1.0',
  text: 'urn:oasis:names:tc:opendocument:xmlns:text:1.0',
  table: 'urn:oasis:names:tc:opendocument:xmlns:table:1.0',
  draw: 'urn:oasis:names:tc:opendocument:xmlns:drawing:1.0',
  style: 'urn:oasis:names:tc:opendocument:xmlns:style:1.0',
  fo: 'urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0',
  xlink: 'http://www.w3.org/1999/xlink',
  pres: 'urn:oasis:names:tc:opendocument:xmlns:presentation:1.0',
  svg: 'urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0',
};

export const kids = (el: Element) => Array.from(el.children);
export const is = (el: Element, ns: string, name: string) => el.namespaceURI === ns && el.localName === name;
export const at = (el: Element | null | undefined, ns: string, name: string) => el?.getAttributeNS(ns, name) ?? null;

export type Fmt = { b?: boolean; i?: boolean; u?: boolean; s?: boolean };
export type ParaStyle = { rtl?: boolean; align?: 'center' | 'right' | 'left' | 'justify'; heading?: number };

/** What the file's styles say about text runs, paragraphs and lists (by style name). */
export class OdfStyles {
  text = new Map<string, Fmt>();
  para = new Map<string, ParaStyle>();
  /** list style → is level n (0-based) numbered? */
  lists = new Map<string, boolean[]>();
  add(doc: Document | null) {
    if (!doc) return;
    for (const s of Array.from(doc.getElementsByTagNameNS(NS.style, 'style'))) {
      const name = at(s, NS.style, 'name') || '';
      const family = at(s, NS.style, 'family');
      const tp = kids(s).find((c) => is(c, NS.style, 'text-properties'));
      const pp = kids(s).find((c) => is(c, NS.style, 'paragraph-properties'));
      const parent = at(s, NS.style, 'parent-style-name') || '';
      if (tp) {
        const f: Fmt = { ...(this.text.get(parent) ?? {}) };
        const w = at(tp, NS.fo, 'font-weight'); if (w) f.b = w === 'bold' || +w >= 600;
        const st = at(tp, NS.fo, 'font-style'); if (st) f.i = st === 'italic' || st === 'oblique';
        const u = at(tp, NS.style, 'text-underline-style'); if (u) f.u = u !== 'none';
        const lt = at(tp, NS.style, 'text-line-through-style'); if (lt) f.s = lt !== 'none';
        this.text.set(name, f);
      }
      if (family === 'paragraph' || pp) {
        const p: ParaStyle = { ...(this.para.get(parent) ?? {}) };
        const wm = at(pp, NS.style, 'writing-mode');
        if (wm) p.rtl = /^rl/.test(wm);
        const al = at(pp, NS.fo, 'text-align');
        // In a right-to-left paragraph "start" is the right edge.
        if (al) p.align = al === 'center' ? 'center' : al === 'justify' ? 'justify' : al === 'end' ? (p.rtl ? 'left' : 'right') : al === 'right' ? 'right' : al === 'left' ? 'left' : undefined;
        const disp = (at(s, NS.style, 'display-name') || name).toLowerCase();
        const lvl = at(s, NS.style, 'default-outline-level');
        if (/^title$/.test(disp)) p.heading = 1;
        else if (/^subtitle$/.test(disp)) p.heading = 2;
        else if (lvl && /heading/.test(disp)) p.heading = Math.min(6, +lvl || 1);
        this.para.set(name, p);
      }
    }
    for (const l of Array.from(doc.getElementsByTagNameNS(NS.text, 'list-style'))) {
      const levels: boolean[] = [];
      for (const lv of kids(l)) {
        const n = Number(at(lv, NS.text, 'level') || 1) - 1;
        if (n >= 0 && n < 10) levels[n] = lv.localName === 'list-level-style-number';
      }
      this.lists.set(at(l, NS.style, 'name') || '', levels);
    }
  }
  /** A paragraph's text style (bold title paragraphs and the like). */
  paraFmt(name: string): Fmt { return this.text.get(name) ?? {}; }
}

/** Is an image address inside the file (`Pictures/x.png`), and where? Outside addresses → null. */
export function internalPart(href: string): string | null {
  if (!href || /^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('/') || href.startsWith('//')) return null;
  const p = resolvePart('content.xml', href.replace(/^\.\//, ''));
  return p && !p.startsWith('..') ? p : null;
}

/** The text of an element as plain text (paragraphs on lines of their own). For notes, table cells,
 *  slide titles. */
export function plainText(el: Element): string {
  const out: string[] = [];
  const walk = (n: Element) => {
    for (const c of Array.from(n.childNodes)) {
      if (c.nodeType === 3) { out.push(c.textContent ?? ''); continue; }
      if (c.nodeType !== 1) continue;
      const e = c as Element;
      if (e.namespaceURI === NS.text) {
        if (e.localName === 's') { out.push(' '.repeat(Math.min(100, Number(at(e, NS.text, 'c') || 1)))); continue; }
        if (e.localName === 'tab') { out.push('\t'); continue; }
        if (e.localName === 'line-break') { out.push('\n'); continue; }
        if (e.localName === 'note') continue;
        if (e.localName === 'p' || e.localName === 'h') { if (out.length && !out[out.length - 1].endsWith('\n')) out.push('\n'); walk(e); out.push('\n'); continue; }
      }
      if (is(e, NS.office, 'annotation')) continue;
      walk(e);
    }
  };
  walk(el);
  return out.join('').replace(/\n+$/, '').replace(/^\n+/, '');
}

export type OdtView = { node: DocumentFragment; urls: string[]; skipped: boolean };

const SAFE_LINK = /^(https?:|mailto:)/i;

/** An OpenDocument text file as a page (headings, paragraphs with bold/italic/underline, lists,
 *  tables, links to web pages, pictures, alignment and right-to-left paragraphs). Not shown:
 *  footnotes, comments, headers and footers, text boxes, charts and page layout. */
export async function odtToDom(bytes: Uint8Array): Promise<OdtView> {
  const z = new OfficeZip(bytes);
  const contentText = await z.text('content.xml');
  if (!contentText) throw new OfficeError('wrongKind', 'no content');
  const doc = parseXml(contentText);
  const body = doc.getElementsByTagNameNS(NS.office, 'text')[0];
  if (!body) throw new OfficeError('wrongKind', 'not a text document');
  const styles = new OdfStyles();
  const stylesText = await z.text('styles.xml');
  styles.add(stylesText ? parseXml(stylesText) : null);
  styles.add(doc);
  const pics = new Pictures(z);
  let skipped = false;
  try {
    const frag = document.createDocumentFragment();
    const ctx: OdfCtx = { styles, pics, skip: () => { skipped = true; } };
    await odfBlocks(body, frag, ctx, { list: '', depth: 0 });
    return { node: frag, urls: pics.urls, skipped };
  } catch (e) {
    for (const u of pics.urls) URL.revokeObjectURL(u);
    throw e;
  }
}

export type OdfCtx = { styles: OdfStyles; pics: Pictures; skip: () => void };

/** Text runs of a paragraph-like element into `out`. */
export async function odfInline(el: Element, out: Node, ctx: OdfCtx, fmt: Fmt = {}): Promise<void> {
  for (const c of Array.from(el.childNodes)) {
    if (c.nodeType === 3) { out.appendChild(wrapFmt(document.createTextNode(c.textContent ?? ''), fmt)); continue; }
    if (c.nodeType !== 1) continue;
    const e = c as Element;
    if (e.namespaceURI === NS.text) {
      switch (e.localName) {
        case 's': out.appendChild(document.createTextNode(' '.repeat(Math.min(100, Math.max(1, Number(at(e, NS.text, 'c') || 1)))))); continue;
        case 'tab': out.appendChild(document.createTextNode('\t')); continue;
        case 'line-break': out.appendChild(document.createElement('br')); continue;
        case 'note': ctx.skip(); continue;
        case 'bookmark': case 'bookmark-start': case 'bookmark-end': case 'soft-page-break': case 'change-start': case 'change-end': case 'change': case 'reference-mark-start': case 'reference-mark-end': continue;
        case 'span': await odfInline(e, out, ctx, { ...fmt, ...ctx.styles.text.get(at(e, NS.text, 'style-name') || '') }); continue;
        case 'a': {
          const href = at(e, NS.xlink, 'href') || '';
          if (SAFE_LINK.test(href)) { const a = document.createElement('a'); a.href = href; await odfInline(e, a, ctx, fmt); out.appendChild(a); }
          else await odfInline(e, out, ctx, fmt);
          continue;
        }
      }
    }
    if (is(e, NS.office, 'annotation') || is(e, NS.office, 'annotation-end')) { continue; }
    if (is(e, NS.draw, 'frame') || is(e, NS.draw, 'a')) { await odfFrame(e, out, ctx); continue; }
    if (e.namespaceURI === NS.draw) { ctx.skip(); continue; } // shapes, charts
    // Fields (page numbers, dates…) carry their shown text.
    await odfInline(e, out, ctx, fmt);
  }
}

function wrapFmt(n: Node, f: Fmt): Node {
  let node = n;
  for (const [on, tag] of [[f.s, 's'], [f.u, 'u'], [f.i, 'em'], [f.b, 'strong']] as const) {
    if (!on) continue;
    const w = document.createElement(tag); w.appendChild(node); node = w;
  }
  return node;
}

async function odfFrame(frame: Element, out: Node, ctx: OdfCtx) {
  const alt = kids(frame).find((c) => is(c, NS.svg, 'desc') || is(c, NS.svg, 'title'))?.textContent ?? '';
  // A frame may hold the same picture several ways (a newer format and a PNG for older readers):
  // the first one this window can show.
  for (const c of kids(frame)) {
    if (is(c, NS.draw, 'image')) {
      const part = internalPart(at(c, NS.xlink, 'href') || '');
      const url = part && imgType(part) ? await ctx.pics.url(part) : null;
      if (url) { const img = document.createElement('img'); img.src = url; img.alt = alt; out.appendChild(img); return; }
    }
  }
  if (kids(frame).some((c) => is(c, NS.draw, 'text-box'))) {
    // Text boxes: their text, inline, rather than nothing.
    const box = kids(frame).find((c) => is(c, NS.draw, 'text-box'))!;
    const span = document.createElement('span');
    span.textContent = plainText(box);
    if (span.textContent) out.appendChild(span);
    return;
  }
  ctx.skip();
}

type BlockCtx = { list: string; depth: number };

/** Block content (paragraphs, headings, lists, tables, sections) of `parent` into `out`. */
export async function odfBlocks(parent: Element, out: Node, ctx: OdfCtx, b: BlockCtx): Promise<void> {
  for (const e of kids(parent)) {
    if (e.namespaceURI === NS.text) {
      switch (e.localName) {
        case 'h': {
          const lvl = Math.min(6, Math.max(1, Number(at(e, NS.text, 'outline-level') || 1)));
          await para(e, out, ctx, `h${lvl}`);
          continue;
        }
        case 'p': {
          const st = ctx.styles.para.get(at(e, NS.text, 'style-name') || '');
          await para(e, out, ctx, st?.heading ? `h${st.heading}` : 'p');
          continue;
        }
        case 'list': {
          const styleName = at(e, NS.text, 'style-name') || b.list;
          const numbered = ctx.styles.lists.get(styleName)?.[Math.min(9, b.depth)] ?? false;
          const list = document.createElement(numbered ? 'ol' : 'ul');
          for (const item of kids(e)) {
            if (!is(item, NS.text, 'list-item') && !is(item, NS.text, 'list-header')) continue;
            const li = document.createElement('li');
            if (is(item, NS.text, 'list-header')) li.style.listStyle = 'none';
            await odfBlocks(item, li, ctx, { list: styleName, depth: b.depth + 1 });
            // A list item's paragraph shows as the item itself, not a paragraph inside it.
            if (li.childElementCount === 1 && li.firstElementChild!.tagName === 'P') {
              const p = li.firstElementChild as HTMLElement;
              li.replaceChildren(...Array.from(p.childNodes));
              if (p.dir) li.dir = p.dir;
            }
            list.appendChild(li);
          }
          if (list.childElementCount) out.appendChild(list);
          continue;
        }
        case 'section': case 'index-body': case 'table-of-content': case 'alphabetical-index': case 'illustration-index': case 'table-index': case 'bibliography': case 'user-index': case 'object-index':
          await odfBlocks(e, out, ctx, b); continue;
        case 'tracked-changes': case 'sequence-decls': case 'variable-decls': case 'user-field-decls': case 'soft-page-break': case 'index-title-template': continue;
      }
    }
    if (is(e, NS.table, 'table')) { await odfTable(e, out, ctx); continue; }
    if (is(e, NS.draw, 'frame')) { const p = document.createElement('p'); await odfFrame(e, p, ctx); if (p.childNodes.length) out.appendChild(p); continue; }
  }
}

async function para(e: Element, out: Node, ctx: OdfCtx, tag: string) {
  const styleName = at(e, NS.text, 'style-name') || '';
  const st = ctx.styles.para.get(styleName);
  const el = document.createElement(tag);
  el.setAttribute('dir', st?.rtl ? 'rtl' : 'auto');
  if (st?.align && st.align !== (st.rtl ? 'right' : 'left')) el.setAttribute('align', st.align);
  await odfInline(e, el, ctx, tag === 'p' ? ctx.styles.paraFmt(styleName) : {});
  if (tag === 'p' && !el.textContent?.trim() && !el.querySelector('img')) el.appendChild(document.createElement('br'));
  out.appendChild(el);
}

async function odfTable(t: Element, out: Node, ctx: OdfCtx) {
  const tbl = document.createElement('table');
  const thead = document.createElement('thead');
  const tbody = document.createElement('tbody');
  const rowsOf = async (parent: Element, into: HTMLElement, head: boolean) => {
    for (const r of kids(parent)) {
      if (is(r, NS.table, 'table-header-rows')) { await rowsOf(r, thead, true); continue; }
      if (is(r, NS.table, 'table-rows') || is(r, NS.table, 'table-row-group')) { await rowsOf(r, into, head); continue; }
      if (!is(r, NS.table, 'table-row')) continue;
      const repeat = Math.min(50, Math.max(1, Number(at(r, NS.table, 'number-rows-repeated') || 1)));
      for (let k = 0; k < repeat; k++) {
        const tr = document.createElement('tr');
        for (const c of kids(r)) {
          if (!is(c, NS.table, 'table-cell')) continue; // covered cells belong to a merged cell
          const cell = document.createElement(head ? 'th' : 'td');
          if (head) cell.setAttribute('scope', 'col');
          const cs = Number(at(c, NS.table, 'number-columns-spanned') || 1), rs = Number(at(c, NS.table, 'number-rows-spanned') || 1);
          if (cs > 1) cell.colSpan = Math.min(cs, 50);
          if (rs > 1) cell.rowSpan = Math.min(rs, 500);
          await odfBlocks(c, cell, ctx, { list: '', depth: 0 });
          const rep = Math.min(50, Math.max(1, Number(at(c, NS.table, 'number-columns-repeated') || 1)));
          tr.appendChild(cell);
          for (let j = 1; j < rep; j++) tr.appendChild(cell.cloneNode(true));
        }
        into.appendChild(tr);
      }
    }
  };
  await rowsOf(t, tbody, false);
  if (thead.childElementCount) tbl.appendChild(thead);
  tbl.appendChild(tbody);
  out.appendChild(tbl);
}
