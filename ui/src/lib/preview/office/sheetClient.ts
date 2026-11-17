// The window's side of reading a spreadsheet: in a Web Worker (sheet.worker.ts) where one can start,
// otherwise right here (the same code, just on the window's own time).
import { openBook, type Book, type SheetData, type SheetInfo } from './sheet';
import { OfficeError, problemOf, type OfficeProblem } from './safezip';

export type SheetKind = 'xlsx' | 'ods' | 'csv';

export interface SheetClient {
  open(kind: SheetKind, data: Blob, name: string): Promise<SheetInfo[]>;
  sheet(i: number): Promise<SheetData>;
  close(): void;
}

class WorkerClient implements SheetClient {
  private w: Worker;
  private n = 0;
  private waiting = new Map<number, { ok: (v: any) => void; no: (e: unknown) => void }>();
  /** Set when the worker itself failed (could not load): the caller falls back to the window. */
  broken = false;
  constructor() {
    this.w = new Worker(new URL('./sheet.worker.ts', import.meta.url), { type: 'module' });
    this.w.onmessage = (ev) => {
      const m = ev.data as { id: number; ok: boolean; problem?: OfficeProblem; sheets?: SheetInfo[]; sheet?: SheetData };
      const p = this.waiting.get(m.id);
      if (!p) return;
      this.waiting.delete(m.id);
      if (m.ok) p.ok(m.sheets ?? m.sheet); else p.no(new OfficeError(m.problem ?? 'damaged'));
    };
    this.w.onerror = (ev) => {
      ev.preventDefault();
      this.broken = true;
      for (const p of this.waiting.values()) p.no(new Error('worker failed'));
      this.waiting.clear();
    };
  }
  private ask<T>(msg: Record<string, unknown>, transfer: Transferable[] = []): Promise<T> {
    const id = ++this.n;
    return new Promise<T>((ok, no) => { this.waiting.set(id, { ok, no }); this.w.postMessage({ ...msg, id }, transfer); });
  }
  async open(kind: SheetKind, data: Blob, name: string) {
    const buf = await data.arrayBuffer();
    return this.ask<SheetInfo[]>({ op: 'open', kind, name, buf }, [buf]);
  }
  sheet(index: number) { return this.ask<SheetData>({ op: 'sheet', index }); }
  close() { this.w.terminate(); this.waiting.clear(); }
}

class InlineClient implements SheetClient {
  private book: Book | null = null;
  async open(kind: SheetKind, data: Blob, name: string) {
    try { this.book = await openBook(kind, new Uint8Array(await data.arrayBuffer()), name); }
    catch (e) { throw e instanceof OfficeError ? e : new OfficeError(problemOf(e)); }
    return this.book.sheets;
  }
  async sheet(i: number) {
    if (!this.book) throw new OfficeError('damaged');
    try { return await this.book.sheet(i); } catch (e) { throw e instanceof OfficeError ? e : new OfficeError(problemOf(e)); }
  }
  close() { this.book = null; }
}

/** Open a spreadsheet; resolves to a client for its sheets and the list of sheets. */
export async function openSheets(kind: SheetKind, data: Blob, name: string): Promise<{ client: SheetClient; sheets: SheetInfo[] }> {
  if (typeof Worker === 'function') {
    let w: WorkerClient | null = null;
    try {
      w = new WorkerClient();
      return { client: w, sheets: await w.open(kind, data, name) };
    } catch (e) {
      w?.close();
      // A real answer about the file stands; only a worker that could not run falls back.
      if (e instanceof OfficeError) throw e;
    }
  }
  const c = new InlineClient();
  return { client: c, sheets: await c.open(kind, data, name) };
}
