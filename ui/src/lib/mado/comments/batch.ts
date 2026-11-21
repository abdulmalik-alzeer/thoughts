// MADO: which comments and diagram edits belong to which step, and how they become ONE change
// request (`POST /changes`). Plain functions, no window: tested in batch.test.ts.
import type { ChangeRequest, DocName } from '../api';
import type { Comment, CommentTarget, GraphName, GraphOp, StepId } from '../types';

/** The file in `<project>/mado/` behind each document and diagram. */
export const DOC_FILES: Record<DocName, string> = { brief: 'brief.md', spec: 'spec.md', tech: 'tech.md', design: 'design.md' };
export const GRAPH_FILES: Record<GraphName, string> = { flow: 'flow.graph.json', architecture: 'architecture.graph.json', schema: 'schema.graph.json', tasks: 'plan.json' };

/** What each step shows. The tasks diagram is drawn from the plan and cannot be edited. */
export const STEP_DOCS: Record<StepId, DocName[]> = { brief: ['brief'], spec: ['spec'], tech: ['tech'], design: ['design'], plan: [], review: [], build: [] };
export const STEP_GRAPHS: Record<StepId, GraphName[]> = { brief: [], spec: ['flow'], tech: ['architecture', 'schema'], design: [], plan: ['tasks'], review: [], build: [] };

const FILE_STEP: Record<string, StepId> = {
  'brief.md': 'brief', 'spec.md': 'spec', 'tech.md': 'tech', 'design.md': 'design',
  'flow.graph.json': 'spec', 'architecture.graph.json': 'tech', 'schema.graph.json': 'tech', 'plan.json': 'plan',
};

/** The step a comment belongs to (null: about a file this window does not know). */
export function stepOf(target: CommentTarget): StepId | null {
  switch (target.kind) {
    case 'text': case 'node': case 'edge': return FILE_STEP[target.file] ?? null;
    case 'token': case 'screen': return 'design';
    case 'task': return 'plan';
    default: return null;
  }
}

export const commentsOfStep = (comments: Comment[], step: StepId) => comments.filter((c) => stepOf(c.target) === step);
export const commentsOnDoc = (comments: Comment[], name: DocName) => comments.filter((c) => c.target.kind === 'text' && c.target.file === DOC_FILES[name]);

/** The comments a diagram marks on its nodes and edges. On the tasks diagram a node is a task. */
export function commentsOnGraph(comments: Comment[], name: GraphName): Comment[] {
  const file = GRAPH_FILES[name];
  if (name !== 'tasks') return comments.filter((c) => (c.target.kind === 'node' || c.target.kind === 'edge') && c.target.file === file);
  const out: Comment[] = [];
  for (const c of comments) {
    if (c.target.kind === 'task') out.push({ ...c, target: { kind: 'node', file, nodeId: c.target.taskId } });
    else if (c.target.kind === 'edge' && c.target.file === file) out.push(c);
  }
  return out;
}
export const commentsOnDesign = (comments: Comment[]) => comments.filter((c) => c.target.kind === 'token' || c.target.kind === 'screen');

/** What a diagram's "comment on this" means as a comment target. */
export function graphTarget(name: GraphName, on: { kind: 'node'; nodeId: string } | { kind: 'edge'; edgeId: string }): CommentTarget {
  if (on.kind === 'edge') return { kind: 'edge', file: GRAPH_FILES[name], edgeId: on.edgeId };
  return name === 'tasks' ? { kind: 'task', taskId: on.nodeId } : { kind: 'node', file: GRAPH_FILES[name], nodeId: on.nodeId };
}

/** Diagram edits not sent yet, per diagram file. */
export type PendingOps = Partial<Record<string, GraphOp[]>>;

/** The pending diagram edits of one step. */
export function opsOfStep(pending: PendingOps, step: StepId): { file: string; ops: GraphOp[] }[] {
  const out: { file: string; ops: GraphOp[] }[] = [];
  for (const name of STEP_GRAPHS[step]) {
    const file = GRAPH_FILES[name];
    const ops = pending[file];
    if (name !== 'tasks' && ops?.length) out.push({ file, ops: [...ops] });
  }
  return out;
}

/** How many things "Send changes" would send for this step: open comments plus diagram edits. */
export function changeCount(comments: Comment[], pending: PendingOps, step: StepId): number {
  const open = commentsOfStep(comments, step).filter((c) => c.status === 'open').length;
  return open + opsOfStep(pending, step).reduce((n, g) => n + g.ops.length, 0);
}

/** Everything open for a step as one change request; null when there is nothing to send. */
export function changeRequest(step: StepId, comments: Comment[], pending: PendingOps, note = ''): ChangeRequest | null {
  const commentIds = commentsOfStep(comments, step).filter((c) => c.status === 'open').map((c) => c.id);
  const graphOps = opsOfStep(pending, step);
  if (!commentIds.length && !graphOps.length) return null;
  const req: ChangeRequest = { step, commentIds };
  if (graphOps.length) req.graphOps = graphOps;
  if (note.trim()) req.note = note.trim();
  return req;
}
