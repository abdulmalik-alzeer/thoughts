// Round 0.5.0: a plan for a project that is already built, and the code graph card — the rules the
// Plan view draws from (how a plan starts, step names, switching kinds, the card's states).
import { describe, it, expect, vi } from 'vitest';

vi.mock('../i18n', () => ({ t: (k: string) => k }));
const { MadoStore } = await import('./store.svelte');
const { ApiError } = await import('../api');
const x = await import('./existing');
const dict = (await import('../i18n/mado-v050')).default;
import { STEPS, type CodeGraphStatus, type MadoOverview, type StepId, type StepStatus } from './types';
import type { MadoApi } from './api';

function overview(s: Partial<Record<StepId, StepStatus>>, extra: Partial<MadoOverview> = {}, mode?: 'existing'): MadoOverview {
  const steps = Object.fromEntries(STEPS.map((id) => [id, { status: s[id] ?? 'locked' }])) as any;
  return { project: { id: 'p1', name: 'Shop', path: '/p' }, exists: true, state: { version: 1, steps, ...(mode ? { mode } : {}) }, files: [], ...extra };
}
const none = (detected: MadoOverview['detected']): MadoOverview => ({ project: { id: 'p1', name: 'Shop', path: '/p' }, exists: false, state: null, files: [], detected });
const detected = (hasCode: boolean, more: Partial<NonNullable<MadoOverview['detected']>> = {}) => ({ hasCode, languages: [{ lang: 'TypeScript', files: 12 }, { lang: 'CSS', files: 3 }], files: 15, hasReadme: true, manifests: ['package.json'], capped: false, ...more });
const graph = (more: Partial<CodeGraphStatus> = {}): CodeGraphStatus => ({
  tool: { found: true, version: '2.4.0', usable: true }, built: false, builtAt: null, files: null, stale: false, building: false, progress: null, reason: null, error: null,
  recommend: { text: 'r', command: 'pipx install code-review-graph', url: 'https://code-review-graph.com/' }, ...more,
});

describe('starting a plan', () => {
  it('offers the two choices when the folder holds code, or when the app may not look yet', () => {
    expect(x.startChoice(none(detected(true)))).toBe('choose');
    expect(x.startChoice(none(null))).toBe('choose');
    expect(x.startChoice(none(detected(false)))).toBe('start');
    expect(x.startChoice(none(undefined))).toBe('start'); // a service from before 0.5.0
    expect(x.startChoice(overview({ brief: 'ready' }))).toBe('start');
    expect(x.startChoice(null)).toBe('start');
  });
  it('says what was found in one line, bounded', () => {
    expect(x.foundSummary(detected(true))).toEqual({ langs: [{ lang: 'TypeScript', files: 12 }, { lang: 'CSS', files: 3 }], more: false, files: 15, manifests: ['package.json'] });
    const many = detected(true, { languages: ['A', 'B', 'C', 'D', 'E'].map((lang) => ({ lang, files: 1 })) });
    expect(x.foundSummary(many, 4)!.langs.length).toBe(4);
    expect(x.foundSummary(many, 4)!.more).toBe(true);
    expect(x.foundSummary(detected(true, { capped: true }))!.more).toBe(true);
    expect(x.foundSummary(detected(true, { languages: [{ lang: '', files: 3 }, { lang: 'Go', files: 0 }, { lang: 'Rust', files: 2 }] }))!.langs).toEqual([{ lang: 'Rust', files: 2 }]);
    expect(x.foundSummary(null)).toBeNull();
  });
});

describe('the steps of a built project', () => {
  it('are named for what they do there; review and build are as before', () => {
    expect(STEPS.map((s) => x.stepNameKey(s, true))).toEqual(['mado-v050.step.brief', 'mado-v050.step.spec', 'mado-v050.step.tech', 'mado-v050.step.design', 'mado-v050.step.plan', 'mado.step.review', 'mado.step.build']);
    expect(STEPS.map((s) => x.stepNameKey(s, false))).toEqual(STEPS.map((s) => `mado.step.${s}`));
    expect(x.stepWhatKey('plan', true)).toBe('mado-v050.stepWhat.plan');
    expect(x.stepWhatKey('plan', false)).toBe('mado.stepWhat.plan');
    for (const s of ['brief', 'spec', 'tech', 'design', 'plan']) for (const k of [`mado-v050.step.${s}`, `mado-v050.stepWhat.${s}`]) { expect(dict.en[k as keyof typeof dict.en], k).toBeTruthy(); expect(dict.ar[k as keyof typeof dict.ar], k).toBeTruthy(); }
    expect(x.isExisting(overview({}, {}, 'existing'))).toBe(true);
    expect(x.isExisting(overview({}))).toBe(false);
  });
  it('the kind of plan may be switched only while nothing is approved and no brief is being written', () => {
    expect(x.modeSwitchable(overview({ brief: 'review' }))).toBe(true);
    expect(x.modeSwitchable(overview({ brief: 'drafting' }))).toBe(false);
    expect(x.modeSwitchable(overview({ brief: 'approved', spec: 'ready' }))).toBe(false);
    expect(x.modeSwitchable(overview({ brief: 'stale' }))).toBe(false);
    expect(x.modeSwitchable(none(detected(true)))).toBe(false);
  });
});

describe('the code graph card', () => {
  it('has one state for each answer, and builds only when it can', () => {
    const states: [Partial<CodeGraphStatus>, string][] = [
      [{ tool: { found: false, version: null, usable: false }, reason: { code: 'missing', message: 'm' } }, 'missing'],
      [{ tool: { found: true, version: '2.3.9', usable: false }, reason: { code: 'old-tool', message: 'm' } }, 'old'],
      [{ building: true, progress: { done: 2, total: 4 } }, 'building'],
      [{ reason: { code: 'no-repo', message: 'not a repository' } }, 'blocked'],
      [{ error: { message: 'failed' } }, 'failed'],
      [{ built: true, stale: true }, 'stale'],
      [{ built: true, builtAt: '2026-11-23T08:00:00Z', files: 6 }, 'ready'],
      [{}, 'unbuilt'],
    ];
    for (const [g, want] of states) expect(x.graphCardState(graph(g)), want).toBe(want);
    expect(x.graphCardState(null)).toBeNull();
    expect(x.graphCardState({} as any)).toBeNull();
    expect(['missing', 'old', 'building', 'blocked'].map((s) => x.canBuildGraph(s as any))).toEqual([false, false, false, false]);
    expect(['unbuilt', 'stale', 'ready', 'failed'].map((s) => x.canBuildGraph(s as any))).toEqual([true, true, true, true]);
    for (const s of ['missing', 'old', 'building', 'blocked', 'failed', 'stale', 'ready', 'unbuilt']) expect(dict.ar[`mado-v050.graph.state.${s}` as keyof typeof dict.ar], s).toBeTruthy();
  });
  it('shows progress as a share, and only a plain command and the tool’s own page', () => {
    expect(x.graphProgress(graph({ progress: { done: 3, total: 6 } }))).toBe(0.5);
    expect(x.graphProgress(graph({ progress: { done: 9, total: 6 } }))).toBe(1);
    expect(x.graphProgress(graph({ progress: { done: 1, total: 0 } }))).toBeNull();
    expect(x.graphProgress(graph())).toBeNull();
    expect(x.installCommand('pipx install code-review-graph')).toBe('pipx install code-review-graph');
    for (const bad of ['pipx install x; rm -rf ~', 'curl x | sh', '$(evil)', '', 7, 'a'.repeat(201)]) expect(x.installCommand(bad), String(bad)).toBeNull();
    expect(x.safeToolPage('https://code-review-graph.com/')).toBe('https://code-review-graph.com/');
    for (const bad of ['http://code-review-graph.com/', 'https://evil.example/', 'javascript:alert(1)', null]) expect(x.safeToolPage(bad)).toBeNull();
  });
  it('can be put away per project (and storage that throws is no trouble)', () => {
    x.setCardHidden('p1', true);
    expect(x.cardHidden('p1')).toBe(true);
    expect(x.cardHidden('p2')).toBe(false);
    x.setCardHidden('p1', false);
    expect(x.cardHidden('p1')).toBe(false);
    const get = Storage.prototype.getItem;
    Storage.prototype.getItem = () => { throw new Error('closed'); };
    try { expect(x.cardHidden('p1')).toBe(false); } finally { Storage.prototype.getItem = get; }
  });
});

describe('the store, round 0.5.0', () => {
  function fake() {
    const calls: string[] = [];
    let g = graph();
    const ov = overview({ brief: 'ready', plan: 'review' }, { files: ['change.md', 'plan.json'] }, 'existing');
    const api = {
      overview: async () => structuredClone(ov),
      comments: async () => [],
      start: async (mode?: string) => { calls.push(`start ${mode ?? ''}`.trim()); return {}; },
      doc: async (name: string) => { if (name === 'spec') throw new ApiError('spec.md is not shown: it seems to hold a secret', 422, 'validation'); return { text: '# ' + name }; },
      plan: async () => ({ version: 1, tasks: [], change: { title: 'Export', features: ['F2'] } }),
      graph: async () => ({ version: 1, kind: 'tasks', title: 't', nodes: [], edges: [] }),
      change: async () => ({ text: '# Export — Change', exists: true }),
      codeGraph: async () => { calls.push('codeGraph'); return structuredClone(g); },
      buildCodeGraph: async () => { calls.push('buildCodeGraph'); g = graph({ building: true }); return structuredClone(g); },
    } as unknown as MadoApi;
    return { api, calls, setGraph: (v: CodeGraphStatus) => { g = v; } };
  }
  it('starts with the mode asked for', async () => {
    const f = fake();
    const s = new MadoStore('p1', f.api);
    await s.start('existing');
    await s.start();
    expect(f.calls).toEqual(['start existing', 'start']);
  });
  it('shows the service’s reason in place of a file it will not show, and forgets it once it shows it', async () => {
    const f = fake();
    const s = new MadoStore('p1', f.api);
    await s.load();
    await s.refreshDoc('spec');
    expect(s.docs.spec).toBeNull();
    expect(s.refusal('doc:spec')).toMatch(/secret/);
    (f.api as any).doc = async () => ({ text: '# ok' });
    await s.refreshDoc('spec');
    expect(s.refusal('doc:spec')).toBe('');
    expect(s.docs.spec).toBe('# ok');
  });
  it('fetches the change with the plan step, and the card on a graph event', async () => {
    const f = fake();
    const s = new MadoStore('p1', f.api);
    await s.load();
    await s.ensureStep('plan');
    expect(s.change).toBe('# Export — Change');
    expect(s.plan?.change?.title).toBe('Export');
    await s.onEvent({ projectId: 'p1', what: 'graph' } as any);
    expect(f.calls).not.toContain('codeGraph'); // not asked for until the card is shown
    await s.refreshCodeGraph();
    f.setGraph(graph({ built: true, files: 6 }));
    await s.onEvent({ projectId: 'p1', what: 'graph' } as any);
    expect(s.codeGraph?.built).toBe(true);
    await s.buildCodeGraph();
    expect(s.codeGraph?.building).toBe(true);
  });
  it('a service without the card (before 0.5.0) has none', async () => {
    const f = fake();
    (f.api as any).codeGraph = async () => { throw new ApiError('not found', 404); };
    const s = new MadoStore('p1', f.api);
    await s.refreshCodeGraph();
    expect(s.codeGraph).toBeNull();
  });
});
