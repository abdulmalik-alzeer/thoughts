// A Word document (.docx) shown inside the library: its text with headings, bold/italic/underline,
// lists, tables (with merged cells), links to web pages, pictures, alignment and right-to-left
// paragraphs (set on the paragraph or by its style). The page is
// built with DOM calls only (never from the file's markup), so nothing in the file can run or style
// the app. Not shown: headers and footers, footnotes, comments, text boxes, charts and page layout.
import { OfficeError, OfficeZip, resolvePart } from '../preview/office/safezip';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const A = 'http://schemas.openxmlformats.org/drawingml/2006/main';
const VML = 'urn:schemas-microsoft-com:vml';

/** `cut`: tables nested deeper than MAX_TABLE_DEPTH were left out. */
export type DocxView = { node: DocumentFragment; urls: string[]; skipped: boolean; cut: boolean };

const MAX_TABLE_DEPTH = 6;

const kids = (el: Element) => Array.from(el.children);
const wAttr = (el: Element | null | undefined, name: string) => el?.getAttributeNS(W, name) ?? el?.getAttribute(`w:${name}`) ?? null;
const first = (el: Element, local: string, ns = W) => kids(el).find((c) => c.localName === local && c.namespaceURI === ns) ?? null;
/** `<w:b/>`, `<w:b w:val="true"/>` are on; `w:val="0|false|none"` is off. */
const on = (el: Element | null) => !!el && !/^(0|false|off|none)$/i.test(wAttr(el, 'val') ?? '');

function parse(text: string): Document {
  const d = new DOMParser().parseFromString(text, 'application/xml');
  if (d.getElementsByTagName('parsererror').length) throw new OfficeError('damaged', 'unreadable');
  return d;
}

const IMG_TYPE: Record<string, string> = { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', bmp: 'image/bmp', webp: 'image/webp', tif: 'image/tiff', tiff: 'image/tiff' };

export async function docxToDom(bytes: Uint8Array): Promise<DocxView> {
  // Parts are unpacked within limits, so a file built to unpack to gigabytes is refused.
  const z = new OfficeZip(bytes);
  const docText = await z.text('word/document.xml');
  if (!docText) throw new OfficeError('wrongKind', 'not a word document');
  const doc = parse(docText);
  const rels = new Map<string, { target: string; external: boolean }>();
  const relText = await z.text('word/_rels/document.xml.rels');
  if (relText) for (const r of Array.from(parse(relText).getElementsByTagName('Relationship'))) {
    const external = r.getAttribute('TargetMode') === 'External';
    const target = r.getAttribute('Target') || '';
    rels.set(r.getAttribute('Id') || '', { target: external ? target : resolvePart('word/document.xml', target), external });
  }
  // Paragraph styles: which are headings, the title, quotes, lists.
  const styleKind = new Map<string, string>();
  // Paragraph styles (and the document's defaults) that set right-to-left: an Arabic document often
  // says it once there rather than on every paragraph.
  const styleRtl = new Map<string, boolean>();
  const styleBase = new Map<string, string>();
  let defaultRtl = false;
  let defaultStyle = '';
  const stylesText = await z.text('word/styles.xml');
  const stylesDoc = stylesText ? parse(stylesText) : null;
  if (stylesDoc) {
    const dpp = stylesDoc.getElementsByTagNameNS(W, 'pPrDefault')[0];
    const bidi = dpp?.getElementsByTagNameNS(W, 'bidi')[0];
    if (bidi && on(bidi)) defaultRtl = true;
  }
  if (stylesDoc) for (const s of Array.from(stylesDoc.getElementsByTagNameNS(W, 'style'))) {
    const id = wAttr(s, 'styleId') || '';
    if (wAttr(s, 'type') === 'paragraph') {
      const b = first(first(s, 'pPr') ?? s, 'bidi');
      if (b) styleRtl.set(id, on(b));
      const based = wAttr(first(s, 'basedOn'), 'val');
      if (based) styleBase.set(id, based);
      if (/^(1|true)$/.test(wAttr(s, 'default') ?? '')) defaultStyle = id;
    }
    const name = (wAttr(first(s, 'name'), 'val') || id).toLowerCase();
    const lvl = wAttr(first(first(s, 'pPr') ?? s, 'outlineLvl'), 'val');
    const m = /^heading\s*(\d)/.exec(name) || /^heading(\d)$/i.exec(id);
    if (m) styleKind.set(id, `h${Math.min(6, +m[1])}`);
    else if (name === 'title') styleKind.set(id, 'h1');
    else if (name === 'subtitle') styleKind.set(id, 'h2');
    else if (/quote/.test(name)) styleKind.set(id, 'quote');
    else if (/code|preformatted|source/.test(name)) styleKind.set(id, 'code');
    else if (lvl !== null && +lvl < 6) styleKind.set(id, `h${+lvl + 1}`);
  }
  /** Does a paragraph style (or one it is based on) say right to left? */
  const rtlOfStyle = (id: string): boolean => {
    for (let k = 0, cur = id || defaultStyle; cur && k < 20; k++, cur = styleBase.get(cur) ?? '') {
      const v = styleRtl.get(cur);
      if (v !== undefined) return v;
    }
    return defaultRtl;
  };
  // Numbering: which lists are numbered and which are bullets.
  const numFmt = new Map<string, (lvl: number) => string>();
  const numText = await z.text('word/numbering.xml');
  if (numText) {
    const n = parse(numText);
    const abs = new Map<string, Element>();
    for (const a of Array.from(n.getElementsByTagNameNS(W, 'abstractNum'))) abs.set(wAttr(a, 'abstractNumId') || '', a);
    for (const num of Array.from(n.getElementsByTagNameNS(W, 'num'))) {
      const a = abs.get(wAttr(first(num, 'abstractNumId'), 'val') || '');
      numFmt.set(wAttr(num, 'numId') || '', (l) => {
        const lv = a && kids(a).find((x) => x.localName === 'lvl' && wAttr(x, 'ilvl') === String(l));
        return wAttr(lv && first(lv, 'numFmt'), 'val') || 'bullet';
      });
    }
  }

  const urls: string[] = [];
  const pictures = new Map<string, string>();
  let skipped = false;
  let cut = false;
  let tableDepth = 0;
  async function picture(rid: string, alt: string): Promise<Node> {
    const rel = rels.get(rid);
    const ext = rel?.target.split('.').pop()?.toLowerCase() ?? '';
    if (rel && !rel.external && IMG_TYPE[ext]) {
      let url = pictures.get(rid);
      if (!url) {
        const data = await z.bytes(rel.target).catch((e) => { if (e instanceof OfficeError && e.problem === 'bomb') throw e; return null; });
        if (data) { url = URL.createObjectURL(new Blob([data as BlobPart], { type: IMG_TYPE[ext] })); urls.push(url); pictures.set(rid, url); }
      }
      if (url) { const img = document.createElement('img'); img.src = url; img.alt = alt; return img; }
    }
    skipped = true;
    return document.createTextNode(alt ? `[${alt}]` : '');
  }

  async function runs(parent: Element, out: Node) {
    // Field codes: `HYPERLINK "url"` between begin and separate makes the shown runs a link; the
    // code itself is never shown.
    let field: { instr: string; shown: boolean; link: HTMLAnchorElement | null } | null = null;
    const target = () => field?.link ?? out;
    for (const c of kids(parent)) {
      const tag = c.namespaceURI === W ? c.localName : '';
      if (tag === 'r') {
        const fc = wAttr(first(c, 'fldChar'), 'fldCharType');
        if (fc === 'begin') { field = { instr: '', shown: false, link: null }; continue; }
        if (fc === 'separate' && field) {
          field.shown = true;
          const m = /HYPERLINK\s+"([^"]+)"/i.exec(field.instr);
          if (m && /^(https?:|mailto:)/i.test(m[1])) { field.link = document.createElement('a'); field.link.href = m[1]; out.appendChild(field.link); }
          continue;
        }
        if (fc === 'end') { field = null; continue; }
        if (field && !field.shown) { field.instr += kids(c).filter((x) => x.localName === 'instrText').map((x) => x.textContent ?? '').join(''); continue; }
        await run(c, target());
      }
      else if (tag === 'hyperlink') {
        const rel = rels.get(c.getAttributeNS(R, 'id') ?? '');
        const href = rel?.external ? rel.target : '';
        if (/^(https?:|mailto:)/i.test(href)) {
          const a = document.createElement('a');
          a.href = href;
          await runs(c, a);
          target().appendChild(a);
        } else await runs(c, target());
      }
      // Tracked insertions, smart tags, fields and content controls hold ordinary runs.
      else if (['ins', 'smartTag', 'fldSimple', 'customXml', 'bdo', 'dir'].includes(tag)) await runs(c, target());
      else if (tag === 'sdt') { const content = first(c, 'sdtContent'); if (content) await runs(content, target()); }
    }
  }

  async function run(r: Element, out: Node) {
    const pr = first(r, 'rPr');
    let host: Node = out;
    const wrap = (name: string) => { const e = document.createElement(name); host.appendChild(e); host = e; };
    if (pr) {
      if (on(first(pr, 'b')) || on(first(pr, 'bCs'))) wrap('strong');
      if (on(first(pr, 'i')) || on(first(pr, 'iCs'))) wrap('em');
      const u = first(pr, 'u');
      if (u && !/^none$/i.test(wAttr(u, 'val') ?? '')) wrap('u');
      if (on(first(pr, 'strike')) || on(first(pr, 'dstrike'))) wrap('s');
      const va = wAttr(first(pr, 'vertAlign'), 'val');
      if (va === 'superscript') wrap('sup'); else if (va === 'subscript') wrap('sub');
      const hl = first(pr, 'highlight');
      if (hl && wAttr(hl, 'val') !== 'none') wrap('mark');
      if (on(first(pr, 'vanish'))) return; // hidden text stays hidden, as in Word
    }
    for (const c of kids(r)) {
      if (c.namespaceURI !== W) continue;
      switch (c.localName) {
        case 't': host.appendChild(document.createTextNode(c.textContent ?? '')); break;
        case 'tab': host.appendChild(document.createTextNode('\t')); break;
        case 'br': case 'cr': host.appendChild(document.createElement('br')); break;
        case 'noBreakHyphen': host.appendChild(document.createTextNode('‑')); break;
        case 'sym': host.appendChild(document.createTextNode(String.fromCharCode(parseInt(wAttr(c, 'char') || '0', 16) & 0xffff || 0x20))); break;
        case 'drawing': case 'pict': case 'object': {
          const blips = [...Array.from(c.getElementsByTagNameNS(A, 'blip')), ...Array.from(c.getElementsByTagNameNS(VML, 'imagedata'))];
          const alt = c.querySelector('docPr')?.getAttribute('descr') || c.querySelector('docPr')?.getAttribute('title') || '';
          if (!blips.length) { skipped = true; break; }
          for (const b of blips) host.appendChild(await picture(b.getAttributeNS(R, 'embed') || b.getAttributeNS(R, 'id') || '', alt));
          break;
        }
        case 'footnoteReference': case 'endnoteReference': skipped = true; break;
      }
    }
  }

  const frag = document.createDocumentFragment();
  async function paragraph(p: Element, out: Node, lists: { stack: { id: string; lvl: number; el: HTMLElement }[] }) {
    const pr = first(p, 'pPr');
    const style = wAttr(pr && first(pr, 'pStyle'), 'val') || '';
    const kind = styleKind.get(style) ?? '';
    const numPr = pr && first(pr, 'numPr');
    const numId = wAttr(numPr && first(numPr, 'numId'), 'val');
    const lvl = Math.min(8, Number(wAttr(numPr && first(numPr, 'ilvl'), 'val') || 0));
    const bidi = pr && first(pr, 'bidi');
    const rtl = bidi ? on(bidi) : rtlOfStyle(style);
    const jc = wAttr(pr && first(pr, 'jc'), 'val') || '';
    let el: HTMLElement;
    if (numId && numId !== '0' && !kind.startsWith('h')) {
      // Consecutive list paragraphs become one list, nested by level.
      const ordered = numFmt.get(numId)?.(lvl) !== 'bullet';
      const s = lists.stack;
      while (s.length && (s[s.length - 1].lvl > lvl || (s[s.length - 1].lvl === lvl && s[s.length - 1].id !== numId))) s.pop();
      if (!s.length || s[s.length - 1].lvl < lvl) {
        const list = document.createElement(ordered ? 'ol' : 'ul');
        if (rtl) list.setAttribute('dir', 'rtl');
        const parentLi = s.length ? s[s.length - 1].el.lastElementChild : null;
        (parentLi ?? out).appendChild(list);
        s.push({ id: numId, lvl, el: list });
      }
      el = document.createElement('li');
      s[s.length - 1].el.appendChild(el);
    } else if (kind === 'code') {
      // Consecutive code lines are one block, as they were written.
      lists.stack = [];
      const last = (out as Element).lastElementChild ?? (out as DocumentFragment).lastElementChild;
      const pre = last?.tagName === 'PRE' ? (last as HTMLElement) : out.appendChild(document.createElement('pre'));
      if (pre.childNodes.length) pre.appendChild(document.createTextNode('\n'));
      const line = document.createElement('span');
      await runs(p, line);
      pre.appendChild(document.createTextNode(line.textContent ?? ''));
      return;
    } else {
      lists.stack = [];
      el = document.createElement(kind.startsWith('h') ? kind : kind === 'quote' ? 'blockquote' : 'p');
      out.appendChild(el);
    }
    el.setAttribute('dir', rtl ? 'rtl' : 'auto');
    // In a right-to-left paragraph Word's left and right mean start and end, so "right" is the far side.
    const align = jc === 'center' ? 'center' : jc === 'both' || jc === 'distribute' ? 'justify' : jc === 'right' || jc === 'end' ? (rtl ? 'left' : 'right') : '';
    if (align) el.setAttribute('align', align);
    await runs(p, el);
    if (!el.textContent?.trim() && !el.querySelector('img') && el.tagName === 'P') el.appendChild(document.createElement('br'));
  }

  async function body(parent: Element, out: Node) {
    const lists = { stack: [] as { id: string; lvl: number; el: HTMLElement }[] };
    for (const c of kids(parent)) {
      if (c.namespaceURI !== W) continue;
      if (c.localName === 'p') await paragraph(c, out, lists);
      else if (c.localName === 'tbl') { lists.stack = []; await table(c, out); }
      else if (c.localName === 'sdt') { const content = first(c, 'sdtContent'); if (content) await body(content, out); }
      else if (c.localName === 'customXml') await body(c, out);
    }
  }

  async function table(t: Element, out: Node) {
    if (tableDepth >= MAX_TABLE_DEPTH) { cut = true; return; }
    tableDepth++;
    try { await tableAt(t, out); } finally { tableDepth--; }
  }
  async function tableAt(t: Element, out: Node) {
    const tbl = document.createElement('table');
    const pr = first(t, 'tblPr');
    if (pr && on(first(pr, 'bidiVisual'))) tbl.setAttribute('dir', 'rtl');
    const thead = document.createElement('thead');
    const tb = document.createElement('tbody');
    // Cells merged downwards: the first one gets a row span, the rest are left out. `above` holds,
    // per grid column, the cell that a "continue" cell below extends.
    const above = new Map<number, HTMLTableCellElement>();
    let bodyStarted = false;
    for (const tr of kids(t).filter((x) => x.localName === 'tr' && x.namespaceURI === W)) {
      const row = document.createElement('tr');
      // Header rows (repeated on each page in Word) come first; one after body rows is just a row.
      const head = !bodyStarted && !!first(first(tr, 'trPr') ?? tr, 'tblHeader');
      if (!head) bodyStarted = true;
      const trPr = first(tr, 'trPr');
      let col = Number(wAttr(trPr && first(trPr, 'gridBefore'), 'val') || 0);
      for (const tc of kids(tr).filter((x) => x.localName === 'tc' && x.namespaceURI === W)) {
        const tcPr = first(tc, 'tcPr');
        const span = Math.max(1, Math.min(50, Number(wAttr(tcPr && first(tcPr, 'gridSpan'), 'val') || 1)));
        const merge = tcPr && first(tcPr, 'vMerge');
        if (merge && wAttr(merge, 'val') !== 'restart') {
          const top = above.get(col);
          if (top) top.rowSpan = (top.rowSpan || 1) + 1;
          col += span;
          continue; // continuation of a merged cell above
        }
        const cell = document.createElement(head ? 'th' : 'td');
        if (head) cell.setAttribute('scope', 'col');
        if (span > 1) cell.colSpan = span;
        await body(tc, cell);
        row.appendChild(cell);
        for (let k = 0; k < span; k++) above.delete(col + k);
        if (merge) above.set(col, cell);
        col += span;
      }
      (head ? thead : tb).appendChild(row);
    }
    if (thead.childElementCount) tbl.appendChild(thead);
    tbl.appendChild(tb);
    out.appendChild(tbl);
  }

  const b = doc.getElementsByTagNameNS(W, 'body')[0];
  if (!b) throw new OfficeError('wrongKind', 'not a word document');
  try { await body(b, frag); }
  catch (e) { for (const u of urls) URL.revokeObjectURL(u); throw e; }
  if (z.has('word/footnotes.xml') && doc.getElementsByTagNameNS(W, 'footnoteReference').length) skipped = true;
  if (doc.getElementsByTagNameNS(W, 'txbxContent').length) skipped = true;
  return { node: frag, urls, skipped, cut };
}
