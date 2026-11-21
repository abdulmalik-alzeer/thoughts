import { describe, expect, it } from 'vitest';
import type { GraphDoc, GraphOp } from '../types';
import {
  EDGE_LABEL_MAX, LABEL_MAX, MAX_NODES, cleanFields, cleanText, emptyHistory, pushOp, record, redo, reduceOps, refuseEdge,
  refuseLabel, refuseNode, restore, undo,
} from './ops';

const doc = (kind: GraphDoc['kind'] = 'flow'): GraphDoc => ({
  version: 1, kind, title: 'T',
  nodes: [
    { id: 'a', label: 'A', kind: 'screen', props: { fields: [{ name: 'id', type: 'uuid', key: 'pk' }] } },
    { id: 'b', label: 'B', kind: 'screen' },
    { id: 'c', label: 'C', kind: 'decision' },
  ],
  edges: [{ id: 'ab', from: 'a', to: 'b', label: 'go' }, { id: 'bc', from: 'b', to: 'c' }],
});

describe('reducing the pending edits', () => {
  it('keeps one rename per node, and none when the name is back to what the file says', () => {
    const ops: GraphOp[] = [{ op: 'rename', id: 'a', label: 'One' }, { op: 'rename', id: 'a', label: 'Two' }];
    expect(reduceOps(doc(), ops)).toEqual([{ op: 'rename', id: 'a', label: 'Two' }]);
    expect(reduceOps(doc(), [...ops, { op: 'rename', id: 'a', label: ' A ' }])).toEqual([]);
  });

  it('adding then removing leaves nothing', () => {
    const ops: GraphOp[] = [{ op: 'add-node', label: 'New', kind: 'screen', near: 'a' }, { op: 'remove-node', id: 'new:1' }];
    expect(reduceOps(doc(), ops)).toEqual([]);
  });

  it('renaming an added node rewrites the add', () => {
    expect(reduceOps(doc(), [{ op: 'add-node', label: 'New', kind: 'screen' }, { op: 'rename', id: 'new:1', label: 'Sign in' }]))
      .toEqual([{ op: 'add-node', label: 'Sign in', kind: 'screen' }]);
  });

  it('removing a node drops the edits on it, on its edges, and the pending edges to it', () => {
    const ops: GraphOp[] = [
      { op: 'rename', id: 'b', label: 'Bee' },
      { op: 'set-prop', id: 'b', key: 'fields', value: [{ name: 'x', type: 'int' }] },
      { op: 'relabel-edge', id: 'ab', label: 'next' },
      { op: 'remove-edge', id: 'bc' },
      { op: 'add-node', label: 'New', kind: 'screen', near: 'b' },
      { op: 'add-edge', from: 'b', to: 'new:1' },
      { op: 'add-edge', from: 'a', to: 'new:1' },
      { op: 'remove-node', id: 'b' },
    ];
    expect(reduceOps(doc(), ops)).toEqual([
      { op: 'add-node', label: 'New', kind: 'screen' },
      { op: 'add-edge', from: 'a', to: 'new:1' },
      { op: 'remove-node', id: 'b' },
    ]);
  });

  it('removing an added node moves the later ones up and keeps their edges and fields', () => {
    const ops: GraphOp[] = [
      { op: 'add-node', label: 'One', kind: 'screen' },
      { op: 'add-node', label: 'Two', kind: 'screen', near: 'new:1' },
      { op: 'add-node', label: 'Three', kind: 'screen', near: 'new:2' },
      { op: 'add-edge', from: 'new:1', to: 'new:2' },
      { op: 'add-edge', from: 'new:2', to: 'new:3', label: 'then' },
      { op: 'set-prop', id: 'new:3', key: 'fields', value: [{ name: 'id', type: 'int' }] },
      { op: 'remove-node', id: 'new:1' },
    ];
    expect(reduceOps(doc(), ops)).toEqual([
      { op: 'add-node', label: 'Two', kind: 'screen' },
      { op: 'add-node', label: 'Three', kind: 'screen', near: 'new:1' },
      { op: 'add-edge', from: 'new:1', to: 'new:2', label: 'then' },
      { op: 'set-prop', id: 'new:2', key: 'fields', value: [{ name: 'id', type: 'int' }] },
    ]);
  });

  it('refuses a second edge between the same two nodes, and an edge from a node to itself in a flow', () => {
    expect(reduceOps(doc(), [{ op: 'add-edge', from: 'a', to: 'b' }])).toEqual([]);
    expect(reduceOps(doc(), [{ op: 'add-edge', from: 'a', to: 'c' }, { op: 'add-edge', from: 'a', to: 'c', label: 'x' }])).toEqual([{ op: 'add-edge', from: 'a', to: 'c' }]);
    expect(reduceOps(doc(), [{ op: 'add-edge', from: 'a', to: 'a' }])).toEqual([]);
    expect(reduceOps(doc('erd'), [{ op: 'add-edge', from: 'a', to: 'a' }])).toEqual([{ op: 'add-edge', from: 'a', to: 'a' }]);
    expect(reduceOps(doc(), [{ op: 'add-edge', from: 'b', to: 'a' }])).toEqual([{ op: 'add-edge', from: 'b', to: 'a' }]);
  });

  it('drawing a removed edge again puts it back', () => {
    expect(reduceOps(doc(), [{ op: 'remove-edge', id: 'ab' }, { op: 'add-edge', from: 'a', to: 'b' }])).toEqual([]);
    expect(reduceOps(doc(), [{ op: 'remove-edge', id: 'ab' }, { op: 'add-edge', from: 'a', to: 'b', label: 'other' }]))
      .toEqual([{ op: 'remove-edge', id: 'ab' }, { op: 'add-edge', from: 'a', to: 'b', label: 'other' }]);
  });

  it('edits a pending edge in place, and removing it leaves nothing', () => {
    const add: GraphOp = { op: 'add-edge', from: 'a', to: 'c' };
    expect(reduceOps(doc(), [add, { op: 'relabel-edge', id: 'new-edge:1', label: 'maybe' }])).toEqual([{ ...add, label: 'maybe' }]);
    expect(reduceOps(doc(), [add, { op: 'relabel-edge', id: 'new-edge:1', label: 'maybe' }, { op: 'relabel-edge', id: 'new-edge:1', label: '' }])).toEqual([add]);
    expect(reduceOps(doc(), [add, { op: 'remove-edge', id: 'new-edge:1' }])).toEqual([]);
  });

  it('relabelling back, or setting a property to what it was, is nothing', () => {
    expect(reduceOps(doc(), [{ op: 'relabel-edge', id: 'ab', label: 'x' }, { op: 'relabel-edge', id: 'ab', label: 'go' }])).toEqual([]);
    expect(reduceOps(doc(), [{ op: 'relabel-edge', id: 'bc', label: '' }])).toEqual([]);
    expect(reduceOps(doc(), [{ op: 'set-prop', id: 'a', key: 'fields', value: [{ name: 'id', type: 'uuid', key: 'pk' }] }])).toEqual([]);
    expect(reduceOps(doc(), [{ op: 'set-prop', id: 'a', key: 'fields', value: [] }, { op: 'set-prop', id: 'a', key: 'fields', value: [{ name: 'n', type: 't' }] }]))
      .toEqual([{ op: 'set-prop', id: 'a', key: 'fields', value: [{ name: 'n', type: 't' }] }]);
  });

  it('drops what cannot apply: unknown ids, edits on removed things, junk', () => {
    const junk = [
      null, 7, { op: 'explode' }, { op: 'rename', id: 'zz', label: 'x' }, { op: 'rename', id: 'a', label: '   ' },
      { op: 'remove-node', id: 'nope' }, { op: 'remove-edge', id: 'nope' }, { op: 'add-edge', from: 'a', to: 'new:4' },
      { op: 'add-node', label: '', kind: 'screen' }, { op: 'set-prop', id: 'new:1', key: 'fields', value: [] },
    ] as unknown as GraphOp[];
    expect(reduceOps(doc(), junk)).toEqual([]);
    expect(reduceOps(doc(), [{ op: 'remove-node', id: 'a' }, { op: 'rename', id: 'a', label: 'x' }, { op: 'relabel-edge', id: 'ab', label: 'x' }, { op: 'remove-node', id: 'a' }]))
      .toEqual([{ op: 'remove-node', id: 'a' }]);
  });

  it('cleans and bounds text, and never changes what it was given', () => {
    const long = 'x'.repeat(500);
    const ops: GraphOp[] = [{ op: 'rename', id: 'a', label: `  two\n lines\u202e ${long}` }, { op: 'add-edge', from: 'a', to: 'c', label: long }];
    const frozen = JSON.stringify(ops);
    const out = reduceOps(doc(), ops);
    expect(JSON.stringify(ops)).toBe(frozen);
    expect((out[0] as { label: string }).label.startsWith('two lines x')).toBe(true);
    expect((out[0] as { label: string }).label.length).toBe(LABEL_MAX);
    expect((out[1] as { label: string }).label.length).toBe(EDGE_LABEL_MAX);
  });

  it('is stable: reducing a reduced list changes nothing', () => {
    const ops: GraphOp[] = [
      { op: 'add-node', label: 'N', kind: 'action', near: 'a' }, { op: 'add-edge', from: 'a', to: 'new:1' }, { op: 'rename', id: 'b', label: 'Bee' },
      { op: 'remove-edge', id: 'bc' }, { op: 'remove-node', id: 'c' },
    ];
    const once = reduceOps(doc(), ops);
    expect(reduceOps(doc(), once)).toEqual(once);
    expect(pushOp(doc(), once, { op: 'rename', id: 'b', label: 'B' })).toEqual(once.filter((o) => o.op !== 'rename'));
  });

  it('restores a removed node or edge', () => {
    const ops = reduceOps(doc(), [{ op: 'remove-node', id: 'a' }, { op: 'remove-edge', id: 'bc' }]);
    expect(restore(ops, 'node', 'a')).toEqual([{ op: 'remove-edge', id: 'bc' }]);
    expect(restore(ops, 'edge', 'bc')).toEqual([{ op: 'remove-node', id: 'a' }]);
  });
});

describe('what the user may do', () => {
  it('says why an edge is refused', () => {
    expect(refuseEdge(doc(), [], 'a', 'c')).toBeNull();
    expect(refuseEdge(doc(), [], 'a', 'b')).toBe('duplicate');
    expect(refuseEdge(doc(), [], 'a', 'a')).toBe('self');
    expect(refuseEdge(doc('erd'), [], 'a', 'a')).toBeNull();
    expect(refuseEdge(doc(), [{ op: 'remove-node', id: 'c' }], 'a', 'c')).toBe('gone');
    expect(refuseEdge(doc(), [{ op: 'remove-edge', id: 'ab' }], 'a', 'b')).toBeNull();
    expect(refuseEdge(doc('tasks'), [], 'a', 'c')).toBe('readonly');
  });

  it('checks names', () => {
    expect(refuseLabel('  ')).toBe('empty');
    expect(refuseLabel('', true)).toBeNull();
    expect(refuseLabel('x'.repeat(LABEL_MAX))).toBeNull();
    expect(refuseLabel('x'.repeat(LABEL_MAX + 1))).toBe('long');
    expect(refuseLabel('x'.repeat(EDGE_LABEL_MAX + 1), true)).toBe('long');
    expect(refuseLabel('تسجيل الدخول')).toBeNull();
  });

  it('stops at the largest diagram the contract allows', () => {
    const big = doc();
    big.nodes = Array.from({ length: MAX_NODES }, (_, i) => ({ id: `n${i}`, label: `N${i}`, kind: 'screen' }));
    big.edges = [];
    expect(refuseNode(big, [])).toBe('full');
    expect(reduceOps(big, [{ op: 'add-node', label: 'x', kind: 'screen' }])).toEqual([]);
    expect(refuseNode(big, [{ op: 'remove-node', id: 'n1' }])).toBeNull();
    expect(refuseNode(doc('tasks'), [])).toBe('readonly');
  });

  it('cleans text and fields', () => {
    expect(cleanText(' a\tb\n\nc ', 10)).toBe('a b c');
    expect(cleanText({ toString: () => 'x' }, 10)).toBe('');
    expect(cleanText('😀😀😀', 2)).toBe('😀😀');
    expect(cleanFields([{ name: ' id ', type: 'uuid', key: 'pk', extra: 1 }, { name: '', type: 'x' }, 'no', { name: 'u', key: 'zz', note: '<b>n</b>' }]))
      .toEqual([{ name: 'id', type: 'uuid', key: 'pk' }, { name: 'u', type: '', note: '<b>n</b>' }]);
    expect(cleanFields('nope')).toEqual([]);
  });
});

describe('undo and redo', () => {
  it('walks back and forward, and a new edit forgets the redo', () => {
    const s0: GraphOp[] = [];
    const s1: GraphOp[] = [{ op: 'rename', id: 'a', label: 'x' }];
    const s2: GraphOp[] = [...s1, { op: 'remove-node', id: 'b' }];
    let h = record(record(emptyHistory(), s0), s1);
    expect(redo(h, s2)).toBeNull();
    const u1 = undo(h, s2)!;
    expect(u1.ops).toEqual(s1);
    const u2 = undo(u1.history, u1.ops)!;
    expect(u2.ops).toEqual(s0);
    expect(undo(u2.history, u2.ops)).toBeNull();
    const r1 = redo(u2.history, u2.ops)!;
    expect(r1.ops).toEqual(s1);
    expect(redo(r1.history, r1.ops)!.ops).toEqual(s2);
    h = record(r1.history, r1.ops);
    expect(redo(h, [])).toBeNull();
    expect(undo(h, [])!.ops).toEqual(s1);
  });
});
