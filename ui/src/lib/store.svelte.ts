// Global reactive app state (Svelte 5 runes).
import { api, inTauri, post, put } from './api';
import { syncDockIcon } from './logo';

export type View = { kind: string; id?: string; path?: string };

/** Options for `toast(text, opts)`. All optional — `toast('Saved')` still works. */
export type ToastOpts = {
  /** `error` is announced assertively and stays 10s; default `info`. */
  kind?: 'info' | 'success' | 'error';
  /** One button inside the toast, e.g. `{ label: t('undo'), run: restore }`. The toast closes when it is pressed. */
  action?: { label: string; run: () => void };
  /** Long or multi-line output (logs…) shown in a scrollable block with a Copy button. */
  detail?: string;
  /** Milliseconds on screen. Default scales with length (min 5s, errors 10s). 0 = stays until dismissed. */
  duration?: number;
  /** Toasts with the same key replace each other instead of stacking. */
  key?: string;
};
export type ToastItem = { id: number; text: string; kind: 'info' | 'success' | 'error'; action?: { label: string; run: () => void }; detail?: string; duration: number; key?: string };
/** A toast as remembered in "Recent messages" after it has left the screen. */
export type ToastLogItem = { id: number; text: string; kind: 'info' | 'success' | 'error'; detail?: string; at: number; key?: string };

/** `render(el)` mounts the plugin's sandboxed frame into `el` and returns a cleanup function. */
export type PluginView = { id: string; title: string; icon?: string; plugin?: string; render: (el: HTMLElement) => any };
export type PluginCommand = { id: string; title: string; icon?: string; plugin?: string; run: () => void };
export type HomeWidget = { id: string; title: string; plugin: string; render: (el: HTMLElement) => any };
export type HomeChip = { id: string; label: string; prompt: string; plugin?: string };
export type ItemAction = { target: 'todo' | 'idea'; id: string; label: string; plugin: string; run: (item: any) => void };
export type HomeSection = 'stats' | 'ideas' | 'upcoming' | 'todos' | 'chats' | 'projects';
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
export type TodosConfig = { groupBy?: 'list' | 'due' | 'tag' | 'none'; sort?: 'list' | 'due'; filter?: 'all' | 'soon' | 'overdue'; showDone?: boolean };

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
  sidebarCollapsed: ls.get('thoughts.sidebar') === 'collapsed',
  /** "Show estimated cost" (a per-device preference; see `costVisible`). */
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
  const min = kind === 'error' ? 10000 : opts.action ? 8000 : 5000;
  const duration = opts.duration ?? Math.min(30000, Math.max(min, 2500 + msg.length * 70 + (detail ? 6000 : 0)));
  const id = ++toastSeq;
  const rest = app.toasts.filter((x) => !(opts.key && x.key === opts.key));
  app.toasts = [...rest.slice(-4), { id, text: msg, kind, action: opts.action, detail, duration, key: opts.key }];
  // Kept after the toast is gone (or pushed out by newer ones). A keyed toast replaces its older entry.
  app.toastLog = [{ id, text: msg, kind, detail, at: Date.now(), key: opts.key }, ...app.toastLog.filter((x) => !(opts.key && x.key === opts.key))].slice(0, 20);
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
  ls.set('thoughts.sidebar', v ? 'collapsed' : 'open');
}

export function setShowCost(v: boolean) {
  app.showCost = v;
  ls.set('thoughts.showCost', v ? '1' : '0');
}
/** Dollar estimates are hidden for people on a Claude plan (they don't pay per message) unless they ask to see them. */
export const costVisible = () => app.showCost || !!app.settings?.ai?.claude?.hasApiToken;

export async function refreshChats() {
  app.chats = await api('/chats');
}

export async function refreshWrap() {
  const prev = app.wrap.count;
  app.wrap = await api('/wrap');
  if (app.wrap.count > app.wrap.threshold && prev <= app.wrap.threshold && prev !== 0) app.showWrapNag = true;
}

/** Save settings. `query` carries a one-off confirmation (`?confirm=bypassPermissions`). */
export async function saveSettings(patch: Record<string, any> = {}, query = '') {
  app.settings = await put('/settings' + query, { ...app.settings, ...patch });
  applyTheme();
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
  const next = { ...a11y(), ...patch };
  paintA11y(next);
  try { await saveSettings({ accessibility: next }); }
  catch (e) { applyTheme(); throw e; }
}

/** Zoom one step in (1) or out (-1), or back to actual size (0). Resolves the new percentage. */
export async function zoomStep(dir: -1 | 0 | 1): Promise<number> {
  const cur = a11y().zoom;
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

export function applyTheme() {
  const s = app.settings;
  if (!s) return;
  paint(s);
  // The native menus are built in the shell: ask it to rebuild them when the language changes.
  if (menuLang !== undefined && menuLang !== s.language && inTauri()) {
    import('@tauri-apps/api/core').then(({ invoke }) => invoke('refresh_menu')).catch(() => {});
  }
  menuLang = s.language;
  ls.set('thoughts.look', JSON.stringify({ theme: s.theme, mode: s.mode, language: s.language, accessibility: s.accessibility ?? null }));
  requestAnimationFrame(() => syncDockIcon().catch(() => {}));
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

/** Navigate. A view with unsaved work (`app.leaveGuard`) is asked first; resolves false when the user stays. */
export async function go(v: View): Promise<boolean> {
  if (!(await leave())) return false;
  if (v.kind !== app.view.kind) track('view.' + v.kind);
  if (!sameView(v, trail[at])) {
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
