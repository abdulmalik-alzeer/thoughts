// A note as a real Word document (.docx), built from the note as rendered (already sanitised): headings,
// paragraphs, bold/italic/underline/strike, code, links to web pages, bullet and numbered lists
// (nested), task lists, quotes, tables, pictures and lines. Arabic paragraphs are written right to
// left (Word's own bidi settings), mixed Arabic/English lines keep each part's direction, and a
// mostly-Arabic note gets right-to-left pages.
import { zip, type ZipEntry } from './zip';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';

/** Strong right-to-left letters (Hebrew, Arabic, Syriac, Thaana, N'Ko, Arabic presentation forms). */
const RTL = /[֐-ࣿיִ-﷿ﹰ-﻿]/;
const LTR = /[A-Za-zÀ-ɏͰ-ϿЀ-ӿ]/;
/** The direction of a piece of text: its first strong letter decides (like `dir="auto"`). */
export function textDir(s: string): 'rtl' | 'ltr' {
  for (const ch of s) { if (RTL.test(ch)) return 'rtl'; if (LTR.test(ch)) return 'ltr'; }
  return 'ltr';
}
/** The direction of a whole document: right to left when most of its letters are. */
export function docDir(s: string): 'rtl' | 'ltr' {
  let r = 0, l = 0;
  for (const ch of s) { if (RTL.test(ch)) r++; else if (LTR.test(ch)) l++; }
  return r > l ? 'rtl' : 'ltr';
}

const xml = (s: string) => s.replace(/[\u0000-\u0008\u000B\u000C\u000E-\u001F￾￿]/g, '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

type Fmt = { b?: boolean; i?: boolean; u?: boolean; s?: boolean; code?: boolean; sup?: boolean; sub?: boolean; mark?: boolean; link?: boolean };
type Pic = { id: number; rid: string; name: string; data: Uint8Array; cx: number; cy: number; alt: string };

class Writer {
  body: string[] = [];
  rels: string[] = [];
  pics: Pic[] = [];
  nums: string[] = [];
  private relN = 10;
  private numN = 2;
  private picN = 0;
  /** Paragraph runs being collected, with the paragraph's own properties. */
  runs: string[] = [];
  text = '';

  rel(type: string, target: string, external = false) {
    const id = `rId${++this.relN}`;
    this.rels.push(`<Relationship Id="${id}" Type="${R}/${type}" Target="${xml(target)}"${external ? ' TargetMode="External"' : ''}/>`);
    return id;
  }
  /** A new numbered list (each starts at its own number). */
  newNumbered(start: number): number {
    const id = this.numN++;
    const overrides = Array.from({ length: 9 }, (_, l) => `<w:lvlOverride w:ilvl="${l}"><w:startOverride w:val="${l === 0 ? start : 1}"/></w:lvlOverride>`).join('');
    this.nums.push(`<w:num w:numId="${id}"><w:abstractNumId w:val="1"/>${overrides}</w:num>`);
    return id;
  }

  run(t: string, f: Fmt) {
    // HTML does not show spaces at the start of a line; neither does the document.
    if (!this.runs.length && !this.text) t = t.replace(/^ +/, '');
    if (!t) return;
    this.text += t;
    // Split into right-to-left and left-to-right stretches; spaces and punctuation stay with the stretch before.
    const parts: { t: string; rtl: boolean }[] = [];
    for (const ch of t) {
      const strong = RTL.test(ch) ? true : LTR.test(ch) || /\d/.test(ch) ? false : null;
      const last = parts[parts.length - 1];
      if (last && (strong === null || strong === last.rtl)) last.t += ch;
      else parts.push({ t: ch, rtl: strong === true });
    }
    for (const p of parts) {
      const pr = [
        f.link ? '<w:rStyle w:val="Hyperlink"/>' : f.code ? '<w:rStyle w:val="CodeChar"/>' : '',
        f.b ? '<w:b/><w:bCs/>' : '', f.i ? '<w:i/><w:iCs/>' : '', f.s ? '<w:strike/>' : '',
        f.mark ? '<w:highlight w:val="yellow"/>' : '', f.u && !f.link ? '<w:u w:val="single"/>' : '',
        f.sup ? '<w:vertAlign w:val="superscript"/>' : f.sub ? '<w:vertAlign w:val="subscript"/>' : '',
        p.rtl ? '<w:rtl/>' : '',
      ].join('');
      const pieces = p.t.split(/(\t|\n)/);
      const inner = pieces.map((x) => (x === '\t' ? '<w:tab/>' : x === '\n' ? '<w:br/>' : x ? `<w:t xml:space="preserve">${xml(x)}</w:t>` : '')).join('');
      this.runs.push(`<w:r>${pr ? `<w:rPr>${pr}</w:rPr>` : ''}${inner}</w:r>`);
    }
  }
  raw(r: string) { this.runs.push(r); }
  hyperlink(rid: string, inner: Writer) {
    if (!inner.runs.length) return;
    this.runs.push(`<w:hyperlink r:id="${rid}" w:history="1">${inner.runs.join('')}</w:hyperlink>`);
    this.text += inner.text;
  }
  hasContent() { return this.runs.length > 0; }

  /** Finish the paragraph being collected. */
  para(o: { style?: string; num?: [number, number]; dir?: 'rtl' | 'ltr'; align?: string; keep?: boolean; force?: boolean } = {}) {
    if (!this.runs.length && !o.force) return;
    const dir = o.dir ?? textDir(this.text);
    // In a right-to-left paragraph Word reads left and right as start and end.
    const jc = o.align === 'center' ? 'center' : o.align === 'justify' ? 'both'
      : o.align === 'right' ? (dir === 'rtl' ? 'left' : 'right') : o.align === 'left' && dir === 'rtl' ? 'right' : '';
    const ppr = [
      o.style ? `<w:pStyle w:val="${o.style}"/>` : '',
      o.keep ? '<w:keepNext/>' : '',
      o.num ? `<w:numPr><w:ilvl w:val="${Math.min(8, o.num[1])}"/><w:numId w:val="${o.num[0]}"/></w:numPr>` : '',
      dir === 'rtl' ? '<w:bidi/>' : '',
      jc ? `<w:jc w:val="${jc}"/>` : '',
    ].join('');
    this.body.push(`<w:p>${ppr ? `<w:pPr>${ppr}</w:pPr>` : ''}${this.runs.join('')}</w:p>`);
    this.runs = []; this.text = '';
  }
  /** Take the collected runs out (for a table cell), leaving the writer empty. */
  take(): string[] { const b = this.body; this.body = []; return b; }
  put(b: string[]) { this.body.push(...b); }

  async picture(img: HTMLImageElement) {
    const src = img.getAttribute('src') || '';
    const alt = img.getAttribute('alt') || '';
    let data: Uint8Array | null = null, ext = '';
    if (src.startsWith('blob:') || src.startsWith('data:')) {
      try {
        const blob = await (await fetch(src)).blob();
        const head = new Uint8Array(await blob.slice(0, 12).arrayBuffer());
        ext = head[0] === 0x89 && head[1] === 0x50 ? 'png' : head[0] === 0xff && head[1] === 0xd8 ? 'jpeg' : head[0] === 0x47 && head[1] === 0x49 ? 'gif' : head[0] === 0x42 && head[1] === 0x4d ? 'bmp' : '';
        if (!ext && blob.size < 30 * 1024 * 1024) {
          // Other picture types (WebP, SVG, AVIF…) are turned into PNG, which Word always shows.
          const png = await toPng(blob);
          if (png) { data = png.data; ext = 'png'; }
        } else if (ext) data = new Uint8Array(await blob.arrayBuffer());
      } catch { data = null; }
    }
    if (!data) { if (alt) this.run(`[${alt}]`, { i: true }); return; }
    const size = await pictureSize(data, ext);
    // At most the width of the page between the margins (6 inches); 96 pixels to the inch.
    const EMU = 9525, MAX = 6 * 914400;
    let cx = Math.round((size.w || 400) * EMU), cy = Math.round((size.h || 300) * EMU);
    if (cx > MAX) { cy = Math.round(cy * (MAX / cx)); cx = MAX; }
    const id = ++this.picN;
    const name = `image${id}.${ext === 'jpeg' ? 'jpg' : ext}`;
    const rid = this.rel('image', `media/${name}`);
    this.pics.push({ id, rid, name, data, cx, cy, alt });
    this.raw(`<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="${cx}" cy="${cy}"/><wp:docPr id="${id}" name="Picture ${id}" descr="${xml(alt)}"/>`
      + `<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">`
      + `<pic:pic><pic:nvPicPr><pic:cNvPr id="${id}" name="${name}" descr="${xml(alt)}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="${rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>`
      + `<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>`);
  }
}

async function toPng(blob: Blob): Promise<{ data: Uint8Array } | null> {
  try {
    const bmp = await createImageBitmap(blob);
    const c = document.createElement('canvas');
    c.width = bmp.width; c.height = bmp.height;
    c.getContext('2d')!.drawImage(bmp, 0, 0);
    const out = await new Promise<Blob | null>((res) => c.toBlob(res, 'image/png'));
    return out ? { data: new Uint8Array(await out.arrayBuffer()) } : null;
  } catch { return null; }
}
async function pictureSize(data: Uint8Array, ext: string): Promise<{ w: number; h: number }> {
  try {
    const bmp = await createImageBitmap(new Blob([data as BlobPart], { type: `image/${ext}` }));
    return { w: bmp.width, h: bmp.height };
  } catch { return { w: 0, h: 0 }; }
}

const BLOCK = new Set(['P', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'UL', 'OL', 'LI', 'BLOCKQUOTE', 'PRE', 'TABLE', 'HR', 'DIV', 'DETAILS', 'SUMMARY', 'DL', 'DT', 'DD', 'FIGURE', 'FIGCAPTION']);

/** `list`: the bullet the next paragraph carries (only a list item's first one does); `depth`: list nesting. */
type Ctx = { style?: string; list?: { num: number; level: number }; depth?: number; align?: string };

function alignOf(el: Element): string | undefined {
  const a = el.getAttribute('align')?.toLowerCase();
  return a === 'center' || a === 'right' || a === 'justify' || a === 'left' ? a : undefined;
}
function dirOf(el: Element): 'rtl' | 'ltr' | undefined {
  const d = el.getAttribute('dir')?.toLowerCase();
  return d === 'rtl' || d === 'ltr' ? d : undefined;
}

async function inline(w: Writer, node: Node, f: Fmt) {
  if (node.nodeType === Node.TEXT_NODE) { w.run((node.textContent ?? '').replace(/\s+/g, ' '), f); return; }
  if (node.nodeType !== Node.ELEMENT_NODE) return;
  const el = node as HTMLElement;
  const tag = el.tagName;
  if (tag === 'BR') { w.run('\n', f); return; }
  if (tag === 'IMG') { await w.picture(el as HTMLImageElement); return; }
  if (el.classList.contains('md-task')) { w.run(/\[x\]/i.test(el.textContent ?? '') ? '☑' : '☐', f); return; }
  if (tag === 'CODE' || tag === 'KBD' || tag === 'SAMP') { w.run(el.textContent ?? '', { ...f, code: true }); return; }
  const g: Fmt = { ...f };
  if (tag === 'B' || tag === 'STRONG' || tag === 'TH') g.b = true;
  if (tag === 'I' || tag === 'EM' || tag === 'CITE' || tag === 'DFN' || tag === 'VAR') g.i = true;
  if (tag === 'U' || tag === 'INS') g.u = true;
  if (tag === 'S' || tag === 'DEL') g.s = true;
  if (tag === 'SUP') g.sup = true;
  if (tag === 'SUB') g.sub = true;
  if (tag === 'MARK') g.mark = true;
  if (tag === 'A') {
    const href = el.getAttribute('href') || '';
    // Only links to the web or e-mail make sense outside the library; a link to another note keeps its words.
    if (/^(https?:|mailto:)/i.test(href)) {
      const inner = new Writer();
      inner.text = w.text || ' '; // keeps the inner runs from dropping a leading space mid-line
      const before = inner.text.length;
      for (const c of Array.from(el.childNodes)) await inline(inner, c, { ...g, link: true });
      inner.text = inner.text.slice(before);
      w.hyperlink(w.rel('hyperlink', href, true), inner);
      return;
    }
  }
  for (const c of Array.from(el.childNodes)) await inline(w, c, g);
}

async function blocks(w: Writer, parent: Element, ctx: Ctx) {
  // Loose inline content between blocks (a list item's own text) becomes its own paragraph.
  const flush = (el?: Element) => {
    if (!w.hasContent()) return;
    w.para({ style: ctx.style, num: ctx.list ? [ctx.list.num, ctx.list.level] : undefined, align: ctx.align, dir: el ? dirOf(el) : undefined });
    if (ctx.list) ctx = { ...ctx, list: undefined, style: 'ListContinue' };
  };
  for (const node of Array.from(parent.childNodes)) {
    const el = node.nodeType === Node.ELEMENT_NODE ? (node as HTMLElement) : null;
    if (!el || !BLOCK.has(el.tagName)) {
      if (node.nodeType === Node.TEXT_NODE && !w.hasContent() && !(node.textContent ?? '').trim()) continue;
      await inline(w, node, {});
      continue;
    }
    flush(parent);
    const tag = el.tagName;
    if (/^H[1-6]$/.test(tag)) {
      for (const c of Array.from(el.childNodes)) await inline(w, c, {});
      w.para({ style: `Heading${tag[1]}`, keep: true, dir: dirOf(el), align: alignOf(el) });
    } else if (tag === 'P' || tag === 'DT' || tag === 'SUMMARY' || tag === 'FIGCAPTION') {
      for (const c of Array.from(el.childNodes)) await inline(w, c, tag === 'DT' || tag === 'SUMMARY' ? { b: true } : {});
      w.para({ style: ctx.style, num: ctx.list ? [ctx.list.num, ctx.list.level] : undefined, dir: dirOf(el), align: alignOf(el) ?? ctx.align });
      // Only the first paragraph of a list item carries its bullet.
      if (ctx.list) ctx = { ...ctx, list: undefined, style: 'ListContinue' };
    } else if (tag === 'UL' || tag === 'OL') {
      const level = Math.min(8, ctx.depth ?? 0);
      const num = tag === 'OL' ? w.newNumbered(Number(el.getAttribute('start')) || 1) : 1;
      for (const li of Array.from(el.children)) {
        if (li.tagName !== 'LI') continue;
        await blocks(w, li, { list: { num, level }, depth: level + 1, align: ctx.align });
      }
    } else if (tag === 'LI') {
      await blocks(w, el, ctx);
    } else if (tag === 'BLOCKQUOTE' || tag === 'DD') {
      await blocks(w, el, { style: 'Quote', align: ctx.align, depth: ctx.depth });
    } else if (tag === 'PRE') {
      const lines = (el.textContent ?? '').replace(/\n$/, '').split('\n');
      for (const line of lines) { w.run(line, {}); w.para({ style: 'CodeBlock', dir: 'ltr', force: true }); }
    } else if (tag === 'HR') {
      w.para({ style: 'Rule', force: true });
    } else if (tag === 'TABLE') {
      await table(w, el as HTMLTableElement);
    } else {
      // DIV (code and table wrappers), DETAILS, FIGURE, DL: their content.
      await blocks(w, el, { ...ctx, align: alignOf(el) ?? ctx.align });
    }
  }
  flush(parent);
}

async function table(w: Writer, t: HTMLTableElement) {
  const rows = Array.from(t.rows);
  if (!rows.length) return;
  const cols = Math.max(...rows.map((r) => Array.from(r.cells).reduce((n, c) => n + (c.colSpan || 1), 0)));
  const rtl = textDir(t.textContent ?? '') === 'rtl';
  const before = w.take();
  const out: string[] = [];
  const width = Math.floor(9000 / Math.max(1, cols));
  out.push(`<w:tbl><w:tblPr><w:tblStyle w:val="Table"/>${rtl ? '<w:bidiVisual/>' : ''}<w:tblW w:w="0" w:type="auto"/><w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="0" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr>`);
  out.push(`<w:tblGrid>${Array.from({ length: cols }, () => `<w:gridCol w:w="${width}"/>`).join('')}</w:tblGrid>`);
  for (const r of rows) {
    const head = r.parentElement?.tagName === 'THEAD' || Array.from(r.cells).every((c) => c.tagName === 'TH');
    out.push(`<w:tr>${head ? '<w:trPr><w:tblHeader/></w:trPr>' : ''}`);
    for (const c of Array.from(r.cells)) {
      for (const n of Array.from(c.childNodes)) await inline(w, n, head ? { b: true } : {});
      w.para({ align: alignOf(c), force: true, dir: dirOf(c) });
      const span = c.colSpan > 1 ? `<w:gridSpan w:val="${c.colSpan}"/>` : '';
      out.push(`<w:tc><w:tcPr><w:tcW w:w="${width * (c.colSpan || 1)}" w:type="dxa"/>${span}</w:tcPr>${w.take().join('')}</w:tc>`);
    }
    out.push('</w:tr>');
  }
  out.push('</w:tbl>');
  w.put(before);
  w.put(out);
  // Word needs a paragraph between a table and whatever follows.
  w.para({ force: true });
}

function stylesXml(rtl: boolean, lang: string) {
  const heading = (n: number, size: number) => `<w:style w:type="paragraph" w:styleId="Heading${n}"><w:name w:val="heading ${n}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>`
    + `<w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="${n <= 2 ? 360 : 240}" w:after="120"/><w:outlineLvl w:val="${n - 1}"/></w:pPr><w:rPr><w:b/><w:bCs/><w:color w:val="1F2937"/><w:sz w:val="${size}"/><w:szCs w:val="${size}"/></w:rPr></w:style>`;
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="${W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Arial"/><w:sz w:val="22"/><w:szCs w:val="24"/><w:lang w:val="${lang === 'ar' ? 'ar-SA' : 'en-US'}" w:eastAsia="en-US" w:bidi="ar-SA"/></w:rPr></w:rPrDefault>`
    + `<w:pPrDefault><w:pPr>${rtl ? '<w:bidi/>' : ''}<w:spacing w:after="160" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>`
    + `<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>`
    + heading(1, 36) + heading(2, 30) + heading(3, 26) + heading(4, 24) + heading(5, 22) + heading(6, 22)
    + `<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:pBdr><w:left w:val="single" w:sz="18" w:space="8" w:color="BBBBBB"/></w:pBdr><w:ind w:left="360"/></w:pPr><w:rPr><w:color w:val="444444"/></w:rPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="ListContinue"><w:name w:val="List Continue"/><w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="720"/></w:pPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="CodeBlock"><w:name w:val="Code Block"/><w:basedOn w:val="Normal"/><w:pPr><w:shd w:val="clear" w:color="auto" w:fill="F4F4F4"/><w:bidi w:val="0"/><w:spacing w:after="0" w:line="240" w:lineRule="auto"/><w:jc w:val="left"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="19"/><w:szCs w:val="19"/></w:rPr></w:style>`
    + `<w:style w:type="paragraph" w:styleId="Rule"><w:name w:val="Horizontal Line"/><w:basedOn w:val="Normal"/><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="999999"/></w:pBdr></w:pPr></w:style>`
    + `<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style>`
    + `<w:style w:type="character" w:styleId="CodeChar"><w:name w:val="Code"/><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="20"/><w:shd w:val="clear" w:color="auto" w:fill="F0F0F0"/></w:rPr></w:style>`
    + `<w:style w:type="table" w:styleId="Table"><w:name w:val="Table Grid"/><w:pPr><w:spacing w:after="0"/></w:pPr><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:color="999999"/><w:left w:val="single" w:sz="4" w:color="999999"/><w:bottom w:val="single" w:sz="4" w:color="999999"/><w:right w:val="single" w:sz="4" w:color="999999"/><w:insideH w:val="single" w:sz="4" w:color="999999"/><w:insideV w:val="single" w:sz="4" w:color="999999"/></w:tblBorders><w:tblCellMar><w:top w:w="60" w:type="dxa"/><w:left w:w="100" w:type="dxa"/><w:bottom w:w="60" w:type="dxa"/><w:right w:w="100" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>`
    + `</w:styles>`;
}

function numberingXml(nums: string[]) {
  const bullets = ['•', '◦', '▪'];
  const lvls = (fmt: (l: number) => string) => Array.from({ length: 9 }, (_, l) => fmt(l)).join('');
  const ind = (l: number) => `<w:pPr><w:ind w:left="${720 * (l + 1)}" w:hanging="360"/></w:pPr>`;
  const bullet = lvls((l) => `<w:lvl w:ilvl="${l}"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="${bullets[l % 3]}"/><w:lvlJc w:val="left"/>${ind(l)}</w:lvl>`);
  const fmts = ['decimal', 'lowerLetter', 'lowerRoman'];
  const decimal = lvls((l) => `<w:lvl w:ilvl="${l}"><w:start w:val="1"/><w:numFmt w:val="${fmts[l % 3]}"/><w:lvlText w:val="%${l + 1}."/><w:lvlJc w:val="left"/>${ind(l)}</w:lvl>`);
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="${W}"><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/>${bullet}</w:abstractNum><w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>${decimal}</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>${nums.join('')}</w:numbering>`;
}

/** The rendered note `root` as a .docx file. */
export async function toDocx(root: HTMLElement, o: { title: string; lang: string }): Promise<Blob> {
  const w = new Writer();
  const rtl = docDir(root.textContent ?? '') === 'rtl';
  await blocks(w, root, {});
  if (!w.body.length) w.para({ force: true });
  const stamp = new Date().toISOString().replace(/\.\d+Z$/, 'Z');
  const page = `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/>${rtl ? '<w:bidi/>' : ''}</w:sectPr>`;
  const document = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="${W}" xmlns:r="${R}" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body>${w.body.join('')}${page}</w:body></w:document>`;
  const types = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpg" ContentType="image/jpeg"/><Default Extension="gif" ContentType="image/gif"/><Default Extension="bmp" ContentType="image/bmp"/>`
    + `<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>`;
  const rootRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${R}/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="${R}/extended-properties" Target="docProps/app.xml"/></Relationships>`;
  const docRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${R}/styles" Target="styles.xml"/><Relationship Id="rId2" Type="${R}/numbering" Target="numbering.xml"/><Relationship Id="rId3" Type="${R}/settings" Target="settings.xml"/>${w.rels.join('')}</Relationships>`;
  const settings = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="${W}"><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>`;
  const core = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>${xml(o.title)}</dc:title><dc:language>${o.lang === 'ar' ? 'ar-SA' : 'en-US'}</dc:language><dcterms:created xsi:type="dcterms:W3CDTF">${stamp}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">${stamp}</dcterms:modified></cp:coreProperties>`;
  const app = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Thoughts</Application></Properties>`;
  const files: ZipEntry[] = [
    { name: '[Content_Types].xml', data: types }, { name: '_rels/.rels', data: rootRels },
    { name: 'word/document.xml', data: document }, { name: 'word/_rels/document.xml.rels', data: docRels },
    { name: 'word/styles.xml', data: stylesXml(rtl, o.lang) }, { name: 'word/numbering.xml', data: numberingXml(w.nums) },
    { name: 'word/settings.xml', data: settings }, { name: 'docProps/core.xml', data: core }, { name: 'docProps/app.xml', data: app },
    ...w.pics.map((p) => ({ name: `word/media/${p.name}`, data: p.data })),
  ];
  const blob = await zip(files);
  return new Blob([blob], { type: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' });
}
