// The stepper's states in plain words, the tabs of each step, and the rules of the steps: what may
// be written, approved, reopened — and the review gate that keeps an unbuildable plan from being approved.
import { describe, it, expect, vi } from 'vitest';

vi.mock('../../lib/i18n', () => ({ t: (k: string) => k }));
const { neighbour, statusKey, stepperItems, tabKey, tabsOf } = await import('./steps');
const { blockingProblems, canApprove, canReopen, canRun, firstOpenStep, isEditable, statusOf } = await import('../../lib/mado/store.svelte');
const dict = (await import('../../lib/i18n/mado-plan')).default;
const more = (await import('../../lib/i18n/mado-v040')).default;
import { STEPS, type MadoOverview, type ProblemKind, type Review, type StepId, type StepStatus } from '../../lib/mado/types';

function overview(s: Partial<Record<StepId, StepStatus>>): MadoOverview {
  const steps = Object.fromEntries(STEPS.map((id) => [id, { status: s[id] ?? 'locked' }])) as MadoOverview['state'] extends infer T ? any : never;
  return { project: { id: 'p', name: 'Shop', path: '/p' }, exists: true, state: { version: 1, steps }, files: [] };
}
const review = (kinds: ProblemKind[], planHash = 'h1'): Review => ({ features: [], screens: [], waves: [], commands: ['npm test'], planHash, problems: kinds.map((kind) => ({ kind, message: kind })) });

describe('the stepper', () => {
  const o = overview({ brief: 'approved', spec: 'review', tech: 'drafting', design: 'stale', plan: 'ready' });
  const items = stepperItems(o, 'spec');

  it('lists the seven steps in order, each with its state', () => {
    expect(items.map((i) => i.id)).toEqual(['brief', 'spec', 'tech', 'design', 'plan', 'review', 'build']);
    expect(items.map((i) => i.n)).toEqual([1, 2, 3, 4, 5, 6, 7]);
    expect(items.map((i) => i.status)).toEqual(['approved', 'review', 'drafting', 'stale', 'ready', 'locked', 'locked']);
    expect(items.map((i) => i.tone)).toEqual(['done', 'you', 'busy', 'warn', 'next', 'off', 'off']);
    expect(items.filter((i) => i.current).map((i) => i.id)).toEqual(['spec']);
    expect(items.filter((i) => i.locked).map((i) => i.id)).toEqual(['review', 'build']);
  });

  it('every state of every step has plain words in English and in Arabic', () => {
    const statuses: StepStatus[] = ['locked', 'ready', 'drafting', 'review', 'approved', 'stale'];
    for (const step of STEPS) for (const s of statuses) {
      const k = statusKey(step, s);
      expect(dict.en[k as keyof typeof dict.en], k).toBeTruthy();
      expect(dict.ar[k as keyof typeof dict.ar], k).toBeTruthy();
      // No state is shown under its internal name.
      expect(dict.en[k as keyof typeof dict.en].toLowerCase()).not.toMatch(/\b(locked|drafting|stale)\b/);
    }
    expect(statusKey('review', 'ready')).toBe('mado.status.check');
    expect(statusKey('brief', 'ready')).toBe('mado.status.briefReady');
    expect(statusKey('build', 'approved')).toBe('mado.status.built');
    expect(statusKey('spec', 'stale')).toBe('mado.status.stale');
  });

  it('with nothing loaded every step is "not yet"', () => {
    expect(stepperItems(null, 'brief').every((i) => i.status === 'locked')).toBe(true);
    expect(statusOf(null, 'plan')).toBe('locked');
  });

  it('arrow keys move one step and stop at the ends', () => {
    expect(neighbour('spec', 1)).toBe('tech');
    expect(neighbour('spec', -1)).toBe('brief');
    expect(neighbour('brief', -1)).toBe('brief');
    expect(neighbour('build', 1)).toBe('build');
    expect(neighbour('plan', 'first')).toBe('brief');
    expect(neighbour('plan', 'last')).toBe('build');
  });

  it('opens on the earliest step that needs the user', () => {
    expect(firstOpenStep(o)).toBe('spec');
    expect(firstOpenStep(overview({ brief: 'ready' }))).toBe('brief');
    expect(firstOpenStep(overview(Object.fromEntries(STEPS.map((s) => [s, 'approved']))))).toBe('build');
    expect(firstOpenStep(null)).toBe('brief');
  });
});

describe('tabs', () => {
  it('each step shows its documents and diagrams', () => {
    const ids = (s: StepId) => tabsOf(s).map((x) => x.id);
    expect(ids('brief')).toEqual(['doc:brief']);
    expect(ids('spec')).toEqual(['doc:spec', 'graph:flow']);
    expect(ids('tech')).toEqual(['doc:tech', 'graph:architecture', 'graph:schema']);
    expect(ids('design')).toEqual(['design', 'doc:design']);
    expect(ids('plan')).toEqual(['graph:tasks', 'tasks']);
    expect(ids('review')).toEqual([]);
    expect(ids('build')).toEqual([]);
  });
  it('every tab has a name in both languages, and no two tabs of a step share one', () => {
    for (const s of STEPS) {
      const keys = tabsOf(s).map(tabKey);
      expect(new Set(keys).size).toBe(keys.length);
      for (const k of keys) { expect(dict.en[k as keyof typeof dict.en], k).toBeTruthy(); expect(dict.ar[k as keyof typeof dict.ar], k).toBeTruthy(); }
    }
  });
});

describe('what may be done in a step', () => {
  it('writing: only steps the assistant writes, only while they are with the user', () => {
    expect((['locked', 'ready', 'drafting', 'review', 'approved', 'stale'] as StepStatus[]).map((s) => canRun('spec', s))).toEqual([false, true, false, true, false, true]);
    expect(canRun('review', 'ready')).toBe(false);
    expect(canRun('build', 'ready')).toBe(false);
  });
  it('editing and commenting stop while it is being written or approved', () => {
    expect((['locked', 'ready', 'drafting', 'review', 'approved', 'stale'] as StepStatus[]).map(isEditable)).toEqual([false, true, false, true, false, true]);
  });
  it('approving and reopening', () => {
    expect(canApprove('spec', 'review')).toBe(true);
    expect(canApprove('spec', 'stale')).toBe(true);
    expect(canApprove('spec', 'ready')).toBe(false); // nothing written yet
    expect(canApprove('spec', 'drafting')).toBe(false);
    expect(canApprove('spec', 'approved')).toBe(false);
    expect(canApprove('build', 'ready')).toBe(false);
    expect(canReopen('spec', 'approved')).toBe(true);
    expect(canReopen('spec', 'review')).toBe(false);
    expect(canReopen('build', 'approved')).toBe(false);
  });
});

describe('the review gate', () => {
  it('Approve is off while a problem that makes the plan unbuildable exists', () => {
    for (const kind of ['cycle', 'missing-dep', 'bad-path', 'bad-command', 'empty-acceptance', 'unknown-feature'] as ProblemKind[]) {
      expect(canApprove('review', 'ready', review([kind])), kind).toBe(false);
      expect(canApprove('review', 'ready', review(['overlap', kind, 'untraced-screen'])), kind).toBe(false);
    }
  });
  it('other problems are shown but do not stop the approval', () => {
    const soft: ProblemKind[] = ['overlap', 'untraced-feature', 'untraced-screen'];
    expect(canApprove('review', 'ready', review(soft))).toBe(true);
    expect(blockingProblems(review(soft))).toEqual([]);
    expect(blockingProblems(review(['cycle', 'overlap', 'bad-path'])).map((p) => p.kind)).toEqual(['cycle', 'bad-path']);
  });
  it('off until the review is here, names its plan, and the step is open', () => {
    expect(canApprove('review', 'ready', null)).toBe(false);
    expect(canApprove('review', 'ready', review([], ''))).toBe(false);
    expect(canApprove('review', 'locked', review([]))).toBe(false);
    expect(canApprove('review', 'approved', review([]))).toBe(false);
    expect(canApprove('review', 'ready', review([]))).toBe(true);
    expect(canApprove('review', 'stale', review([]))).toBe(true);
  });
  it('every kind of problem has plain words in both languages', () => {
    const kinds: ProblemKind[] = ['cycle', 'missing-dep', 'overlap', 'untraced-feature', 'untraced-screen', 'unknown-feature', 'empty-acceptance', 'bad-path'];
    for (const k of kinds) { expect(dict.en[`mado.problem.${k}`], k).toBeTruthy(); expect(dict.ar[`mado.problem.${k}`], k).toBeTruthy(); }
  });
});

describe('round 0.4.0', () => {
  it('shows the conventions beside the technical design only when the service lists the file', () => {
    expect(tabsOf('tech').map((x) => x.id)).not.toContain('doc:conventions');
    expect(tabsOf('tech', ['tech.md', 'conventions.md']).map((x) => x.id)).toEqual(['doc:tech', 'doc:conventions', 'graph:architecture', 'graph:schema']);
    expect(tabsOf('spec', ['conventions.md']).map((x) => x.id)).not.toContain('doc:conventions');
    expect(tabKey({ id: 'doc:conventions', kind: 'conventions' })).toBe('mado.tab.conventions');
    expect(more.en['mado.tab.conventions'] && more.ar['mado.tab.conventions']).toBeTruthy();
  });
  it('has plain words for the new kinds of problem, none of which blocks the build', () => {
    const kinds: ProblemKind[] = ['no-verify', 'no-run', 'no-contracts', 'no-tests', 'unchecked-screen'];
    for (const k of kinds) { expect(dict.en[`mado.problem.${k}`], k).toBeTruthy(); expect(dict.ar[`mado.problem.${k}`], k).toBeTruthy(); }
    expect(blockingProblems(review(kinds))).toEqual([]);
  });
});

describe('the dictionary', () => {
  it('has Arabic for every English string, with the same placeholders', () => {
    const base = (k: string) => k.replace(/_(zero|one|two|few|many|other)$/, '');
    const arBases = new Set(Object.keys(dict.ar).map(base));
    for (const [k, v] of Object.entries(dict.en)) {
      expect(arBases.has(base(k)), k).toBe(true);
      const ar = (dict.ar as Record<string, string>)[k];
      if (ar && !/_(one|two)$/.test(k)) expect((ar.match(/\{\w+\}/g) ?? []).sort(), k).toEqual((v.match(/\{\w+\}/g) ?? []).sort());
    }
  });
  it('every key a view asks for exists', () => {
    const files = import.meta.glob(['./*.svelte', '../../lib/mado/**/*.svelte', '../Projects.svelte'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    // The strings of round 0.4.0 live in a dictionary of their own (lib/i18n/mado-v040.ts).
    const en = { ...dict.en, ...more.en } as Record<string, string>;
    const missing: string[] = [];
    for (const text of Object.values(files)) {
      for (const m of text.matchAll(/'(mado\.[A-Za-z0-9.-]+)'/g)) {
        const k = m[1];
        if (!en[k] && !en[`${k}_other`]) missing.push(k);
      }
    }
    expect(missing).toEqual([]);
  });
});
