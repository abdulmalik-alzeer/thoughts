// Presentations (PowerPoint .pptx, OpenDocument .odp) read slide by slide into what each slide says:
// its title, its text with bullets and levels, its tables, its own pictures and the speaker notes.
// The slide's exact layout is not kept: a readable outline is the aim. XML is parsed with DOMParser
// and only read; pictures pointing outside the file are never loaded.
import { OfficeError, OfficeZip, Pictures, parseXml, relsOf } from './safezip';
import { NS as ODF, OdfStyles, at, internalPart, is, kids, plainText, type Fmt } from './odf';
import { imgType } from './safezip';

const P = 'http://schemas.openxmlformats.org/presentationml/2006/main';
const A = 'http://schemas.openxmlformats.org/drawingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006';

export type Run = Fmt & { t: string; href?: string };
export type Para = { runs: Run[]; level: number; list: 'none' | 'bullet' | 'number'; rtl?: boolean; align?: 'center' | 'right' | 'justify' };
export type Block =
  | { kind: 'text'; role: 'title' | 'subtitle' | 'body'; paras: Para[] }
  | { kind: 'image'; src: string; alt: string }
  | { kind: 'table'; rows: string[][] };
export type Slide = { title: string; blocks: Block[]; notes: string; hidden: boolean; skipped: boolean };
export type Deck = { slides: Slide[]; urls: string[] };

/** Slides beyond this many are not read (a deck that long is not meant to be read here). */
export const MAX_SLIDES = 500;
/** Shapes grouped inside groups deeper than this are left out. */
const MAX_DEPTH = 16;

const SAFE_LINK = /^(https?:|mailto:)/i;
const child = (el: Element | null | undefined, ns: string, name: string) => (el ? kids(el).find((c) => is(c, ns, name)) ?? null : null);
const paraText = (p: Para) => p.runs.map((r) => r.t).join('');

/** One DrawingML text body (`a:txBody`) → paragraphs. `bulleted`: a body placeholder, whose
 *  paragraphs carry the layout's bullets unless they say otherwise. */
export function drawingParas(body: Element, bulleted: boolean, links: Map<string, { target: string; external: boolean }> = new Map()): Para[] {
  const out: Para[] = [];
  for (const p of kids(body).filter((c) => is(c, A, 'p'))) {
    const pr = child(p, A, 'pPr');
    const level = Math.min(8, Number(pr?.getAttribute('lvl') || 0));
    let list: Para['list'] = bulleted ? 'bullet' : 'none';
    if (child(pr, A, 'buNone')) list = 'none';
    else if (child(pr, A, 'buAutoNum')) list = 'number';
    else if (child(pr, A, 'buChar') || child(pr, A, 'buBlip')) list = 'bullet';
    const algn = pr?.getAttribute('algn');
    const rtl = pr?.getAttribute('rtl') === '1';
    const runs: Run[] = [];
    for (const r of kids(p)) {
      if (is(r, A, 'br')) { runs.push({ t: '\n' }); continue; }
      if (!is(r, A, 'r') && !is(r, A, 'fld')) continue;
      const rp = child(r, A, 'rPr');
      const t = child(r, A, 't')?.textContent ?? '';
      if (!t) continue;
      const run: Run = { t };
      if (rp?.getAttribute('b') === '1') run.b = true;
      if (rp?.getAttribute('i') === '1') run.i = true;
      const u = rp?.getAttribute('u'); if (u && u !== 'none') run.u = true;
      const st = rp?.getAttribute('strike'); if (st && st !== 'noStrike') run.s = true;
      const link = links.get(child(rp, A, 'hlinkClick')?.getAttributeNS(R, 'id') || '');
      if (link?.external && SAFE_LINK.test(link.target)) run.href = link.target;
      runs.push(run);
    }
    const para: Para = { runs, level, list, rtl: rtl || undefined, align: algn === 'ctr' ? 'center' : algn === 'r' ? 'right' : algn === 'just' ? 'justify' : undefined };
    if (!paraText(para).trim()) continue; // empty lines between bullets
    out.push(para);
  }
  return out;
}

/** A PowerPoint file's slides. Picture object URLs are in `urls`; the caller revokes them. */
export async function readPptx(bytes: Uint8Array): Promise<Deck> {
  const z = new OfficeZip(bytes);
  const rootRels = await relsOf(z, '').catch(() => new Map<string, { target: string; external: boolean; type: string }>());
  const presPart = [...rootRels.values()].find((r) => r.type === 'officeDocument')?.target ?? 'ppt/presentation.xml';
  const presText = await z.text(presPart);
  if (!presText) throw new OfficeError('wrongKind', 'not a presentation');
  const pres = parseXml(presText);
  if (pres.documentElement.namespaceURI !== P) throw new OfficeError('wrongKind', 'not a presentation');
  const rels = await relsOf(z, presPart);
  const pics = new Pictures(z);
  try {
    const slides: Slide[] = [];
    const ids = Array.from(pres.getElementsByTagNameNS(P, 'sldId')).slice(0, MAX_SLIDES);
    for (const id of ids) {
      const part = rels.get(id.getAttributeNS(R, 'id') || '');
      if (!part || part.external) continue;
      const text = await z.text(part.target);
      if (!text) continue;
      slides.push(await pptxSlide(z, part.target, parseXml(text), pics));
    }
    return { slides, urls: pics.urls };
  } catch (e) {
    for (const u of pics.urls) URL.revokeObjectURL(u);
    throw e;
  }
}

async function pptxSlide(z: OfficeZip, part: string, doc: Document, pics: Pictures): Promise<Slide> {
  const links = await relsOf(z, part);
  const slide: Slide = { title: '', blocks: [], notes: '', hidden: doc.documentElement.getAttribute('show') === '0', skipped: false };
  const titles: Block[] = [];
  const walk = async (parent: Element, depth = 0) => {
    // Groups inside groups: past MAX_DEPTH the rest is left out (and the slide says so).
    if (depth > MAX_DEPTH) { slide.skipped = true; return; }
    for (const el of kids(parent)) {
      if (is(el, MC, 'AlternateContent')) {
        const choice = child(el, MC, 'Choice') ?? child(el, MC, 'Fallback');
        if (choice) await walk(choice, depth + 1);
        continue;
      }
      if (el.namespaceURI !== P) continue;
      if (el.localName === 'grpSp') { await walk(el, depth + 1); continue; }
      if (el.localName === 'sp') {
        const ph = child(child(child(el, P, 'nvSpPr'), P, 'nvPr'), P, 'ph');
        const type = ph ? ph.getAttribute('type') || 'body' : '';
        // The slide number, date and footer repeat on every slide: left out.
        if (type === 'sldNum' || type === 'dt' || type === 'ftr' || type === 'hdr') continue;
        const body = child(el, P, 'txBody');
        if (!body) continue;
        const role = type === 'title' || type === 'ctrTitle' ? 'title' : type === 'subTitle' ? 'subtitle' : 'body';
        const paras = drawingParas(body, role === 'body' && !!ph && (type === 'body' || type === 'obj'), links);
        if (!paras.length) continue;
        const block: Block = { kind: 'text', role, paras };
        if (role === 'title') titles.push(block); else slide.blocks.push(block);
        continue;
      }
      if (el.localName === 'pic') {
        const blip = el.getElementsByTagNameNS(A, 'blip')[0];
        const rel = links.get(blip?.getAttributeNS(R, 'embed') || '');
        const cnv = child(child(el, P, 'nvPicPr'), P, 'cNvPr');
        const alt = cnv?.getAttribute('descr') || cnv?.getAttribute('title') || '';
        const url = rel && !rel.external && imgType(rel.target) ? await pics.url(rel.target) : null;
        if (url) slide.blocks.push({ kind: 'image', src: url, alt });
        else slide.skipped = true; // a linked picture, or a kind this window can't show
        continue;
      }
      if (el.localName === 'graphicFrame') {
        const tbl = el.getElementsByTagNameNS(A, 'tbl')[0];
        if (!tbl) { slide.skipped = true; continue; } // charts, diagrams
        const rows: string[][] = [];
        for (const tr of kids(tbl).filter((c) => is(c, A, 'tr'))) {
          const row: string[] = [];
          for (const tc of kids(tr).filter((c) => is(c, A, 'tc'))) {
            if (tc.getAttribute('hMerge') === '1' || tc.getAttribute('vMerge') === '1') continue;
            const tb = child(tc, A, 'txBody');
            row.push(tb ? drawingParas(tb, false).map(paraText).join('\n') : '');
          }
          rows.push(row);
        }
        if (rows.length) slide.blocks.push({ kind: 'table', rows });
        continue;
      }
    }
  };
  const tree = doc.getElementsByTagNameNS(P, 'spTree')[0];
  if (tree) await walk(tree);
  slide.blocks = [...titles, ...slide.blocks];
  const firstTitle = titles[0];
  slide.title = firstTitle && firstTitle.kind === 'text' ? firstTitle.paras.map(paraText).join(' ').replace(/\s+/g, ' ').trim() : '';
  // Speaker notes: the notes page's body text.
  const notesRel = [...links.values()].find((r) => r.type === 'notesSlide' && !r.external);
  const notesText = notesRel ? await z.text(notesRel.target) : null;
  if (notesText) {
    const nd = parseXml(notesText);
    for (const sp of Array.from(nd.getElementsByTagNameNS(P, 'sp'))) {
      const ph = sp.getElementsByTagNameNS(P, 'ph')[0];
      if (!ph || (ph.getAttribute('type') || 'body') !== 'body') continue;
      const tb = child(sp, P, 'txBody');
      if (tb) slide.notes = drawingParas(tb, false).map(paraText).join('\n').trim();
    }
  }
  return slide;
}

// ── OpenDocument presentations ─────────────────────────────────────────────────────────────────

/** ODF text content (`text:p`, `text:h`, `text:list`) → paragraphs. */
function odfParas(parent: Element, styles: OdfStyles, out: Para[], depth = -1, listStyle = '', bulleted = false) {
  for (const e of kids(parent)) {
    if (is(e, ODF.text, 'p') || is(e, ODF.text, 'h')) {
      const runs: Run[] = [];
      const walk = (n: Element, f: Fmt) => {
        for (const c of Array.from(n.childNodes)) {
          if (c.nodeType === 3) { if (c.textContent) runs.push({ ...f, t: c.textContent }); continue; }
          if (c.nodeType !== 1) continue;
          const x = c as Element;
          if (is(x, ODF.text, 's')) runs.push({ t: ' '.repeat(Math.min(100, Math.max(1, Number(at(x, ODF.text, 'c') || 1)))) });
          else if (is(x, ODF.text, 'tab')) runs.push({ t: '\t' });
          else if (is(x, ODF.text, 'line-break')) runs.push({ t: '\n' });
          else if (is(x, ODF.text, 'note') || is(x, ODF.office, 'annotation')) continue;
          else if (is(x, ODF.text, 'span')) walk(x, { ...f, ...styles.text.get(at(x, ODF.text, 'style-name') || '') });
          else if (is(x, ODF.text, 'a')) {
            const href = at(x, ODF.xlink, 'href') || '';
            const before = runs.length;
            walk(x, f);
            if (SAFE_LINK.test(href)) for (let i = before; i < runs.length; i++) runs[i].href = href;
          } else walk(x, f);
        }
      };
      const st = styles.para.get(at(e, ODF.text, 'style-name') || '');
      walk(e, styles.paraFmt(at(e, ODF.text, 'style-name') || ''));
      const numbered = styles.lists.get(listStyle)?.[Math.max(0, depth)] ?? false;
      const p: Para = { runs, level: Math.max(0, depth), list: depth >= 0 && bulleted ? (numbered ? 'number' : 'bullet') : 'none', rtl: st?.rtl || undefined, align: st?.align === 'left' ? undefined : st?.align };
      if (paraText(p).trim()) out.push(p);
    } else if (is(e, ODF.text, 'list')) {
      const ls = at(e, ODF.text, 'style-name') || listStyle;
      for (const item of kids(e)) {
        if (is(item, ODF.text, 'list-item')) odfParas(item, styles, out, depth + 1, ls, true);
        else if (is(item, ODF.text, 'list-header')) odfParas(item, styles, out, depth + 1, ls, false);
      }
    }
  }
}

/** An OpenDocument presentation's slides (pages). */
export async function readOdp(bytes: Uint8Array): Promise<Deck> {
  const z = new OfficeZip(bytes);
  const text = await z.text('content.xml');
  if (!text) throw new OfficeError('wrongKind', 'no content');
  const doc = parseXml(text);
  const presentation = doc.getElementsByTagNameNS(ODF.office, 'presentation')[0];
  if (!presentation) throw new OfficeError('wrongKind', 'not a presentation');
  const styles = new OdfStyles();
  const st = await z.text('styles.xml');
  styles.add(st ? parseXml(st) : null);
  styles.add(doc);
  const pics = new Pictures(z);
  try {
    const slides: Slide[] = [];
    for (const page of kids(presentation).filter((c) => is(c, ODF.draw, 'page')).slice(0, MAX_SLIDES)) {
      const slide: Slide = { title: '', blocks: [], notes: '', hidden: false, skipped: false };
      const titles: Block[] = [];
      const walk = async (parent: Element, depth = 0) => {
        if (depth > MAX_DEPTH) { slide.skipped = true; return; }
        for (const el of kids(parent)) {
          if (is(el, ODF.pres, 'notes')) {
            const frame = Array.from(el.getElementsByTagNameNS(ODF.draw, 'frame')).find((f) => at(f, ODF.pres, 'class') === 'notes');
            if (frame) slide.notes = plainText(frame).trim();
            continue;
          }
          if (el.namespaceURI !== ODF.draw) continue;
          if (el.localName === 'g') { await walk(el, depth + 1); continue; }
          const cls = at(el, ODF.pres, 'class') || '';
          if (/^(page-number|date-time|footer|header)$/.test(cls)) continue;
          if (el.localName === 'frame') {
            const image = kids(el).find((c) => is(c, ODF.draw, 'image'));
            const box = kids(el).find((c) => is(c, ODF.draw, 'text-box'));
            const table = kids(el).find((c) => is(c, ODF.table, 'table'));
            if (box) {
              const paras: Para[] = [];
              odfParas(box, styles, paras);
              const role = cls === 'title' ? 'title' : cls === 'subtitle' ? 'subtitle' : 'body';
              if (paras.length) (role === 'title' ? titles : slide.blocks).push({ kind: 'text', role, paras });
            } else if (table) {
              const rows: string[][] = [];
              for (const tr of Array.from(table.getElementsByTagNameNS(ODF.table, 'table-row'))) rows.push(kids(tr).filter((c) => is(c, ODF.table, 'table-cell')).map((c) => plainText(c)));
              if (rows.length) slide.blocks.push({ kind: 'table', rows });
            } else if (image) {
              const part = internalPart(at(image, ODF.xlink, 'href') || '');
              const url = part && imgType(part) ? await pics.url(part) : null;
              const alt = kids(el).find((c) => is(c, ODF.svg, 'desc') || is(c, ODF.svg, 'title'))?.textContent ?? '';
              if (url) slide.blocks.push({ kind: 'image', src: url, alt }); else slide.skipped = true;
            } else slide.skipped = true; // charts, embedded objects
            continue;
          }
          // Drawn shapes with words in them.
          const paras: Para[] = [];
          odfParas(el, styles, paras);
          if (paras.length) slide.blocks.push({ kind: 'text', role: 'body', paras });
        }
      };
      await walk(page);
      slide.blocks = [...titles, ...slide.blocks];
      const ft = titles[0];
      slide.title = ft && ft.kind === 'text' ? ft.paras.map(paraText).join(' ').replace(/\s+/g, ' ').trim() : '';
      slides.push(slide);
    }
    return { slides, urls: pics.urls };
  } catch (e) {
    for (const u of pics.urls) URL.revokeObjectURL(u);
    throw e;
  }
}
