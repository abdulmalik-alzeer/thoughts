// MADO: how the seven steps are shown — plain words for each state, what the stepper reads out,
// which tabs a step has. Plain functions (steps.test.ts); the rules themselves are in the store.
import type { DocName } from '../../lib/mado/api';
import { STEP_DOCS, STEP_GRAPHS } from '../../lib/mado/comments/batch';
import { statusOf } from '../../lib/mado/store.svelte';
import { STEPS, type GraphName, type MadoOverview, type StepId, type StepStatus } from '../../lib/mado/types';

export type Tone = 'off' | 'next' | 'busy' | 'you' | 'done' | 'warn';
/** How a state is drawn. Never by colour alone: each one also has its own words and its own mark. */
export const TONE: Record<StepStatus, Tone> = { locked: 'off', ready: 'next', drafting: 'busy', review: 'you', approved: 'done', stale: 'warn' };

export interface StepperItem {
  id: StepId;
  /** 1-based place in the order. */
  n: number;
  status: StepStatus;
  tone: Tone;
  /** i18n keys of its name and of its state in plain words. */
  nameKey: string;
  statusKey: string;
  /** A locked step can be looked at (it explains what will happen) but nothing can be done in it. */
  locked: boolean;
  current: boolean;
}

/** `review` reads differently for the steps nobody drafts: there it is simply the user's turn. */
export function statusKey(step: StepId, status: StepStatus): string {
  if (step === 'build') return status === 'locked' ? 'mado.status.locked' : status === 'approved' ? 'mado.status.built' : 'mado.status.buildReady';
  if (step === 'review' && (status === 'ready' || status === 'review')) return 'mado.status.check';
  if (step === 'brief' && status === 'ready') return 'mado.status.briefReady';
  return `mado.status.${status}`;
}

export function stepperItems(o: MadoOverview | null, current: StepId): StepperItem[] {
  return STEPS.map((id, i) => {
    const status = statusOf(o, id);
    return { id, n: i + 1, status, tone: TONE[status], nameKey: `mado.step.${id}`, statusKey: statusKey(id, status), locked: status === 'locked', current: id === current };
  });
}

/** The step the arrow keys move to from `from` (`dir` already accounts for right-to-left). */
export function neighbour(from: StepId, dir: -1 | 1 | 'first' | 'last'): StepId {
  if (dir === 'first') return STEPS[0];
  if (dir === 'last') return STEPS[STEPS.length - 1];
  const i = STEPS.indexOf(from);
  return STEPS[Math.min(STEPS.length - 1, Math.max(0, i + dir))];
}

export type Tab =
  | { id: string; kind: 'doc'; name: DocName }
  | { id: string; kind: 'graph'; name: GraphName }
  | { id: string; kind: 'tasks' }
  | { id: string; kind: 'design' };

/** The tabs of a step's workspace, in the order they are shown. */
export function tabsOf(step: StepId): Tab[] {
  const tabs: Tab[] = [];
  if (step === 'design') tabs.push({ id: 'design', kind: 'design' });
  for (const name of STEP_DOCS[step]) tabs.push({ id: `doc:${name}`, kind: 'doc', name });
  for (const name of STEP_GRAPHS[step]) tabs.push({ id: `graph:${name}`, kind: 'graph', name });
  if (step === 'plan') tabs.push({ id: 'tasks', kind: 'tasks' });
  return tabs;
}
export const tabKey = (tab: Tab) => (tab.kind === 'doc' ? `mado.tab.${tab.name}` : tab.kind === 'graph' ? (tab.name === 'tasks' ? 'mado.tab.graph.tasks' : `mado.tab.${tab.name}`) : tab.kind === 'design' ? 'mado.tab.designStep' : 'mado.tab.tasks');
