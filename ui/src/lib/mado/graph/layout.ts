// Where things go. Nodes that have a saved place keep it; the rest are arranged with dagre (left to
// right for flows and data models, top to bottom for the architecture), and the tasks diagram is set in
// columns, one per round of the build. "Tidy up" arranges everything again.
import dagre from '@dagrejs/dagre';
import type { GraphKind, Pos } from '../types';
import type { VNode, View } from './model';

export interface Size { w: number; h: number }
export interface Band { index: number; x: number; y: number; w: number; h: number }
export interface Layout { pos: Record<string, Pos>; size: Record<string, Size>; bands: Band[]; columns: boolean }

export const ERD_ROWS_SHOWN = 12;
const ERD_HEAD = 42, ERD_ROW = 26, ERD_W = 252;
const TASK: Size = { w: 240, h: 96 };
const COL_GAP = 96, ROW_GAP = 26, LANE_GAP = 18, BAND_PAD = 18, BAND_HEAD = 38;

/** The drawn size of a node. Sizes are fixed per look so the layout never waits for the page to measure. */
export function sizeOf(graph: GraphKind, n: VNode): Size {
  if (graph === 'erd') {
    const rows = Math.min(n.fields.length, ERD_ROWS_SHOWN) + (n.fields.length > ERD_ROWS_SHOWN ? 1 : 0);
    return { w: ERD_W, h: ERD_HEAD + Math.max(1, rows) * ERD_ROW + 8 };
  }
  if (graph === 'tasks') return TASK;
  if (graph === 'architecture') return n.look === 'queue' ? { w: 216, h: 68 } : n.look === 'store' ? { w: 204, h: 92 } : { w: 216, h: 80 };
  switch (n.look) {
    case 'screen': return { w: 208, h: 84 };
    case 'decision': return { w: 176, h: 116 };
    case 'action': return { w: 196, h: 58 };
    case 'start': case 'end': return { w: 140, h: 50 };
    default: return { w: 196, h: 64 };
  }
}

/** Left-to-right diagrams read in columns. */
export const columnsFor = (graph: GraphKind) => graph !== 'architecture';

const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : typeof v === 'string' && /^\d{1,4}$/.test(v) ? Number(v) : null);

/** The round each task is built in: what the file says (`props.wave`), or how deep it sits behind what it waits for. */
export function wavesOf(view: View): Map<string, number> {
  const given = view.nodes.map((n) => num(n.props.wave));
  const out = new Map<string, number>();
  if (view.nodes.length && given.every((g) => g !== null)) {
    const order = [...new Set(given as number[])].sort((a, b) => a - b);
    view.nodes.forEach((n, i) => out.set(n.key, order.indexOf(given[i] as number)));
    return out;
  }
  // Depth along the edges, with the edges that close a loop left out (a loop in a plan is reported by the
  // review; the drawing just carries on).
  const next = new Map<string, string[]>();
  const fed = new Set<string>();
  for (const e of view.edges) if (e.from !== e.to) { next.set(e.from, [...(next.get(e.from) ?? []), e.to]); fed.add(e.to); }
  const before = new Map<string, string[]>();
  const state = new Map<string, 1 | 2>(); // 1: on the path being walked, 2: done
  const walk = (k: string) => {
    state.set(k, 1);
    for (const to of next.get(k) ?? []) {
      const s = state.get(to);
      if (s === 1) continue; // closes a loop
      before.set(to, [...(before.get(to) ?? []), k]);
      if (!s) walk(to);
    }
    state.set(k, 2);
  };
  for (const n of view.nodes) if (!fed.has(n.key) && !state.has(n.key)) walk(n.key);
  for (const n of view.nodes) if (!state.has(n.key)) walk(n.key);
  const depth = (k: string): number => {
    const known = out.get(k);
    if (known !== undefined) return known;
    let d = 0;
    for (const p of before.get(k) ?? []) d = Math.max(d, depth(p) + 1);
    out.set(k, d);
    return d;
  };
  for (const n of view.nodes) depth(n.key);
  return out;
}

function taskColumns(view: View): { pos: Record<string, Pos>; bands: Band[] } {
  const waves = wavesOf(view);
  const cols: VNode[][] = Array.from({ length: Math.max(0, ...waves.values()) + 1 }, () => []);
  for (const n of view.nodes) cols[waves.get(n.key) ?? 0].push(n);
  const pos: Record<string, Pos> = {};
  const bands: Band[] = [];
  const lane = (n: VNode) => String(n.props.lane ?? '');
  for (let c = 0; c < cols.length; c++) {
    const col = (cols[c] ?? []).slice().sort((a, b) => lane(a).localeCompare(lane(b), 'en', { numeric: true }));
    const x = c * (TASK.w + COL_GAP);
    let y = 0;
    col.forEach((n, i) => {
      if (i) y += ROW_GAP + (lane(n) !== lane(col[i - 1]) ? LANE_GAP : 0);
      pos[n.key] = { x, y };
      y += TASK.h;
    });
    if (col.length) bands.push({ index: c, x: x - BAND_PAD, y: -BAND_PAD - BAND_HEAD, w: TASK.w + BAND_PAD * 2, h: y + BAND_PAD * 2 + BAND_HEAD });
  }
  const tall = Math.max(0, ...bands.map((b) => b.h));
  for (const b of bands) b.h = tall;
  return { pos, bands };
}

/** Above this, dagre takes seconds (its cost grows with every long edge): a quicker arrangement is used instead. */
export const DAGRE_MAX = { nodes: 120, edges: 300 };

/**
 * A quick layered arrangement for large diagrams: each node sits one layer after the deepest node that leads to
 * it, layers are ordered so linked nodes sit near each other, and nothing overlaps. Not as fine as dagre, but
 * it takes milliseconds where dagre takes seconds.
 */
export function quickLayers(view: View, size: Record<string, Size>, columns: boolean): Record<string, Pos> {
  // The layer of a node is its distance from where the diagram starts (the shortest way there), which keeps a
  // tangled diagram to a few layers instead of one long ribbon.
  const next = new Map<string, string[]>();
  const fed = new Set<string>();
  for (const e of view.edges) if (e.from !== e.to && size[e.from] && size[e.to]) { next.set(e.from, [...(next.get(e.from) ?? []), e.to]); fed.add(e.to); }
  const rank = new Map<string, number>();
  const spread = (starts: string[]) => {
    let edge = starts.filter((k) => !rank.has(k));
    for (const k of edge) rank.set(k, 0);
    for (let d = 1; edge.length; d++) {
      const far: string[] = [];
      for (const k of edge) for (const to of next.get(k) ?? []) if (!rank.has(to)) { rank.set(to, d); far.push(to); }
      edge = far;
    }
  };
  spread(view.nodes.filter((n) => !fed.has(n.key)).map((n) => n.key));
  for (const n of view.nodes) if (!rank.has(n.key)) spread([n.key]);
  const deep: string[][] = Array.from({ length: Math.max(0, ...rank.values()) + 1 }, () => []);
  for (const n of view.nodes) deep[rank.get(n.key) ?? 0].push(n.key);
  const around = new Map<string, string[]>();
  for (const e of view.edges) {
    if (e.from === e.to || !size[e.from] || !size[e.to]) continue;
    around.set(e.from, [...(around.get(e.from) ?? []), e.to]);
    around.set(e.to, [...(around.get(e.to) ?? []), e.from]);
  }
  // Order each layer by where its neighbours sit, a few passes back and forth.
  const slot = new Map<string, number>();
  for (const l of deep) l.forEach((k, i) => slot.set(k, i));
  const sweep = (l: string[]) => {
    const mean = new Map(l.map((k) => {
      const ns = (around.get(k) ?? []).filter((o) => rank.get(o) !== rank.get(k));
      return [k, ns.length ? ns.reduce((a, o) => a + (slot.get(o) ?? 0) / Math.max(1, deep[rank.get(o) ?? 0].length), 0) / ns.length : (slot.get(k) ?? 0) / Math.max(1, l.length)];
    }));
    l.sort((a, b) => (mean.get(a) ?? 0) - (mean.get(b) ?? 0));
    l.forEach((k, i) => slot.set(k, i));
  };
  for (let pass = 0; pass < 4; pass++) for (const l of pass % 2 ? [...deep].reverse() : deep) sweep(l);
  // A crowded layer is folded into several rows of its own, so the whole stays roughly screen-shaped.
  const per = Math.max(6, Math.ceil(Math.sqrt(view.nodes.length) * (columns ? 0.8 : 1.3)));
  const layers: string[][] = [];
  for (const l of deep) for (let i = 0; i < l.length; i += per) layers.push(l.slice(i, i + per));
  const gapAlong = columns ? 90 : 80, gapAcross = columns ? 28 : 36;
  const pos: Record<string, Pos> = {};
  const spans = layers.map((l) => (l ?? []).reduce((a, k) => a + (columns ? size[k].h : size[k].w) + gapAcross, -gapAcross));
  const widest = Math.max(0, ...spans);
  let along = 0;
  layers.forEach((l, i) => {
    if (!l?.length) return;
    let across = (widest - spans[i]) / 2;
    let thick = 0;
    for (const k of l) {
      pos[k] = columns ? { x: Math.round(along), y: Math.round(across) } : { x: Math.round(across), y: Math.round(along) };
      across += (columns ? size[k].h : size[k].w) + gapAcross;
      thick = Math.max(thick, columns ? size[k].w : size[k].h);
    }
    along += thick + gapAlong;
  });
  return pos;
}

function arrange(view: View, size: Record<string, Size>): Record<string, Pos> {
  if (view.nodes.length > DAGRE_MAX.nodes || view.edges.length > DAGRE_MAX.edges) return quickLayers(view, size, columnsFor(view.kind));
  const g = new dagre.graphlib.Graph({ multigraph: true });
  g.setGraph({
    rankdir: columnsFor(view.kind) ? 'LR' : 'TB', nodesep: view.kind === 'erd' ? 48 : 40, ranksep: view.kind === 'erd' ? 110 : view.kind === 'flow' ? 64 : 84, edgesep: 16, marginx: 0, marginy: 0,
  });
  g.setDefaultEdgeLabel(() => ({}));
  for (const n of view.nodes) g.setNode(n.key, { width: size[n.key].w, height: size[n.key].h });
  for (const e of view.edges) {
    if (e.from === e.to || !size[e.from] || !size[e.to]) continue;
    const text = Array.from(e.label).length;
    g.setEdge(e.from, e.to, text ? { width: Math.min(180, 16 + text * 7), height: 22, labelpos: 'c' } : {}, e.key);
  }
  dagre.layout(g);
  const pos: Record<string, Pos> = {};
  for (const n of view.nodes) {
    const p = g.node(n.key) as { x: number; y: number } | undefined;
    pos[n.key] = p && Number.isFinite(p.x) && Number.isFinite(p.y) ? { x: Math.round(p.x - size[n.key].w / 2), y: Math.round(p.y - size[n.key].h / 2) } : { x: 0, y: 0 };
  }
  return pos;
}

interface Box { x: number; y: number; w: number; h: number }
const hits = (a: Box, b: Box, gap: number) => a.x < b.x + b.w + gap && b.x < a.x + a.w + gap && a.y < b.y + b.h + gap && b.y < a.y + a.h + gap;

/** A free place for a box of `size` beside `anchor` (ahead of it first, the way the diagram reads), or under everything. */
export function placeNear(anchor: Box | null, size: Size, taken: Box[], columns: boolean): Pos {
  const free = (p: Pos) => !taken.some((t) => hits({ ...p, ...size }, t, 20));
  if (anchor) {
    const ahead = columns ? { x: anchor.w + 80, y: 0 } : { x: 0, y: anchor.h + 70 };
    const side = columns ? { x: 0, y: Math.max(anchor.h, size.h) + 30 } : { x: Math.max(anchor.w, size.w) + 40, y: 0 };
    for (let ring = 0; ring < 12; ring++) {
      for (const a of [1, 0, 2, -1]) {
        for (const s of ring ? [ring, -ring] : [0]) {
          if (!a && !s) continue;
          const p = { x: Math.round(anchor.x + ahead.x * a + side.x * s), y: Math.round(anchor.y + ahead.y * a + side.y * s) };
          if (free(p)) return p;
        }
      }
    }
  }
  if (!taken.length) return { x: 0, y: 0 };
  const left = Math.min(...taken.map((t) => t.x));
  const bottom = Math.max(...taken.map((t) => t.y + t.h));
  let p = { x: left, y: bottom + 60 };
  while (!free(p)) p = { x: p.x + size.w + 40, y: p.y };
  return p;
}

/**
 * The place of every node. `local` are places the window holds itself (nodes the user added or has
 * just moved); with `all`, saved places are ignored and everything is arranged afresh.
 */
export function layout(view: View, opts: { all?: boolean; local?: Record<string, Pos> } = {}): Layout {
  const columns = columnsFor(view.kind);
  const size: Record<string, Size> = {};
  for (const n of view.nodes) size[n.key] = sizeOf(view.kind, n);
  const fixed: Record<string, Pos> = {};
  if (!opts.all) for (const n of view.nodes) { const p = opts.local?.[n.key] ?? n.pos; if (p) fixed[n.key] = p; }
  const tasks = view.kind === 'tasks' ? taskColumns(view) : null;
  const loose = view.nodes.filter((n) => !fixed[n.key]);
  let pos: Record<string, Pos> = { ...fixed };
  if (loose.length) {
    if (tasks) pos = { ...tasks.pos, ...fixed };
    else if (!Object.keys(fixed).length) pos = arrange(view, size);
    else {
      const taken: Box[] = Object.entries(fixed).map(([k, p]) => ({ ...p, ...size[k] }));
      for (const n of loose) {
        const link = n.near ?? view.edges.find((e) => (e.from === n.key && pos[e.to]) || (e.to === n.key && pos[e.from]));
        const anchorKey = typeof link === 'string' ? link : link ? (link.from === n.key ? link.to : link.from) : null;
        const anchor = anchorKey && pos[anchorKey] ? { ...pos[anchorKey], ...size[anchorKey] } : null;
        const p = placeNear(anchor, size[n.key], taken, columns);
        pos[n.key] = p;
        taken.push({ ...p, ...size[n.key] });
      }
    }
  }
  return { pos, size, bands: tasks?.bands ?? [], columns };
}

/** The middle of every node, for moving the focus and for reading order. */
export function centres(l: Layout): Record<string, Pos> {
  const out: Record<string, Pos> = {};
  for (const [k, p] of Object.entries(l.pos)) out[k] = { x: p.x + (l.size[k]?.w ?? 0) / 2, y: p.y + (l.size[k]?.h ?? 0) / 2 };
  return out;
}
