// Which comments belong to which step, and how they and the diagram edits become one change request.
import { describe, it, expect } from 'vitest';
import { changeCount, changeRequest, commentsOfStep, commentsOnDesign, commentsOnDoc, commentsOnGraph, graphTarget, opsOfStep, stepOf, type PendingOps } from './batch';
import type { Comment, CommentStatus, CommentTarget } from '../types';

let n = 0;
const c = (target: CommentTarget, status: CommentStatus = 'open'): Comment => ({ id: `c${++n}`, target, body: 'change this', createdAt: '2026-11-21T08:00:00Z', status });
const text = (file: string): CommentTarget => ({ kind: 'text', file, quote: 'q', prefix: '', suffix: '' });

const all: Comment[] = [
  c(text('spec.md')),                                                   // c1 spec, open
  c(text('spec.md'), 'sent'),                                           // c2 spec, sent
  c({ kind: 'node', file: 'flow.graph.json', nodeId: 'home' }),         // c3 spec, open
  c({ kind: 'edge', file: 'architecture.graph.json', edgeId: 'e1' }),   // c4 tech
  c(text('tech.md'), 'resolved'),                                       // c5 tech, done
  c({ kind: 'token', tokenPath: 'color.primary' }),                     // c6 design
  c({ kind: 'screen', screenId: 'home', x: 0.2, y: 0.4 }),              // c7 design
  c({ kind: 'task', taskId: 'auth' }),                                  // c8 plan
  c(text('brief.md'), 'orphaned'),                                      // c9 brief, lost
  c(text('unknown.md')),                                                // c10 nowhere
];

describe('which step a comment belongs to', () => {
  it('by its file or its kind', () => {
    expect(all.map((x) => stepOf(x.target))).toEqual(['spec', 'spec', 'spec', 'tech', 'tech', 'design', 'design', 'plan', 'brief', null]);
    expect(commentsOfStep(all, 'spec').map((x) => x.id)).toEqual(['c1', 'c2', 'c3']);
    expect(commentsOfStep(all, 'review')).toEqual([]);
  });
  it('per document, per diagram, for the design', () => {
    expect(commentsOnDoc(all, 'spec').map((x) => x.id)).toEqual(['c1', 'c2']);
    expect(commentsOnGraph(all, 'flow').map((x) => x.id)).toEqual(['c3']);
    expect(commentsOnGraph(all, 'schema')).toEqual([]);
    expect(commentsOnDesign(all).map((x) => x.id)).toEqual(['c6', 'c7']);
  });
  it('on the tasks diagram a comment on a task is a mark on its node', () => {
    expect(commentsOnGraph(all, 'tasks').map((x) => x.target)).toEqual([{ kind: 'node', file: 'plan.json', nodeId: 'auth' }]);
    expect(graphTarget('tasks', { kind: 'node', nodeId: 'auth' })).toEqual({ kind: 'task', taskId: 'auth' });
    expect(graphTarget('flow', { kind: 'node', nodeId: 'home' })).toEqual({ kind: 'node', file: 'flow.graph.json', nodeId: 'home' });
    expect(graphTarget('schema', { kind: 'edge', edgeId: 'e9' })).toEqual({ kind: 'edge', file: 'schema.graph.json', edgeId: 'e9' });
  });
});

describe('one change request per step', () => {
  const pending: PendingOps = {
    'flow.graph.json': [{ op: 'rename', id: 'home', label: 'Start' }, { op: 'remove-node', id: 'old' }],
    'schema.graph.json': [{ op: 'add-node', label: 'Order', kind: 'entity' }],
    'plan.json': [{ op: 'remove-node', id: 'auth' }], // the tasks diagram cannot be edited: never sent
  };

  it('carries every open comment of the step and its diagram edits, and nothing else', () => {
    expect(changeRequest('spec', all, pending, '  keep it small ')).toEqual({
      step: 'spec', commentIds: ['c1', 'c3'],
      graphOps: [{ file: 'flow.graph.json', ops: pending['flow.graph.json'] }],
      note: 'keep it small',
    });
  });
  it('diagram edits alone are enough; both diagrams of a step go together', () => {
    expect(changeRequest('tech', [], { ...pending, 'architecture.graph.json': [{ op: 'remove-edge', id: 'e1' }] })).toEqual({
      step: 'tech', commentIds: [],
      graphOps: [{ file: 'architecture.graph.json', ops: [{ op: 'remove-edge', id: 'e1' }] }, { file: 'schema.graph.json', ops: pending['schema.graph.json'] }],
    });
  });
  it('comments alone: no graphOps field, no empty note', () => {
    expect(changeRequest('design', all, {})).toEqual({ step: 'design', commentIds: ['c6', 'c7'] });
    expect(changeRequest('plan', all, pending)).toEqual({ step: 'plan', commentIds: ['c8'] });
  });
  it('nothing open: nothing to send', () => {
    expect(changeRequest('brief', all, pending)).toBeNull(); // its only comment lost its place
    expect(changeRequest('review', all, pending)).toBeNull();
  });
  it('counts what would be sent', () => {
    expect(changeCount(all, pending, 'spec')).toBe(4); // 2 open comments + 2 edits
    expect(changeCount(all, pending, 'tech')).toBe(2); // 1 open comment + 1 edit
    expect(changeCount(all, pending, 'plan')).toBe(1);
    expect(changeCount(all, {}, 'brief')).toBe(0);
    expect(opsOfStep(pending, 'plan')).toEqual([]);
  });
  it('the request holds copies: sending does not hand out the store\'s own lists', () => {
    const req = changeRequest('spec', all, pending)!;
    expect(req.graphOps![0].ops).not.toBe(pending['flow.graph.json']);
  });
});
