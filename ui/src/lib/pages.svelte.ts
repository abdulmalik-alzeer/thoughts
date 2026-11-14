// `pages.json` from the customisation layer: details of the app's own pages — a page's heading and
// the line under it, named parts hidden, named texts reworded, add-on boxes at its top or bottom, and
// an add-on page shown in its place. The names are the service's (`staging/pages.rs`), which checks
// them before a change can be kept; anything this version does not know is ignored, never guessed.
// With customisations off (safe mode) there is no config, so every page is the app's own.
import { app, type HomeWidget, type PluginView } from './store.svelte';
import { lang } from './i18n';
import { navLabel } from './terms';
import { findView } from './layer';

type Text = { en?: string; ar?: string };
export type PageCfg = { title?: Text; intro?: Text; hidden: string[]; labels: Record<string, Text>; boxes: { top: string[]; bottom: string[] }; replace?: string };

/** Pages a customisation may change, what each may hide and reword (same lists as the service). */
export const PAGES: Record<string, { parts: string[]; labels: string[]; replace: boolean }> = {
  home: { parts: ['date', 'greeting', 'intro', 'emblem', 'todoChip', 'noteChip', 'checkInChip'], labels: ['askPlaceholder', 'todoChip', 'noteChip', 'ideasTitle', 'ideasIntro', 'upcomingTitle', 'todosTitle', 'chatsTitle', 'projectsTitle'], replace: false },
  chats: { parts: ['newChat', 'folderChats', 'search', 'select', 'filters', 'systemChats', 'keep'], labels: ['newChat', 'search', 'empty'], replace: true },
  ideas: { parts: ['capture', 'areas', 'search', 'statusFilter', 'sort', 'count'], labels: ['capturePlaceholder', 'capture', 'search'], replace: true },
  todos: { parts: ['count', 'views', 'calendarExport', 'filters', 'grouping', 'sorting', 'showDone', 'moveDone', 'quickAdd'], labels: ['quickAddPlaceholder', 'add', 'listPlaceholder'], replace: true },
  docs: { parts: ['filter'], labels: ['filter'], replace: false },
  projects: { parts: ['intro', 'linkForm'], labels: ['link', 'newChat'], replace: true },
};

const st = $state({
  cfg: {} as Record<string, PageCfg>,
  /** Pages the person asked to see as the app's own for now, although an add-on page replaces them. */
  original: {} as Record<string, boolean>,
});

const text = (v: any, max: number): Text | undefined => {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return undefined;
  const one = (x: any) => (typeof x === 'string' && x.trim() ? x.trim().replace(/[\r\n]+/g, ' ').slice(0, max) : '');
  const en = one(v.en), ar = one(v.ar);
  return en || ar ? { ...(en ? { en } : {}), ...(ar ? { ar } : {}) } : undefined;
};
const strs = (v: any): string[] => (Array.isArray(v) ? v.filter((x) => typeof x === 'string') : []);
const boxList = (v: any) => strs(v).filter((x) => /^widget:[\w.:-]{1,120}$/.test(x)).slice(0, 6);

/** Read `pages.json` as the service hands it over (null when there is none or customisations are off). */
export function setPages(v: any) {
  const cfg: Record<string, PageCfg> = {};
  if (v && typeof v === 'object' && !Array.isArray(v)) {
    for (const [id, spec] of Object.entries(PAGES)) {
      const c = (v as any)[id];
      if (!c || typeof c !== 'object' || Array.isArray(c)) continue;
      const labels: Record<string, Text> = {};
      if (c.labels && typeof c.labels === 'object') for (const k of spec.labels) { const l = text(c.labels[k], 60); if (l) labels[k] = l; }
      cfg[id] = {
        title: id === 'docs' ? undefined : text(c.title, 60),
        intro: id === 'docs' ? undefined : text(c.intro, 300),
        hidden: strs(c.hidden).filter((x) => spec.parts.includes(x)),
        labels,
        boxes: id === 'home' ? { top: [], bottom: [] } : { top: boxList(c.boxes?.top), bottom: boxList(c.boxes?.bottom) },
        replace: spec.replace && typeof c.replace === 'string' && /^[\w.:-]{1,120}$/.test(c.replace) ? c.replace : undefined,
      };
    }
  }
  st.cfg = cfg;
}

const pick = (l?: Text) => (l ? (lang() === 'ar' ? l.ar || l.en : l.en || l.ar) || '' : '');

/** Is this part of the page shown? (Hidden only when `pages.json` names it.) */
export const shows = (page: string, part: string): boolean => !st.cfg[page]?.hidden.includes(part);
/** A text of the page: the customisation's wording, or the app's own. */
export const pageText = (page: string, name: string, own: string): string => pick(st.cfg[page]?.labels[name]) || own;
/** The page's heading: the customisation's, else the sidebar name the person gave it, else the app's own. */
export const pageTitle = (page: string, own?: string): string => pick(st.cfg[page]?.title) || own || navLabel(page);
/** A line of the customisation's under the heading ('' when there is none). */
export const pageIntro = (page: string): string => pick(st.cfg[page]?.intro);
/** Is the heading the customisation's? (Home then fills in `{greeting}` and `{name}` itself.) */
export const ownTitle = (page: string): boolean => !!pick(st.cfg[page]?.title);

/** The add-on boxes placed at the top or bottom of a page, in the order given. */
export function pageBoxes(page: string, place: 'top' | 'bottom'): HomeWidget[] {
  const keys = st.cfg[page]?.boxes[place] ?? [];
  return keys.flatMap((k) => app.homeWidgets.filter((w) => 'widget:' + w.id === k));
}
/** Boxes placed on another page: Home leaves them out unless home.json places them there too. */
export function placedElsewhere(): Set<string> {
  return new Set(Object.values(st.cfg).flatMap((c) => [...c.boxes.top, ...c.boxes.bottom]));
}

/** The add-on page shown in place of the app's own, while it is there and loaded. A replacement
 *  that is missing (switched off, removed, failed to start) leaves the app's own page in place. */
export function pageSwap(page: string): PluginView | undefined {
  if (app.safeMode || st.original[page]) return undefined;
  const id = st.cfg[page]?.replace;
  return id ? findView(id) : undefined;
}
/** The add-on page that replaces this one, even while the app's own is shown instead. */
export const swapFor = (page: string): PluginView | undefined => (app.safeMode ? undefined : (st.cfg[page]?.replace ? findView(st.cfg[page].replace!) : undefined));
/** Show the app's own page (until the app is reloaded or `useSwap`). */
export function showOriginal(page: string) { st.original = { ...st.original, [page]: true }; }
export function useSwap(page: string) { const { [page]: _, ...rest } = st.original; st.original = rest; }
export const showingOriginal = (page: string): boolean => !!st.original[page];

// Which page is open, on the document (`<html data-page="todos">`), so the customisation's styles can
// change one page and leave the others alone: `[data-page="todos"] main h1 { … }`.
$effect.root(() => {
  $effect(() => { const kind = app.view.kind; try { document.documentElement.dataset.page = kind; } catch { /* no document */ } });
});
