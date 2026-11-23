// MADO, round 0.5.0 (docs/mado-spec.md "Round 0.5.0"): a plan for a project that is already built, and
// the code graph of the user's own code-review-graph tool. Plain functions over what the service
// answers (existing.test.ts); the views only draw what these say.
import type { CodeGraphStatus, Detected, MadoOverview, StepId } from './types';
import { STEPS } from './types';

export const isExisting = (o: MadoOverview | null | undefined) => o?.state?.mode === 'existing';

/** How a plan is started. 'choose': the folder holds code (or the app may not look yet, so it cannot
 *  tell): offer "Document this project as it is" and "Plan a new project here". 'start': the usual
 *  start (an empty folder, or a service from before 0.5.0 that does not say). */
export function startChoice(o: MadoOverview | null | undefined): 'choose' | 'start' {
  if (!o || o.exists) return 'start';
  if (o.detected === null) return 'choose';
  return o.detected?.hasCode ? 'choose' : 'start';
}

/** What was found, for one line: the languages with the most files first (at most `max`), and
 *  whether there is more than that (more languages, or a walk that stopped at its limits). */
export function foundSummary(d: Detected | null | undefined, max = 4): { langs: { lang: string; files: number }[]; more: boolean; files: number; manifests: string[] } | null {
  if (!d || typeof d !== 'object') return null;
  const all = (Array.isArray(d.languages) ? d.languages : [])
    .filter((l) => l && typeof l.lang === 'string' && l.lang.trim() && Number.isFinite(l.files) && l.files > 0)
    .map((l) => ({ lang: l.lang.slice(0, 40), files: Math.floor(l.files) }));
  const manifests = (Array.isArray(d.manifests) ? d.manifests : []).filter((m) => typeof m === 'string' && m.trim()).slice(0, 3).map((m) => m.slice(0, 120));
  return { langs: all.slice(0, max), more: all.length > max || !!d.capped, files: Math.max(0, Math.floor(Number(d.files) || 0)), manifests };
}

/** The steps whose names and words differ in existing mode (review and build are as before). */
const RENAMED: readonly StepId[] = ['brief', 'spec', 'tech', 'design', 'plan'];
/** The i18n key of a step's name, in the plan's mode. */
export const stepNameKey = (step: StepId, existing: boolean) => (existing && RENAMED.includes(step) ? `mado-v050.step.${step}` : `mado.step.${step}`);
/** The i18n key of what a step is for, in the plan's mode. */
export const stepWhatKey = (step: StepId, existing: boolean) => (existing && RENAMED.includes(step) ? `mado-v050.stepWhat.${step}` : `mado.stepWhat.${step}`);

/** May the plan's kind still be switched? Only while no step is approved or stale and no brief is
 *  being written (the service says the same, `mado.mode_locked`, and refuses otherwise). */
export function modeSwitchable(o: MadoOverview | null | undefined): boolean {
  const steps = o?.state?.steps;
  if (!o?.exists || !steps) return false;
  if (steps.brief?.status === 'drafting') return false;
  return !STEPS.some((s) => steps[s]?.status === 'approved' || steps[s]?.status === 'stale');
}

/** What the code graph card shows, in one word.
 *  missing / old: the tool is not there (or too old) · building · blocked: a build cannot run now (`reason`) ·
 *  failed: the last build failed and none is there · stale: built, but the code moved since · ready · unbuilt. */
export type GraphCardState = 'missing' | 'old' | 'building' | 'blocked' | 'failed' | 'stale' | 'ready' | 'unbuilt';
export function graphCardState(g: CodeGraphStatus | null | undefined): GraphCardState | null {
  if (!g || typeof g !== 'object' || !g.tool) return null;
  if (!g.tool.found) return 'missing';
  if (!g.tool.usable) return 'old';
  if (g.building) return 'building';
  if (g.reason && g.reason.code !== 'missing' && g.reason.code !== 'old-tool') return 'blocked';
  if (g.built) return g.stale ? 'stale' : 'ready';
  return g.error ? 'failed' : 'unbuilt';
}
/** May the Build/Update button be pressed? */
export const canBuildGraph = (s: GraphCardState | null) => s === 'unbuilt' || s === 'stale' || s === 'ready' || s === 'failed';

/** A build's progress as a share (0–1), or null when the tool has not said yet. */
export function graphProgress(g: CodeGraphStatus | null | undefined): number | null {
  const p = g?.progress;
  if (!p || !(p.total > 0) || !(p.done >= 0)) return null;
  return Math.min(1, p.done / p.total);
}

/** The only page the card opens: the tool's own (the service names it; anything else is not opened). */
export const TOOL_PAGE = 'https://code-review-graph.com/';
export const safeToolPage = (url: unknown) => (url === TOOL_PAGE ? TOOL_PAGE : null);
/** Open the tool's page in the browser (only that page; anything else is not opened). */
export async function openToolPage(url: unknown): Promise<void> {
  const page = safeToolPage(url);
  if (!page) return;
  try { const { openUrl } = await import('@tauri-apps/plugin-opener'); await openUrl(page); }
  catch { window.open(page, '_blank', 'noopener'); }
}
/** The install command as the service gives it, shown to copy (never run by the app): one plain line. */
export function installCommand(cmd: unknown): string | null {
  if (typeof cmd !== 'string') return null;
  const c = cmd.trim();
  return c && c.length <= 200 && /^[\w@./:= -]+$/.test(c) ? c : null;
}

/** The card can be put away per project; kept in this window's storage only (a convenience). */
const KEY = (pid: string) => `mado.codeGraph.hidden.${pid}`;
export function cardHidden(pid: string): boolean {
  try { return localStorage.getItem(KEY(pid)) === '1'; } catch { return false; }
}
export function setCardHidden(pid: string, hidden: boolean): void {
  try { if (hidden) localStorage.setItem(KEY(pid), '1'); else localStorage.removeItem(KEY(pid)); } catch { /* storage closed: shown again next time */ }
}
