// The drawn diagram as the elements @xyflow/svelte takes. Plain data in, plain data out.
import type { Edge, Node } from '@xyflow/svelte';
import type { GraphKind } from '../types';
import type { Band, Layout } from './layout';
import { centres } from './layout';
import { readingOrder, type Marks, type VEdge, type VNode, type View } from './model';

export type Selection = { kind: 'node' | 'edge'; key: string } | null;
export interface NodeData extends Record<string, unknown> { v: VNode; graph: GraphKind; marks: Marks | null; columns: boolean }
export interface BandData extends Record<string, unknown> { band: Band }
export interface EdgeData extends Record<string, unknown> { v: VEdge; graph: GraphKind; marks: Marks | null }
export type FlowNode = Node<NodeData, 'card' | 'entity'> | Node<BandData, 'band'>;
export type FlowEdge = Edge<EdgeData, 'mado'>;

export const BAND_PREFIX = 'band:';
/** Above this many nodes only what is in view is drawn, and the small map is shown. */
export const LARGE = 120;
export const MINIMAP_FROM = 24;

/** Left/right or top/bottom, as the strings xyflow's `Position` uses. */
const SIDES = { columns: { source: 'right', target: 'left' }, rows: { source: 'bottom', target: 'top' } } as const;

export function toFlow(
  view: View, l: Layout, marks: { nodes: Map<string, Marks>; edges: Map<string, Marks> }, sel: Selection,
  opts: { editable: boolean; describe: (n: VNode) => string; describeEdge: (e: VEdge) => string },
): { nodes: FlowNode[]; edges: FlowEdge[] } {
  const side = l.columns ? SIDES.columns : SIDES.rows;
  const by = new Map(view.nodes.map((n) => [n.key, n]));
  const nodes: FlowNode[] = l.bands.map((band) => ({
    id: `${BAND_PREFIX}${band.index}`, type: 'band', position: { x: band.x, y: band.y }, width: band.w, height: band.h,
    data: { band }, draggable: false, selectable: false, focusable: false, connectable: false, deletable: false, zIndex: -1,
  }));
  for (const key of readingOrder(view, centres(l), l.columns)) {
    const v = by.get(key)!;
    const size = l.size[key];
    nodes.push({
      id: key, type: view.kind === 'erd' ? 'entity' : 'card', position: l.pos[key] ?? { x: 0, y: 0 }, width: size.w, height: size.h,
      data: { v, graph: view.kind, marks: v.id ? marks.nodes.get(v.id) ?? null : null, columns: l.columns },
      // xyflow's enum values are these same strings.
      sourcePosition: side.source as Node['sourcePosition'], targetPosition: side.target as Node['targetPosition'],
      selected: sel?.kind === 'node' && sel.key === key, draggable: true, deletable: false, focusable: true,
      connectable: opts.editable && v.state !== 'removed', ariaLabel: opts.describe(v),
    });
  }
  const edges: FlowEdge[] = [];
  for (const v of view.edges) {
    if (!by.has(v.from) || !by.has(v.to)) continue;
    edges.push({
      id: v.key, source: v.from, target: v.to, type: 'mado',
      data: { v, graph: view.kind, marks: v.id ? marks.edges.get(v.id) ?? null : null },
      selected: sel?.kind === 'edge' && sel.key === v.key, deletable: false, focusable: true,
      ariaLabel: opts.describeEdge(v), zIndex: sel?.kind === 'edge' && sel.key === v.key ? 2 : 0,
    });
  }
  return { nodes, edges };
}
