// Lays out the Graph page away from the window (layout.ts does the work), so a library of a few
// thousand notes never freezes the app while it is arranged.
import { layout, type LayoutInput } from './layout';

self.onmessage = (ev: MessageEvent) => {
  const m = ev.data as { id: number } & LayoutInput;
  try {
    const r = layout(m);
    (self as unknown as Worker).postMessage({ id: m.id, ok: true, x: r.x, y: r.y, ms: r.ms }, [r.x.buffer, r.y.buffer]);
  } catch (e) {
    (self as unknown as Worker).postMessage({ id: m.id, ok: false, error: String(e) });
  }
};
