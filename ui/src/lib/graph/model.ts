// The Graph page's data: what the service sends (`GET /api/library/graph`) turned into indices the
// canvas and the list can use, plus the plain geometry the page needs (fitting, finding the note
// under the pointer, moving to the next note with the arrow keys). No drawing here.

export type GNode = { path: string; title: string; summary: string; area: string; status: string; kind: 'item' | 'note' };
export type GEdge = { from: string; to: string; count: number };
export type GraphData = { nodes: GNode[]; edges: GEdge[]; truncated?: boolean; total?: { nodes: number; edges: number }; focus?: string | null };

export type Node = GNode & { i: number; label: string; group: number; deg: number; r: number };
export type Model = {
  nodes: Node[];
  /** Links as index pairs, with how many links each stands for. */
  edges: { s: number; t: number; count: number }[];
  /** For each node: the nodes it links to, and the nodes that link to it. */
  out: number[][];
  inn: number[][];
  /** Areas, most notes first: index = `Node.group`. */
  groups: { area: string; count: number }[];
  byPath: Map<string, number>;
};

/** A node's radius on the map (world units): items a little larger, well-linked notes larger still. */
export const radius = (kind: string, deg: number) => Math.min(14, (kind === 'item' ? 4.5 : 3) + Math.sqrt(deg) * 1.1);

export function buildModel(data: GraphData, labelOf: (n: GNode) => string): Model {
  const raw = Array.isArray(data?.nodes) ? data.nodes.filter((n) => n && typeof n.path === 'string') : [];
  const byPath = new Map<string, number>();
  raw.forEach((n, i) => byPath.set(n.path, i));
  const out: number[][] = raw.map(() => []), inn: number[][] = raw.map(() => []);
  const edges: Model['edges'] = [];
  for (const e of Array.isArray(data?.edges) ? data.edges : []) {
    const s = byPath.get(e?.from), t = byPath.get(e?.to);
    if (s === undefined || t === undefined || s === t) continue;
    edges.push({ s, t, count: Math.max(1, Number(e.count) || 1) });
    out[s].push(t); inn[t].push(s);
  }
  const counts = new Map<string, number>();
  for (const n of raw) counts.set(n.area ?? '', (counts.get(n.area ?? '') ?? 0) + 1);
  const groups = [...counts].map(([area, count]) => ({ area, count }))
    .sort((a, b) => (Number(a.area === '') - Number(b.area === '')) || b.count - a.count || a.area.localeCompare(b.area));
  const gi = new Map(groups.map((g, i) => [g.area, i]));
  const nodes: Node[] = raw.map((n, i) => {
    const deg = new Set([...out[i], ...inn[i]]).size;
    return { ...n, title: String(n.title ?? ''), summary: String(n.summary ?? ''), status: String(n.status ?? ''), area: String(n.area ?? ''),
      kind: n.kind === 'item' ? 'item' : 'note', i, label: labelOf(n), group: gi.get(n.area ?? '') ?? 0, deg, r: radius(n.kind, deg) };
  });
  return { nodes, edges, out, inn, groups, byPath };
}

/** The nodes whose title, summary or path holds every word of `query` (`fold` evens out case and accents). */
export function search(m: Model, query: string, fold: (s: string) => string, shown?: (i: number) => boolean): number[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const hits: number[] = [];
  for (const n of m.nodes) {
    if (shown && !shown(n.i)) continue;
    const hay = fold(`${n.label} ${n.title} ${n.summary} ${n.path}`);
    if (words.every((w) => hay.includes(w))) hits.push(n.i);
  }
  // Titles that start with the query first.
  const q = words.join(' ');
  return hits.sort((a, b) => Number(!fold(m.nodes[a].label).startsWith(q)) - Number(!fold(m.nodes[b].label).startsWith(q)) || m.nodes[b].deg - m.nodes[a].deg);
}

export type Camera = { k: number; x: number; y: number };
export const MIN_ZOOM = 0.05, MAX_ZOOM = 8;
export const clampZoom = (k: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, k));

/** The view that shows all of `which` in a `w`×`h` box with some room around. */
export function fit(xs: ArrayLike<number>, ys: ArrayLike<number>, which: number[], w: number, h: number, pad = 48): Camera {
  if (!which.length || w <= 0 || h <= 0) return { k: 1, x: w / 2, y: h / 2 };
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const i of which) { x0 = Math.min(x0, xs[i]); x1 = Math.max(x1, xs[i]); y0 = Math.min(y0, ys[i]); y1 = Math.max(y1, ys[i]); }
  const k = clampZoom(Math.min(2, (w - pad * 2) / Math.max(x1 - x0, 1), (h - pad * 2) / Math.max(y1 - y0, 1)));
  return { k, x: w / 2 - ((x0 + x1) / 2) * k, y: h / 2 - ((y0 + y1) / 2) * k };
}

/** The node under screen point (px, py), or -1. Small nodes are given a few pixels of slack. */
export function hit(m: Model, xs: ArrayLike<number>, ys: ArrayLike<number>, cam: Camera, px: number, py: number, shown: (i: number) => boolean): number {
  const wx = (px - cam.x) / cam.k, wy = (py - cam.y) / cam.k;
  let best = -1, bestD = Infinity;
  for (const n of m.nodes) {
    if (!shown(n.i)) continue;
    const d = Math.hypot(xs[n.i] - wx, ys[n.i] - wy);
    const reach = n.r + 6 / cam.k;
    if (d <= reach && d < bestD) { best = n.i; bestD = d; }
  }
  return best;
}

/** The next node from `from` in a direction (screen directions: y grows downwards): the nearest one
 *  roughly that way, its own connections preferred when they are about as near. -1 when none. */
export function step(m: Model, xs: ArrayLike<number>, ys: ArrayLike<number>, from: number, dir: 'left' | 'right' | 'up' | 'down', shown: (i: number) => boolean): number {
  const [ux, uy] = dir === 'left' ? [-1, 0] : dir === 'right' ? [1, 0] : dir === 'up' ? [0, -1] : [0, 1];
  const near = new Set([...(m.out[from] ?? []), ...(m.inn[from] ?? [])]);
  let best = -1, score = Infinity;
  for (const n of m.nodes) {
    if (n.i === from || !shown(n.i)) continue;
    const dx = xs[n.i] - xs[from], dy = ys[n.i] - ys[from];
    const along = dx * ux + dy * uy, across = Math.abs(dx * uy - dy * ux);
    if (along <= 0 || across > along * 2) continue;
    const s = (along + across * 2) * (near.has(n.i) ? 0.6 : 1);
    if (s < score) { score = s; best = n.i; }
  }
  return best;
}
