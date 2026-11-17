// Opening an Office or OpenDocument file (both are ZIPs of XML files) for a preview, with limits so a
// file built to unpack to gigabytes ("zip bomb") is refused instead of filling memory. Works in the
// window and in a Web Worker (no DOM).
import { ZipReader } from '../../content/zip';

/** Why a file could not be shown. The renderers turn these into plain sentences. */
export type OfficeProblem = 'bomb' | 'protected' | 'damaged' | 'wrongKind';
export class OfficeError extends Error {
  constructor(public problem: OfficeProblem, detail = '') { super(detail || problem); }
}
export const problemOf = (e: unknown): OfficeProblem => (e instanceof OfficeError ? e.problem : /too large/.test(String((e as Error)?.message)) ? 'bomb' : 'damaged');

/** Limits for one file: entries in it, and what all of it may unpack to (each part is also held to
 *  ZipReader.LIMIT). Real documents stay far below both. */
export const CAPS = { entries: 10_000, total: 300 * 1024 * 1024 };

/** An Office/OpenDocument file's parts, read on demand within CAPS. */
export class OfficeZip {
  private z: ZipReader;
  private used = 0;
  constructor(bytes: Uint8Array, private caps = CAPS) {
    // An encrypted (password-protected) Office file is an old-style compound file, not a ZIP; so is a
    // .doc/.xls/.ppt renamed to the new extension.
    if (bytes[0] === 0xd0 && bytes[1] === 0xcf && bytes[2] === 0x11 && bytes[3] === 0xe0) throw new OfficeError('protected');
    try { this.z = new ZipReader(bytes); } catch { throw new OfficeError('damaged', 'not a zip'); }
    const list = this.z.list();
    if (list.length > caps.entries) throw new OfficeError('bomb', 'too many parts');
    const declared = list.reduce((n, e) => n + e.size, 0);
    if (declared > caps.total) throw new OfficeError('bomb', 'unpacks too large');
  }
  has(name: string) { return this.z.has(name); }
  names() { return this.z.names(); }
  /** A part's bytes (null when there is no such part). What has been unpacked so far counts against
   *  the total, whatever the sizes written in the file claim. */
  async bytes(name: string): Promise<Uint8Array | null> {
    let b: Uint8Array | null;
    try { b = await this.z.bytes(name); }
    catch (e) { throw /too large/.test(String((e as Error)?.message)) ? new OfficeError('bomb', 'part too large') : new OfficeError('damaged', String((e as Error)?.message ?? e)); }
    if (b) { this.used += b.length; if (this.used > this.caps.total) throw new OfficeError('bomb', 'unpacks too large'); }
    return b;
  }
  async text(name: string): Promise<string | null> {
    const b = await this.bytes(name);
    return b ? new TextDecoder().decode(b) : null;
  }
}

/** The part a relationship points at: `target` relative to the folder of `from` (a part name like
 *  `ppt/slides/slide1.xml`), or from the root when it starts with `/`. `..` and `.` are resolved. */
export function resolvePart(from: string, target: string): string {
  const t = target.replace(/\\/g, '/');
  const base = t.startsWith('/') ? [] : from.split('/').slice(0, -1);
  for (const seg of t.split('/')) {
    if (!seg || seg === '.') continue;
    if (seg === '..') base.pop(); else base.push(seg);
  }
  return base.join('/');
}

/** Picture types a preview shows, by extension. Anything else (EMF, WMF, linked pictures) is skipped. */
export const IMG_TYPE: Record<string, string> = {
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', jpe: 'image/jpeg', gif: 'image/gif', bmp: 'image/bmp',
  webp: 'image/webp', tif: 'image/tiff', tiff: 'image/tiff', svg: 'image/svg+xml',
};
export const imgType = (path: string) => IMG_TYPE[path.split('.').pop()?.toLowerCase() ?? ''] ?? '';

/** Parse XML from inside a file. `application/xml` never fetches external entities or DTDs; the
 *  result is only ever read, never put into the page. */
export function parseXml(text: string): Document {
  const d = new DOMParser().parseFromString(text, 'application/xml');
  if (d.getElementsByTagName('parsererror').length) throw new OfficeError('damaged', 'unreadable');
  return d;
}

/** Relationships of a part (`word/document.xml` → `word/_rels/document.xml.rels`): id → part name or
 *  external address. */
export async function relsOf(z: OfficeZip, part: string): Promise<Map<string, { target: string; external: boolean; type: string }>> {
  const i = part.lastIndexOf('/');
  const relPath = `${part.slice(0, i + 1)}_rels/${part.slice(i + 1)}.rels`;
  const out = new Map<string, { target: string; external: boolean; type: string }>();
  const text = await z.text(relPath);
  if (!text) return out;
  for (const r of Array.from(parseXml(text).getElementsByTagName('Relationship'))) {
    const external = r.getAttribute('TargetMode') === 'External';
    const raw = r.getAttribute('Target') || '';
    out.set(r.getAttribute('Id') || '', { target: external ? raw : resolvePart(part, raw), external, type: (r.getAttribute('Type') || '').split('/').pop() || '' });
  }
  return out;
}

/** Pictures inside a file as object URLs (one per part), all revoked by the caller via `urls`. */
export class Pictures {
  urls: string[] = [];
  private byPart = new Map<string, string>();
  constructor(private z: OfficeZip) {}
  async url(part: string): Promise<string | null> {
    const type = imgType(part);
    if (!type) return null;
    const known = this.byPart.get(part);
    if (known) return known;
    const data = await this.z.bytes(part).catch((e) => { if (e instanceof OfficeError && e.problem === 'bomb') throw e; return null; });
    if (!data) return null;
    const url = URL.createObjectURL(new Blob([data as BlobPart], { type }));
    this.urls.push(url);
    this.byPart.set(part, url);
    return url;
  }
}
