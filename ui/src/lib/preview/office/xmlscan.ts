// A small, forward-only XML reader for spreadsheets, which are parsed in a Web Worker where the
// browser's DOMParser does not exist. It reads tags, attributes and text, and nothing else: document
// type declarations are skipped unread, so no entity is ever defined, expanded or fetched; only the
// five built-in entities and numeric character references are decoded.

export type Attrs = Record<string, string>;

/** Return `false` from a callback to stop reading. */
export interface ScanHandler {
  open?(name: string, attrs: Attrs, selfClosing: boolean): void | boolean;
  close?(name: string): void | boolean;
  text?(text: string): void | boolean;
}

const ENT: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" };

/** Decode the built-in entities and character references; anything else is left as written. */
export function decodeXml(s: string): string {
  if (s.indexOf('&') < 0) return s;
  return s.replace(/&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);/g, (m, e: string) => {
    if (e[0] === '#') {
      const n = e[1] === 'x' || e[1] === 'X' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
      return n > 0 && n <= 0x10ffff && !(n >= 0xd800 && n <= 0xdfff) ? String.fromCodePoint(n) : '';
    }
    return ENT[e] ?? m;
  });
}

/** `table:table-cell` → `table-cell`. */
export const local = (qname: string) => { const i = qname.indexOf(':'); return i < 0 ? qname : qname.slice(i + 1); };

/** An attribute by its local name, whatever prefix the file used (`r`, `table:number-columns-repeated`…). */
export function attr(a: Attrs, name: string): string | undefined {
  if (name in a) return a[name];
  for (const k in a) if (local(k) === name) return a[k];
  return undefined;
}

const ATTR = /([^\s=/>]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g;

/** Read `xml` from `from` on, calling the handler for each tag (by local name) and each piece of text. */
export function scanXml(xml: string, h: ScanHandler, from = 0): void {
  let i = from;
  const n = xml.length;
  while (i < n) {
    const lt = xml.indexOf('<', i);
    const end = lt < 0 ? n : lt;
    if (end > i && h.text && h.text(decodeXml(xml.slice(i, end))) === false) return;
    if (lt < 0) return;
    const c = xml.charCodeAt(lt + 1);
    if (c === 63 /* ? */) { const e = xml.indexOf('?>', lt + 2); i = e < 0 ? n : e + 2; continue; }
    if (c === 33 /* ! */) {
      if (xml.startsWith('<!--', lt)) { const e = xml.indexOf('-->', lt + 4); i = e < 0 ? n : e + 3; continue; }
      if (xml.startsWith('<![CDATA[', lt)) {
        const e = xml.indexOf(']]>', lt + 9);
        const stop = e < 0 ? n : e;
        if (h.text && stop > lt + 9 && h.text(xml.slice(lt + 9, stop)) === false) return;
        i = e < 0 ? n : e + 3; continue;
      }
      // <!DOCTYPE …> with an optional [internal subset]: skipped, never interpreted.
      let j = lt + 2, depth = 0;
      for (; j < n; j++) { const ch = xml[j]; if (ch === '[') depth++; else if (ch === ']') depth--; else if (ch === '>' && depth <= 0) break; }
      i = j + 1; continue;
    }
    if (c === 47 /* / */) {
      const e = xml.indexOf('>', lt + 2);
      if (e < 0) return;
      if (h.close && h.close(local(xml.slice(lt + 2, e).trim())) === false) return;
      i = e + 1; continue;
    }
    // A start tag: find its end, skipping '>' inside quoted attribute values.
    let j = lt + 1, q = 0;
    for (; j < n; j++) {
      const ch = xml.charCodeAt(j);
      if (q) { if (ch === q) q = 0; } else if (ch === 34 || ch === 39) q = ch; else if (ch === 62) break;
    }
    if (j >= n) return;
    const self = xml.charCodeAt(j - 1) === 47;
    const body = xml.slice(lt + 1, self ? j - 1 : j);
    let k = 0;
    while (k < body.length && !/\s/.test(body[k])) k++;
    const name = local(body.slice(0, k));
    const attrs: Attrs = {};
    if (k < body.length) {
      ATTR.lastIndex = k;
      for (let m = ATTR.exec(body); m; m = ATTR.exec(body)) attrs[m[1]] = decodeXml(m[2] ?? m[3] ?? '');
    }
    if (h.open && h.open(name, attrs, self) === false) return;
    if (self && h.close && h.close(name) === false) return;
    i = j + 1;
  }
}
