// The sidebar as `nav.json` arranges it: hidden entries, their order, icons and headed groups.
// Pure (the shell passes its entries and the config), so it is tested directly. Names come from
// `navLabel` / `viewLabel`, which read `nav.json` labels themselves.
import type { NavConfig } from './store.svelte';
import REF from './layer/builtin-pages.json';

export type NavEntry = { id: string; alias?: string; icon: string };
export type NavGroup<T> = { id: string | null; label: { en?: string; ar?: string } | null; items: T[] };

const NEVER_HIDDEN: string[] = (REF as any).nav.neverHidden;
const FIXED: string[] = (REF as any).nav.fixedLook;
const ICONS: string[] = (REF as any).icons;

/** `items` in sidebar order with `nav` applied: `flat` (the order shortcuts number them in) and
 *  `groups` (no heading first, then each group of `nav.groups` that has an entry to show). */
export function layoutNav<T extends NavEntry>(items: T[], nav: NavConfig | null | undefined): { flat: T[]; groups: NavGroup<T>[] } {
  const named = (i: T, id: string) => i.id === id || (!!i.alias && i.alias === id);
  const hidden = new Set((nav?.hidden ?? []).filter((id) => !NEVER_HIDDEN.includes(id)));
  const shown = items.filter((i) => NEVER_HIDDEN.includes(i.id) || !(hidden.has(i.id) || (i.alias && hidden.has(i.alias))));
  const first: T[] = [];
  for (const id of nav?.order ?? []) {
    const it = shown.find((i) => i.id === id) ?? shown.find((i) => i.alias === id && !first.includes(i));
    if (it && !first.includes(it)) first.push(it);
  }
  const ordered = [...first, ...shown.filter((i) => !first.includes(i))].map((i) => {
    if (FIXED.includes(i.id)) return i;
    const icon = nav?.icons?.[i.id] ?? (i.alias ? nav?.icons?.[i.alias] : undefined);
    return icon && ICONS.includes(icon) ? { ...i, icon } : i;
  });
  const inGroup = new Map<T, number>();
  (nav?.groups ?? []).forEach((g, gi) => { for (const id of g.items) { const it = ordered.find((i) => named(i, id)); if (it && !inGroup.has(it)) inGroup.set(it, gi); } });
  const groups: NavGroup<T>[] = [{ id: null, label: null, items: ordered.filter((i) => !inGroup.has(i)) }];
  (nav?.groups ?? []).forEach((g, gi) => {
    const list = ordered.filter((i) => inGroup.get(i) === gi);
    if (list.length) groups.push({ id: g.id, label: g.label, items: list });
  });
  return { flat: groups.flatMap((g) => g.items), groups: groups.filter((g) => g.items.length) };
}

/** Move `list[from]` to `to` (a new list). */
export function moved<T>(list: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || from >= list.length || to < 0 || to >= list.length) return list;
  const out = list.slice();
  const [x] = out.splice(from, 1);
  out.splice(to, 0, x);
  return out;
}
