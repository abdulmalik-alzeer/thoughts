// One page per item (an idea, a client, a matter, a class, a project…): which library paths are an
// item's front page, the words for an item of its kind, and how the rest of the app opens its page.
// Docs keeps opening the plain file when the user browses the file tree.
import { go } from './store.svelte';
import { t } from './i18n';
import { areaWord, isWorkItem, type AreaWord } from './terms';

/** `<area>/<item>` when `path` is an item's front page (`<area>/<item>/README.md`) or its folder
 *  `<area>/<item>` — the items the Ideas page lists (never `me/`, `inbox/` or a hidden folder). */
export function itemOf(path: string, folderToo = false): string | null {
  const p = String(path ?? '').replace(/\\/g, '/').replace(/^\/+|\/+$/g, '');
  const m = /^([^/]+)\/([^/]+)(\/README\.md)?$/.exec(p);
  if (!m || (!m[3] && !folderToo)) return null;
  if (m[1].startsWith('.') || m[1] === 'me' || m[1] === 'inbox' || m[2] === '.' || m[2] === '..') return null;
  return `${m[1]}/${m[2]}`;
}

/** Open an item's page (`tab`: the part to show first — overview, todos, meetings, notes, outputs, log). */
export function openItem(path: string, tab?: string): Promise<boolean> {
  const dir = itemOf(path, true) ?? path;
  return go(tab ? { kind: 'item', path: dir, id: tab } : { kind: 'item', path: dir });
}

/** Open a library file the way the app does for its kind: an item's front page as the item's page,
 *  anything else in Docs. */
export function openPath(path: string, tab?: string): Promise<boolean> {
  return itemOf(path) ? openItem(path, tab) : go({ kind: 'docs', path });
}

// The `kind:` values the starter writes, and the word family each one is spoken of with.
const KIND: Record<string, AreaWord> = { idea: 'ideas', client: 'clients', matter: 'matters', engagement: 'engagements', class: 'classes', project: 'projects' };

/** The word family for this item: an idea is an idea; a work item takes its own kind's word, else the user's. */
export function wordOf(i: { kind?: string; area?: string } | null | undefined): AreaWord {
  if (!i || !isWorkItem(i)) return 'ideas';
  return KIND[String(i.kind ?? '').trim().toLowerCase()] ?? (areaWord() === 'ideas' ? 'ideas' : areaWord());
}

/** A sentence about one item in its own words (`term.<word>.<form>` keys, as in lib/terms.ts). */
export function itemTerm(word: AreaWord, form: string, vars: Record<string, string | number> = {}): string {
  const key = `term.${word}.${form}`;
  const s = t(key, vars);
  return s === key ? t(`term.ideas.${form}`, vars) : s;
}
