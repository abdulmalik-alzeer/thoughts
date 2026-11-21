import { describe, expect, it } from 'vitest';
import type { GraphDoc } from '../types';
import { toFlow } from './flow';
import { layout, placeNear, quickLayers, sizeOf, wavesOf } from './layout';
import { buildView, commentMarks, sanitize } from './model';

const flow: GraphDoc = {
  version: 1, kind: 'flow', title: 'F',
  nodes: [{ id: 'a', label: 'A', kind: 'start' }, { id: 'b', label: 'B', kind: 'screen' }, { id: 'c', label: 'C', kind: 'decision' }, { id: 'd', label: 'D', kind: 'screen' }],
  edges: [{ id: 'ab', from: 'a', to: 'b' }, { id: 'bc', from: 'b', to: 'c', label: 'submit' }, { id: 'cd', from: 'c', to: 'd' }, { id: 'cb', from: 'c', to: 'b', label: 'retry' }],
};

const overlap = (l: ReturnType<typeof layout>) => {
  const ks = Object.keys(l.pos);
  for (let i = 0; i < ks.length; i++) for (let j = i + 1; j < ks.length; j++) {
    const a = { ...l.pos[ks[i]], ...l.size[ks[i]] }, b = { ...l.pos[ks[j]], ...l.size[ks[j]] };
    if (a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h) return [ks[i], ks[j]];
  }
  return null;
};

describe('layout', () => {
  it('arranges a flow left to right and an architecture top to bottom, without overlaps', () => {
    const l = layout(buildView(flow, []));
    expect(l.columns).toBe(true);
    expect(l.pos.a.x).toBeLessThan(l.pos.b.x);
    expect(l.pos.b.x).toBeLessThan(l.pos.c.x);
    expect(l.pos.c.x).toBeLessThan(l.pos.d.x);
    expect(overlap(l)).toBeNull();
    const t = layout(buildView({ ...flow, kind: 'architecture' }, []));
    expect(t.columns).toBe(false);
    expect(t.pos.a.y).toBeLessThan(t.pos.b.y);
    expect(overlap(t)).toBeNull();
  });

  it('keeps saved places, and puts the rest beside what they are linked to', () => {
    const doc: GraphDoc = { ...flow, nodes: flow.nodes.map((n) => (n.id === 'd' ? n : { ...n, pos: { x: flow.nodes.indexOf(n) * 300, y: 500 } })) };
    const l = layout(buildView(doc, []));
    expect(l.pos.a).toEqual({ x: 0, y: 500 });
    expect(l.pos.c).toEqual({ x: 600, y: 500 });
    expect(l.pos.d.x).toBeGreaterThan(600);
    expect(overlap(l)).toBeNull();
    const all = layout(buildView(doc, []), { all: true });
    expect(all.pos.a).not.toEqual({ x: 0, y: 500 });
  });

  it('puts an added node beside the node it was added near, and the window\'s own places win', () => {
    const doc: GraphDoc = { ...flow, nodes: flow.nodes.map((n, i) => ({ ...n, pos: { x: i * 300, y: 0 } })) };
    const v = buildView(doc, [{ op: 'add-node', label: 'New', kind: 'screen', near: 'b' }, { op: 'add-node', label: 'Lost', kind: 'screen' }]);
    const l = layout(v);
    expect(Math.abs(l.pos['new:1'].x - 300)).toBeLessThan(400);
    expect(overlap(l)).toBeNull();
    expect(layout(v, { local: { 'new:1': { x: -50, y: -60 }, a: { x: 9, y: 9 } } }).pos).toMatchObject({ 'new:1': { x: -50, y: -60 }, a: { x: 9, y: 9 } });
  });

  it('finds a free place even in a crowd', () => {
    const taken = Array.from({ length: 40 }, (_, i) => ({ x: (i % 8) * 220, y: Math.floor(i / 8) * 100, w: 200, h: 80 }));
    const p = placeNear(taken[9], { w: 200, h: 80 }, taken, true);
    expect(taken.some((t) => p.x < t.x + t.w && t.x < p.x + 200 && p.y < t.y + t.h && t.y < p.y + 80)).toBe(false);
    expect(placeNear(null, { w: 10, h: 10 }, [], true)).toEqual({ x: 0, y: 0 });
  });

  it('sets tasks in one column per round, by what they wait for or by what the file says', () => {
    const tasks: GraphDoc = {
      version: 1, kind: 'tasks', title: 'Plan',
      nodes: [{ id: 't1', label: 'One', kind: 'task' }, { id: 't2', label: 'Two', kind: 'task' }, { id: 't3', label: 'Three', kind: 'task' }, { id: 't4', label: 'Four', kind: 'task' }],
      edges: [{ id: 'e1', from: 't1', to: 't3' }, { id: 'e2', from: 't2', to: 't3' }, { id: 'e3', from: 't3', to: 't4' }, { id: 'e4', from: 't4', to: 't3' }],
    };
    const v = buildView(tasks, []);
    const w = wavesOf(v);
    expect([w.get('t1'), w.get('t2')]).toEqual([0, 0]);
    expect(w.get('t3')).toBeGreaterThan(0);
    const l = layout(v);
    expect(l.pos.t1.x).toBe(l.pos.t2.x);
    expect(l.pos.t3.x).toBeGreaterThan(l.pos.t1.x);
    expect(l.bands.length).toBeGreaterThanOrEqual(2);
    expect(overlap(l)).toBeNull();
    const said = buildView({ ...tasks, nodes: tasks.nodes.map((n, i) => ({ ...n, props: { wave: [3, 1, 1, 2][i], lane: 'a' } })) }, []);
    expect([...wavesOf(said).values()]).toEqual([2, 0, 0, 1]);
  });

  it('sizes an entity by its fields, up to a point', () => {
    const f = (n: number) => buildView(sanitize({ kind: 'erd', nodes: [{ id: 'e', label: 'E', kind: 'entity', props: { fields: Array.from({ length: n }, (_, i) => ({ name: `f${i}`, type: 'int' })) } }] }).doc, []).nodes[0];
    expect(sizeOf('erd', f(3)).h).toBeGreaterThan(sizeOf('erd', f(0)).h);
    expect(sizeOf('erd', f(50)).h).toBe(sizeOf('erd', f(13)).h);
  });

  it('lays out the largest diagram the contract allows quickly, without overlaps', () => {
    for (const kind of ['flow', 'architecture', 'erd'] as const) {
      const nodes = Array.from({ length: 400 }, (_, i) => ({ id: `n${i}`, label: `Node ${i}`, kind: i % 9 ? 'screen' : 'decision' }));
      const edges = Array.from({ length: 1200 }, (_, i) => ({ id: `e${i}`, from: `n${(i * 13 + 5) % 400}`, to: `n${(i * 29 + 17) % 400}`, label: i % 5 ? '' : 'next' }));
      const { doc } = sanitize({ kind, title: 'Big', nodes, edges });
      const v = buildView(doc, []);
      const t0 = performance.now();
      const l = layout(v);
      const ms = performance.now() - t0;
      expect(Object.keys(l.pos).length).toBe(400);
      expect(Object.values(l.pos).every((p) => Number.isFinite(p.x) && Number.isFinite(p.y))).toBe(true);
      expect(overlap(l)).toBeNull();
      // Tens of milliseconds on an idle machine; the bound leaves room for a busy one.
      expect(ms).toBeLessThan(1500);
      const f = toFlow(v, l, commentMarks([]), null, { editable: true, describe: (n) => n.label, describeEdge: (e) => e.key });
      expect(f.nodes.length).toBe(400);
      expect(f.edges.length).toBe(v.edges.length);
    }
  }, 30000);

  it('the quick arrangement puts what leads to a node before it', () => {
    const v = buildView(flow, []);
    const size = Object.fromEntries(v.nodes.map((n) => [n.key, sizeOf('flow', n)]));
    const p = quickLayers(v, size, true);
    expect(p.a.x).toBeLessThan(p.b.x);
    expect(p.b.x).toBeLessThan(p.c.x);
    const q = quickLayers(v, size, false);
    expect(q.a.y).toBeLessThan(q.b.y);
  });
});

describe('the elements handed to the canvas', () => {
  it('carries state, marks and selection, in reading order, bands first', () => {
    const v = buildView(flow, [{ op: 'remove-node', id: 'd' }, { op: 'add-node', label: 'N', kind: 'action', near: 'a' }]);
    const marks = commentMarks([{ id: '1', target: { kind: 'node', file: 'f', nodeId: 'b' }, body: '', createdAt: '', status: 'open' }]);
    const f = toFlow(v, layout(v), marks, { kind: 'node', key: 'b' }, { editable: true, describe: (n) => `node ${n.label}`, describeEdge: (e) => `edge ${e.key}` });
    const b = f.nodes.find((n) => n.id === 'b')!;
    expect(b).toMatchObject({ selected: true, type: 'card', ariaLabel: 'node B', sourcePosition: 'right', targetPosition: 'left' });
    expect((b.data as { marks: unknown }).marks).toEqual({ total: 1, open: 1 });
    expect(f.nodes.find((n) => n.id === 'd')!.connectable).toBe(false);
    expect(f.nodes.find((n) => n.id === 'new:1')!.connectable).toBe(true);
    expect(f.nodes[0].id).toBe('a');
    expect(f.edges.find((e) => e.id === 'cd')!.data!.v.state).toBe('removed');
    const ro = toFlow(v, layout(v), marks, null, { editable: false, describe: () => '', describeEdge: () => '' });
    expect(ro.nodes.every((n) => !n.connectable)).toBe(true);
  });
});
