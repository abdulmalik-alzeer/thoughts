// The window's side of laying out the Graph page: in a Web Worker (layout.worker.ts) where one can
// start, otherwise right here (the same code, on the window's own time).
import { layout, type LayoutInput, type LayoutResult } from './layout';

let n = 0;

/** Lay out a graph; a newer call does not cancel an older one, the caller keeps the latest. */
export function runLayout(inp: LayoutInput): Promise<LayoutResult> {
  if (typeof Worker !== 'function') return Promise.resolve(layout(inp));
  return new Promise<LayoutResult>((ok, no) => {
    let w: Worker;
    try { w = new Worker(new URL('./layout.worker.ts', import.meta.url), { type: 'module' }); }
    catch { ok(layout(inp)); return; }
    const id = ++n;
    const done = () => w.terminate();
    w.onmessage = (ev) => {
      const m = ev.data as { id: number; ok: boolean; x?: Float32Array; y?: Float32Array; ms?: number; error?: string };
      if (m.id !== id) return;
      done();
      if (m.ok && m.x && m.y) ok({ x: m.x, y: m.y, ms: m.ms ?? 0 }); else no(new Error(m.error || 'layout failed'));
    };
    // The worker could not run (blocked, failed to load): the same layout, here.
    w.onerror = (ev) => { ev.preventDefault(); done(); try { ok(layout(inp)); } catch (e) { no(e); } };
    const edges = Int32Array.from(inp.edges);
    const groups = inp.groups ? Int32Array.from(inp.groups) : undefined;
    w.postMessage({ id, n: inp.n, edges, groups, steps: inp.steps }, groups ? [edges.buffer, groups.buffer] : [edges.buffer]);
  });
}
