// The words the app uses for the "Ideas" area follow the user's line of work (settings.profession),
// and `nav.json` labels from the customisation layer override any built-in label.
import { app } from './store.svelte';
import { lang, t, tn } from './i18n';

export type AreaWord = 'ideas' | 'clients' | 'matters' | 'classes' | 'topics';
const BY_PROFESSION: Record<string, AreaWord> = {
  finance: 'clients', accounting: 'clients', consulting: 'clients', legal: 'matters', teaching: 'classes', research: 'topics',
};

/** Which word family the Ideas area uses for this user. */
export function areaWord(): AreaWord {
  return BY_PROFESSION[String(app.settings?.profession ?? app.status?.profession ?? '').toLowerCase()] ?? 'ideas';
}

/** A string about the Ideas area in the user's words: `term('title')` → "Clients", `term('active')` → "Active clients"…
 *  Forms: title · active (stat label, use `termN`) · empty · emptyBody · capture · new · one. Keys live in i18n/ui.ts as `term.<word>.<form>`. */
export function term(form: string, vars: Record<string, string | number> = {}): string {
  const key = `term.${areaWord()}.${form}`;
  const s = t(key, vars);
  return s === key ? t(`term.ideas.${form}`, vars) : s;
}

/** Plural-aware form, e.g. `termN('active', 3)` → "Active clients" (the label only; show the number beside it). */
export function termN(form: string, n: number): string {
  const key = `term.${areaWord()}.${form}`;
  const s = tn(key, n);
  return s === key ? tn(`term.ideas.${form}`, n) : s;
}

/** Label for a sidebar item / view id: nav.json label → profession word (ideas) → built-in name. */
export function navLabel(id: string, fallback?: string): string {
  const l = app.nav?.labels?.[id];
  const custom = l && (lang() === 'ar' ? l.ar || l.en : l.en || l.ar);
  if (custom) return String(custom).slice(0, 40);
  if (id === 'ideas') return term('title');
  return fallback ?? t(id);
}
