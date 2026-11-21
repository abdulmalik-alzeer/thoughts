import { describe, expect, it } from 'vitest';
import { layout } from './layout';

/** A made-up library: `n` notes in `areas` groups, `m` links mostly inside a group. */
function made(n: number, m: number, areas = 12) {
  let seed = 7;
  const rnd = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
  const groups = Int32Array.from({ length: n }, (_, i) => i % areas);
  const edges = new Int32Array(m * 2);
  for (let e = 0; e < m; e++) {
    const s = Math.floor(rnd() * n);
    let d = rnd() < 0.8 ? (Math.floor(rnd() * (n / areas)) * areas + (s % areas)) % n : Math.floor(rnd() * n);
    if (d === s) d = (s + areas) % n;
    edges[2 * e] = s; edges[2 * e + 1] = d;
  }
  return { n, edges, groups };
}

const dist = (r: { x: Float32Array; y: Float32Array }, a: number, b: number) => Math.hypot(r.x[a] - r.x[b], r.y[a] - r.y[b]);

describe('graph layout', () => {
  it('lays out 3,000 notes and 6,000 links in good time, every position a number', () => {
    const g = made(3000, 6000);
    const t = performance.now();
    const r = layout(g);
    const ms = performance.now() - t;
    expect(r.x.length).toBe(3000);
    for (let i = 0; i < 3000; i++) { expect(Number.isFinite(r.x[i])).toBe(true); expect(Number.isFinite(r.y[i])).toBe(true); }
    // Off the main thread in the app. It takes well under a second here when the machine is idle;
    // the bound leaves room for a machine busy with other builds (it once failed at 6 s under such
    // load) and still catches a layout that stops settling.
    expect(ms).toBeLessThan(15000);
  }, 60000);

  it('pulls linked notes together and pushes unlinked ones apart', () => {
    // Two triangles, joined by nothing.
    const r = layout({ n: 6, edges: [0, 1, 1, 2, 2, 0, 3, 4, 4, 5, 5, 3] });
    const inside = Math.max(dist(r, 0, 1), dist(r, 1, 2), dist(r, 3, 4));
    const across = Math.min(dist(r, 0, 3), dist(r, 1, 4), dist(r, 2, 5));
    expect(inside).toBeLessThan(across);
  });

  it('is the same every time and copes with odd input', () => {
    const g = made(200, 400);
    expect(Array.from(layout(g).x)).toEqual(Array.from(layout(g).x));
    expect(layout({ n: 0, edges: [] }).x.length).toBe(0);
    const one = layout({ n: 1, edges: [0, 0, 0, 5, -1, 0] });
    expect(Number.isFinite(one.x[0])).toBe(true);
    // Several links between the same two notes, and notes that start on top of each other.
    const dup = layout({ n: 3, edges: [0, 1, 0, 1, 1, 0], steps: 50 });
    expect(dist(dup, 0, 1)).toBeGreaterThan(1);
  });
});
