// Comma- and tab-separated text (CSV, TSV) for the table preview: the text's encoding worked out
// (UTF-8 or UTF-16 with or without a byte-order mark, else the Arabic or Western Windows code page
// Excel saves in), the separator guessed, and quoted fields read the standard way (separators, line
// breaks and doubled quotes inside quotes).

/** Bytes → text. Never fails: bytes that are not UTF-8 are read in the Windows code page they most
 *  likely are (Arabic when most non-ASCII bytes fall in the Arabic letter range, else Western). */
export function decodeCsv(b: Uint8Array): string {
  if (b[0] === 0xef && b[1] === 0xbb && b[2] === 0xbf) return new TextDecoder().decode(b.subarray(3));
  if (b[0] === 0xff && b[1] === 0xfe) return new TextDecoder('utf-16le').decode(b.subarray(2));
  if (b[0] === 0xfe && b[1] === 0xff) return new TextDecoder('utf-16be').decode(b.subarray(2));
  // UTF-16 without a mark: every other byte of ordinary text is zero.
  const head = b.subarray(0, 4096);
  let evenZero = 0, oddZero = 0;
  for (let i = 0; i < head.length; i++) if (head[i] === 0) { if (i & 1) oddZero++; else evenZero++; }
  if (head.length >= 4 && oddZero > head.length / 4 && evenZero === 0) return new TextDecoder('utf-16le').decode(b);
  if (head.length >= 4 && evenZero > head.length / 4 && oddZero === 0) return new TextDecoder('utf-16be').decode(b);
  try { return new TextDecoder('utf-8', { fatal: true }).decode(b); } catch { /* not UTF-8 */ }
  let high = 0, arabic = 0;
  for (let i = 0; i < b.length && i < 1 << 20; i++) {
    const x = b[i];
    if (x < 0x80) continue;
    high++;
    // windows-1256 letters: C1–D6, D8–DF, E1, E3–E6, EC–ED (Arabic) — in Western text these are
    // accented capitals and a few small letters, seldom most of the non-ASCII bytes.
    if ((x >= 0xc1 && x <= 0xdf && x !== 0xd7) || x === 0xe1 || (x >= 0xe3 && x <= 0xe6) || x === 0xec || x === 0xed) arabic++;
  }
  const cp = high && arabic / high > 0.6 ? 'windows-1256' : 'windows-1252';
  try { return new TextDecoder(cp).decode(b); } catch { return new TextDecoder('utf-8').decode(b); }
}

/** The separator: tab for .tsv/.tab; otherwise whichever of `,` `;` `\t` `|` splits the first lines
 *  most consistently (outside quotes). */
export function guessSeparator(text: string, name = ''): string {
  if (/\.(tsv|tab)$/i.test(name)) return '\t';
  const sample = text.slice(0, 64 * 1024);
  const cands = [',', ';', '\t', '|'];
  let best = ',', bestScore = 0;
  for (const sep of cands) {
    const counts: number[] = [];
    let q = false, c = 0;
    for (let i = 0; i < sample.length && counts.length < 30; i++) {
      const ch = sample[i];
      if (ch === '"') q = !q;
      else if (!q && ch === sep) c++;
      else if (!q && ch === '\n') { counts.push(c); c = 0; }
    }
    if (c) counts.push(c);
    const lines = counts.filter((x) => x > 0);
    if (!lines.length) continue;
    const mode = lines.sort((a, b) => a - b)[Math.floor(lines.length / 2)];
    const same = counts.filter((x) => x === mode).length;
    const score = same * 10 + mode; // consistent across lines first, then more columns
    if (score > bestScore) { bestScore = score; best = sep; }
  }
  return best;
}

export type Delimited = { rows: string[][]; totalRows: number; cols: number; totalCols: number };

/** Split `text` into rows of fields. At most `maxRows` rows and `maxCols` fields per row are kept;
 *  the rest is still counted (`totalRows`, `totalCols`). A blank final line is not a row. */
export function parseDelimited(text: string, sep: string, maxRows = Infinity, maxCols = Infinity): Delimited {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = '';
  let q = false;
  let total = 0, totalCols = 0, cols = 0;
  let rowLen = 0; // fields in the current row, kept or not
  let keep = true;
  const n = text.length;
  const endField = () => {
    if (keep && rowLen < maxCols) row.push(field);
    rowLen++;
    field = '';
  };
  const endRow = () => {
    endField();
    if (rowLen > totalCols) totalCols = rowLen;
    if (keep) { rows.push(row); if (row.length > cols) cols = row.length; }
    total++;
    keep = total < maxRows;
    row = []; rowLen = 0;
  };
  for (let i = 0; i < n; i++) {
    const ch = text[i];
    if (q) {
      if (ch === '"') {
        if (text[i + 1] === '"') { if (keep) field += '"'; i++; } else q = false;
      } else if (keep) field += ch;
      continue;
    }
    if (ch === '"' && field === '') { q = true; continue; }
    if (ch === sep) { endField(); continue; }
    if (ch === '\r' || ch === '\n') {
      if (ch === '\r' && text[i + 1] === '\n') i++;
      endRow();
      continue;
    }
    if (keep) field += ch;
  }
  if (field !== '' || rowLen > 0 || q) endRow();
  return { rows, totalRows: total, cols, totalCols };
}
