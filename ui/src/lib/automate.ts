// Suggest automations (Customise › Automate): the shapes the background service sends
// (`/api/automations`, `/api/people`, the build answers) and the small pure rules the views share.
// Nothing here talks to the service except the thin `load…` helpers at the end.
import { api } from './api';

export type Kind = 'skill' | 'agent' | 'integration' | 'tab';
export const KINDS: readonly Kind[] = ['skill', 'agent', 'integration', 'tab'];

export type Suggestion = {
  id: string; kind: Kind; title: string; what: string; why: string; build: string;
  request: string; template: string | null; people: boolean;
};
export type Blocked = null | 'noLibrary' | 'spend' | 'planLimit' | 'signedOut';
export type AutoState = {
  running: boolean; error: string | null; generatedAt: string | null; suggestions: Suggestion[];
  nothing: string; program: 'claude' | 'codex'; canSuggest: boolean; blocked: Blocked;
};
export type Guide = { title: string; intro: string; steps: string[]; chatPrompt: string };
/** A routine or helper as the editor holds it: `body` is the routine's steps or the helper's instructions. */
export type Draft = { title: string; description: string; body: string };
export type Person = {
  slug?: string; hash?: string; name: string; email: string; role: string; organisation: string;
  relationship: string; language: string; tone: string; summary: string; notes: string; extra?: string[];
};

const str = (v: unknown, max = 20_000) => (typeof v === 'string' ? v : v == null ? '' : String(v)).slice(0, max);
const BLOCKED = new Set(['noLibrary', 'spend', 'planLimit', 'signedOut']);

/** One suggestion as the service sent it, or null when it is not one this version can show. */
export function normSuggestion(x: any): Suggestion | null {
  if (!x || typeof x !== 'object' || typeof x.id !== 'string' || !x.id) return null;
  if (!KINDS.includes(x.kind)) return null;
  const title = str(x.title, 200).trim();
  const what = str(x.what, 1000).trim();
  if (!title || !what) return null;
  return {
    id: x.id, kind: x.kind, title, what, why: str(x.why, 1000).trim(), build: str(x.build, 1000).trim(),
    request: str(x.request, 8000), template: typeof x.template === 'string' && x.template ? x.template : null, people: x.people === true,
  };
}

/** The `GET /api/automations` answer (also what dismiss/restore/built return), made safe to show. */
export function normState(x: any): AutoState {
  const o = x && typeof x === 'object' ? x : {};
  const seen = new Set<string>();
  const suggestions = (Array.isArray(o.suggestions) ? o.suggestions : []).map(normSuggestion)
    .filter((s: Suggestion | null): s is Suggestion => !!s && !seen.has(s.id) && !!seen.add(s.id));
  const blocked: Blocked = BLOCKED.has(o.blocked) ? o.blocked : null;
  const running = o.running === true;
  return {
    running, error: typeof o.error === 'string' && o.error.trim() ? o.error.trim() : null,
    generatedAt: typeof o.generatedAt === 'string' && !Number.isNaN(Date.parse(o.generatedAt)) ? o.generatedAt : null,
    suggestions, nothing: str(o.nothing, 600).trim(), program: o.program === 'codex' ? 'codex' : 'claude',
    canSuggest: o.canSuggest === true && !blocked && !running, blocked,
  };
}

/** What the page shows. `again` = the person pressed Suggest again (or Back from it). */
export type Phase = 'before' | 'running' | 'error' | 'nothing' | 'done' | 'result';
export function phaseOf(s: AutoState, again: boolean): Phase {
  if (s.running) return 'running';
  if (again) return 'before';
  if (s.error && !s.suggestions.length) return 'error';
  if (s.suggestions.length) return 'result';
  if (!s.generatedAt) return 'before';
  // Suggestions were made: none came (the assistant's "nothing" reason), or all were used up.
  return s.nothing ? 'nothing' : 'done';
}

/** The i18n key for why Suggest cannot be pressed now, or '' when it can. */
export function blockedKey(s: AutoState): string {
  if (s.running) return 'auto-ui.blocked.running';
  if (s.blocked) return 'auto-ui.blocked.' + s.blocked;
  return s.canSuggest ? '' : 'auto-ui.blocked.unknown';
}

export const kindKey = (k: Kind) => 'auto-ui.kind.' + k;
export const kindIcon = (k: Kind) => ({ skill: 'todos', agent: 'ai', integration: 'external', tab: 'modules' } as const)[k];

/** The same rule the service applies to routines and helpers: nothing that would run a program. */
export function runsCommands(s: string): boolean {
  return s.includes('!`') || s.split('\n').some((l) => { const x = l.trimStart(); return x.startsWith('!') && !x.startsWith('![') && x.length > 1; });
}

export const emptyDraft = (): Draft => ({ title: '', description: '', body: '' });
/** A draft from the service's `{title, description, steps}` (routines) or `{…, instructions}` (helpers). */
export function draftFrom(x: any, kind: 'skill' | 'agent'): Draft {
  const o = x && typeof x === 'object' ? x : {};
  return { title: str(o.title, 200), description: str(o.description, 1000), body: str(kind === 'skill' ? o.steps : o.instructions) };
}
/** One-line title and description, trimmed body: what is reviewed is exactly what is saved. */
export function tidyDraft(d: Draft): Draft {
  return { title: d.title.split(/\s+/).join(' ').trim(), description: d.description.split(/\s+/).join(' ').trim(), body: d.body.trim() };
}
export const draftReady = (d: Draft) => !!(d.title.trim() && d.description.trim() && d.body.trim());
export const draftUnsafe = (d: Draft) => [d.title, d.description, d.body].some(runsCommands);
/** The body the save endpoints take for each kind. */
export function saveBody(d: Draft, kind: 'skill' | 'agent') {
  const x = tidyDraft(d);
  return kind === 'skill'
    ? { id: null, hash: null, routine: { title: x.title, description: x.description, steps: x.body } }
    : { agent: { title: x.title, description: x.description, instructions: x.body } };
}

/** Steps to show, one per line, as plain text: blank ones dropped, at most `max`. */
export function guideSteps(steps: unknown, max = 10): string[] {
  return (Array.isArray(steps) ? steps : []).map((s) => str(s, 600).split(/\s+/).join(' ').trim()).filter(Boolean).slice(0, max);
}
export function normGuide(x: any): Guide | null {
  const g = x && typeof x === 'object' ? x : null;
  if (!g) return null;
  const title = str(g.title, 200).trim();
  const chatPrompt = str(g.chatPrompt, 8000).trim();
  const steps = guideSteps(g.steps);
  if (!title || (!steps.length && !chatPrompt)) return null;
  return { title, intro: str(g.intro, 2000).trim(), steps, chatPrompt };
}

/** The shape the chat view keeps its own unsent text in (`thoughts.chatdraft:<id>`). */
export function chatDraft(text: string): string {
  return JSON.stringify({ text: text.slice(0, 20_000), mentions: [], files: [] });
}

export const emptyPerson = (): Person => ({ name: '', email: '', role: '', organisation: '', relationship: '', language: '', tone: '', summary: '', notes: '' });
export function normPerson(x: any): Person | null {
  if (!x || typeof x !== 'object' || typeof x.slug !== 'string' || !x.slug) return null;
  const p: Person = { ...emptyPerson(), slug: x.slug, hash: str(x.hash, 200) };
  for (const k of ['name', 'email', 'role', 'organisation', 'relationship', 'language', 'tone', 'summary', 'notes'] as const) p[k] = str(x[k]);
  p.extra = Array.isArray(x.extra) ? x.extra.map((e: unknown) => str(e, 80)).filter(Boolean) : [];
  return p.name.trim() ? p : null;
}
/** `GET /api/people` → `{people, private}` (an older service sent a bare list). */
export function normPeople(x: any): { people: Person[]; private: boolean } {
  const list = Array.isArray(x) ? x : Array.isArray(x?.people) ? x.people : [];
  return { people: list.map(normPerson).filter((p: Person | null): p is Person => !!p), private: x?.private === true };
}
/** The fields the app writes for a person (one-line details trimmed; notes kept as written). */
export function personBody(p: Person) {
  const one = (s: string) => s.split(/\s+/).join(' ').trim();
  return {
    name: one(p.name), email: p.email.trim(), role: one(p.role), organisation: one(p.organisation), relationship: one(p.relationship),
    language: one(p.language), tone: one(p.tone), summary: p.summary, notes: p.notes.replace(/\s+$/, ''),
  };
}
/** A quick check before sending (the service checks again, more strictly): '' or an i18n key. */
export function personProblem(p: Person): string {
  if (!p.name.trim()) return 'auto-ui.ppl.needName';
  if (p.name.trim().length > 120) return 'auto-ui.ppl.longName';
  const e = p.email.trim();
  if (e && !/^[^\s@?&,<>;:"()[\]]+@[^\s@?&,<>;:"()[\]]+\.[^\s@?&,<>;:"()[\]]{2,}$/u.test(e)) return 'auto-ui.ppl.badEmail';
  return '';
}
export function samePerson(a: Person, b: Person): boolean {
  const x = personBody(a), y = personBody(b);
  return (Object.keys(x) as (keyof typeof x)[]).every((k) => x[k] === y[k]);
}
/** "role, organisation" for a person's line in a list. */
export const personLine = (p: Person) => [p.role.trim(), p.organisation.trim()].filter(Boolean).join(', ');

export const loadState = async () => normState(await api('/automations'));
export const loadPeople = async () => normPeople(await api('/people'));
