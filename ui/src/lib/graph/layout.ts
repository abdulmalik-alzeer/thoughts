// The Graph page's layout: a small force simulation (repulsion between every pair of notes through a
// Barnes–Hut quadtree, a spring along every link, a gentle pull to the middle). It runs a fixed number
// of steps and stops, so the page never animates on its own; it is run in a Web Worker
// (layout.worker.ts) and the window only draws the settled result. Same input → same layout.

export type LayoutInput = {
  /** How many nodes. */
  n: number;
  /** Links as pairs of node indices: [from0, to0, from1, to1, …]. */
  edges: Int32Array | number[];
  /** Optional group of each node (an area): nodes of one group start next to each other. */
  groups?: Int32Array | number[];
  /** Simulation steps; by default fewer for very large graphs. */
  steps?: number;
};
export type LayoutResult = { x: Float32Array; y: Float32Array; ms: number };

const THETA2 = 0.81; // Barnes–Hut accuracy (θ = 0.9), as d3-force
const REPEL = -40; // many-body strength
const DIST_MIN2 = 1;
const LINK_DIST = 34;
const CENTER = 0.035;
const VELOCITY_KEEP = 0.6;

/** A deterministic tiny offset, for points that sit on top of each other. */
function jiggle(i: number): number { return ((Math.sin(i * 12.9898) * 43758.5453) % 1) * 1e-3 + 1e-6; }

export function defaultSteps(n: number): number { return n > 2500 ? 160 : n > 1000 ? 220 : 300; }

export function layout(inp: LayoutInput): LayoutResult {
  const t0 = typeof performance !== 'undefined' ? performance.now() : Date.now();
  const n = inp.n;
  const x = new Float64Array(n), y = new Float64Array(n), vx = new Float64Array(n), vy = new Float64Array(n);
  // Start on a sunflower spiral (as d3-force does), in group order so that an area starts together.
  const order = Array.from({ length: n }, (_, i) => i);
  const g = inp.groups;
  if (g) order.sort((a, b) => (g[a] - g[b]) || a - b);
  const angle = Math.PI * (3 - Math.sqrt(5));
  for (let k = 0; k < n; k++) {
    const i = order[k], r = 10 * Math.sqrt(0.5 + k), a = k * angle;
    x[i] = r * Math.cos(a); y[i] = r * Math.sin(a);
  }
  const m = inp.edges.length >> 1;
  const src = new Int32Array(m), dst = new Int32Array(m);
  const deg = new Float64Array(n);
  let mm = 0;
  for (let e = 0; e < m; e++) {
    const s = inp.edges[2 * e], d = inp.edges[2 * e + 1];
    if (s === d || s < 0 || d < 0 || s >= n || d >= n) continue;
    src[mm] = s; dst[mm] = d; mm++; deg[s]++; deg[d]++;
  }
  const bias = new Float64Array(mm), lstr = new Float64Array(mm);
  for (let e = 0; e < mm; e++) {
    const s = src[e], d = dst[e];
    bias[e] = deg[s] / (deg[s] + deg[d]);
    lstr[e] = 1 / Math.min(deg[s], deg[d]);
  }

  // Quadtree in flat arrays: per cell its box, mass, centre of mass, four children, the body it holds.
  const cap = Math.max(16, n * 4 + 16);
  let cells = cap;
  let cx0 = new Float64Array(cells), cy0 = new Float64Array(cells), csz = new Float64Array(cells);
  let cmass = new Float64Array(cells), cmx = new Float64Array(cells), cmy = new Float64Array(cells);
  let kids = new Int32Array(cells * 4), body = new Int32Array(cells);
  let used = 0;
  const grow = () => {
    const nc = cells * 2;
    const f = (a: Float64Array) => { const b = new Float64Array(nc); b.set(a); return b; };
    cx0 = f(cx0); cy0 = f(cy0); csz = f(csz); cmass = f(cmass); cmx = f(cmx); cmy = f(cmy);
    const k = new Int32Array(nc * 4); k.set(kids); kids = k;
    const b = new Int32Array(nc); b.set(body); body = b;
    cells = nc;
  };
  const cell = (x0: number, y0: number, size: number) => {
    if (used >= cells) grow();
    const c = used++;
    cx0[c] = x0; cy0[c] = y0; csz[c] = size; cmass[c] = 0; cmx[c] = 0; cmy[c] = 0; body[c] = -1;
    kids[c * 4] = kids[c * 4 + 1] = kids[c * 4 + 2] = kids[c * 4 + 3] = -1;
    return c;
  };
  const quad = (c: number, px: number, py: number) => {
    const h = csz[c] / 2;
    return (px >= cx0[c] + h ? 1 : 0) + (py >= cy0[c] + h ? 2 : 0);
  };
  const child = (c: number, q: number) => {
    let k = kids[c * 4 + q];
    if (k < 0) {
      const h = csz[c] / 2;
      k = cell(cx0[c] + (q & 1 ? h : 0), cy0[c] + (q & 2 ? h : 0), h);
      kids[c * 4 + q] = k;
    }
    return k;
  };
  // `body`: -1 an empty leaf, ≥ 0 the one node a leaf holds, -2 a cell split into children.
  const insert = (root: number, i: number) => {
    let c = root;
    for (let depth = 0; ; depth++) {
      if (body[c] === -1) { body[c] = i; return; }
      if (body[c] >= 0) {
        // Points on top of each other: the later one is left out of this step's tree (it still feels the others).
        if (depth > 48) return;
        const j = body[c];
        body[c] = -2;
        body[child(c, quad(c, x[j], y[j]))] = j;
      }
      c = child(c, quad(c, x[i], y[i]));
    }
  };
  // Masses and centres of mass, children before parents (cells are created parent-first).
  const accumulate = () => {
    for (let c = used - 1; c >= 0; c--) {
      if (body[c] >= 0) { cmass[c] = 1; cmx[c] = x[body[c]]; cmy[c] = y[body[c]]; continue; }
      if (body[c] === -1) { cmass[c] = 0; continue; }
      let ms = 0, sx = 0, sy = 0;
      for (let q = 0; q < 4; q++) { const k = kids[c * 4 + q]; if (k >= 0 && cmass[k] > 0) { ms += cmass[k]; sx += cmx[k] * cmass[k]; sy += cmy[k] * cmass[k]; } }
      cmass[c] = ms;
      if (ms > 0) { cmx[c] = sx / ms; cmy[c] = sy / ms; }
    }
  };
  const stack = new Int32Array(1024);

  const steps = inp.steps ?? defaultSteps(n);
  let alpha = 1;
  const alphaMin = 0.001, decay = 1 - Math.pow(alphaMin, 1 / steps);
  for (let step = 0; step < steps; step++) {
    alpha += (0 - alpha) * decay;
    // Links.
    for (let e = 0; e < mm; e++) {
      const s = src[e], d = dst[e];
      let dx = x[d] + vx[d] - x[s] - vx[s], dy = y[d] + vy[d] - y[s] - vy[s];
      if (dx === 0) dx = jiggle(e);
      if (dy === 0) dy = jiggle(e + 1);
      let l = Math.sqrt(dx * dx + dy * dy);
      l = ((l - LINK_DIST) / l) * alpha * lstr[e];
      dx *= l; dy *= l;
      const b = bias[e];
      vx[d] -= dx * b; vy[d] -= dy * b;
      vx[s] += dx * (1 - b); vy[s] += dy * (1 - b);
    }
    // Repulsion.
    if (n > 1) {
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (let i = 0; i < n; i++) { if (x[i] < minX) minX = x[i]; if (x[i] > maxX) maxX = x[i]; if (y[i] < minY) minY = y[i]; if (y[i] > maxY) maxY = y[i]; }
      const size = Math.max(maxX - minX, maxY - minY, 1) * 1.0001 + 1;
      used = 0;
      const root = cell(minX, minY, size);
      for (let i = 0; i < n; i++) insert(root, i);
      accumulate();
      for (let i = 0; i < n; i++) {
        let sp = 0;
        stack[sp++] = root;
        const xi = x[i], yi = y[i];
        let fx = 0, fy = 0;
        while (sp > 0) {
          const c = stack[--sp];
          if (cmass[c] === 0 || body[c] === i) continue;
          let dx = cmx[c] - xi, dy = cmy[c] - yi;
          let l = dx * dx + dy * dy;
          const leaf = body[c] >= 0;
          if (leaf || (csz[c] * csz[c]) / THETA2 < l) {
            if (dx === 0) { dx = jiggle(i); l += dx * dx; }
            if (dy === 0) { dy = jiggle(i + 7); l += dy * dy; }
            if (l < DIST_MIN2) l = Math.sqrt(DIST_MIN2 * l);
            const w = (REPEL * alpha * cmass[c]) / l;
            fx += dx * w; fy += dy * w;
            continue;
          }
          for (let q = 0; q < 4; q++) { const k = kids[c * 4 + q]; if (k >= 0) { if (sp >= stack.length) break; stack[sp++] = k; } }
        }
        vx[i] += fx; vy[i] += fy;
      }
    }
    // Towards the middle, so that loose groups stay in view.
    for (let i = 0; i < n; i++) { vx[i] -= x[i] * CENTER * alpha; vy[i] -= y[i] * CENTER * alpha; }
    for (let i = 0; i < n; i++) { vx[i] *= VELOCITY_KEEP; vy[i] *= VELOCITY_KEEP; x[i] += vx[i]; y[i] += vy[i]; }
  }
  const t1 = typeof performance !== 'undefined' ? performance.now() : Date.now();
  return { x: Float32Array.from(x), y: Float32Array.from(y), ms: t1 - t0 };
}
