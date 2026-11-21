// The list of edits a user made to a diagram and has not sent yet. The window never applies them to the
// file: they are kept as `GraphOp`s, drawn as pending, and sent as a change request.
//
// A thing the user added has no id until the assistant applies it. Here it is known by a temporary key:
// `new:<n>` for the n-th `add-node` of the list (1-based), `new-edge:<n>` for the n-th `add-edge`.
// `new:<n>` may appear where an op needs to name a pending node (`add-edge.from/to`, `set-prop.id`,
// `add-node.near`); it can never be mistaken for a real id, which has no colon. `new-edge:<n>` never
// leaves this module: editing a pending edge rewrites its `add-edge`.
import type { ErdField, GraphDoc, GraphKind, GraphOp } from '../types';

export const LABEL_MAX = 80;
export const EDGE_LABEL_MAX = 60;
export const FIELD_TEXT_MAX = 40;
export const FIELDS_MAX = 60;
export const MAX_NODES = 400;
export const MAX_EDGES = 1200;
export const HISTORY_MAX = 100;

const NEW_NODE = /^new:([1-9][0-9]{0,3})$/;
const NEW_EDGE = /^new-edge:([1-9][0-9]{0,3})$/;
export const nodeKey = (n: number) => `new:${n}`;
export const edgeKey = (n: number) => `new-edge:${n}`;
export const isNewNode = (id: string) => NEW_NODE.test(id);
export const isNewEdge = (id: string) => NEW_EDGE.test(id);
const ordinal = (re: RegExp, id: string) => { const m = re.exec(id); return m ? Number(m[1]) : 0; };

/** One line of plain text: no control characters, no line breaks, single spaces, cut to `max` characters. */
export function cleanText(v: unknown, max: number): string {
  const s = typeof v === 'string' ? v : v == null ? '' : typeof v === 'number' || typeof v === 'boolean' ? String(v) : '';
  // eslint-disable-next-line no-control-regex
  const flat = s.replace(/[\u0000-\u001f\u007f-\u009f\u2028\u2029\u202a-\u202e\u2066-\u2069]/g, ' ').replace(/\s+/g, ' ').trim();
  return Array.from(flat).slice(0, max).join('');
}

/** The fields of an entity, made safe: text only, bounded, nameless rows dropped. */
export function cleanFields(v: unknown): ErdField[] {
  if (!Array.isArray(v)) return [];
  const out: ErdField[] = [];
  for (const raw of v) {
    if (out.length >= FIELDS_MAX) break;
    if (!raw || typeof raw !== 'object') continue;
    const r = raw as Record<string, unknown>;
    const name = cleanText(r.name, FIELD_TEXT_MAX);
    if (!name) continue;
    const f: ErdField = { name, type: cleanText(r.type, FIELD_TEXT_MAX) };
    if (r.key === 'pk' || r.key === 'fk') f.key = r.key;
    const note = cleanText(r.note, 120);
    if (note) f.note = note;
    out.push(f);
  }
  return out;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);

type Entry = { key: string; op: GraphOp };

/** Whether a diagram of this kind may have an edge from a node to itself (an entity may relate to itself). */
export const selfEdgeAllowed = (kind: GraphKind) => kind === 'erd';

/**
 * Folds a list of edits into the shortest list that says the same thing, dropping what cannot apply to
 * `doc`. Renaming twice is one rename; renaming back is nothing; adding then removing is nothing;
 * removing a node drops the edits on it and the pending edges that touch it. The order of what
 * remains is the order in which the user first did it.
 */
export function reduceOps(doc: GraphDoc, ops: readonly GraphOp[]): GraphOp[] {
  const nodes = new Map(doc.nodes.map((n) => [n.id, n]));
  const edges = new Map(doc.edges.map((e) => [e.id, e]));
  let list: Entry[] = [];
  const at = (key: string) => list.findIndex((e) => e.key === key);
  const drop = (pred: (e: Entry) => boolean) => { list = list.filter((e) => !pred(e)); };
  const put = (key: string, op: GraphOp | null) => {
    const i = at(key);
    if (!op) { if (i >= 0) list.splice(i, 1); return; }
    if (i >= 0) list[i] = { key, op }; else list.push({ key, op });
  };
  const added = () => list.filter((e) => e.op.op === 'add-node');
  const addedEdges = () => list.filter((e) => e.op.op === 'add-edge');
  const removed = (id: string) => at(`remove-node:${id}`) >= 0;
  /** A node an edit may name: one of the file that is not pending removal, or a pending one. */
  const live = (id: string) => (isNewNode(id) ? ordinal(NEW_NODE, id) <= added().length : nodes.has(id) && !removed(id));
  const edgeLive = (id: string) => {
    const e = edges.get(id);
    return !!e && at(`remove-edge:${id}`) < 0 && live(e.from) && live(e.to);
  };
  const hasEdge = (from: string, to: string) =>
    [...edges.values()].some((e) => e.from === from && e.to === to && edgeLive(e.id)) ||
    addedEdges().some((e) => e.op.op === 'add-edge' && e.op.from === from && e.op.to === to);

  for (const raw of Array.isArray(ops) ? ops : []) {
    if (!raw || typeof raw !== 'object') continue;
    const op = raw as GraphOp;
    switch (op.op) {
      case 'rename': {
        const label = cleanText(op.label, LABEL_MAX);
        if (!label || typeof op.id !== 'string' || !live(op.id)) break;
        if (isNewNode(op.id)) {
          const e = added()[ordinal(NEW_NODE, op.id) - 1];
          if (e.op.op === 'add-node') e.op = { ...e.op, label };
        } else put(`rename:${op.id}`, label === nodes.get(op.id)!.label ? null : { op: 'rename', id: op.id, label });
        break;
      }
      case 'add-node': {
        const label = cleanText(op.label, LABEL_MAX);
        const kind = cleanText(op.kind, 40);
        if (!label || !kind) break;
        if (doc.nodes.length - list.filter((e) => e.op.op === 'remove-node').length + added().length >= MAX_NODES) break;
        const next: GraphOp = { op: 'add-node', label, kind };
        if (typeof op.near === 'string' && live(op.near)) next.near = op.near;
        list.push({ key: `add-node#${Math.random()}`, op: next });
        break;
      }
      case 'remove-node': {
        if (typeof op.id !== 'string' || !live(op.id)) break;
        if (isNewNode(op.id)) {
          const n = ordinal(NEW_NODE, op.id);
          const gone = added()[n - 1];
          drop((e) => e === gone
            || (e.op.op === 'set-prop' && e.op.id === op.id)
            || (e.op.op === 'add-edge' && (e.op.from === op.id || e.op.to === op.id)));
          // Every pending node after it moves up one place: rewrite what names them.
          const shift = (id: string) => { const k = ordinal(NEW_NODE, id); return k > n ? nodeKey(k - 1) : id; };
          for (const e of list) {
            if (e.op.op === 'add-edge') e.op = { ...e.op, from: shift(e.op.from), to: shift(e.op.to) };
            else if (e.op.op === 'set-prop') { e.op = { ...e.op, id: shift(e.op.id) }; e.key = `set-prop:${e.op.id}:${e.op.key}`; }
            else if (e.op.op === 'add-node' && e.op.near) {
              if (e.op.near === op.id) { const { near: _near, ...rest } = e.op; e.op = rest; }
              else e.op = { ...e.op, near: shift(e.op.near) };
            }
          }
        } else {
          const incident = new Set(doc.edges.filter((e) => e.from === op.id || e.to === op.id).map((e) => e.id));
          drop((e) => (e.op.op === 'rename' && e.op.id === op.id)
            || (e.op.op === 'set-prop' && e.op.id === op.id)
            || (e.op.op === 'add-edge' && (e.op.from === op.id || e.op.to === op.id))
            || ((e.op.op === 'remove-edge' || e.op.op === 'relabel-edge') && incident.has(e.op.id)));
          for (const e of list) if (e.op.op === 'add-node' && e.op.near === op.id) { const { near: _near, ...rest } = e.op; e.op = rest; }
          put(`remove-node:${op.id}`, { op: 'remove-node', id: op.id });
        }
        break;
      }
      case 'add-edge': {
        if (typeof op.from !== 'string' || typeof op.to !== 'string' || !live(op.from) || !live(op.to)) break;
        if (op.from === op.to && !selfEdgeAllowed(doc.kind)) break;
        const label = cleanText(op.label, EDGE_LABEL_MAX);
        // Drawing again an edge that is pending removal puts it back instead of adding a twin.
        const back = doc.edges.find((e) => e.from === op.from && e.to === op.to && at(`remove-edge:${e.id}`) >= 0);
        if (back && (!label || label === (back.label ?? ''))) { put(`remove-edge:${back.id}`, null); break; }
        if (hasEdge(op.from, op.to)) break;
        if (doc.edges.length + addedEdges().length >= MAX_EDGES) break;
        const next: GraphOp = { op: 'add-edge', from: op.from, to: op.to };
        if (label) next.label = label;
        list.push({ key: `add-edge#${Math.random()}`, op: next });
        break;
      }
      case 'remove-edge': {
        if (typeof op.id !== 'string') break;
        if (isNewEdge(op.id)) { const gone = addedEdges()[ordinal(NEW_EDGE, op.id) - 1]; if (gone) drop((e) => e === gone); break; }
        if (!edgeLive(op.id)) break;
        put(`relabel-edge:${op.id}`, null);
        put(`remove-edge:${op.id}`, { op: 'remove-edge', id: op.id });
        break;
      }
      case 'relabel-edge': {
        if (typeof op.id !== 'string') break;
        const label = cleanText(op.label, EDGE_LABEL_MAX);
        if (isNewEdge(op.id)) {
          const e = addedEdges()[ordinal(NEW_EDGE, op.id) - 1];
          if (e && e.op.op === 'add-edge') { const { label: _label, ...rest } = e.op; e.op = label ? { ...rest, label } : rest; }
          break;
        }
        if (!edgeLive(op.id)) break;
        put(`relabel-edge:${op.id}`, label === (edges.get(op.id)!.label ?? '') ? null : { op: 'relabel-edge', id: op.id, label });
        break;
      }
      case 'set-prop': {
        if (typeof op.id !== 'string' || !live(op.id)) break;
        const key = cleanText(op.key, 40);
        if (!key) break;
        const value = key === 'fields' ? cleanFields(op.value) : op.value;
        const before = isNewNode(op.id) ? undefined : nodes.get(op.id)!.props?.[key];
        const unchanged = key === 'fields' ? same(value, cleanFields(before)) : same(value, before);
        put(`set-prop:${op.id}:${key}`, unchanged ? null : { op: 'set-prop', id: op.id, key, value });
        break;
      }
    }
  }
  return list.map((e) => e.op);
}

/** The list after one more edit. */
export const pushOp = (doc: GraphDoc, ops: readonly GraphOp[], op: GraphOp): GraphOp[] => reduceOps(doc, [...ops, op]);

/** Takes back the pending removal of a node or an edge of the file. */
export const restore = (ops: readonly GraphOp[], kind: 'node' | 'edge', id: string): GraphOp[] =>
  ops.filter((o) => !(o.op === (kind === 'node' ? 'remove-node' : 'remove-edge') && o.id === id));

export const sameOps = (a: readonly GraphOp[], b: readonly GraphOp[]) => JSON.stringify(a) === JSON.stringify(b);

// ----- what the user may do -----

export type Refusal = 'self' | 'duplicate' | 'gone' | 'full' | 'empty' | 'long' | 'readonly';

/** May an edge be drawn from `from` to `to`, on top of what is pending? Returns why not, or null. */
export function refuseEdge(doc: GraphDoc, ops: readonly GraphOp[], from: string, to: string): Refusal | null {
  if (doc.kind === 'tasks') return 'readonly';
  if (from === to && !selfEdgeAllowed(doc.kind)) return 'self';
  const before = reduceOps(doc, ops);
  const after = reduceOps(doc, [...before, { op: 'add-edge', from, to }]);
  if (!sameOps(before, after)) return null;
  const count = (l: GraphOp[], k: string) => l.filter((o) => o.op === k).length;
  if (doc.edges.length + count(before, 'add-edge') >= MAX_EDGES) return 'full';
  const news = count(before, 'add-node');
  const there = (id: string) => (isNewNode(id) ? ordinal(NEW_NODE, id) <= news : doc.nodes.some((n) => n.id === id) && !before.some((o) => o.op === 'remove-node' && o.id === id));
  return there(from) && there(to) ? 'duplicate' : 'gone';
}

/** May this be the name of a node (or, with `edge`, the label of an edge, which may be empty)? */
export function refuseLabel(text: string, edge = false): Refusal | null {
  const flat = cleanText(text, 10_000);
  if (!flat) return edge ? null : 'empty';
  return Array.from(flat).length > (edge ? EDGE_LABEL_MAX : LABEL_MAX) ? 'long' : null;
}

/** May one more node be added? */
export function refuseNode(doc: GraphDoc, ops: readonly GraphOp[]): Refusal | null {
  if (doc.kind === 'tasks') return 'readonly';
  const r = reduceOps(doc, ops);
  const n = doc.nodes.length - r.filter((o) => o.op === 'remove-node').length + r.filter((o) => o.op === 'add-node').length;
  return n >= MAX_NODES ? 'full' : null;
}

// ----- undo and redo -----

export interface History { past: GraphOp[][]; future: GraphOp[][] }
export const emptyHistory = (): History => ({ past: [], future: [] });

/** Remembers the list as it was before an edit. A new edit forgets what could be redone. */
export function record(h: History, before: readonly GraphOp[]): History {
  return { past: [...h.past, [...before]].slice(-HISTORY_MAX), future: [] };
}
export function undo(h: History, now: readonly GraphOp[]): { history: History; ops: GraphOp[] } | null {
  if (!h.past.length) return null;
  return { history: { past: h.past.slice(0, -1), future: [...h.future, [...now]] }, ops: h.past[h.past.length - 1] };
}
export function redo(h: History, now: readonly GraphOp[]): { history: History; ops: GraphOp[] } | null {
  if (!h.future.length) return null;
  return { history: { past: [...h.past, [...now]], future: h.future.slice(0, -1) }, ops: h.future[h.future.length - 1] };
}
