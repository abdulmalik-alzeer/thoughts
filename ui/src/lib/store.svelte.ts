// Global reactive app state (Svelte 5 runes).
import { api, put } from './api';
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

export type PluginView = { id: string; title: string; icon?: string; plugin?: string; render: (el: HTMLElement) => any };
export type PluginCommand = { id: string; title: string; icon?: string; plugin?: string; run: () => void };

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

export function applyTheme() {
  const s = app.settings;
  if (!s) return;
  paint(s);
  ls.set('thoughts.look', JSON.stringify({ theme: s.theme, mode: s.mode, language: s.language }));
  requestAnimationFrame(() => syncDockIcon().catch(() => {}));
}

{ const look = cachedLook(); if (look) paint(look); }

matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applyTheme);

export function go(v: View) {
  app.view = v;
}
