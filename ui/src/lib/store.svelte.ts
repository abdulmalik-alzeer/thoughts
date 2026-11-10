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
};

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
  wrap: { count: 0, threshold: 5, chats: [] as any[] },
  pluginViews: [] as PluginView[],
  /** Commands registered by plugins (`pluginApi.addCommand`), listed in the command palette. */
  commands: [] as PluginCommand[],
  customThemes: [] as { id: string; name: string }[],
  /** Slots filled by the customisation layer: `home.json` plus what plugins register. */
  home: { chips: [], hidden: [], order: [] } as HomeConfig,
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
  showPalette: false,
  showRollback: false,
  showWrapNag: false,
});

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

export async function refreshChats() {
  app.chats = await api('/chats');
}

export async function refreshWrap() {
  const prev = app.wrap.count;
  app.wrap = await api('/wrap');
  if (app.wrap.count > app.wrap.threshold && prev <= app.wrap.threshold && prev !== 0) app.showWrapNag = true;
}

export async function saveSettings(patch: Record<string, any> = {}) {
  app.settings = await put('/settings', { ...app.settings, ...patch });
  applyTheme();
}

/** Local-only usage signal (`POST /api/usage`) that feeds suggestions. Fire-and-forget: never blocks,
 *  never reports failures, and sends nothing when the user switched the signals off. */
export function track(event: string) {
  if (!app.settings?.onboarded || app.settings.usageSignals === false) return;
  try { post('/usage', { event: String(event).slice(0, 120) }).catch(() => {}); } catch { /* ignore */ }
}

type Look = { theme?: string; mode?: string; language?: string };

function paint(s: Look) {
  const dark = s.mode === 'dark' || (s.mode === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
  const root = document.documentElement;
  root.dataset.theme = s.theme || 'saudi';
  root.dataset.mode = dark ? 'dark' : 'light';
  root.lang = s.language || 'en';
  root.dir = s.language === 'ar' ? 'rtl' : 'ltr';
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
  ls.set('thoughts.look', JSON.stringify({ theme: s.theme, mode: s.mode, language: s.language }));
  requestAnimationFrame(() => syncDockIcon().catch(() => {}));
}

{ const look = cachedLook(); if (look) paint(look); }

matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applyTheme);

let leaving = false;

/** Navigate. A view with unsaved work (`app.leaveGuard`) is asked first; resolves false when the user stays. */
export async function go(v: View): Promise<boolean> {
  if (leaving) return false;
  const guard = app.leaveGuard;
  if (guard) {
    leaving = true;
    try { if (!(await guard())) return false; }
    catch (e) { console.error(e); }
    finally { leaving = false; }
    if (app.leaveGuard === guard) app.leaveGuard = null;
  }
  if (v.kind !== app.view.kind) track('view.' + v.kind);
  app.view = v;
  return true;
}
