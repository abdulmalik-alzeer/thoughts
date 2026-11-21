// MADO, round 0.4.0: plain rules for what the window shows about plans and builds (plans.test.ts).
// Everything that comes from the service or from a looked-at screen is checked and cut to size here
// before it is shown or sent on.
import { STEPS, type BuildReview, type BuildStatus, type PlanEntry, type ScreenResult, type StepId, type TaskKind } from './types';

const ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/;
const BUILD: readonly BuildStatus[] = ['idle', 'running', 'paused', 'waiting', 'failed', 'done', 'stopped', 'checking', 'reviewing'];
const STEP_STATUS = ['locked', 'ready', 'drafting', 'review', 'approved', 'stale'];
const cut = (v: unknown, max: number) => (typeof v === 'string' ? v : v == null ? '' : String(v)).slice(0, max);

/** A build that is doing something by itself right now (`checking` and `reviewing` count). */
export const underWay = (s: BuildStatus | undefined | null) => s === 'running' || s === 'checking' || s === 'reviewing';

/** `GET /api/mado` as a list the sidebar can trust: known shapes only, most recently touched first. */
export function cleanPlans(raw: unknown): PlanEntry[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  const out: PlanEntry[] = [];
  for (const r of raw.slice(0, 200)) {
    if (!r || typeof r !== 'object') continue;
    const e = r as Record<string, unknown>;
    const projectId = String(e.projectId ?? '');
    if (!ID.test(projectId) || seen.has(projectId)) continue;
    seen.add(projectId);
    const step = STEPS.includes(e.step as StepId) ? String(e.step) : 'brief';
    const stepStatus = STEP_STATUS.includes(String(e.stepStatus)) ? String(e.stepStatus) : 'ready';
    const build = BUILD.includes(e.build as BuildStatus) ? (e.build as BuildStatus) : undefined;
    out.push({ projectId, name: cut(e.name, 200).trim(), step, stepStatus, ...(build ? { build } : {}), updatedAt: cut(e.updatedAt, 40) });
  }
  return out.sort((a, b) => (a.updatedAt < b.updatedAt ? 1 : a.updatedAt > b.updatedAt ? -1 : 0));
}

/** The small mark beside a plan: something is at work, or it waits for the user. */
export function planMark(e: PlanEntry): 'run' | 'you' | '' {
  if (e.build === 'waiting') return 'you';
  if (underWay(e.build) || e.stepStatus === 'drafting') return 'run';
  return '';
}

/** Where a plan stands: the i18n key of its step and of that step's state in plain words. */
export function planWhere(e: PlanEntry): { stepKey: string; stateKey: string } {
  const step = e.step as StepId;
  const stepKey = `mado.step.${step}`;
  if (step === 'build' && e.stepStatus === 'approved' && !underWay(e.build)) return { stepKey, stateKey: 'mado.status.built' };
  if (e.build && e.build !== 'idle' && (step === 'build' || underWay(e.build) || e.build === 'waiting')) return { stepKey: 'mado.step.build', stateKey: `mado.build.${e.build}` };
  if (step === 'build') return { stepKey, stateKey: e.stepStatus === 'locked' ? 'mado.status.locked' : e.stepStatus === 'approved' ? 'mado.status.built' : 'mado.status.buildReady' };
  if (step === 'review' && (e.stepStatus === 'ready' || e.stepStatus === 'review')) return { stepKey, stateKey: 'mado.status.check' };
  if (step === 'brief' && e.stepStatus === 'ready') return { stepKey, stateKey: 'mado.status.briefReady' };
  return { stepKey, stateKey: `mado.status.${e.stepStatus}` };
}

/** The plain label of a task's kind (none for an ordinary feature task). */
export const kindKey = (k: TaskKind | undefined | null): string => (k === 'contracts' || k === 'tests' ? `mado.kind.${k}` : '');

/** Only `http://localhost:<port>` or `http://127.0.0.1:<port>` is an address of the built product. */
export function localUrl(url: unknown): string {
  const m = /^http:\/\/(localhost|127\.0\.0\.1):(\d{1,5})\/?$/.exec(typeof url === 'string' ? url : '');
  return m && Number(m[2]) > 0 && Number(m[2]) < 65536 ? `http://${m[1]}:${m[2]}` : '';
}

/** The screens the build asked to have looked at, as far as they are well formed (at most 60). */
export function askedScreens(raw: unknown): { id: string; path: string }[] {
  if (!Array.isArray(raw)) return [];
  const out: { id: string; path: string }[] = [];
  for (const s of raw) {
    const id = String(s?.id ?? ''), path = String(s?.path ?? '');
    if (!id || id.length > 128 || !path.startsWith('/') || path.startsWith('//') || path.length > 2000 || /[\s\\]/.test(path)) continue;
    if (out.some((o) => o.id === id)) continue;
    out.push({ id, path });
    if (out.length >= 60) break;
  }
  return out;
}

/** What the shell saw, cut to the contract's sizes and kept to the screens that were asked for.
 *  All of it is untrusted text: it is only ever sent on as data. */
export function clampResults(results: unknown, asked: { id: string; path: string }[]): ScreenResult[] {
  if (!Array.isArray(results)) return [];
  const out: ScreenResult[] = [];
  for (const want of asked) {
    const r = results.find((x) => x && typeof x === 'object' && (x as ScreenResult).id === want.id) as Partial<ScreenResult> | undefined;
    if (!r) continue;
    const status = Number(r.status);
    out.push({
      id: want.id, path: want.path, ok: r.ok === true,
      ...(Number.isInteger(status) && status >= 0 && status < 1000 ? { status } : {}),
      errors: (Array.isArray(r.errors) ? r.errors : []).slice(0, 20).map((e) => cut(e, 500)),
      text: cut(r.text, 4000),
      fields: (Array.isArray(r.fields) ? r.fields : []).slice(0, 60).map((f) => ({ label: cut(f?.label, 200), value: cut(f?.value, 500) })),
    });
  }
  return out;
}

/** How many findings the review still has, by weight. */
export function findingCounts(review: BuildReview | null | undefined): { must: number; should: number; all: number } {
  const list = Array.isArray(review?.findings) ? review!.findings : [];
  const must = list.filter((f) => f?.severity === 'must').length;
  return { must, should: list.length - must, all: list.length };
}
