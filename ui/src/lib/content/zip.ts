// Minimal ZIP reading and writing for Word documents (a .docx is a ZIP of XML files). Compression
// uses the browser's own `CompressionStream` / `DecompressionStream` ("deflate-raw"), so no library
// is needed; where the writer has no compression it stores the files as they are, which Word reads too.

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();
export function crc32(b: Uint8Array): number {
  let c = 0xffffffff;
  for (let i = 0; i < b.length; i++) c = CRC_TABLE[(c ^ b[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

async function through(data: Uint8Array, stream: { readable: ReadableStream<Uint8Array>; writable: WritableStream<Uint8Array> }, limit = Infinity): Promise<Uint8Array> {
  const writer = stream.writable.getWriter();
  writer.write(data).catch(() => {});
  writer.close().catch(() => {});
  const reader = stream.readable.getReader();
  const parts: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.length;
    if (size > limit) { reader.cancel().catch(() => {}); throw new Error('too large'); }
    parts.push(value);
  }
  const out = new Uint8Array(size);
  let at = 0;
  for (const p of parts) { out.set(p, at); at += p.length; }
  return out;
}

const canDeflate = () => typeof (globalThis as any).CompressionStream === 'function';

export type ZipEntry = { name: string; data: Uint8Array | string };

/** A ZIP file holding `entries` (names in UTF-8). */
export async function zip(entries: ZipEntry[]): Promise<Blob> {
  const enc = new TextEncoder();
  const now = new Date();
  const time = (now.getHours() << 11) | (now.getMinutes() << 5) | (now.getSeconds() >> 1);
  const date = ((now.getFullYear() - 1980) << 9) | ((now.getMonth() + 1) << 5) | now.getDate();
  const chunks: Uint8Array[] = [];
  const central: Uint8Array[] = [];
  let offset = 0;
  for (const e of entries) {
    const raw = typeof e.data === 'string' ? enc.encode(e.data) : e.data;
    const name = enc.encode(e.name);
    let body = raw, method = 0;
    // Pictures are already compressed; text shrinks a lot.
    if (canDeflate() && raw.length > 256 && !/\.(png|jpe?g|gif|webp)$/i.test(e.name)) {
      try {
        const d = await through(raw, new (globalThis as any).CompressionStream('deflate-raw'));
        if (d.length < raw.length) { body = d; method = 8; }
      } catch { /* store it */ }
    }
    const crc = crc32(raw);
    const local = new DataView(new ArrayBuffer(30));
    local.setUint32(0, 0x04034b50, true); local.setUint16(4, 20, true); local.setUint16(6, 0x0800, true);
    local.setUint16(8, method, true); local.setUint16(10, time, true); local.setUint16(12, date, true);
    local.setUint32(14, crc, true); local.setUint32(18, body.length, true); local.setUint32(22, raw.length, true);
    local.setUint16(26, name.length, true); local.setUint16(28, 0, true);
    chunks.push(new Uint8Array(local.buffer), name, body);
    const cd = new DataView(new ArrayBuffer(46));
    cd.setUint32(0, 0x02014b50, true); cd.setUint16(4, 20, true); cd.setUint16(6, 20, true); cd.setUint16(8, 0x0800, true);
    cd.setUint16(10, method, true); cd.setUint16(12, time, true); cd.setUint16(14, date, true);
    cd.setUint32(16, crc, true); cd.setUint32(20, body.length, true); cd.setUint32(24, raw.length, true);
    cd.setUint16(28, name.length, true); cd.setUint32(42, offset, true);
    central.push(new Uint8Array(cd.buffer), name);
    offset += 30 + name.length + body.length;
  }
  const cdSize = central.reduce((n, c) => n + c.length, 0);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true); end.setUint16(8, entries.length, true); end.setUint16(10, entries.length, true);
  end.setUint32(12, cdSize, true); end.setUint32(16, offset, true);
  return new Blob([...chunks, ...central, new Uint8Array(end.buffer)] as BlobPart[], { type: 'application/zip' });
}

/** The files in a ZIP, read on demand. Throws on anything that is not a readable ZIP. */
export class ZipReader {
  private entries = new Map<string, { method: number; size: number; csize: number; offset: number }>();
  /** Most a single file inside may unpack to (protects against "zip bombs"). */
  static LIMIT = 64 * 1024 * 1024;
  constructor(private buf: Uint8Array) {
    const v = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    let eocd = -1;
    for (let i = buf.length - 22; i >= Math.max(0, buf.length - 22 - 65535); i--) if (v.getUint32(i, true) === 0x06054b50) { eocd = i; break; }
    if (eocd < 0) throw new Error('not a zip');
    const count = v.getUint16(eocd + 10, true);
    let p = v.getUint32(eocd + 16, true);
    const dec = new TextDecoder();
    for (let n = 0; n < count; n++) {
      if (p + 46 > buf.length || v.getUint32(p, true) !== 0x02014b50) throw new Error('broken zip');
      const method = v.getUint16(p + 10, true), csize = v.getUint32(p + 20, true), size = v.getUint32(p + 24, true);
      const nl = v.getUint16(p + 28, true), xl = v.getUint16(p + 30, true), cl = v.getUint16(p + 32, true), off = v.getUint32(p + 42, true);
      const name = dec.decode(buf.subarray(p + 46, p + 46 + nl));
      this.entries.set(name.replace(/\\/g, '/'), { method, size, csize, offset: off });
      p += 46 + nl + xl + cl;
    }
  }
  has(name: string) { return this.entries.has(name); }
  names() { return [...this.entries.keys()]; }
  async bytes(name: string): Promise<Uint8Array | null> {
    const e = this.entries.get(name);
    if (!e) return null;
    if (e.size > ZipReader.LIMIT) throw new Error('too large');
    const v = new DataView(this.buf.buffer, this.buf.byteOffset, this.buf.byteLength);
    if (e.offset + 30 > this.buf.length || v.getUint32(e.offset, true) !== 0x04034b50) throw new Error('broken zip');
    const start = e.offset + 30 + v.getUint16(e.offset + 26, true) + v.getUint16(e.offset + 28, true);
    const body = this.buf.subarray(start, start + e.csize);
    if (e.method === 0) return body;
    if (e.method !== 8 || typeof (globalThis as any).DecompressionStream !== 'function') throw new Error('unsupported');
    return through(body, new (globalThis as any).DecompressionStream('deflate-raw'), ZipReader.LIMIT);
  }
  async text(name: string): Promise<string | null> {
    const b = await this.bytes(name);
    return b ? new TextDecoder().decode(b) : null;
  }
}
