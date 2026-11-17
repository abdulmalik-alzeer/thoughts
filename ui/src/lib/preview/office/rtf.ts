// Rich Text Format (.rtf) read for its text: paragraphs, bold/italic/underline/strike, right-to-left
// paragraphs and alignment, characters in any Windows code page or Unicode. Pictures, embedded
// objects, headers, footers, footnotes and comments are skipped; nothing in the file is ever run.
import { OfficeError } from './safezip';

export type RtfRun = { t: string; b?: boolean; i?: boolean; u?: boolean; s?: boolean };
export type RtfPara = { runs: RtfRun[]; rtl: boolean; align?: 'center' | 'right' | 'left' | 'justify' };

/** Font character set (`\fcharsetN`) → the code page its bytes are in. */
const CHARSET: Record<number, string> = {
  0: 'windows-1252', 1: 'windows-1252', 77: 'macintosh', 128: 'shift_jis', 129: 'euc-kr', 134: 'gbk', 136: 'big5',
  161: 'windows-1253', 162: 'windows-1254', 163: 'windows-1258', 177: 'windows-1255', 178: 'windows-1256', 186: 'windows-1257',
  204: 'windows-1251', 222: 'windows-874', 238: 'windows-1250',
};
/** `\ansicpgN` → code page name. */
const cpName = (n: number) => (n === 10000 ? 'macintosh' : n === 932 ? 'shift_jis' : n === 936 ? 'gbk' : n === 949 ? 'euc-kr' : n === 950 ? 'big5' : `windows-${n}`);

/** Groups whose content is not the document's text. */
const SKIP = new Set([
  'colortbl', 'stylesheet', 'info', 'pict', 'object', 'header', 'headerl', 'headerr', 'headerf', 'footer', 'footerl', 'footerr', 'footerf',
  'footnote', 'annotation', 'atnid', 'atnauthor', 'xmlnstbl', 'listtable', 'listoverridetable', 'rsidtbl', 'generator', 'themedata',
  'colorschememapping', 'datastore', 'latentstyles', 'pgdsctbl', 'revtbl', 'fldinst', 'filetbl', 'mmathPr', 'nonshppict', 'shppict',
  'bkmkstart', 'bkmkend', 'template', 'userprops', 'xe', 'tc', 'pnseclvl', 'pntxta', 'pntxtb', 'operator', 'author', 'title', 'company',
]);
/** Skipped groups that held something the reader might miss (so the preview says some parts aren't shown). */
const MISSED = new Set(['pict', 'object', 'header', 'headerl', 'headerr', 'headerf', 'footer', 'footerl', 'footerr', 'footerf', 'footnote', 'annotation', 'shppict']);

type State = { b: boolean; i: boolean; u: boolean; s: boolean; font: number; uc: number; rtl: boolean; align?: RtfPara['align']; dest: '' | 'fonttbl' };

export function parseRtf(bytes: Uint8Array): { paras: RtfPara[]; skipped: boolean } {
  const head = String.fromCharCode(...bytes.subarray(0, 6));
  if (!head.startsWith('{\\rtf')) throw new OfficeError('wrongKind', 'not rtf');
  const n = bytes.length;
  const paras: RtfPara[] = [];
  let runs: RtfRun[] = [];
  let skipped = false;
  let st: State = { b: false, i: false, u: false, s: false, font: -1, uc: 1, rtl: false, dest: '' };
  const stack: State[] = [];
  let ansi = 'windows-1252';
  let defFont = -1;
  const fontCp = new Map<number, string>();
  let tblFont = -1;
  let text = '';
  let bytesBuf: number[] = [];
  let toSkip = 0; // characters still to skip after \uN
  const decoders = new Map<string, TextDecoder>();
  const decode = (b: number[]) => {
    const font = st.font >= 0 ? st.font : defFont;
    const cp = fontCp.get(font) ?? ansi;
    let d = decoders.get(cp);
    if (!d) { try { d = new TextDecoder(cp); } catch { d = new TextDecoder('windows-1252'); } decoders.set(cp, d); }
    return d.decode(new Uint8Array(b));
  };
  const flushBytes = () => { if (bytesBuf.length) { text += decode(bytesBuf); bytesBuf = []; } };
  const flushText = () => {
    flushBytes();
    if (!text) return;
    if (st.dest === '') {
      const last = runs[runs.length - 1];
      if (last && !!last.b === st.b && !!last.i === st.i && !!last.u === st.u && !!last.s === st.s) last.t += text;
      else { const r: RtfRun = { t: text }; if (st.b) r.b = true; if (st.i) r.i = true; if (st.u) r.u = true; if (st.s) r.s = true; runs.push(r); }
    }
    text = '';
  };
  const endPara = () => { flushText(); paras.push({ runs, rtl: st.rtl, align: st.align }); runs = []; };
  const addChar = (s: string) => { if (toSkip > 0) { toSkip--; return; } flushBytes(); text += s; };
  const addByte = (b: number) => { if (toSkip > 0) { toSkip--; return; } bytesBuf.push(b); };

  /** Skip the rest of the current group (from just inside it) without reading it. */
  const skipGroup = (i: number): number => {
    let depth = 1;
    while (i < n && depth > 0) {
      const c = bytes[i];
      if (c === 0x5c /* \ */) {
        // \binN: N raw bytes follow.
        if (bytes[i + 1] === 0x62 && bytes[i + 2] === 0x69 && bytes[i + 3] === 0x6e) {
          let j = i + 4, num = 0;
          while (j < n && bytes[j] >= 0x30 && bytes[j] <= 0x39) num = num * 10 + bytes[j++] - 0x30;
          if (bytes[j] === 0x20) j++;
          i = j + num; continue;
        }
        i += 2; continue;
      }
      if (c === 0x7b) depth++; else if (c === 0x7d) depth--;
      i++;
    }
    return i;
  };

  let i = 0;
  while (i < n) {
    const c = bytes[i];
    if (c === 0x7b /* { */) { flushText(); stack.push({ ...st }); i++; continue; }
    if (c === 0x7d /* } */) { flushText(); st = stack.pop() ?? st; toSkip = 0; i++; continue; }
    if (c === 0x0d || c === 0x0a) { i++; continue; }
    if (c !== 0x5c) { if (st.dest === '') { if (c >= 0x80) addByte(c); else addChar(String.fromCharCode(c)); } i++; continue; }
    // A control word or symbol.
    const d = bytes[i + 1];
    if (d === undefined) break;
    const isLetter = (x: number) => (x >= 0x61 && x <= 0x7a) || (x >= 0x41 && x <= 0x5a);
    if (!isLetter(d)) {
      i += 2;
      if (d === 0x27 /* ' */) { const hex = String.fromCharCode(bytes[i] ?? 0, bytes[i + 1] ?? 0); i += 2; const v = parseInt(hex, 16); if (!Number.isNaN(v) && st.dest === '') addByte(v); continue; }
      if (st.dest !== '') continue;
      if (d === 0x5c || d === 0x7b || d === 0x7d) addChar(String.fromCharCode(d));
      else if (d === 0x7e /* ~ */) addChar(' ');
      else if (d === 0x5f /* _ */) addChar('‑');
      else if (d === 0x0a || d === 0x0d) endPara(); // "\" + line end is a paragraph mark
      else if (d === 0x2a /* * */) {
        // \*\word: an optional destination this reader doesn't know: skipped whole.
        flushText();
        i = skipGroup(i);
        st = stack.pop() ?? st;
      }
      continue;
    }
    let j = i + 1;
    while (j < n && isLetter(bytes[j])) j++;
    const word = String.fromCharCode(...bytes.subarray(i + 1, j));
    let param: number | null = null;
    let neg = false;
    if (bytes[j] === 0x2d) { neg = true; j++; }
    if (bytes[j] >= 0x30 && bytes[j] <= 0x39) { param = 0; while (j < n && bytes[j] >= 0x30 && bytes[j] <= 0x39) param = param * 10 + bytes[j++] - 0x30; if (neg) param = -param; }
    if (bytes[j] === 0x20) j++;
    i = j;

    if (st.dest === 'fonttbl') {
      if (word === 'f' && param !== null) tblFont = param;
      else if (word === 'fcharset' && param !== null && tblFont >= 0 && CHARSET[param]) fontCp.set(tblFont, CHARSET[param]);
      continue;
    }
    if (word === 'bin') { i += Math.max(0, param ?? 0); continue; }
    if (word === 'fonttbl') { flushText(); st.dest = 'fonttbl'; continue; }
    if (SKIP.has(word)) { flushText(); if (MISSED.has(word)) skipped = true; i = skipGroup(i); st = stack.pop() ?? st; continue; }
    const on = param === null || param !== 0;
    switch (word) {
      case 'ansicpg': if (param) ansi = cpName(param); break;
      case 'deff': if (param !== null) defFont = param; break;
      case 'f': flushText(); if (param !== null) st.font = param; break;
      case 'uc': if (param !== null) st.uc = Math.max(0, param); break;
      case 'u': if (param !== null) { toSkip = 0; flushBytes(); text += String.fromCharCode(param < 0 ? param + 65536 : param); toSkip = st.uc; } break;
      case 'par': case 'sect': case 'page': case 'row': endPara(); break;
      case 'line': addChar('\n'); break;
      case 'tab': case 'cell': addChar('\t'); break;
      case 'pard': flushText(); st.rtl = false; st.align = undefined; break;
      case 'rtlpar': st.rtl = true; break;
      case 'ltrpar': st.rtl = false; break;
      case 'qc': st.align = 'center'; break;
      case 'qj': st.align = 'justify'; break;
      case 'qr': st.align = 'right'; break;
      case 'ql': st.align = 'left'; break;
      case 'b': flushText(); st.b = on; break;
      case 'i': flushText(); st.i = on; break;
      case 'ul': flushText(); st.u = on; break;
      case 'ulnone': flushText(); st.u = false; break;
      case 'strike': flushText(); st.s = on; break;
      case 'plain': flushText(); st.b = st.i = st.u = st.s = false; break;
      case 'emdash': addChar('—'); break;
      case 'endash': addChar('–'); break;
      case 'bullet': addChar('•'); break;
      case 'lquote': addChar('‘'); break;
      case 'rquote': addChar('’'); break;
      case 'ldblquote': addChar('“'); break;
      case 'rdblquote': addChar('”'); break;
      case 'emspace': case 'enspace': case 'qmspace': addChar(' '); break;
    }
  }
  flushText();
  if (runs.length) paras.push({ runs, rtl: st.rtl, align: st.align });
  // Blank paragraphs at either end say nothing.
  const blank = (p: RtfPara) => !p.runs.some((r) => r.t.trim());
  while (paras.length && blank(paras[0])) paras.shift();
  while (paras.length && blank(paras[paras.length - 1])) paras.pop();
  return { paras, skipped };
}

/** An RTF file as a page (built with DOM calls and text nodes only). */
export function rtfToDom(bytes: Uint8Array): { node: DocumentFragment; urls: string[]; skipped: boolean; cut: boolean } {
  const { paras, skipped } = parseRtf(bytes);
  const frag = document.createDocumentFragment();
  for (const p of paras) {
    const el = document.createElement('p');
    el.setAttribute('dir', p.rtl ? 'rtl' : 'auto');
    const natural = p.rtl ? 'right' : 'left';
    if (p.align && p.align !== natural) el.setAttribute('align', p.align);
    for (const r of p.runs) {
      let node: Node = document.createTextNode(r.t);
      for (const [yes, tag] of [[r.s, 's'], [r.u, 'u'], [r.i, 'em'], [r.b, 'strong']] as const) {
        if (!yes) continue;
        const w = document.createElement(tag); w.appendChild(node); node = w;
      }
      el.appendChild(node);
    }
    if (!el.textContent?.trim()) el.appendChild(document.createElement('br'));
    frag.appendChild(el);
  }
  return { node: frag, urls: [], skipped, cut: false };
}
