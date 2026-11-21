// Looking at a chat's files: how a file is opened from its chip, and the bytes of a file still
// waiting in the message box. Pure: no DOM, no requests.

/** What one click on a sent file does: a picture opens in the picture viewer, anything else in the
 *  file preview — which shows it, or says plainly that it can't and offers the file's own app. */
export function chipAction(name: string): 'picture' | 'preview' {
  // (The same names as `isImageName` in lib/content/util.ts, which a test here holds it to: the
  // kinds every window's engine draws. Other pictures — HEIC, TIFF — go to the preview, which tries them.)
  return /\.(png|jpe?g|gif|webp|svg|bmp|avif)$/i.test(name) ? 'picture' : 'preview';
}

/** The last part of a path, whichever way its slashes lean. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** The bytes of a `data:` URL as the message box keeps a waiting file (base64, or plain text
 *  percent-encoded). Nothing is fetched: the URL is decoded here. Throws on anything else. */
export function dataUrlBytes(url: string): Uint8Array {
  const m = /^data:([^,]*),/i.exec(url);
  if (!m) throw new Error('not a data URL');
  const body = url.slice(m[0].length);
  if (/;base64$/i.test(m[1])) {
    const bin = atob(body.replace(/\s+/g, ''));
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }
  return new TextEncoder().encode(decodeURIComponent(body));
}
