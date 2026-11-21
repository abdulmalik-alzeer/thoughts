// From a diagram file (which a model wrote: nothing in it is trusted) and the user's pending edits to
// what is drawn. Everything here is plain data; labels stay text all the way to the screen.
import type { Comment, ErdField, GraphDoc, GraphEdge, GraphKind, GraphNode, GraphOp, Pos } from '../types';
import { MAX_EDGES, MAX_NODES, cleanFields, cleanText, edgeKey, isNewNode, nodeKey, reduceOps } from './ops';

/** Longest label drawn; the rest is cut with an ellipsis (the full text of the file is never needed on screen). */
export const SHOWN_LABEL_MAX = 120;
const ID = /^[a-z0-9][a-z0-9-]{0,63}$/;
const KINDS: GraphKind[] = ['flow', 'architecture', 'erd', 'tasks'];

/** The kinds each diagram draws in a look of its own; anything else is drawn as a plain card. */
export const NODE_KINDS: Record<GraphKind, string[]> = {
  flow: ['screen', 'decision', 'action', 'start', 'end'],
  architecture: ['client', 'service', 'store', 'external', 'queue'],
  erd: ['entity'],
  tasks: ['task'],
};
/** The look a node of this kind gets: one of the known kinds of the diagram, or `other`. */
export function lookOf(graph: GraphKind, kind: string): string {
  const k = String(kind ?? '').toLowerCase();
  if (NODE_KINDS[graph].includes(k)) return k;
  if (graph === 'architecture') {
    if (/(^|-)(db|database|cache|storage|bucket)$/.test(k)) return 'store';
    if (/(^|-)(app|web|mobile|ui|frontend)$/.test(k)) return 'client';
    if (/(^|-)(api|server|backend|worker|job)$/.test(k)) return 'service';
    if (/(^|-)(topic|stream|bus)$/.test(k)) return 'queue';
    if (/third-party|vendor|saas/.test(k)) return 'external';
  }
  if (graph === 'erd' || graph === 'tasks') return NODE_KINDS[graph][0];
  return 'other';
}
/** What may be added to a diagram of this kind (nothing to the tasks diagram: it is worked out from the plan). */
export const paletteFor = (graph: GraphKind): string[] => (graph === 'tasks' ? [] : NODE_KINDS[graph]);

export interface Skipped { nodes: number; edges: number; dangling: number }

function shown(v: unknown): string {
  const s = cleanText(v, SHOWN_LABEL_MAX + 1);
  return Array.from(s).length > SHOWN_LABEL_MAX ? Array.from(s).slice(0, SHOWN_LABEL_MAX - 1).join('') + '…' : s;
}

/**
 * A diagram that is safe to draw, whatever was in the file: unknown shapes are dropped and counted,
 * text is flattened and bounded, an edge whose end is missing is skipped, and the limits of the
 * contract (400 nodes, 1200 edges) hold.
 */
export function sanitize(input: unknown): { doc: GraphDoc; skipped: Skipped } {
  const raw = (input && typeof input === 'object' ? input : {}) as Record<string, unknown>;
  const kind = KINDS.includes(raw.kind as GraphKind) ? (raw.kind as GraphKind) : 'flow';
  const skipped: Skipped = { nodes: 0, edges: 0, dangling: 0 };
  const nodes: GraphNode[] = [];
  const seen = new Set<string>();
  for (const n of Array.isArray(raw.nodes) ? raw.nodes : []) {
    const r = (n && typeof n === 'object' ? n : {}) as Record<string, unknown>;
    const id = typeof r.id === 'string' ? r.id : '';
    if (!ID.test(id) || seen.has(id) || nodes.length >= MAX_NODES) { skipped.nodes++; continue; }
    seen.add(id);
    const node: GraphNode = { id, label: shown(r.label) || id, kind: cleanText(r.kind, 40).toLowerCase() || 'other' };
    if (r.props && typeof r.props === 'object' && !Array.isArray(r.props)) node.props = r.props as Record<string, unknown>;
    const p = r.pos as Record<string, unknown> | undefined;
    if (p && typeof p === 'object' && Number.isFinite(p.x) && Number.isFinite(p.y) && Math.abs(p.x as number) < 1e6 && Math.abs(p.y as number) < 1e6) {
      node.pos = { x: p.x as number, y: p.y as number };
    }
    nodes.push(node);
  }
  const edges: GraphEdge[] = [];
  const seenE = new Set<string>();
  for (const e of Array.isArray(raw.edges) ? raw.edges : []) {
    const r = (e && typeof e === 'object' ? e : {}) as Record<string, unknown>;
    const id = typeof r.id === 'string' ? r.id : '';
    if (!ID.test(id) || seenE.has(id) || edges.length >= MAX_EDGES) { skipped.edges++; continue; }
    if (typeof r.from !== 'string' || typeof r.to !== 'string' || !seen.has(r.from) || !seen.has(r.to)) { skipped.dangling++; continue; }
    seenE.add(id);
    const edge: GraphEdge = { id, from: r.from, to: r.to };
    const label = shown(r.label);
    if (label) edge.label = label;
    const k = cleanText(r.kind, 40).toLowerCase();
    if (k) edge.kind = k;
    edges.push(edge);
  }
  return { doc: { version: 1, kind, title: shown(raw.title), nodes, edges }, skipped };
}

export type Change = 'same' | 'added' | 'removed' | 'changed';
export interface VNode {
  /** The id of the file, or the temporary key of a node the user added (`new:<n>`). */
  key: string;
  /** Only for a node of the file. */
  id?: string;
  label: string;
  /** The name in the file, when the user renamed it. */
  was?: string;
  kind: string;
  look: string;
  props: Record<string, unknown>;
  /** Entities only. */
  fields: ErdField[];
  state: Change;
  pos?: Pos;
  /** For an added node: the node it should be placed near. */
  near?: string;
}
export interface VEdge {
  key: string;
  id?: string;
  from: string;
  to: string;
  label: string;
  was?: string;
  kind: string;
  state: Change;
  /** Removed only because one of its ends is being removed. */
  withNode: boolean;
}
export interface View { kind: GraphKind; title: string; nodes: VNode[]; edges: VEdge[]; ops: GraphOp[] }

/** The diagram as drawn: the (sanitized) file with the pending edits laid over it, nothing applied. */
export function buildView(doc: GraphDoc, pending: readonly GraphOp[]): View {
  const ops = reduceOps(doc, pending);
  const renames = new Map<string, string>();
  const gone = new Set<string>();
  const goneEdges = new Set<string>();
  const relabels = new Map<string, string>();
  const props = new Map<string, Record<string, unknown>>();
  for (const o of ops) {
    if (o.op === 'rename') renames.set(o.id, o.label);
    else if (o.op === 'remove-node') gone.add(o.id);
    else if (o.op === 'remove-edge') goneEdges.add(o.id);
    else if (o.op === 'relabel-edge') relabels.set(o.id, o.label);
    else if (o.op === 'set-prop') props.set(o.id, { ...(props.get(o.id) ?? {}), [o.key]: o.value });
  }
  const node = (key: string, id: string | undefined, label: string, kind: string, base: Record<string, unknown>, state: Change): VNode => {
    const merged = { ...base, ...(props.get(key) ?? {}) };
    return { key, id, label, kind, look: lookOf(doc.kind, kind), props: merged, fields: doc.kind === 'erd' ? cleanFields(merged.fields) : [], state };
  };
  const nodes: VNode[] = doc.nodes.map((n) => {
    const renamed = renames.get(n.id);
    const v = node(n.id, n.id, renamed ?? n.label, n.kind, n.props ?? {}, gone.has(n.id) ? 'removed' : renamed !== undefined || props.has(n.id) ? 'changed' : 'same');
    if (renamed !== undefined) v.was = n.label;
    if (n.pos) v.pos = n.pos;
    return v;
  });
  const edges: VEdge[] = doc.edges.map((e) => {
    const withNode = gone.has(e.from) || gone.has(e.to);
    const relabel = relabels.get(e.id);
    const v: VEdge = {
      key: e.id, id: e.id, from: e.from, to: e.to, label: relabel ?? e.label ?? '', kind: e.kind ?? '',
      state: withNode || goneEdges.has(e.id) ? 'removed' : relabel !== undefined ? 'changed' : 'same', withNode,
    };
    if (relabel !== undefined) v.was = e.label ?? '';
    return v;
  });
  let n = 0;
  let m = 0;
  for (const o of ops) {
    if (o.op === 'add-node') {
      const v = node(nodeKey(++n), undefined, o.label, o.kind, {}, 'added');
      if (o.near) v.near = o.near;
      nodes.push(v);
    } else if (o.op === 'add-edge') {
      edges.push({ key: edgeKey(++m), from: o.from, to: o.to, label: o.label ?? '', kind: '', state: 'added', withNode: false });
    }
  }
  return { kind: doc.kind, title: doc.title, nodes, edges, ops };
}

export interface Marks { total: number; open: number }
/** How many comments sit on each node and edge (by id), and how many of them are still open. */
export function commentMarks(comments: readonly Comment[] | undefined): { nodes: Map<string, Marks>; edges: Map<string, Marks> } {
  const nodes = new Map<string, Marks>();
  const edges = new Map<string, Marks>();
  for (const c of Array.isArray(comments) ? comments : []) {
    const t = c?.target;
    if (!t || c.status === 'resolved' || c.status === 'orphaned') continue;
    const [map, id] = t.kind === 'node' ? [nodes, t.nodeId] : t.kind === 'edge' ? [edges, t.edgeId] : [null, ''];
    if (!map || typeof id !== 'string') continue;
    const cur = map.get(id) ?? { total: 0, open: 0 };
    cur.total++;
    if (c.status === 'open') cur.open++;
    map.set(id, cur);
  }
  return { nodes, edges };
}

/** How the two ends of a relation are marked: `one` or `many`, or nothing for an edge that is not a relation. */
export function cardinality(kind: string): { from: 'one' | 'many'; to: 'one' | 'many' } | null {
  const k = String(kind ?? '').toLowerCase().replace(/\s*(?:to|:|_|\.\.|–|—)\s*/g, '-').replace(/\s+/g, '').replace(/-+/g, '-');
  if (k === '1-1') return { from: 'one', to: 'one' };
  if (k === '1-n' || k === '1-m' || k === '1-*') return { from: 'one', to: 'many' };
  if (k === 'n-1' || k === 'm-1' || k === '*-1') return { from: 'many', to: 'one' };
  if (k === 'n-n' || k === 'm-n' || k === 'n-m' || k === 'm-m' || k === '*-*') return { from: 'many', to: 'many' };
  return null;
}

// ----- moving the focus with the keyboard -----

export type Dir = 'left' | 'right' | 'up' | 'down';
const AXIS: Record<Dir, [number, number]> = { left: [-1, 0], right: [1, 0], up: [0, -1], down: [0, 1] };

/**
 * The node an arrow key moves to: the connected node that lies most in that direction; when no
 * connected node lies that way, the nearest node that does. Null when there is nothing that way.
 */
export function stepFocus(view: View, at: Record<string, Pos>, from: string, dir: Dir): string | null {
  const here = at[from];
  if (!here) return null;
  const [ax, ay] = AXIS[dir];
  const linked = new Set<string>();
  for (const e of view.edges) { if (e.from === from) linked.add(e.to); else if (e.to === from) linked.add(e.from); }
  let best: string | null = null;
  let bestScore = Infinity;
  for (const pass of [true, false]) {
    for (const n of view.nodes) {
      if (n.key === from || pass !== linked.has(n.key)) continue;
      const p = at[n.key];
      if (!p) continue;
      const dx = p.x - here.x, dy = p.y - here.y;
      const along = dx * ax + dy * ay;
      const across = Math.abs(dx * ay) + Math.abs(dy * ax);
      if (along <= 1 || across > along * 2.5) continue; // not in that direction
      const score = along + across * 2;
      if (score < bestScore) { bestScore = score; best = n.key; }
    }
    if (best) return best;
  }
  return null;
}

/** The order in which Tab walks the nodes: the way the diagram reads (columns for left-to-right diagrams, rows otherwise). */
export function readingOrder(view: View, at: Record<string, Pos>, columns: boolean): string[] {
  const band = 60;
  return view.nodes.map((n) => n.key).sort((a, b) => {
    const p = at[a] ?? { x: 0, y: 0 }, q = at[b] ?? { x: 0, y: 0 };
    const [p1, p2, q1, q2] = columns ? [p.x, p.y, q.x, q.y] : [p.y, p.x, q.y, q.x];
    const d = Math.round(p1 / band) - Math.round(q1 / band);
    return d || p2 - q2 || (a < b ? -1 : a > b ? 1 : 0);
  });
}

/** The viewport after zooming to `zoom` while the point under the pointer (in the canvas's own pixels) stays put. */
export function zoomAround(vp: { x: number; y: number; zoom: number }, point: Pos, zoom: number, min = 0.1, max = 2.5) {
  const z = Math.min(max, Math.max(min, Number.isFinite(zoom) ? zoom : vp.zoom));
  const k = z / vp.zoom;
  return { x: point.x - (point.x - vp.x) * k, y: point.y - (point.y - vp.y) * k, zoom: z };
}

export { isNewNode };
