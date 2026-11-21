// MADO: the window's client for /api/mado/:projectId (docs/mado-spec.md). One function per route,
// nothing else: what a reply means is the store's business. Ids and names are checked here before
// they go into an address, so nothing typed or read from a file can reach another route.
import { api, del, patch, post, put } from '../api';
import type {
  BuildState, Comment, CommentStatus, CommentTarget, DesignTokens, GraphDoc, GraphName, GraphOp, MadoOverview,
  PlanDoc, Pos, Review, ScreenInfo, StepId, StyleCandidate,
} from './types';
import { STEPS } from './types';

export type DocName = 'brief' | 'spec' | 'tech' | 'design';
export const DOC_NAMES: readonly DocName[] = ['brief', 'spec', 'tech', 'design'];
export const GRAPH_NAMES: readonly GraphName[] = ['flow', 'architecture', 'schema', 'tasks'];

/** What one change request carries (`POST /changes`). */
export interface ChangeRequest { step: StepId; commentIds: string[]; graphOps?: { file: string; ops: GraphOp[] }[]; note?: string }

const ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/;
function seg(v: string, what: string): string {
  if (typeof v !== 'string' || !ID.test(v)) throw new Error(`MADO: not a valid ${what}`);
  return encodeURIComponent(v);
}
function oneOf<T extends string>(v: string, list: readonly T[], what: string): T {
  if (!list.includes(v as T)) throw new Error(`MADO: not a valid ${what}`);
  return v as T;
}

/** The client for one project. */
export function madoApi(projectId: string) {
  const base = `/mado/${seg(projectId, 'project')}`;
  const step = (s: StepId) => oneOf(s, STEPS, 'step');
  return {
    overview: () => api<MadoOverview>(base),
    start: () => post<unknown>(`${base}/start`),

    doc: (name: DocName) => api<{ text: string }>(`${base}/doc/${oneOf(name, DOC_NAMES, 'document')}`),
    saveDoc: (name: DocName, text: string) => put<unknown>(`${base}/doc/${oneOf(name, DOC_NAMES, 'document')}`, { text }),

    graph: (name: GraphName) => api<GraphDoc>(`${base}/graph/${oneOf(name, GRAPH_NAMES, 'diagram')}`),
    savePositions: (name: GraphName, positions: Record<string, Pos>) =>
      put<unknown>(`${base}/graph/${oneOf(name, GRAPH_NAMES, 'diagram')}/positions`, { positions }),
    plan: () => api<PlanDoc>(`${base}/plan`),

    run: (s: StepId, note?: string) => post<{ chatId: string }>(`${base}/steps/${step(s)}/run`, note?.trim() ? { note: note.trim() } : {}),
    approve: (s: StepId) => post<unknown>(`${base}/steps/${step(s)}/approve`),
    reopen: (s: StepId) => post<unknown>(`${base}/steps/${step(s)}/reopen`),

    comments: async (): Promise<Comment[]> => {
      // The contract's file shape is `{version, comments}`; a bare list is accepted too.
      const r = await api<{ comments?: Comment[] } | Comment[]>(`${base}/comments`);
      return Array.isArray(r) ? r : Array.isArray(r?.comments) ? r.comments : [];
    },
    addComment: (target: CommentTarget, body: string) => post<Comment>(`${base}/comments`, { target, body }),
    editComment: (id: string, change: { body?: string; status?: CommentStatus }) => patch<Comment>(`${base}/comments/${seg(id, 'comment')}`, change),
    removeComment: (id: string) => del<unknown>(`${base}/comments/${seg(id, 'comment')}`),

    changes: (req: ChangeRequest) => {
      const body: ChangeRequest = { step: step(req.step), commentIds: req.commentIds };
      if (req.graphOps?.length) body.graphOps = req.graphOps;
      if (req.note?.trim()) body.note = req.note.trim();
      return post<{ chatId: string }>(`${base}/changes`, body);
    },
    review: () => api<Review>(`${base}/review`),

    styles: () => api<{ styles: StyleCandidate[]; picked?: string }>(`${base}/design/styles`),
    pickStyle: (id: string) => post<unknown>(`${base}/design/style`, { id }),
    tokens: () => api<DesignTokens>(`${base}/design/tokens`),
    saveTokens: (tokens: DesignTokens) => put<unknown>(`${base}/design/tokens`, tokens),
    screens: () => api<ScreenInfo[]>(`${base}/design/screens`),
    screen: (id: string) => api<{ html: string }>(`${base}/design/screens/${seg(id, 'screen')}`),

    build: () => api<BuildState>(`${base}/build`),
    buildStart: (maxParallel?: number) => post<unknown>(`${base}/build/start`, maxParallel ? { maxParallel: clampParallel(maxParallel) } : {}),
    buildPause: () => post<unknown>(`${base}/build/pause`),
    buildResume: () => post<unknown>(`${base}/build/resume`),
    buildStop: () => post<unknown>(`${base}/build/stop`),
    retryTask: (id: string) => post<unknown>(`${base}/build/tasks/${seg(id, 'task')}/retry`),
    land: () => post<unknown>(`${base}/build/land`),
  };
}
export type MadoApi = ReturnType<typeof madoApi>;

/** Builders working side by side: 1 to 6. */
export const clampParallel = (n: number) => Math.min(6, Math.max(1, Math.round(Number(n) || 1)));
