// Global reactive app state (Svelte 5 runes).
import { api, inTauri, post, put } from './api';
import { syncDockIcon } from './logo';
import { newestOnly, oneAtATime } from './serial';
import { isSecondary, MAIN_ONLY, showInMain, windowLabel } from './windows';

export type View = { kind: string; id?: string; path?: string };

/** Options for `toast(text, opts)`. All optional — `toast('Saved')` still works. */
export type ToastOpts = {
  /** `error` is announced assertively and stays a little longer (6s); default `info`. */
  kind?: 'info' | 'success' | 'error';
  /** One button inside the toast, e.g. `{ label: t('undo'), run: restore }`. The toast closes when it is pressed. */
  action?: { label: string; run: () => void };
  /** Long or multi-line output (logs…) shown in a scrollable block with a Copy button. */
  detail?: string;
  /** Technical words behind an error (git's, the assistant's), folded away under "Show details". */
  tech?: string;
  /** Milliseconds on screen. Every toast fades away by itself: 4s (5s with a button, 6s for errors).
   *  A shorter value is honoured; longer ones and 0 are capped, since everything is kept in
   *  "Recent messages" (with its button) after it fades. */
  duration?: number;
  /** Only for something still in progress (an update installing): stays until replaced or closed. */
  stay?: boolean;
  /** Toasts with the same key replace each other instead of stacking. */
  key?: string;
};
export type ToastItem = { id: number; text: string; kind: 'info' | 'success' | 'error'; action?: { label: string; run: () => void }; detail?: string; tech?: string; duration: number; key?: string };
/** A toast as remembered in "Recent messages" after it has left the screen. */
export type ToastLogItem = { id: number; text: string; kind: 'info' | 'success' | 'error'; detail?: string; at: number; key?: string; action?: { label: string; run: () => void } };

/** `render(el)` mounts the plugin's sandboxed frame into `el` and returns a cleanup function. */
/** `id` is unique in the app (it carries the add-on's file name); `short` is the id the add-on chose. */
export type PluginView = { id: string; short?: string; title: string; icon?: string; plugin?: string; render: (el: HTMLElement) => any };
export type PluginCommand = { id: string; short?: string; title: string; icon?: string; plugin?: string; run: () => void };
export type HomeWidget = { id: string; title: string; plugin: string; render: (el: HTMLElement) => any };
export type HomeChip = { id: string; label: string; prompt: string; plugin?: string };
export type ItemAction = { target: 'todo' | 'idea'; id: string; label: string; plugin: string; run: (item: any) => void };
export type HomeSection = 'stats' | 'usage' | 'ideas' | 'upcoming' | 'todos' | 'chats' | 'projects';
export type HomeConfig = { chips: { label: string; prompt: string }[]; hidden: string[]; order: string[] };
/** A staged change to the customisation layer waiting for the user's review (`GET /api/layer/pending`). */
export type PendingChange = {
  exists: boolean; summary?: string; files?: { path: string; status: 'added' | 'modified' | 'deleted' }[];
  pluginChanged?: boolean; origin?: 'customize' | 'import' | 'update'; outsideChanges?: string[]; chatId?: string; running?: boolean;
  workflowChanged?: boolean; workflows?: { name: string; trigger: any; prompt: string }[];
  /** Hash of the staged content, sent back on Keep; and validation problems that block Keep. */
  signature?: string; problems?: { path: string; message: string }[];
  [extra: string]: any;
};
/** `nav.json` from the customisation layer: hide, reorder and relabel sidebar items. */
export type NavConfig = { hidden: string[]; order: string[]; labels: Record<string, { en?: string; ar?: string }> };
/** `todos.json` from the customisation layer: default grouping, order and filter of the Todos view. */
export type TodosConfig = { groupBy?: 'list' | 'due' | 'tag' | 'none'; sort?: 'list' | 'due'; filter?: 'all' | 'soon' | 'overdue'; showDone?: boolean; moveDone?: boolean };

const ls = {
  get(k: string): string | null { try { return localStorage.getItem(k); } catch { return null; } },
  set(k: string, v: string) { try { localStorage.setItem(k, v); } catch { /* private mode */ } },
};

export const app = $state({
  ready: false,
  error: '' as string,
  status: null as any,
  settings: null as any,
  view: { kind: 'home' } as View,
  chats: [] as any[],
  toasts: [] as ToastItem[],
  /** The last 20 messages shown as toasts, newest first ("Recent messages" in the sidebar), and
   *  whether an error arrived since that list was last opened. */
  toastLog: [] as ToastLogItem[],
  toastLogUnread: false,
  wrap: { count: 0, threshold: 5, chats: [] as any[] },
  pluginViews: [] as PluginView[],
  /** Commands registered by plugins (`pluginApi.addCommand`), listed in the command palette. */
  commands: [] as PluginCommand[],
  customThemes: [] as { id: string; name: string }[],
  /** Slots filled by the customisation layer: `home.json` plus what plugins register. */
  home: { chips: [], hidden: [], order: [] } as HomeConfig,
  /** `nav.json` / `todos.json` from the layer (null when the user has none). Set by lib/layer.ts. */
  nav: null as NavConfig | null,
  todosCfg: null as TodosConfig | null,
  homeWidgets: [] as HomeWidget[],
  homeChips: [] as HomeChip[],
  itemActions: [] as ItemAction[],
  /** A staged change waiting for review, and whether the review panel is open. */
  pending: null as PendingChange | null,
  showReview: false,
  /** True while the staged layer (not the kept one) is loaded into the running app ("Try it"). */
  preview: false,
  /** Set by a view with unsaved work: awaited before navigating away; resolve false to stay. */
  leaveGuard: null as (() => Promise<boolean>) | null,
  safeMode: new URLSearchParams(location.search).has('safe'),
  /** Why customisations are off: the user asked, or the last start never finished. */
  safeReason: (new URLSearchParams(location.search).has('safe') ? 'manual' : '') as '' | 'manual' | 'crash',
  /** False while the background service is unreachable mid-session. */
  online: true,
  /** A second window (lib/windows.ts) starts with the narrow rail, leaving the room to what it shows. */
  sidebarCollapsed: isSecondary() || ls.get('thoughts.sidebar') === 'collapsed',
  /** "Show cost on Wrap, reorganise and setup chats" as an older window kept it on this device;
   *  the saved setting (`settings.showCost`) wins once there is one (see `costVisible`). */
  showCost: ls.get('thoughts.showCost') === '1',
  /** Open todos due today or overdue (kept fresh by lib/reminders.ts); shown on the Todos nav item. */
  dueToday: 0,
  /** Whether Back / Forward have somewhere to go (view history, see `goBack`). */
  canBack: false,
  canForward: false,
  showPalette: false,
  showRollback: false,
  showWrapNag: false,
});

/** The release this window was built from. */
export const APP_VERSION: string = __APP_VERSION__;
/** The version to show: the window's, plus the background service's when it is a different one
 *  (an older service kept running after an update). */
export function versionLabel(): string {
  const svc = app.status?.version;
  return svc && svc !== APP_VERSION ? `${APP_VERSION} · ${svc}` : APP_VERSION;
}

let toastSeq = 0;

/** Show a toast. Returns its id (pass to `dismissToast`). */
export function toast(text: string, opts: ToastOpts = {}): number {
  let msg = String(text ?? '');
  let detail = opts.detail;
  // Long or multi-line messages (logs, stack traces) move into the scrollable detail block.
  if (!detail && (msg.includes('\n') || msg.length > 280)) {
    detail = msg;
    msg = msg.split('\n')[0].slice(0, 140);
  }
  const kind = opts.kind ?? 'info';
  const max = kind === 'error' ? 6000 : opts.action ? 5000 : 4000;
  const duration = opts.stay ? 0 : opts.duration && opts.duration > 0 ? Math.min(opts.duration, max) : max;
  const id = ++toastSeq;
  const rest = app.toasts.filter((x) => !(opts.key && x.key === opts.key));
  const tech = opts.tech && opts.tech !== detail && opts.tech !== msg ? opts.tech : undefined;
  app.toasts = [...rest.slice(-4), { id, text: msg, kind, action: opts.action, detail, tech, duration, key: opts.key }];
  // Kept after the toast is gone (or pushed out by newer ones). A keyed toast replaces its older entry.
  app.toastLog = [{ id, text: msg, kind, detail, at: Date.now(), key: opts.key, action: opts.action }, ...app.toastLog.filter((x) => !(opts.key && x.key === opts.key))].slice(0, 20);
  if (kind === 'error') app.toastLogUnread = true;
  return id;
}

export function dismissToast(id: number) {
  app.toasts = app.toasts.filter((x) => x.id !== id);
}

/** Ask the shell to do something (and tell every view it happened): dispatches the window
 *  CustomEvent `thoughts:command` with `detail = { id, ...extra }`. See lib/shortcuts.ts for ids. */
export function command(id: string, extra: Record<string, any> = {}) {
  window.dispatchEvent(new CustomEvent('thoughts:command', { detail: { id, ...extra } }));
}

export function setSidebarCollapsed(v: boolean) {
  app.sidebarCollapsed = v;
  if (!isSecondary()) ls.set('thoughts.sidebar', v ? 'collapsed' : 'open');
}

/** The cost choice is a setting (it travels with "move to another computer"). */
export async function setShowCost(v: boolean) {
  await saveSettings({ showCost: v });
  app.showCost = v;
  ls.set('thoughts.showCost', v ? '1' : '0');
}
/** The cost shown at the top of Wrap, reorganise and setup chats (ordinary chats always have it in
 *  their usage ring): hidden on a Claude plan (they don't pay per message) unless asked for. */
export const costVisible = () => (app.settings?.showCost ?? app.showCost) || !!app.settings?.ai?.claude?.hasApiToken;
/** A choice an older window kept only on this device becomes the setting, once. */
export function keepShowCostChoice() {
  if (app.settings && app.settings.showCost == null && app.showCost && !app.status?.settingsNewer) setShowCost(true).catch(() => {});
}

// Events come in bursts (`chat.updated` then `chat.done`) and each starts a refresh: an older answer
// that arrives last must not bring back a finished chat's "running" dot or an old Wrap count.
const chatsOrder = newestOnly();
const wrapOrder = newestOnly();

export async function refreshChats() {
  await chatsOrder(() => api('/chats'), (v) => { app.chats = v; });
}

export async function refreshWrap() {
  await wrapOrder(() => api('/wrap'), (v) => {
    const prev = app.wrap.count;
    app.wrap = v;
    if (app.wrap.count > app.wrap.threshold && prev <= app.wrap.threshold && prev !== 0) remindToWrap();
  });
}

/** Show the Wrap reminder — but never under someone's fingers. The count crosses the threshold at
 *  the moment a message is sent, so the person is usually still typing: wait until the keyboard has
 *  been quiet for a few seconds (and the reminder is still due) before the dialog takes the focus. */
let nagWait: ReturnType<typeof setTimeout> | undefined;
function remindToWrap() {
  if (app.showWrapNag || nagWait !== undefined || isSecondary()) return;
  const QUIET_MS = 4000;
  const typing = () => {
    const el = document.activeElement as HTMLElement | null;
    return !!el && (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT' || el.isContentEditable);
  };
  if (!typing()) { app.showWrapNag = true; return; }
  const done = () => { clearTimeout(nagWait); nagWait = undefined; window.removeEventListener('keydown', again, true); };
  const show = () => { done(); if (app.wrap.count > app.wrap.threshold) app.showWrapNag = true; };
  const again = () => { clearTimeout(nagWait); nagWait = setTimeout(show, QUIET_MS); };
  window.addEventListener('keydown', again, true);
  again();
}

/** Every settings save, one at a time. The service takes the whole settings object, so two saves
 *  running together (a theme pick while Settings saves a switch) would each send what the other
 *  had not seen yet, and the last to land would undo the other's change. */
const settingsSaves = oneAtATime();

/** Save settings. `patch` is laid over the settings as they are when this save's turn comes (after
 *  any save still running), or is a function that builds the whole object from them. `query`
 *  carries a one-off confirmation (`?confirm=bypassPermissions`). */
export function saveSettings(patch: Record<string, any> | ((now: any) => Record<string, any>) = {}, query = ''): Promise<void> {
  return settingsSaves(async () => {
    const body = typeof patch === 'function' ? patch(app.settings) : { ...app.settings, ...patch };
    app.settings = await put('/settings' + query, body);
    applyTheme();
  });
}

/** Read the settings again (`settings.changed`: every save, ours included, announces one). In the
 *  same line as the saves, so an answer read before a save landed never replaces that save's result
 *  and becomes the base the next save is built on. */
export function reloadSettings(): Promise<void> {
  return settingsSaves(async () => {
    app.settings = await api('/settings');
    applyTheme();
  });
}

// ---------- accessibility (Settings › Accessibility) ----------
export type A11y = { zoom: number; reduceMotion: boolean; underlineLinks: boolean; strongFocus: boolean; textSpacing: boolean };
export const A11Y_DEFAULTS: A11y = { zoom: 100, reduceMotion: false, underlineLinks: false, strongFocus: false, textSpacing: false };
/** The sizes Zoom in / Zoom out step through, in percent. */
export const ZOOM_STEPS = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200];
const withDefaults = (a?: Partial<A11y> | null): A11y => ({ ...A11Y_DEFAULTS, ...(a ?? {}) });
/** The accessibility preferences in force (saved settings, with defaults filled in). */
export const a11y = (): A11y => withDefaults(app.settings?.accessibility);

let zoomNow = 1;
/** The accessibility preferences on screen right now: ahead of `app.settings` while a change is
 *  still being saved, so quick changes in a row (Cmd+= pressed three times) build on each other. */
let painted: A11y | null = null;
/** Accessibility changes still being saved: until they are, the screen keeps showing `painted`. */
let a11ySaving = 0;
/** Size the whole window: the webview's own zoom inside the app (the page reflows, like a browser's
 *  zoom), CSS zoom when running in a plain browser or when the shell refuses. */
async function applyZoom(percent: number) {
  const z = Math.min(200, Math.max(50, Math.round(Number(percent) || 100))) / 100;
  if (z === zoomNow) return;
  zoomNow = z;
  if (inTauri()) {
    try {
      const { getCurrentWebview } = await import('@tauri-apps/api/webview');
      await getCurrentWebview().setZoom(z);
      return;
    } catch { /* an older shell without the permission: fall back to CSS */ }
  }
  (document.documentElement.style as any).zoom = z === 1 ? '' : String(z);
}

function paintA11y(a: A11y) {
  painted = a;
  const d = document.documentElement.dataset;
  const flag = (k: string, on: boolean, v: string) => { if (on) d[k] = v; else delete d[k]; };
  flag('motion', a.reduceMotion, 'reduce');
  flag('links', a.underlineLinks, 'underline');
  flag('focus', a.strongFocus, 'strong');
  flag('spacing', a.textSpacing, 'wide');
  applyZoom(a.zoom).catch(() => {});
}

/** Change accessibility preferences: applied at once and saved straight away, like the theme. */
export async function setA11y(patch: Partial<A11y>) {
  paintA11y({ ...(painted ?? a11y()), ...patch });
  a11ySaving++;
  let saved = false;
  // Only what changed here goes over the saved preferences, when this save's turn comes.
  try { await saveSettings((now) => ({ ...now, accessibility: { ...withDefaults(now?.accessibility), ...patch } })); saved = true; }
  finally {
    a11ySaving--;
    // Once nothing else is on its way the screen shows what is saved (a refused change goes back).
    if (!saved || !a11ySaving) applyTheme();
  }
}

/** Zoom one step in (1) or out (-1), or back to actual size (0). Resolves the new percentage. */
export async function zoomStep(dir: -1 | 0 | 1): Promise<number> {
  const cur = (painted ?? a11y()).zoom;
  const next = dir === 0 ? 100 : dir > 0 ? ZOOM_STEPS.find((z) => z > cur) ?? cur : [...ZOOM_STEPS].reverse().find((z) => z < cur) ?? cur;
  if (next !== cur) await setA11y({ zoom: next });
  return next;
}

/** Local-only usage signal (`POST /api/usage`) that feeds suggestions. Fire-and-forget: never blocks,
 *  never reports failures, and sends nothing when the user switched the signals off. */
export function track(event: string) {
  if (!app.settings?.onboarded || app.settings.usageSignals === false) return;
  try { post('/usage', { event: String(event).slice(0, 120) }).catch(() => {}); } catch { /* ignore */ }
}

type Look = { theme?: string; mode?: string; language?: string; accessibility?: Partial<A11y> | null };

function paint(s: Look) {
  const dark = s.mode === 'dark' || (s.mode === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
  const root = document.documentElement;
  root.dataset.theme = s.theme || 'saudi';
  root.dataset.mode = dark ? 'dark' : 'light';
  root.lang = s.language || 'en';
  root.dir = s.language === 'ar' ? 'rtl' : 'ltr';
  paintA11y(withDefaults(s.accessibility));
}

/** The look last applied, remembered so the next start (and the "can't connect" screen)
 *  uses the right theme and language before settings have loaded. */
export function cachedLook(): Look | null {
  try { return JSON.parse(ls.get('thoughts.look') || 'null'); } catch { return null; }
}

let menuLang: string | undefined;
/** The native menus and the Dock icon belong to the main window: never a second one, nor the
 *  quick capture box (which paints the same look through `applyTheme`). */
const ownsShell = () => !isSecondary() && windowLabel() !== 'capture';

export function applyTheme() {
  const s = app.settings;
  if (!s) return;
  // An earlier save that lands while later accessibility changes are on their way must not flash
  // the window back a step (zoom 110 → 125 → 150 would show 110 again for a moment).
  paint(a11ySaving && painted ? { ...s, accessibility: painted } : s);
  // The native menus are built in the shell: ask it to rebuild them when the language changes.
  if (menuLang !== undefined && menuLang !== s.language && inTauri() && ownsShell()) {
    import('@tauri-apps/api/core').then(({ invoke }) => invoke('refresh_menu')).catch(() => {});
  }
  menuLang = s.language;
  ls.set('thoughts.look', JSON.stringify({ theme: s.theme, mode: s.mode, language: s.language, accessibility: s.accessibility ?? null }));
  if (ownsShell()) requestAnimationFrame(() => syncDockIcon().catch(() => {}));
}

{ const look = cachedLook(); if (look) paint(look); }

matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applyTheme);

let leaving = false;

// View history for Back / Forward (Cmd/Ctrl+[ and ], mouse back buttons). Not persisted.
const trail: View[] = [{ kind: 'home' }];
let at = 0;
const sameView = (a: View, b: View) => a.kind === b.kind && (a.id ?? '') === (b.id ?? '') && (a.path ?? '') === (b.path ?? '');
function syncTrail() { app.canBack = at > 0; app.canForward = at < trail.length - 1; }

async function leave(): Promise<boolean> {
  if (leaving) return false;
  const guard = app.leaveGuard;
  if (guard) {
    leaving = true;
    try { if (!(await guard())) return false; }
    catch (e) { console.error(e); }
    finally { leaving = false; }
    if (app.leaveGuard === guard) app.leaveGuard = null;
  }
  return true;
}

/** Navigate. A view with unsaved work (`app.leaveGuard`) is asked first; resolves false when the user stays.
 *  `replace`: correct the current entry of Back/Forward instead of adding one (a view filling in its default). */
export async function go(v: View, opts: { replace?: boolean } = {}): Promise<boolean> {
  // Settings and Customise belong to the main window: a second window sends the person there.
  if (MAIN_ONLY.includes(v.kind) && isSecondary()) { showInMain(v); return false; }
  if (!(await leave())) return false;
  if (v.kind !== app.view.kind) track('view.' + v.kind);
  if (opts.replace) trail[at] = { ...v };
  else if (!sameView(v, trail[at])) {
    trail.splice(at + 1);
    trail.push({ ...v });
    if (trail.length > 60) trail.shift();
    at = trail.length - 1;
    syncTrail();
  }
  app.view = v;
  return true;
}

async function step(d: number): Promise<boolean> {
  const to = trail[at + d];
  if (!to || !(await leave())) return false;
  at += d;
  syncTrail();
  if (to.kind !== app.view.kind) track('view.' + to.kind);
  app.view = { ...to };
  return true;
}
/** Back / forward through the views visited in this session. */
export const goBack = () => step(-1);
export const goForward = () => step(1);
