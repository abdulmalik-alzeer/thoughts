// `pages.json` from the customisation layer: the app's own pages — a page's heading and the line
// under it, named parts hidden, named texts reworded, add-on boxes at its top or bottom, its
// sections (order, hidden, size, one replaced by an add-on box, boxes inserted between), and an
// add-on page shown in place of the whole page.
//
// Every name comes from `layer/builtin-pages.json`, the reference the service checks a change
// against before it can be kept (`staging/pages.rs`) and from which the layer's README reference is
// generated. Anything this version does not know is ignored, never guessed — and said
// (`pageWarnings`), so a customisation never silently stops working after an update.
// With customisations off (safe mode) there is no config, so every page is the app's own.
import { app, type HomeWidget, type PluginView } from './store.svelte';
import { lang } from './i18n';
import { navLabel } from './terms';
import { findView, addonFailure } from './layer';
import { fullAccessOn } from './fullAccess';
import REF from './layer/builtin-pages.json';

type Text = { en?: string; ar?: string };
export type Size = 'full' | 'wide' | 'half' | 'third';
export type Sections = { order: string[]; hidden: string[]; size: Record<string, Size>; replace: Record<string, string>; insert: { box: string; before?: string; after?: string }[] };
export type PageCfg = { title?: Text; intro?: Text; hidden: string[]; labels: Record<string, Text>; boxes: { top: string[]; bottom: string[] }; replace?: string; sections: Sections; unknown: string[] };
type RefPage = { title: boolean; intro: boolean; boxes: boolean; replace: boolean; sections: string[]; locked?: string[]; parts: string[]; labels: string[] };

/** The reference (shared with the service). */
export const REFERENCE = REF as unknown as { version: number; sizes: Size[]; pages: Record<string, RefPage>; outOfReach: string[]; nav: { ids: string[]; neverHidden: string[]; fixedLook: string[]; maxGroups: number }; icons: string[]; renamed: Record<string, string>; permissions: string[]; viewPermissions: string[]; api: Record<string, string> };
/** May `page` be replaced as a whole? As the reference says; with full access on, any page. */
export const mayReplace = (page: string): boolean => !!PAGES[page]?.replace || fullAccessOn();
/** Pages a customisation may change, and what of each (same lists as the service). */
export const PAGES: Record<string, RefPage> = REFERENCE.pages;

const st = $state({
  cfg: {} as Record<string, PageCfg>,
  /** Pages the person asked to see as the app's own for now, although an add-on page replaces them. */
  original: {} as Record<string, boolean>,
  /** Pages the person keeps as the app made them (`addons.json` `builtin`, a history entry each). */
  builtin: [] as string[],
  /** Why a replacement could not be shown, by page. */
  failures: {} as Record<string, string>,
});

const text = (v: any, max: number): Text | undefined => {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return undefined;
  const one = (x: any) => (typeof x === 'string' && x.trim() ? x.trim().replace(/[\r\n]+/g, ' ').slice(0, max) : '');
  const en = one(v.en), ar = one(v.ar);
  return en || ar ? { ...(en ? { en } : {}), ...(ar ? { ar } : {}) } : undefined;
};
const strs = (v: any): string[] => (Array.isArray(v) ? v.filter((x) => typeof x === 'string') : []);
const WIDGET = /^widget:[\w.:-]{1,120}$/;
const boxList = (v: any) => strs(v).filter((x) => WIDGET.test(x)).slice(0, 6);
/** A section id as this version knows it (an id renamed in a later version still works). */
const current = (id: string) => REFERENCE.renamed[id] ?? id;
const obj = (v: any) => (v && typeof v === 'object' && !Array.isArray(v) ? v : {});

/** `sections` of one page, keeping only what this page has; `unknown` collects what was dropped. */
function sectionsOf(spec: RefPage, v: any, unknown: string[]): Sections {
  const o = obj(v);
  const known = (x: string) => spec.sections.includes(current(x));
  // Full access (lib/fullAccess.ts): no section is locked.
  const locked = (x: string) => !fullAccessOn() && (spec.locked ?? []).includes(current(x));
  const keep = (x: string) => { if (known(x)) return true; unknown.push(x); return false; };
  const order = strs(o.order).filter((x) => WIDGET.test(x) || keep(x)).map(current);
  const hidden = strs(o.hidden).filter((x) => keep(x) && !locked(x)).map(current);
  const size: Record<string, Size> = {};
  for (const [k, s] of Object.entries(obj(o.size))) if ((WIDGET.test(k) || keep(k)) && REFERENCE.sizes.includes(s as Size)) size[WIDGET.test(k) ? k : current(k)] = s as Size;
  const replace: Record<string, string> = {};
  for (const [k, w] of Object.entries(obj(o.replace))) if (keep(k) && !locked(k) && typeof w === 'string' && WIDGET.test(w)) replace[current(k)] = w;
  const insert = (Array.isArray(o.insert) ? o.insert : []).slice(0, 12).flatMap((e: any) => {
    if (!e || typeof e.box !== 'string' || !WIDGET.test(e.box)) return [];
    const at = typeof e.before === 'string' ? e.before : typeof e.after === 'string' ? e.after : '';
    if (!at || !keep(at)) return [];
    return [typeof e.before === 'string' ? { box: e.box, before: current(at) } : { box: e.box, after: current(at) }];
  });
  return { order, hidden, size, replace, insert };
}

/** Read `pages.json` as the service hands it over (null when there is none or customisations are
 *  off), and the pages kept as the app made them (`builtin`). */
export function setPages(v: any, builtin: any = []) {
  const cfg: Record<string, PageCfg> = {};
  if (v && typeof v === 'object' && !Array.isArray(v)) {
    for (const [id, spec] of Object.entries(PAGES)) {
      const c = (v as any)[id];
      if (!c || typeof c !== 'object' || Array.isArray(c)) continue;
      const labels: Record<string, Text> = {};
      if (c.labels && typeof c.labels === 'object') for (const k of spec.labels) { const l = text(c.labels[k], 60); if (l) labels[k] = l; }
      const unknown: string[] = strs(c.hidden).filter((x) => !spec.parts.includes(x));
      cfg[id] = {
        title: spec.title ? text(c.title, 60) : undefined,
        intro: spec.intro ? text(c.intro, 300) : undefined,
        hidden: strs(c.hidden).filter((x) => spec.parts.includes(x)),
        labels,
        boxes: spec.boxes ? { top: boxList(c.boxes?.top), bottom: boxList(c.boxes?.bottom) } : { top: [], bottom: [] },
        replace: mayReplace(id) && typeof c.replace === 'string' && /^[\w.:-]{1,120}$/.test(c.replace) ? c.replace : undefined,
        sections: sectionsOf(spec, spec.sections.length ? c.sections : undefined, unknown),
        unknown,
      };
    }
  }
  // Full access: the pages kept out of reach (History, Customise, Rollback…) may be replaced too.
  if (fullAccessOn() && v && typeof v === 'object' && !Array.isArray(v)) {
    for (const id of REFERENCE.outOfReach) {
      const r = (v as any)[id]?.replace;
      if (typeof r === 'string' && /^[\w.:-]{1,120}$/.test(r)) cfg[id] = { hidden: [], labels: {}, boxes: { top: [], bottom: [] }, replace: r, sections: { order: [], hidden: [], size: {}, replace: {}, insert: [] }, unknown: [] };
    }
  }
  st.cfg = cfg;
  st.builtin = strs(builtin).filter((p) => mayReplace(p));
  st.failures = {};
}

const pick = (l?: Text) => (l ? (lang() === 'ar' ? l.ar || l.en : l.en || l.ar) || '' : '');

/** Is this part of the page shown? (Hidden only when `pages.json` names it.) */
export const shows = (page: string, part: string): boolean => !st.cfg[page]?.hidden.includes(part);
/** A text of the page: the customisation's wording, or the app's own. */
export const pageText = (page: string, name: string, own: string): string => pick(st.cfg[page]?.labels[name]) || own;
/** The page's heading: the customisation's, else the sidebar name the person gave it, else the app's own. */
export const pageTitle = (page: string, own?: string): string => pick(st.cfg[page]?.title) || (app.nav?.labels?.[page] ? navLabel(page) : '') || own || navLabel(page);
/** A line of the customisation's under the heading ('' when there is none). */
export const pageIntro = (page: string): string => pick(st.cfg[page]?.intro);
/** Is the heading the customisation's? (Home then fills in `{greeting}` and `{name}` itself.) */
export const ownTitle = (page: string): boolean => !!pick(st.cfg[page]?.title);
/** Names in this page's customisation that this version of the app does not have (after an update). */
export const pageWarnings = (page: string): string[] => st.cfg[page]?.unknown ?? [];

/** The add-on boxes placed at the top or bottom of a page, in the order given. */
export function pageBoxes(page: string, place: 'top' | 'bottom'): HomeWidget[] {
  const keys = st.cfg[page]?.boxes[place] ?? [];
  return keys.flatMap((k) => app.homeWidgets.filter((w) => 'widget:' + w.id === k));
}
/** Boxes placed on another page: Home leaves them out unless Home places them there too. */
export function placedElsewhere(): Set<string> {
  const out = new Set<string>();
  for (const [id, c] of Object.entries(st.cfg)) {
    if (id === 'home') continue;
    for (const k of [...c.boxes.top, ...c.boxes.bottom, ...Object.values(c.sections.replace), ...c.sections.insert.map((e) => e.box), ...c.sections.order.filter((x) => WIDGET.test(x))]) out.add(k);
  }
  return out;
}
/** Boxes Home's own sections config places (they show on Home even when placed elsewhere too). */
export function homeBoxes(): string[] {
  const s = st.cfg.home?.sections;
  return s ? [...s.order.filter((x) => WIDGET.test(x)), ...s.insert.map((e) => e.box), ...Object.values(s.replace)] : [];
}

// ---------- sections ----------
/** One block of a page as it is drawn: one of the page's own sections, or an add-on box (`instead`:
 *  the section it stands in for, which comes back when the box is missing). */
export type Entry = { key: string; id: string; size: Size; box?: HomeWidget; instead?: string };

/** Where each block of `page` goes. `defaults`: the page's sections in their usual order (only
 *  those it can draw right now); `sizes`: the page's own default sizes. Pure apart from reading the
 *  config and the registered widgets (both can be passed in), so it is tested directly. */
export function sectionPlan(page: string, defaults: string[], sizes: Record<string, Size> = {}, widgets: HomeWidget[] = app.homeWidgets, cfg: PageCfg | undefined = st.cfg[page]): Entry[] {
  const s = cfg?.sections;
  const find = (k: string) => widgets.find((w) => 'widget:' + w.id === k);
  if (!s) return defaults.flatMap((id): Entry[] => {
    if (!WIDGET.test(id)) return [{ key: id, id, size: sizes[id] ?? 'full' }];
    const w = find(id);
    return w ? [{ key: id, id, size: sizes[id] ?? 'full', box: w }] : [];
  });
  const order = [...new Set([...s.order.filter((x) => WIDGET.test(x) || defaults.includes(x)), ...defaults])];
  const out: Entry[] = [];
  const sized = (k: string): Size => s.size[k] ?? sizes[k] ?? 'full';
  const push = (e: Entry | null) => { if (e && !out.some((x) => x.key === e.key)) out.push(e); };
  const boxEntry = (k: string): Entry | null => { const w = find(k); return w ? { key: k, id: k, size: sized(k), box: w } : null; };
  for (const k of order) {
    if (WIDGET.test(k)) { push(boxEntry(k)); continue; }
    if (s.hidden.includes(k)) continue;
    for (const ins of s.insert.filter((e) => e.before === k)) push(boxEntry(ins.box));
    const by = s.replace[k] ? find(s.replace[k]) : undefined;
    push(by ? { key: k, id: k, size: sized(k), box: by, instead: k } : { key: k, id: k, size: sized(k) });
    for (const ins of s.insert.filter((e) => e.after === k)) push(boxEntry(ins.box));
  }
  return out;
}

// ---------- the whole page replaced ----------
/** The add-on page shown in place of the app's own, while it is there and loaded. A replacement
 *  that is missing (switched off, removed, failed to start) leaves the app's own page in place. */
export function pageSwap(page: string): PluginView | undefined {
  if (app.safeMode || st.original[page] || st.builtin.includes(page) || !mayReplace(page)) return undefined;
  const id = st.cfg[page]?.replace;
  return id ? findView(id) : undefined;
}
/** The add-on page that replaces this one, even while the app's own is shown instead. */
export const swapFor = (page: string): PluginView | undefined => (app.safeMode ? undefined : (st.cfg[page]?.replace ? findView(st.cfg[page].replace!) : undefined));
/** The id `pages.json` names as this page's replacement ('' when none). */
export const swapId = (page: string): string => (app.safeMode ? '' : st.cfg[page]?.replace ?? '');
/** Show the app's own page (until the app is reloaded or `useSwap`). `reason`: why, when the
 *  replacement failed (the page says so). */
export function showOriginal(page: string, reason = '') {
  st.original = { ...st.original, [page]: true };
  if (reason) st.failures = { ...st.failures, [page]: reason };
}
export function useSwap(page: string) {
  const { [page]: _, ...rest } = st.original; st.original = rest;
  const { [page]: __, ...f } = st.failures; st.failures = f;
}
export const showingOriginal = (page: string): boolean => !!st.original[page];
export const keptBuiltin = (page: string): boolean => st.builtin.includes(page);
/** Pages `pages.json` replaces as a whole (for the per-page switch in Customise). */
export const replacedPages = (): string[] => Object.entries(st.cfg).filter(([id, c]) => c.replace && mayReplace(id)).map(([id]) => id);

/** Why the replacement of `page` is not on screen although `pages.json` asks for one:
 *  `builtin` (kept as the app made it), `original` (for now), `failed` / `missing` (with `reason`),
 *  or null when it is shown (or none is asked for). */
export function swapState(page: string): { state: 'builtin' | 'original' | 'failed' | 'missing'; reason: string; name: string } | null {
  const id = swapId(page);
  if (!id || !mayReplace(page)) return null;
  const v = findView(id);
  const name = v?.title || v?.short || id.replace(/^view:/, '').replace(/\.(js|html)(:.*)?$/, '').replace(/^.*:/, '');
  if (st.builtin.includes(page)) return { state: 'builtin', reason: '', name };
  if (st.failures[page]) return { state: 'failed', reason: st.failures[page], name };
  if (st.original[page]) return { state: 'original', reason: '', name };
  if (!v) {
    const why = addonFailure(id);
    return { state: why ? 'failed' : 'missing', reason: why, name };
  }
  return null;
}

// Which page is open, on the document (`<html data-page="todos">`), so the customisation's styles can
// change one page and leave the others alone: `[data-page="todos"] main h1 { … }`.
$effect.root(() => {
  $effect(() => { const kind = app.view.kind; try { document.documentElement.dataset.page = kind; } catch { /* no document */ } });
});
