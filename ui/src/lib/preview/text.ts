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

/** Are these bytes text, for a file whose name does not say? Stricter than `decodeText`, which is
 *  for files named as text: here the start must be valid UTF-8 (or UTF-16 with its mark) and all
 *  but a few of its characters printable, so that a program or a packed file is not shown as noise. */
export function looksLikeText(b: Uint8Array): boolean {
  if (!b.length) return true;
  const head = b.subarray(0, 8192);
  const utf16 = (b[0] === 0xff && b[1] === 0xfe) || (b[0] === 0xfe && b[1] === 0xff);
  let s: string;
  try {
    // `stream`: a character cut in two at the end of the sample is not an error.
    s = utf16 ? new TextDecoder(b[0] === 0xff ? 'utf-16le' : 'utf-16be', { fatal: true }).decode(head.subarray(2), { stream: true })
      : new TextDecoder('utf-8', { fatal: true }).decode(head, { stream: true });
  } catch { return false; }
  let odd = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c === 0) return false;
    if ((c < 32 && c !== 9 && c !== 10 && c !== 13 && c !== 12 && c !== 27) || c === 127 || c === 0xfffd) odd++;
  }
  return odd * 100 <= s.length;
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
