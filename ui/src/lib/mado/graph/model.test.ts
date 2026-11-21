import { describe, expect, it } from 'vitest';
import type { Comment, GraphDoc } from '../types';
import { SHOWN_LABEL_MAX, buildView, cardinality, commentMarks, lookOf, paletteFor, readingOrder, sanitize, stepFocus, zoomAround } from './model';

describe('reading a diagram file that a model wrote', () => {
  it('survives anything', () => {
    for (const junk of [null, undefined, 3, 'x', [], { nodes: 'no', edges: 7 }, { kind: 'pie', nodes: [null, 1, {}, { id: 'A B' }] }]) {
      const { doc } = sanitize(junk);
      expect(doc.kind).toBe('flow');
      expect(doc.nodes).toEqual([]);
    }
  });

  it('keeps what is sound, counts what is not', () => {
    const { doc, skipped } = sanitize({
      version: 1, kind: 'erd', title: 'Data\nmodel',
      nodes: [
        { id: 'user', label: '<img src=x onerror=alert(1)>', kind: 'Entity', pos: { x: 10, y: 20 }, props: { fields: [] } },
        { id: 'user', label: 'twice', kind: 'entity' },
        { id: 'order', label: 7, kind: null, pos: { x: 'a', y: 1 } },
        { id: 'far', label: 'x'.repeat(500), kind: 'entity', pos: { x: 1e12, y: 0 }, props: [1] },
        { id: '../etc', label: 'bad id', kind: 'entity' },
      ],
      edges: [
        { id: 'e1', from: 'user', to: 'order', kind: '1-N', label: 'places' },
        { id: 'e1', from: 'user', to: 'order' },
        { id: 'e2', from: 'user', to: 'ghost' },
        { id: 'e3', from: 'order' },
        { from: 'user', to: 'order' },
      ],
    });
    expect(doc.kind).toBe('erd');
    expect(doc.title).toBe('Data model');
    expect(doc.nodes.map((n) => n.id)).toEqual(['user', 'order', 'far']);
    expect(doc.nodes[0]).toMatchObject({ label: '<img src=x onerror=alert(1)>', kind: 'entity', pos: { x: 10, y: 20 } });
    expect(doc.nodes[1]).toEqual({ id: 'order', label: '7', kind: 'other' });
    expect(Array.from(doc.nodes[2].label).length).toBe(SHOWN_LABEL_MAX);
    expect(doc.nodes[2].label.endsWith('…')).toBe(true);
    expect(doc.nodes[2].pos).toBeUndefined();
    expect(doc.nodes[2].props).toBeUndefined();
    expect(doc.edges).toEqual([{ id: 'e1', from: 'user', to: 'order', label: 'places', kind: '1-n' }]);
    expect(skipped).toEqual({ nodes: 2, edges: 2, dangling: 2 });
  });

  it('holds the limits of the contract', () => {
    const nodes = Array.from({ length: 450 }, (_, i) => ({ id: `n${i}`, label: `N${i}`, kind: 'screen' }));
    const edges = Array.from({ length: 1300 }, (_, i) => ({ id: `e${i}`, from: `n${i % 400}`, to: `n${(i * 7 + 1) % 400}` }));
    const { doc, skipped } = sanitize({ kind: 'flow', nodes, edges });
    expect(doc.nodes.length).toBe(400);
    expect(doc.edges.length).toBe(1200);
    expect(skipped.nodes).toBe(50);
    expect(skipped.edges).toBe(100);
  });
});

const doc: GraphDoc = {
  version: 1, kind: 'erd', title: 'T',
  nodes: [
    { id: 'user', label: 'User', kind: 'entity', props: { fields: [{ name: 'id', type: 'uuid', key: 'pk' }] }, pos: { x: 0, y: 0 } },
    { id: 'order', label: 'Order', kind: 'entity' },
    { id: 'item', label: 'Item', kind: 'entity' },
  ],
  edges: [{ id: 'uo', from: 'user', to: 'order', kind: '1-n' }, { id: 'oi', from: 'order', to: 'item', label: 'has' }],
};

describe('the diagram with the pending edits laid over it', () => {
  it('draws the file as it is when nothing is pending', () => {
    const v = buildView(doc, []);
    expect(v.nodes.map((n) => [n.key, n.state])).toEqual([['user', 'same'], ['order', 'same'], ['item', 'same']]);
    expect(v.nodes[0].fields).toEqual([{ name: 'id', type: 'uuid', key: 'pk' }]);
    expect(v.nodes[0].pos).toEqual({ x: 0, y: 0 });
    expect(v.edges.every((e) => e.state === 'same')).toBe(true);
  });

  it('marks renamed, removed, added and changed things, and never touches the file', () => {
    const frozen = JSON.stringify(doc);
    const v = buildView(doc, [
      { op: 'rename', id: 'user', label: 'Customer' },
      { op: 'remove-node', id: 'item' },
      { op: 'add-node', label: 'Invoice', kind: 'entity', near: 'order' },
      { op: 'add-edge', from: 'order', to: 'new:1', label: 'billed by' },
      { op: 'set-prop', id: 'new:1', key: 'fields', value: [{ name: 'total', type: 'money' }] },
      { op: 'set-prop', id: 'order', key: 'fields', value: [{ name: 'id', type: 'uuid', key: 'pk' }] },
      { op: 'relabel-edge', id: 'uo', label: 'places' },
    ]);
    expect(JSON.stringify(doc)).toBe(frozen);
    const by = Object.fromEntries(v.nodes.map((n) => [n.key, n]));
    expect(by.user).toMatchObject({ label: 'Customer', was: 'User', state: 'changed', id: 'user' });
    expect(by.item.state).toBe('removed');
    expect(by.order).toMatchObject({ state: 'changed', fields: [{ name: 'id', type: 'uuid', key: 'pk' }] });
    expect(by['new:1']).toMatchObject({ label: 'Invoice', state: 'added', near: 'order', fields: [{ name: 'total', type: 'money' }] });
    expect(by['new:1'].id).toBeUndefined();
    const eb = Object.fromEntries(v.edges.map((e) => [e.key, e]));
    expect(eb.uo).toMatchObject({ label: 'places', was: '', state: 'changed' });
    expect(eb.oi).toMatchObject({ state: 'removed', withNode: true });
    expect(eb['new-edge:1']).toMatchObject({ from: 'order', to: 'new:1', label: 'billed by', state: 'added' });
  });
});

describe('small things', () => {
  it('counts comments per node and edge, open ones apart', () => {
    const c = (id: string, target: Comment['target'], status: Comment['status']): Comment => ({ id, target, body: 'b', createdAt: '', status });
    const m = commentMarks([
      c('1', { kind: 'node', file: 'f', nodeId: 'user' }, 'open'), c('2', { kind: 'node', file: 'f', nodeId: 'user' }, 'sent'),
      c('3', { kind: 'node', file: 'f', nodeId: 'user' }, 'resolved'), c('4', { kind: 'edge', file: 'f', edgeId: 'uo' }, 'open'),
      c('5', { kind: 'task', taskId: 't' }, 'open'), null as unknown as Comment,
    ]);
    expect(m.nodes.get('user')).toEqual({ total: 2, open: 1 });
    expect(m.edges.get('uo')).toEqual({ total: 1, open: 1 });
    expect(commentMarks(undefined).nodes.size).toBe(0);
  });

  it('reads the two ends of a relation', () => {
    expect(cardinality('1-1')).toEqual({ from: 'one', to: 'one' });
    expect(cardinality('1-n')).toEqual({ from: 'one', to: 'many' });
    expect(cardinality('N:1')).toEqual({ from: 'many', to: 'one' });
    expect(cardinality('n to n')).toEqual({ from: 'many', to: 'many' });
    expect(cardinality('')).toBeNull();
    expect(cardinality('calls')).toBeNull();
  });

  it('gives unknown kinds a plain look and each diagram its own palette', () => {
    expect(lookOf('flow', 'screen')).toBe('screen');
    expect(lookOf('flow', 'wormhole')).toBe('other');
    expect(lookOf('architecture', 'database')).toBe('store');
    expect(lookOf('architecture', 'mystery')).toBe('other');
    expect(lookOf('erd', 'table')).toBe('entity');
    expect(paletteFor('tasks')).toEqual([]);
    expect(paletteFor('erd')).toEqual(['entity']);
    expect(paletteFor('flow')).toContain('decision');
  });

  it('moves the focus along edges first, then to what is nearest that way', () => {
    const v = buildView(doc, []);
    const at = { user: { x: 0, y: 0 }, order: { x: 300, y: 40 }, item: { x: 300, y: -200 } };
    expect(stepFocus(v, at, 'user', 'right')).toBe('order');
    expect(stepFocus(v, at, 'order', 'left')).toBe('user');
    expect(stepFocus(v, at, 'order', 'up')).toBe('item');
    expect(stepFocus(v, at, 'user', 'left')).toBeNull();
    expect(stepFocus(v, at, 'user', 'up')).toBe('item'); // not linked, but the only thing that way
    expect(stepFocus(v, at, 'ghost', 'up')).toBeNull();
  });

  it('orders the nodes the way the diagram reads', () => {
    const v = buildView(doc, []);
    const at = { user: { x: 300, y: 0 }, order: { x: 0, y: 200 }, item: { x: 0, y: 0 } };
    expect(readingOrder(v, at, true)).toEqual(['item', 'order', 'user']);
    expect(readingOrder(v, at, false)).toEqual(['item', 'user', 'order']);
  });

  it('zooms around the pointer and stays within limits', () => {
    const vp = zoomAround({ x: 0, y: 0, zoom: 1 }, { x: 100, y: 50 }, 2);
    expect(vp).toEqual({ x: -100, y: -50, zoom: 2 });
    expect(zoomAround({ x: 0, y: 0, zoom: 1 }, { x: 0, y: 0 }, 99).zoom).toBe(2.5);
    expect(zoomAround({ x: 5, y: 5, zoom: 1 }, { x: 0, y: 0 }, NaN)).toEqual({ x: 5, y: 5, zoom: 1 });
  });
});
