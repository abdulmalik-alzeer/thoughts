// Spreadsheets (Excel .xlsx, OpenDocument .ods, CSV/TSV) read into plain tables for the preview.
// Runs in a Web Worker (sheet.worker.ts) or, where none can start, in the window: no DOM here.
// Formulas are never worked out: a cell shows the value the spreadsheet saved with it. Each sheet is
// cut to MAX_ROWS × MAX_COLS so a huge one never stalls the window; how much there is in all is kept.
import { OfficeError, OfficeZip, resolvePart } from './safezip';
import { attr, scanXml, type Attrs } from './xmlscan';
import { decodeCsv, guessSeparator, parseDelimited } from './csv';

export const MAX_ROWS = 1000;
export const MAX_COLS = 60;

/** One shown cell. `num`: a number, date or time (lined up at the far edge, like a spreadsheet). */
export type Cell = { v: string; num?: boolean };
export type Merge = { r: number; c: number; rs: number; cs: number };
export type SheetData = {
  name: string;
  rows: (Cell | null)[][];
  /** Columns shown. */
  cols: number;
  /** Rows and columns the sheet has in all (≥ what is shown). */
  totalRows: number;
  totalCols: number;
  merges: Merge[];
  /** The sheet is laid out right to left (Excel's "Sheet right-to-left"). */
  rtl: boolean;
  /** A CSV's first line is its column names. */
  header?: boolean;
};
export type SheetInfo = { name: string };

export interface Book {
  sheets: SheetInfo[];
  sheet(i: number): Promise<SheetData>;
}

// ── Cell references ─────────────────────────────────────────────────────────────────────────────

/** `A` → 0, `Z` → 25, `AA` → 26. */
export function colIndex(letters: string): number {
  let n = 0;
  for (const ch of letters.toUpperCase()) n = n * 26 + (ch.charCodeAt(0) - 64);
  return n - 1;
}
/** 0 → `A`, 26 → `AA`. */
export function colName(i: number): string {
  let s = '';
  for (let n = i + 1; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + ((n - 1) % 26)) + s;
  return s;
}
/** `B3` → { r: 2, c: 1 } (also `$B$3`). */
/** Excel's own limits: a reference past them is not a real cell. */
export const EXCEL_ROWS = 1_048_576;
export const EXCEL_COLS = 16_384;

export function cellRef(ref: string): { r: number; c: number } | null {
  const m = /^\$?([A-Za-z]{1,3})\$?(\d{1,7})$/.exec(ref.trim());
  if (!m) return null;
  const r = +m[2] - 1, c = colIndex(m[1]);
  return r >= 0 && r < EXCEL_ROWS && c >= 0 && c < EXCEL_COLS ? { r, c } : null;
}

/** Merged areas kept while their cells together fit in `limit` (the shown grid): many large
 *  overlapping merges can't make the table drawing loop over millions of cells. */
export function capMerges(merges: Merge[], limit: number): Merge[] {
  const out: Merge[] = [];
  let used = 0;
  for (const m of merges) {
    if (m.rs < 1 || m.cs < 1) continue;
    used += m.rs * m.cs;
    if (used > limit) break;
    out.push(m);
  }
  return out;
}

// ── Number formats (the common ones) ───────────────────────────────────────────────────────────

/** Excel's built-in formats that matter for showing a value. */
const BUILTIN: Record<number, string> = {
  0: 'General', 1: '0', 2: '0.00', 3: '#,##0', 4: '#,##0.00', 9: '0%', 10: '0.00%', 11: '0.00E+00', 12: '# ?/?', 13: '# ??/??',
  14: 'yyyy-mm-dd', 15: 'd-mmm-yy', 16: 'd-mmm', 17: 'mmm-yy', 18: 'h:mm AM/PM', 19: 'h:mm:ss AM/PM', 20: 'h:mm', 21: 'h:mm:ss',
  22: 'yyyy-mm-dd h:mm', 37: '#,##0 ;(#,##0)', 38: '#,##0 ;[Red](#,##0)', 39: '#,##0.00;(#,##0.00)', 40: '#,##0.00;[Red](#,##0.00)',
  45: 'mm:ss', 46: '[h]:mm:ss', 47: 'mmss.0', 48: '##0.0E+0', 49: '@',
};
// Formats Excel shows as dates in the user's own locale (ids 27–36, 50–58 for East Asian locales).
for (const id of [27, 28, 29, 30, 31, 34, 35, 36, 50, 51, 52, 53, 54, 55, 56, 57, 58]) BUILTIN[id] = 'yyyy-mm-dd';

const pad = (n: number, w = 2) => String(n).padStart(w, '0');

/** Remove what a format code shows literally (quoted text, escaped characters, colours, conditions). */
const bare = (code: string) => code.replace(/"[^"]*"/g, '').replace(/\\./g, '').replace(/\[(?!h\]|m\]|s\])[^\]]*\]/gi, '').replace(/[_*]./g, '');

export function isDateFormat(code: string): boolean {
  if (!code || /^general$/i.test(code)) return false;
  const b = bare(code.split(';')[0]);
  return /[dmyhs]/i.test(b) && !/[0#?]/.test(b.replace(/s+\.0+/i, ''));
}

/** A date-time serial number (days since the spreadsheet's epoch) as `2024-03-05 14:30`. */
export function serialDate(v: number, code: string, date1904 = false): string {
  // Excel counts 1900-02-29, which never was: serials from 61 on are one day ahead.
  const days = date1904 ? v + 1462 : v < 61 ? v + 1 : v;
  const ms = Math.round((days - 25569) * 86400000); // 25569 = 1970-01-01
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return String(v);
  const b = bare(code.split(';')[0]).toLowerCase();
  const hasTime = /h|s|am\/pm/.test(b) || (/m/.test(b) && !/[dy]/.test(b) && /:/.test(b));
  const hasDate = /[dy]/.test(b) || (/m/.test(b) && !/:/.test(b) && !/h/.test(b));
  const date = `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
  if (/\[h\]/.test(b)) { const total = Math.round(v * 86400); return `${Math.floor(total / 3600)}:${pad(Math.floor(total / 60) % 60)}:${pad(total % 60)}`; }
  const time = `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}${/s/.test(b) ? `:${pad(d.getUTCSeconds())}` : ''}`;
  if (hasDate && hasTime) return `${date} ${time}`;
  if (hasTime) return time;
  return date;
}

/** A number as its format code says (decimals, thousands separators, percent, scientific, literal
 *  text before and after such as a currency, a separate section for negatives). Anything unusual
 *  falls back to the plain number. */
export function formatNumber(v: number, code: string, date1904 = false): string {
  if (!Number.isFinite(v)) return String(v);
  if (isDateFormat(code)) return serialDate(v, code, date1904);
  const sections = (code || 'General').split(';');
  let sec = sections[0], sign = v < 0 ? '-' : '';
  // The negative section writes its own sign or brackets.
  if (v < 0 && sections[1]?.trim()) { sec = sections[1]; sign = ''; }
  else if (v === 0 && sections[2]?.trim()) sec = sections[2];
  const plain = (x: number) => String(parseFloat(x.toPrecision(11)));
  const s = sec.trim();
  if (!s || /^general$/i.test(s) || s === '@') return sign + plain(Math.abs(v));
  const general = /general|@/i.test(bare(sec));
  const lit = literals(general ? sec.replace(/general|@/i, '0') : sec);
  if (general) return sign + lit.pre + plain(Math.abs(v)) + lit.post;
  const b = bare(sec);
  if (!/[0#?]/.test(b)) return lit.pre; // a section that is only text (a dash for zero…)
  const percent = /%/.test(b);
  const x = Math.abs(v) * (percent ? 100 : 1);
  const decM = /\.([0#?]+)/.exec(b);
  const dec = decM ? decM[1].length : 0;
  const minDec = decM ? decM[1].replace(/[#?]/g, '').length : 0;
  const sci = /E[+-]([0#?]+)/i.exec(b);
  let out: string;
  if (sci) {
    const [m, e] = x.toExponential(dec).split('e');
    out = `${m}E${e.startsWith('-') ? '-' : '+'}${e.replace(/^[+-]/, '').padStart(sci[1].length, '0')}`;
  } else {
    // A comma right after the last digit divides by a thousand (#,##0, shows thousands).
    const scale = /[0#?](,+)(?![0#?,])/.exec(b);
    const y = scale ? x / 1000 ** scale[1].length : x;
    out = y.toLocaleString('en-US', { minimumFractionDigits: minDec, maximumFractionDigits: dec, useGrouping: /[0#?],[0#?]/.test(b) });
  }
  if (!/[1-9]/.test(out)) sign = '';
  return sign + lit.pre + out + (percent ? '%' : '') + lit.post;
}

/** The literal text before and after the digits of a format section (`"SAR "#,##0` → `SAR `,
 *  `(#,##0)` → brackets). */
function literals(sec: string): { pre: string; post: string } {
  const toks: { t: string; d: boolean }[] = [];
  const re = /"([^"]*)"|\\(.)|\[\$([^\]-]*)[^\]]*\]|\[[^\]]*\]|[_*].|([0#?][0#?,.]*(?:E[+-][0#?]+)?|\.[0#?]+)|(%)|(.)/gi;
  for (let m = re.exec(sec); m; m = re.exec(sec)) {
    const t = m[1] ?? m[2] ?? m[3] ?? m[6];
    if (m[4] !== undefined) toks.push({ t: m[4], d: true });
    else if (t !== undefined) toks.push({ t, d: false });
  }
  const first = toks.findIndex((x) => x.d);
  if (first < 0) return { pre: toks.map((x) => x.t).join(''), post: '' };
  let last = first;
  for (let i = toks.length - 1; i > first; i--) if (toks[i].d) { last = i; break; }
  return { pre: toks.slice(0, first).map((x) => x.t).join(''), post: toks.slice(last + 1).map((x) => x.t).join('') };
}

// ── Excel (.xlsx) ───────────────────────────────────────────────────────────────────────────────

/** Shared strings: each `<si>`'s text (rich-text runs joined; phonetic guides left out). */
export function readSharedStrings(xml: string): string[] {
  const out: string[] = [];
  let cur: string[] | null = null;
  let inT = false, skip = 0;
  scanXml(xml, {
    open(name, _a, self) {
      if (name === 'si') cur = [];
      else if (name === 'rPh' || name === 'phoneticPr') { if (!self) skip++; }
      else if (name === 't' && !self) inT = true;
    },
    close(name) {
      if (name === 'si') { out.push((cur ?? []).join('')); cur = null; }
      else if (name === 'rPh') skip = Math.max(0, skip - 1);
      else if (name === 't') inT = false;
    },
    text(t) { if (inT && !skip && cur) cur.push(t); },
  });
  return out;
}

/** Each cell style's number format code (index = the cell's `s`). */
export function readCellFormats(xml: string): string[] {
  const custom = new Map<number, string>();
  const xfs: string[] = [];
  let inXfs = false;
  scanXml(xml, {
    open(name, a) {
      if (name === 'numFmt') custom.set(+(attr(a, 'numFmtId') ?? -1), attr(a, 'formatCode') ?? '');
      else if (name === 'cellXfs') inXfs = true;
      else if (name === 'xf' && inXfs) { const id = +(attr(a, 'numFmtId') ?? 0); xfs.push(custom.get(id) ?? BUILTIN[id] ?? 'General'); }
    },
    close(name) { if (name === 'cellXfs') inXfs = false; },
  });
  return xfs;
}

type XlsxCtx = { strings: string[]; formats: string[]; date1904: boolean };

/** One worksheet's XML → a table (cut to the caps). */
export function readWorksheet(xml: string, name: string, ctx: XlsxCtx, maxRows = MAX_ROWS, maxCols = MAX_COLS): SheetData {
  const rows: (Cell | null)[][] = [];
  let rtl = false;
  let declared = 0, declaredCols = 0;
  let r = -1, c = -1;
  let cell: { t: string; s: number; c: number } | null = null;
  let val = '', inV = false, inIs = false, inT = false, skip = 0;
  let maxR = -1, maxC = -1, totalCols = 0, cutAt = -1;
  const sd = xml.indexOf('<sheetData');
  // Before the cells: the sheet's direction and stated size.
  scanXml(sd < 0 ? xml : xml.slice(0, sd), {
    open(n, a) {
      if (n === 'sheetView' && /^(1|true)$/.test(attr(a, 'rightToLeft') ?? '')) rtl = true;
      else if (n === 'dimension') {
        const parts = (attr(a, 'ref') ?? '').split(':');
        const end = cellRef(parts[parts.length - 1] ?? '');
        if (end) { declared = end.r + 1; declaredCols = end.c + 1; }
      }
    },
  });
  const put = (rr: number, cc: number, v: Cell) => {
    if (rr < 0 || rr >= maxRows) return;
    if (cc >= maxCols) { if (cc + 1 > totalCols) totalCols = cc + 1; return; }
    while (rows.length <= rr) rows.push([]);
    rows[rr][cc] = v;
    if (rr > maxR) maxR = rr;
    if (cc > maxC) maxC = cc;
    if (cc + 1 > totalCols) totalCols = cc + 1;
  };
  if (sd >= 0) scanXml(xml, {
    open(n, a, self) {
      if (n === 'row') {
        const rn = attr(a, 'r');
        r = rn && Number.isFinite(+rn) && +rn >= 1 ? Math.min(+rn, EXCEL_ROWS) - 1 : r + 1;
        c = -1;
        if (r >= maxRows) { cutAt = r; return false; }
      } else if (n === 'c') {
        const ref = cellRef(attr(a, 'r') ?? '');
        c = ref ? ref.c : c + 1;
        if (ref) r = ref.r;
        // A cell may name any row: one past the cap is skipped and the sheet marked as cut.
        if (r >= maxRows) { if (r > cutAt) cutAt = r; cell = null; return; }
        cell = { t: attr(a, 't') ?? 'n', s: +(attr(a, 's') ?? 0), c };
        val = '';
        if (self) cell = null;
      } else if (n === 'v' && cell && !self) { inV = true; val = ''; }
      else if (n === 'is' && cell) { inIs = true; val = ''; }
      else if (n === 't' && inIs && !self) inT = true;
      else if (n === 'rPh' && !self) skip++;
      else if (n === 'sheetData' && self) return false;
    },
    close(n) {
      if (n === 'v') inV = false;
      else if (n === 't') inT = false;
      else if (n === 'rPh') skip = Math.max(0, skip - 1);
      else if (n === 'is') inIs = false;
      else if (n === 'c' && cell) {
        const shown = cellValue(cell.t, val, ctx.formats[cell.s] ?? 'General', ctx);
        if (shown && shown.v !== '') put(r, cell.c, shown);
        cell = null;
      } else if (n === 'sheetData') return false;
    },
    text(t) { if ((inV || (inT && !skip)) && cell) val += t; },
  }, sd);
  // How many rows there are in all: the stated size, or the last row's number.
  let totalRows = maxR + 1;
  if (cutAt >= 0) {
    const endSd = xml.indexOf('</sheetData>');
    const lastRow = xml.lastIndexOf('<row ', endSd < 0 ? xml.length : endSd);
    const m = lastRow >= 0 ? /\sr="(\d+)"/.exec(xml.slice(lastRow, lastRow + 300)) : null;
    totalRows = Math.min(EXCEL_ROWS, Math.max(cutAt + 1, declared, m ? +m[1] : 0));
  }
  // Merged cells sit after the cells.
  const merges: Merge[] = [];
  const mi = xml.indexOf('<mergeCells');
  if (mi >= 0) scanXml(xml, {
    open(n, a) {
      if (n !== 'mergeCell') return;
      const [s, e] = (attr(a, 'ref') ?? '').split(':').map(cellRef);
      if (!s || !e || s.r >= maxRows || s.c >= maxCols) return;
      merges.push({ r: s.r, c: s.c, rs: Math.min(e.r, maxRows - 1) - s.r + 1, cs: Math.min(e.c, maxCols - 1) - s.c + 1 });
    },
    close(n) { if (n === 'mergeCells') return false; },
  }, mi);
  merges.splice(0, merges.length, ...capMerges(merges, maxRows * maxCols));
  for (const m of merges) { if (m.r + m.rs - 1 > maxR) maxR = m.r + m.rs - 1; if (m.c + m.cs - 1 > maxC) maxC = m.c + m.cs - 1; }
  while (rows.length < maxR + 1) rows.push([]);
  const cols = maxC + 1;
  return { name, rows: rows.map((row) => Array.from({ length: cols }, (_, i) => row[i] ?? null)), cols, totalRows: Math.max(totalRows, rows.length), totalCols: Math.max(totalCols, cols, cutAt >= 0 ? declaredCols : 0), merges, rtl };
}

function cellValue(t: string, raw: string, code: string, ctx: XlsxCtx): Cell | null {
  switch (t) {
    case 's': { const s = ctx.strings[+raw]; return s === undefined ? null : { v: s }; }
    case 'str': case 'inlineStr': return { v: raw };
    case 'b': return { v: raw === '1' || raw === 'true' ? 'TRUE' : 'FALSE' };
    case 'e': return { v: raw };
    case 'd': return { v: raw.replace('T', ' ').replace(/(:\d\d)(\.\d+)?Z?$/, '$1'), num: true };
    default: {
      if (raw === '') return null;
      const n = Number(raw);
      if (Number.isNaN(n)) return { v: raw };
      return { v: formatNumber(n, code, ctx.date1904), num: true };
    }
  }
}

async function openXlsx(bytes: Uint8Array): Promise<Book> {
  const z = new OfficeZip(bytes);
  const wbPath = z.has('xl/workbook.xml') ? 'xl/workbook.xml' : z.names().find((n) => /^xl\/workbook\d*\.xml$/i.test(n));
  if (!wbPath) throw new OfficeError('wrongKind', 'not a spreadsheet');
  const wb = (await z.text(wbPath)) ?? '';
  const relText = (await z.text('xl/_rels/workbook.xml.rels')) ?? '';
  const rels = new Map<string, { target: string; type: string }>();
  scanXml(relText, { open(n, a) { if (n === 'Relationship') rels.set(attr(a, 'Id') ?? '', { target: resolvePart(wbPath, attr(a, 'Target') ?? ''), type: (attr(a, 'Type') ?? '').split('/').pop() ?? '' }); } });
  const sheets: { name: string; part: string }[] = [];
  let date1904 = false;
  scanXml(wb, {
    open(n, a: Attrs) {
      if (n === 'workbookPr' && /^(1|true)$/.test(attr(a, 'date1904') ?? '')) date1904 = true;
      if (n !== 'sheet') return;
      const state = attr(a, 'state') ?? '';
      const rid = a['r:id'] ?? attr(a, 'id') ?? '';
      const part = rels.get(rid);
      // Hidden sheets stay hidden; chart sheets have no cells.
      if (state === 'hidden' || state === 'veryHidden' || !part || part.type !== 'worksheet') return;
      sheets.push({ name: attr(a, 'name') ?? `Sheet${sheets.length + 1}`, part: part.target });
    },
  });
  const sstRel = [...rels.values()].find((r) => r.type === 'sharedStrings');
  const stRel = [...rels.values()].find((r) => r.type === 'styles');
  const sst = await z.text(sstRel?.target ?? 'xl/sharedStrings.xml');
  const st = await z.text(stRel?.target ?? 'xl/styles.xml');
  const ctx: XlsxCtx = { strings: sst ? readSharedStrings(sst) : [], formats: st ? readCellFormats(st) : [], date1904 };
  return {
    sheets: sheets.map((s) => ({ name: s.name })),
    async sheet(i) {
      const s = sheets[i];
      if (!s) throw new OfficeError('damaged', 'no such sheet');
      const xml = await z.text(s.part);
      if (xml === null) return { name: s.name, rows: [], cols: 0, totalRows: 0, totalCols: 0, merges: [], rtl: false };
      return readWorksheet(xml, s.name, ctx);
    },
  };
}

// ── OpenDocument (.ods) ─────────────────────────────────────────────────────────────────────────

/** Every sheet of an OpenDocument spreadsheet's content.xml (cut to the caps). Repeated rows and
 *  columns (how ODS stores long runs of empty cells) are expanded only as far as the caps. */
export function readOdsContent(xml: string, maxRows = MAX_ROWS, maxCols = MAX_COLS): SheetData[] {
  const out: SheetData[] = [];
  let sheet: SheetData | null = null;
  let maxR = -1, maxC = -1, totalCols = 0;
  let r = 0, c = 0, rowRepeat = 1;
  let rowCells: { c: number; cell: Cell; span: number; rspan: number; repeat: number }[] = [];
  let cell: { num: boolean; value: string; repeat: number; span: number; rspan: number; text: string[]; para: number } | null = null;
  let skip = 0; // inside a comment
  let inP = false;
  scanXml(xml, {
    open(n, a, self) {
      if (skip) { if (!self) skip++; return; }
      if (n === 'annotation') { if (!self) skip = 1; return; }
      if (n === 'table' && !sheet) {
        sheet = { name: attr(a, 'name') ?? `Sheet${out.length + 1}`, rows: [], cols: 0, totalRows: 0, totalCols: 0, merges: [], rtl: false };
        maxR = -1; maxC = -1; totalCols = 0; r = 0;
      } else if (n === 'table-row' && sheet) {
        rowRepeat = Math.max(1, +(attr(a, 'number-rows-repeated') ?? 1) || 1);
        c = 0; rowCells = [];
      } else if ((n === 'table-cell' || n === 'covered-table-cell') && sheet) {
        const type = attr(a, 'value-type') ?? '';
        const value = attr(a, 'date-value') ?? attr(a, 'time-value') ?? attr(a, 'boolean-value') ?? attr(a, 'value') ?? '';
        cell = {
          num: /^(float|percentage|currency|date|time)$/.test(type), value: n === 'covered-table-cell' ? '' : value,
          repeat: Math.max(1, +(attr(a, 'number-columns-repeated') ?? 1) || 1),
          span: n === 'covered-table-cell' ? 0 : Math.max(1, +(attr(a, 'number-columns-spanned') ?? 1) || 1),
          rspan: Math.max(1, +(attr(a, 'number-rows-spanned') ?? 1) || 1), text: [], para: 0,
        };
        if (n === 'covered-table-cell') cell.text = [];
        if (self) finishCell();
      } else if (cell && n === 'p') { if (cell.para++) cell.text.push('\n'); inP = !self; }
      else if (cell && inP && n === 's') cell.text.push(' '.repeat(Math.min(100, Math.max(1, +(attr(a, 'c') ?? 1) || 1))));
      else if (cell && inP && n === 'tab') cell.text.push('\t');
      else if (cell && inP && n === 'line-break') cell.text.push('\n');
    },
    close(n) {
      if (skip) { skip--; return; }
      if (n === 'p') inP = false;
      else if ((n === 'table-cell' || n === 'covered-table-cell') && cell) finishCell();
      else if (n === 'table-row' && sheet) finishRow();
      else if (n === 'table' && sheet) {
        const s: SheetData = sheet;
        const cols = maxC + 1;
        s.rows = s.rows.slice(0, maxR + 1).map((row) => Array.from({ length: cols }, (_, i) => row[i] ?? null));
        s.cols = cols;
        s.totalCols = Math.max(totalCols, cols);
        s.totalRows = Math.max(s.totalRows, s.rows.length);
        s.merges = capMerges(s.merges.filter((m) => m.r <= maxR), maxRows * maxCols);
        out.push(s);
        sheet = null;
      }
    },
    text(t) { if (cell && inP && !skip) cell.text.push(t); },
  });
  function finishCell() {
    const x = cell!;
    cell = null;
    const shown = x.text.join('') || (x.value && x.span ? x.value : '');
    if (shown !== '' || x.span > 1 || x.rspan > 1) rowCells.push({ c, cell: { v: shown, num: x.num || undefined }, span: x.span, rspan: x.rspan, repeat: x.repeat });
    c += x.repeat;
  }
  function finishRow() {
    const s = sheet!;
    if (rowCells.length) {
      // Content in repeated rows is rare (a run of identical rows); a run of empty rows is not content.
      for (let k = 0; k < rowRepeat; k++) {
        const rr = r + k;
        if (rr < maxRows) {
          while (s.rows.length <= rr) s.rows.push([]);
          for (const rc of rowCells) for (let j = 0; j < rc.repeat && rc.c + j < maxCols; j++) {
            const cc = rc.c + j;
            if (rc.cell.v !== '') { s.rows[rr][cc] = rc.cell; if (cc > maxC) maxC = cc; }
            if ((rc.span > 1 || rc.rspan > 1) && k === 0) {
              s.merges.push({ r: rr, c: cc, rs: Math.min(rc.rspan, maxRows - rr), cs: Math.min(rc.span, maxCols - cc) });
              maxC = Math.max(maxC, Math.min(cc + rc.span, maxCols) - 1);
            }
          }
          if (rr > maxR) maxR = rr;
        }
        s.totalRows = rr + 1;
        if (rr >= maxRows) { s.totalRows = r + rowRepeat; break; }
      }
      for (const rc of rowCells) if (rc.cell.v !== '' && rc.c + rc.repeat > totalCols) totalCols = Math.min(rc.c + rc.repeat, 16384);
    }
    r += rowRepeat;
  }
  return out;
}

async function openOds(bytes: Uint8Array): Promise<Book> {
  const z = new OfficeZip(bytes);
  const xml = await z.text('content.xml');
  if (!xml || !/<office:spreadsheet[\s>]/.test(xml)) throw new OfficeError('wrongKind', 'not a spreadsheet');
  const all = readOdsContent(xml);
  return { sheets: all.map((s) => ({ name: s.name })), sheet: async (i) => all[i] ?? Promise.reject(new OfficeError('damaged')) };
}

// ── CSV / TSV ───────────────────────────────────────────────────────────────────────────────────

export function readCsv(bytes: Uint8Array, name: string, maxRows = MAX_ROWS, maxCols = MAX_COLS): SheetData {
  const text = decodeCsv(bytes);
  const p = parseDelimited(text, guessSeparator(text, name), maxRows, maxCols);
  const cols = p.cols;
  const rows = p.rows.map((row) => Array.from({ length: cols }, (_, i) => {
    const v = row[i];
    if (v === undefined || v === '') return null;
    return /^\s*[-+]?(\d{1,3}(,\d{3})+|\d+)?(\.\d+)?([eE][-+]?\d+)?%?\s*$/.test(v) && /\d/.test(v) ? { v, num: true } : { v };
  }));
  return { name, rows, cols, totalRows: p.totalRows, totalCols: p.totalCols, merges: [], rtl: false, header: p.rows.length > 1 };
}

/** Open a spreadsheet of the given preview kind. */
export async function openBook(kind: 'xlsx' | 'ods' | 'csv', bytes: Uint8Array, name: string): Promise<Book> {
  if (kind === 'csv') { const s = readCsv(bytes, name); return { sheets: [{ name }], sheet: async () => s }; }
  return kind === 'ods' ? openOds(bytes) : openXlsx(bytes);
}
