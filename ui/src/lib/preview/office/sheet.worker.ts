// Reads a spreadsheet away from the window (sheet.ts does the work), so opening a big one never
// freezes the app. One worker per open spreadsheet; it is ended when the preview closes.
import { openBook, type Book } from './sheet';
import { problemOf } from './safezip';

let book: Book | null = null;

const fail = (id: number, e: unknown) => (self as unknown as Worker).postMessage({ id, ok: false, problem: problemOf(e) });

self.onmessage = async (ev: MessageEvent) => {
  const m = ev.data as { id: number; op: 'open'; kind: 'xlsx' | 'ods' | 'csv'; name: string; buf: ArrayBuffer } | { id: number; op: 'sheet'; index: number };
  try {
    if (m.op === 'open') {
      book = await openBook(m.kind, new Uint8Array(m.buf), m.name);
      (self as unknown as Worker).postMessage({ id: m.id, ok: true, sheets: book.sheets });
    } else {
      if (!book) throw new Error('not open');
      (self as unknown as Worker).postMessage({ id: m.id, ok: true, sheet: await book.sheet(m.index) });
    }
  } catch (e) { fail(m.id, e); }
};
