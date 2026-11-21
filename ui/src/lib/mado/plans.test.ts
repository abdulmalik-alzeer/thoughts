// Round 0.4.0: the plain rules behind the sidebar's Plans group, the task kinds, the screen check's
// answer and the review's findings. Everything here takes text it must not trust.
import { describe, it, expect, vi } from 'vitest';

vi.mock('../i18n', () => ({ t: (k: string) => k }));
import { askedScreens, clampResults, cleanPlans, findingCounts, kindKey, localUrl, planMark, planWhere, underWay } from './plans';
import type { PlanEntry } from './types';
import dict from '../i18n/mado-plan';
import more from '../i18n/mado-v040';

const entry = (o: Partial<PlanEntry>): PlanEntry => ({ projectId: 'p1', name: 'Shop', step: 'spec', stepStatus: 'review', updatedAt: '2026-11-21T08:00:00Z', ...o });

describe('the list of plans', () => {
  it('keeps well-formed entries only, newest first, one per project', () => {
    const list = cleanPlans([
      { projectId: 'a', name: 'Old', step: 'brief', stepStatus: 'ready', updatedAt: '2026-11-01T00:00:00Z' },
      { projectId: '../etc', name: 'Bad id', step: 'brief', stepStatus: 'ready', updatedAt: '2026-11-30T00:00:00Z' },
      { projectId: 'b', name: 'New', step: 'build', stepStatus: 'ready', build: 'running', updatedAt: '2026-11-20T00:00:00Z' },
      { projectId: 'a', name: 'Twice', step: 'brief', stepStatus: 'ready', updatedAt: '2026-11-25T00:00:00Z' },
      null, 'text', 7,
    ]);
    expect(list.map((p) => p.projectId)).toEqual(['b', 'a']);
    expect(list[1].name).toBe('Old');
  });
  it('is empty for anything that is not a list', () => {
    expect(cleanPlans(null)).toEqual([]);
    expect(cleanPlans({ error: 'no' })).toEqual([]);
  });
  it('replaces what it does not know with something safe and cuts long names', () => {
    const [p] = cleanPlans([{ projectId: 'a', name: 'x'.repeat(900), step: 'launch', stepStatus: 'flying', build: 'exploding', updatedAt: 5 }]);
    expect(p.step).toBe('brief');
    expect(p.stepStatus).toBe('ready');
    expect(p.build).toBeUndefined();
    expect(p.name.length).toBe(200);
  });
});

describe('where a plan stands', () => {
  it('marks a build at work, a step being written, and a build that waits for the user', () => {
    expect(planMark(entry({ build: 'running' }))).toBe('run');
    expect(planMark(entry({ build: 'checking' }))).toBe('run');
    expect(planMark(entry({ build: 'reviewing' }))).toBe('run');
    expect(planMark(entry({ stepStatus: 'drafting' }))).toBe('run');
    expect(planMark(entry({ build: 'waiting' }))).toBe('you');
    expect(planMark(entry({ build: 'done' }))).toBe('');
    expect(planMark(entry({}))).toBe('');
  });
  it('counts the two new phases as under way', () => {
    expect(['running', 'checking', 'reviewing'].every((s) => underWay(s as any))).toBe(true);
    expect(['idle', 'paused', 'waiting', 'failed', 'done', 'stopped', undefined].some((s) => underWay(s as any))).toBe(false);
  });
  it('says the step and its state in words that exist in both languages', () => {
    const all = { ...dict.en, ...more.en } as Record<string, string>, allAr = { ...dict.ar, ...more.ar } as Record<string, string>;
    const cases: PlanEntry[] = [
      entry({}), entry({ step: 'brief', stepStatus: 'ready' }), entry({ step: 'review', stepStatus: 'ready' }), entry({ step: 'tech', stepStatus: 'drafting' }),
      entry({ step: 'build', stepStatus: 'ready' }), entry({ step: 'build', stepStatus: 'approved', build: 'done' }),
      ...(['running', 'paused', 'waiting', 'failed', 'done', 'stopped', 'checking', 'reviewing'] as const).map((build) => entry({ step: 'build', stepStatus: 'ready', build })),
    ];
    for (const c of cases) {
      const w = planWhere(c);
      for (const k of [w.stepKey, w.stateKey]) { expect(all[k], k).toBeTruthy(); expect(allAr[k], k).toBeTruthy(); }
    }
    expect(planWhere(entry({ step: 'build', stepStatus: 'ready', build: 'checking' })).stateKey).toBe('mado.build.checking');
    expect(planWhere(entry({ step: 'build', stepStatus: 'approved', build: 'done' })).stateKey).toBe('mado.status.built');
    expect(planWhere(entry({ step: 'review', stepStatus: 'review' })).stateKey).toBe('mado.status.check');
    // A step reopened while an old build's record is still there reads as the step, not as the build.
    expect(planWhere(entry({ step: 'spec', stepStatus: 'review', build: 'done' }))).toEqual({ stepKey: 'mado.step.spec', stateKey: 'mado.status.review' });
  });
});

describe('a task’s kind', () => {
  it('has a plain label for the two special kinds, in both languages, and none for a feature', () => {
    expect(kindKey('feature')).toBe('');
    expect(kindKey(undefined)).toBe('');
    expect(kindKey('nonsense' as any)).toBe('');
    for (const k of ['contracts', 'tests'] as const) {
      expect(more.en[`mado.kind.${k}`]).toBeTruthy(); expect(more.ar[`mado.kind.${k}`]).toBeTruthy();
      expect(more.en[`mado.kind.${k}.tip`]).toBeTruthy(); expect(more.ar[`mado.kind.${k}.tip`]).toBeTruthy();
    }
    expect(more.en['mado.kind.contracts']).toBe('Agreed examples');
    expect(more.en['mado.kind.tests']).toBe('Tests first');
  });
});

describe('the address of the built product', () => {
  it('is local, plain http and has a port, or it is nothing', () => {
    expect(localUrl('http://localhost:4173')).toBe('http://localhost:4173');
    expect(localUrl('http://127.0.0.1:8080/')).toBe('http://127.0.0.1:8080');
    for (const bad of ['https://localhost:4173', 'http://localhost', 'http://localhost:4173/admin', 'http://localhost:4173@evil.example', 'http://evil.example:80', 'http://localhost:99999', 'http://localhost:0', 'javascript:alert(1)', '', null, 7]) expect(localUrl(bad), String(bad)).toBe('');
  });
  it('keeps only paths that stay on that address', () => {
    expect(askedScreens([{ id: 'a', path: '/' }, { id: 'b', path: '//evil.example/x' }, { id: 'c', path: 'http://evil.example' }, { id: 'd', path: '/a b' }, { id: 'a', path: '/twice' }, { id: '', path: '/x' }, null, { id: 'e', path: '/orders/1?x=1' }]))
      .toEqual([{ id: 'a', path: '/' }, { id: 'e', path: '/orders/1?x=1' }]);
    expect(askedScreens(Array.from({ length: 90 }, (_, i) => ({ id: 's' + i, path: '/' + i }))).length).toBe(60);
    expect(askedScreens('no')).toEqual([]);
  });
});

describe('what the screens showed', () => {
  const asked = [{ id: 'shop', path: '/' }, { id: 'board', path: '/board' }];
  it('is cut to the contract’s sizes', () => {
    const [r] = clampResults([{ id: 'shop', path: '/elsewhere', ok: true, status: 200, errors: Array.from({ length: 50 }, () => 'e'.repeat(900)), text: 't'.repeat(9000), fields: Array.from({ length: 99 }, () => ({ label: 'l'.repeat(999), value: 'v'.repeat(999) })) }], asked);
    expect(r.path).toBe('/'); // the path that was asked for, not the one that came back
    expect(r.text.length).toBe(4000);
    expect(r.errors.length).toBe(20);
    expect(r.errors[0].length).toBe(500);
    expect(r.fields.length).toBe(60);
    expect(r.fields[0]).toEqual({ label: 'l'.repeat(200), value: 'v'.repeat(500) });
  });
  it('keeps only the screens that were asked for, and survives nonsense', () => {
    const out = clampResults([{ id: 'other', ok: true }, { id: 'board', ok: 'yes', status: 'abc', errors: 'x', fields: [null, { label: 3 }] }, null, 'x'], asked);
    expect(out).toEqual([{ id: 'board', path: '/board', ok: false, errors: [], text: '', fields: [{ label: '', value: '' }, { label: '3', value: '' }] }]);
    expect(clampResults(null, asked)).toEqual([]);
  });
});

describe('the review’s findings', () => {
  it('are counted by weight', () => {
    expect(findingCounts({ status: 'changes', round: 2, findings: [{ severity: 'must', text: 'a' }, { severity: 'should', text: 'b' }, { severity: 'should', text: 'c' }] })).toEqual({ must: 1, should: 2, all: 3 });
    expect(findingCounts(null)).toEqual({ must: 0, should: 0, all: 0 });
    expect(findingCounts({ status: 'passed', round: 1 } as any)).toEqual({ must: 0, should: 0, all: 0 });
  });
});

describe('the new words', () => {
  it('exist in Arabic for every English string, with the same placeholders', () => {
    const base = (k: string) => k.replace(/_(zero|one|two|few|many|other)$/, '');
    const arBases = new Set(Object.keys(more.ar).map(base));
    const holes = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort().join();
    for (const [k, v] of Object.entries(more.en)) {
      expect(arBases.has(base(k)), k).toBe(true);
      const ar = (more.ar as Record<string, string>)[k];
      if (ar && !/_(one|two)$/.test(k)) expect(holes(ar), k).toBe(holes(v));
    }
  });
  it('cover every state the Build view can show', () => {
    const en = { ...dict.en, ...more.en } as Record<string, string>;
    for (const s of ['checking', 'reviewing']) { expect(en[`mado.build.${s}`]).toBeTruthy(); expect(en[`mado.build.${s}.what`]).toBeTruthy(); }
    expect(en['mado.build.checking']).toBe('Looking at the screens');
    for (const s of ['waiting', 'running', 'done', 'skipped']) expect(en[`mado.screens.${s}`], s).toBeTruthy();
    for (const s of ['running', 'passed', 'changes', 'failed']) expect(en[`mado.breview.${s}`], s).toBeTruthy();
    for (const s of ['looking', 'sent', 'cannot', 'failed']) expect(en[`mado.screens.look.${s}`], s).toBeTruthy();
    expect(en['mado.entry']).toBe('Build with MADO');
  });
});
