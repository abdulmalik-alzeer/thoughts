// Editing the sidebar in place (round 0.7.2): every change the person can make, as a pure function
// from one `nav.json` to the next. The editor (views/SidebarEdit.svelte) only draws rows and calls
// these, so order, hiding, the protected entries, keyboard moves and "what is saved" are tested here
// without a window. Three rules hold for every function:
//   1. Nothing the person cannot see is lost. An id that names no entry of the sidebar right now
//      (Inbox while it is empty, the page of a switched-off add-on, a page of another computer) keeps
//      its place in `order`, its group, its name, its icon and its hidden state.
//   2. The guards of the reference stay: a never-hidden entry is never written to `hidden`, a
//      fixed-look entry never gets a name or an icon, at most `maxGroups` groups. With full access on
//      (`full`) nothing is locked, exactly as in lib/navLayout.ts and lib/layer.ts.
//   3. The model is `nav.json` as it is: entries without a group come first, then the groups in
//      their order; inside each, `order` decides. So "a place" is (group, index among the shown).
import type { NavConfig } from './store.svelte';
import { layoutNav, moved, type NavEntry } from './navLayout';
import REF from './layer/builtin-pages.json';

export type Lang = 'en' | 'ar';
export type Label = { en?: string; ar?: string };
/** An entry of the sidebar as the editor needs it: `base` is its name before any renaming. */
export type EditItem = NavEntry & { base: string; custom?: boolean };
export type Guards = { neverHidden: string[]; fixed: string[]; maxGroups: number; icons: string[] };
export type Place = { group: string | null; index: number };
export type EditRow<T> =
  | { kind: 'item'; key: string; item: T; group: string | null }
  | { kind: 'head'; key: string; group: string; label: Label; count: number };

const NAME_MAX = 40;
const ref = (REF as any).nav as { neverHidden: string[]; fixedLook: string[]; maxGroups: number };

export function guards(full = false): Guards {
  return { neverHidden: full ? [] : ref.neverHidden, fixed: full ? [] : ref.fixedLook, maxGroups: ref.maxGroups, icons: (REF as any).icons };
}
export const emptyNav = (): NavConfig => ({ hidden: [], order: [], labels: {} });
const names = (it: NavEntry) => (it.alias ? [it.id, it.alias] : [it.id]);
const inList = (list: string[] | undefined, it: NavEntry) => !!list && names(it).some((n) => list.includes(n));
const pick = (l: Label | undefined, lang: Lang) => (l ? (lang === 'ar' ? l.ar || l.en : l.en || l.ar) || '' : '');

export const canHide = (it: NavEntry, full = false) => !guards(full).neverHidden.includes(it.id);
export const canRestyle = (it: NavEntry, full = false) => !guards(full).fixed.includes(it.id);
export const isHidden = (nav: NavConfig, it: NavEntry, full = false) => canHide(it, full) && inList(nav.hidden, it);
export const isStart = (nav: NavConfig, it: NavEntry) => !!nav.start && names(it).includes(nav.start);
/** The name the sidebar shows for `it` under `nav` (the same choice `navLabel` makes). */
export function labelOf(nav: NavConfig, it: EditItem, lang: Lang, full = false): string {
  if (!canRestyle(it, full)) return it.base;
  return (pick(nav.labels?.[it.id], lang) || (it.alias ? pick(nav.labels?.[it.alias], lang) : '') || it.base).slice(0, 60);
}
export const groupName = (l: Label | null | undefined, lang: Lang) => pick(l ?? undefined, lang);
/** Has the person given `it` a name or an icon of their own? */
export const restyled = (nav: NavConfig, it: NavEntry) => names(it).some((n) => !!nav.labels?.[n] || !!nav.icons?.[n]);

/** A copy the editor may change: group ids made unique (two groups with one id could not be told
 *  apart in a list), everything else as it was. */
export function draftOf(nav: NavConfig | null | undefined): NavConfig {
  const n: NavConfig = nav ? JSON.parse(JSON.stringify(nav)) : emptyNav();
  n.hidden ??= []; n.order ??= []; n.labels ??= {};
  const seen = new Set<string>();
  n.groups = (n.groups ?? []).map((g) => {
    let id = g.id, k = 2;
    while (seen.has(id)) id = `${g.id}-${k++}`;
    seen.add(id);
    return { ...g, id };
  });
  return n;
}

// ---------- the sidebar as lists ----------
// `lists`: for "no group" (key '') and every group, the ids of the entries that are in the sidebar
// right now, hidden ones included, in the order the sidebar would show them. `rebuild` writes such
// lists back as `order` + `groups[].items`, keeping every id that names no entry where it was.
type Lists = Map<string, string[]>;
function lists<T extends NavEntry>(items: T[], nav: NavConfig, full: boolean): Lists {
  const all = layoutNav(items, { ...nav, hidden: [] }, full);
  const out: Lists = new Map([['', (all.groups.find((g) => g.id === null)?.items ?? []).map((i) => i.id)]]);
  for (const g of nav.groups ?? []) out.set(g.id, (all.groups.find((x) => x.id === g.id)?.items ?? []).map((i) => i.id));
  return out;
}
function rebuild<T extends NavEntry>(items: T[], nav: NavConfig, l: Lists, groups = nav.groups ?? []): NavConfig {
  const known = (id: string) => items.some((i) => i.id === id || i.alias === id);
  const present = [...(l.get('') ?? []), ...groups.flatMap((g) => l.get(g.id) ?? [])];
  const absent = (nav.order ?? []).filter((id) => !known(id));
  // The app's own order needs no `order` at all: `{}` stays the usual sidebar.
  const usual = !absent.length && present.join('\n') === items.map((i) => i.id).join('\n');
  const out: NavConfig = { ...nav, order: usual ? [] : [...present, ...absent] };
  out.groups = groups.map((g) => ({ ...g, items: [...(l.get(g.id) ?? []), ...g.items.filter((id) => !known(id))] }));
  return out;
}

/** What the editor draws: the shown entries as one flat list of rows (a heading row starts each
 *  group, empty groups included, so there is always somewhere to drop), and the hidden entries. */
export function editRows<T extends EditItem>(items: T[], nav: NavConfig, full = false): { rows: EditRow<T>[]; hidden: T[] } {
  const all = layoutNav(items, { ...nav, hidden: [] }, full); // icons applied, hidden ones in place
  const rows: EditRow<T>[] = [], hidden: T[] = [];
  const put = (list: T[], group: string | null) => { for (const it of list) (isHidden(nav, it, full) ? hidden : rows).push(isHidden(nav, it, full) ? (it as any) : { kind: 'item', key: 'i:' + it.id, item: it, group }); };
  put(all.groups.find((g) => g.id === null)?.items ?? [], null);
  for (const g of nav.groups ?? []) {
    const list = all.groups.find((x) => x.id === g.id)?.items ?? [];
    rows.push({ kind: 'head', key: 'g:' + g.id, group: g.id, label: g.label, count: list.filter((i) => !isHidden(nav, i, full)).length });
    put(list, g.id);
  }
  return { rows, hidden };
}

/** Where gap number `slot` of `rows` (0 = above the first row) puts an entry: the group of the row
 *  above the gap, after that row. A gap right under a heading is the top of that group; a gap right
 *  above a heading is the end of what comes before it. */
export function placeAt<T>(rows: EditRow<T>[], slot: number): Place {
  const s = Math.max(0, Math.min(slot, rows.length));
  const above = rows[s - 1];
  if (!above) return { group: null, index: 0 };
  const group = above.group;
  let index = 0;
  for (let i = 0; i < s; i++) if (rows[i].kind === 'item' && rows[i].group === group) index++;
  return { group, index };
}
/** The gap an entry is dragged to: the number of rows whose middle is above `y`. */
export function slotAt(middles: number[], y: number): number {
  let n = 0;
  for (const m of middles) if (y > m) n++;
  return n;
}

/** Put the entry `id` at `to` (a group, or null for none, and the index among its shown entries). */
export function place<T extends NavEntry>(items: T[], nav: NavConfig, id: string, to: Place, full = false): NavConfig {
  const it = items.find((i) => i.id === id);
  if (!it) return nav;
  const l = lists(items, nav, full);
  for (const list of l.values()) { const at = list.indexOf(id); if (at >= 0) list.splice(at, 1); }
  const target = l.get(to.group ?? '') ?? l.get('')!;
  const shown = target.filter((x) => { const e = items.find((i) => i.id === x); return !e || !isHidden(nav, e, full); });
  const before = shown[Math.max(0, to.index)];
  const at = before !== undefined ? target.indexOf(before) : shown.length ? target.indexOf(shown[shown.length - 1]) + 1 : 0;
  target.splice(at, 0, id);
  return rebuild(items, nav, l);
}

/** One step up (-1) or down (+1) through the rows, into and out of groups: the keyboard move.
 *  `null` when the entry is already at that end. */
export function step<T extends EditItem>(items: T[], nav: NavConfig, id: string, delta: -1 | 1, full = false): { nav: NavConfig; place: Place } | null {
  const { rows } = editRows(items, nav, full);
  const at = rows.findIndex((r) => r.kind === 'item' && r.item.id === id);
  if (at < 0) return null;
  const rest = rows.filter((_, i) => i !== at);
  const slot = at + delta;
  if (slot < 0 || slot > rest.length) return null;
  const to = placeAt(rest, slot);
  return { nav: place(items, nav, id, to, full), place: to };
}

/** Hide or show an entry. A never-hidden entry stays shown; the page the app opens on cannot be a
 *  hidden one, so hiding it puts the start back on Home. */
export function setHidden(nav: NavConfig, it: NavEntry, hide: boolean, full = false): NavConfig {
  if (hide && !canHide(it, full)) return nav;
  const rest = (nav.hidden ?? []).filter((id) => !names(it).includes(id));
  const out: NavConfig = { ...nav, hidden: hide ? [...rest, it.id] : rest };
  if (hide && isStart(nav, it)) delete out.start;
  return out;
}

/** Name an entry in one language (the other language's name stays). Empty text takes the name in
 *  this language away again. A fixed-look entry keeps the app's name. */
export function rename(nav: NavConfig, it: NavEntry, lang: Lang, text: string, full = false): NavConfig {
  if (!canRestyle(it, full)) return nav;
  const name = text.trim().slice(0, NAME_MAX);
  const labels = { ...(nav.labels ?? {}) };
  const now: Label = { ...(labels[it.id] ?? (it.alias ? labels[it.alias] : undefined) ?? {}) };
  if (name) now[lang] = name; else delete now[lang];
  for (const n of names(it)) delete labels[n];
  if (now.en || now.ar) labels[it.id] = now;
  return { ...nav, labels };
}
/** Choose an icon from the reference; '' is the usual one. */
export function setIcon(nav: NavConfig, it: NavEntry, icon: string, full = false): NavConfig {
  if (!canRestyle(it, full) || (icon && !guards(full).icons.includes(icon))) return nav;
  const icons = { ...(nav.icons ?? {}) };
  for (const n of names(it)) delete icons[n];
  if (icon) icons[it.id] = icon;
  return { ...nav, icons };
}
/** The app's own name (both languages) and icon again. */
export function usualLook(nav: NavConfig, it: NavEntry): NavConfig {
  const labels = { ...(nav.labels ?? {}) }, icons = { ...(nav.icons ?? {}) };
  for (const n of names(it)) { delete labels[n]; delete icons[n]; }
  return { ...nav, labels, icons };
}
/** The page the app opens on; `null` (or Home) is Home. A hidden page cannot be it. */
export function setStart(nav: NavConfig, it: NavEntry | null, full = false): NavConfig {
  const out = { ...nav };
  if (!it || it.id === 'home' || isHidden(nav, it, full)) delete out.start; else out.start = it.id;
  return out;
}

/** A new, empty group at the end (`null` at the limit). It is only saved once it holds an entry. */
export function addGroup(nav: NavConfig, lang: Lang, name: string, full = false): { nav: NavConfig; id: string } | null {
  const groups = nav.groups ?? [];
  if (groups.length >= guards(full).maxGroups) return null;
  let n = groups.length + 1;
  while (groups.some((g) => g.id === 'group-' + n)) n++;
  const id = 'group-' + n;
  return { id, nav: { ...nav, groups: [...groups, { id, label: { [lang]: name.trim().slice(0, NAME_MAX) || id }, items: [] }] } };
}
/** Name a group in one language. A group always has a name, so empty text changes nothing. */
export function renameGroup(nav: NavConfig, id: string, lang: Lang, text: string): NavConfig {
  const name = text.trim().slice(0, NAME_MAX);
  if (!name) return nav;
  return { ...nav, groups: (nav.groups ?? []).map((g) => (g.id === id ? { ...g, label: { ...g.label, [lang]: name } } : g)) };
}
/** Take a group away: its entries stay in the sidebar, after the ones that have no group. */
export function removeGroup<T extends NavEntry>(items: T[], nav: NavConfig, id: string, full = false): NavConfig {
  const g = (nav.groups ?? []).find((x) => x.id === id);
  if (!g) return nav;
  const l = lists(items, nav, full);
  l.set('', [...(l.get('') ?? []), ...(l.get(id) ?? [])]);
  return rebuild(items, nav, l, (nav.groups ?? []).filter((x) => x.id !== id));
}
/** Move a whole group to position `to` among the groups. */
export function moveGroup<T extends NavEntry>(items: T[], nav: NavConfig, id: string, to: number, full = false): NavConfig {
  const groups = nav.groups ?? [];
  const from = groups.findIndex((g) => g.id === id);
  const next = moved(groups, from, Math.max(0, Math.min(to, groups.length - 1)));
  return next === groups ? nav : rebuild(items, nav, lists(items, nav, full), next);
}

/** Exactly what is sent to the service: only what says something, in one fixed order of keys, so
 *  two arrangements that show the same sidebar compare equal. `{}` is the usual sidebar. A group
 *  with nothing in it is not kept; neither is a start page that is hidden. */
export function cleanNav(nav: NavConfig | null | undefined, full = false): Record<string, any> {
  const out: Record<string, any> = {};
  if (!nav) return out;
  const g = guards(full);
  if (nav.order?.length) out.order = [...nav.order];
  const hidden = (nav.hidden ?? []).filter((id) => !g.neverHidden.includes(id));
  if (hidden.length) out.hidden = hidden;
  const labels: Record<string, Label> = {};
  for (const [id, l] of Object.entries(nav.labels ?? {})) {
    if (g.fixed.includes(id)) continue;
    const en = l?.en?.trim().slice(0, NAME_MAX), ar = l?.ar?.trim().slice(0, NAME_MAX);
    if (en || ar) labels[id] = { ...(en ? { en } : {}), ...(ar ? { ar } : {}) };
  }
  if (Object.keys(labels).length) out.labels = labels;
  const icons: Record<string, string> = {};
  for (const [id, icon] of Object.entries(nav.icons ?? {})) if (!g.fixed.includes(id) && g.icons.includes(icon)) icons[id] = icon;
  if (Object.keys(icons).length) out.icons = icons;
  const groups = (nav.groups ?? []).filter((x) => x.items.length).slice(0, g.maxGroups).map((x) => {
    const en = x.label?.en?.trim().slice(0, NAME_MAX), ar = x.label?.ar?.trim().slice(0, NAME_MAX);
    return { id: x.id, label: en || ar ? { ...(en ? { en } : {}), ...(ar ? { ar } : {}) } : { en: x.id }, items: [...x.items] };
  });
  if (groups.length) out.groups = groups;
  if (nav.start && !hidden.includes(nav.start)) out.start = nav.start;
  return out;
}
/** Do two arrangements save as the same file? */
export const sameNav = (a: NavConfig | null | undefined, b: NavConfig | null | undefined, full = false) => JSON.stringify(cleanNav(a, full)) === JSON.stringify(cleanNav(b, full));

/** The steps back the editor keeps ("Undo"): each change pushes what was there before. */
export class Undo<T> {
  private past: T[] = [];
  constructor(private max = 50) {}
  push(was: T) { this.past.push(was); if (this.past.length > this.max) this.past.shift(); }
  pop(): T | undefined { return this.past.pop(); }
  clear() { this.past = []; }
  get size() { return this.past.length; }
}
