// Turning a text file's bytes into text for the preview, safely: UTF-8 (or UTF-16 with its byte-order
// mark), files that are not text at all recognised rather than shown as noise, and long files cut to
// what the window can show without stalling.

/** Decode bytes as text. `null`: these bytes are not text (a program, a picture renamed .txt…). */
export function decodeText(b: Uint8Array): string | null {
  let enc = 'utf-8';
  let start = 0;
  if (b[0] === 0xff && b[1] === 0xfe) { enc = 'utf-16le'; start = 2; }
  else if (b[0] === 0xfe && b[1] === 0xff) { enc = 'utf-16be'; start = 2; }
  else if (b[0] === 0xef && b[1] === 0xbb && b[2] === 0xbf) start = 3;
  // A text file has no NUL bytes; binary files nearly always have some near the start.
  if (enc === 'utf-8') { const head = b.subarray(0, 8192); if (head.includes(0)) return null; }
  try { return new TextDecoder(enc).decode(b.subarray(start)); } catch { return null; }
}

export type Cut = { text: string; lines: number; shownLines: number; cut: boolean };

/** The first `maxLines` lines and at most `maxChars` characters of `text`, and how many lines it has. */
export function cutText(text: string, maxLines: number, maxChars: number): Cut {
  let total = 1;
  let endAt = -1;
  for (let i = text.indexOf('\n'); i >= 0; i = text.indexOf('\n', i + 1)) {
    if (total === maxLines && endAt < 0) endAt = i;
    total++;
  }
  // A final line end does not start another line.
  if (text.endsWith('\n')) total--;
  let end = endAt < 0 ? text.length : endAt;
  if (end > maxChars) { end = text.lastIndexOf('\n', maxChars); if (end <= 0) end = maxChars; }
  const shown = text.slice(0, end);
  const cut = end < text.length && text.slice(end).trim() !== '';
  let shownLines = 1;
  for (let i = shown.indexOf('\n'); i >= 0; i = shown.indexOf('\n', i + 1)) shownLines++;
  return { text: cut ? shown : text, lines: Math.max(total, 1), shownLines: cut ? shownLines : Math.max(total, 1), cut };
}
