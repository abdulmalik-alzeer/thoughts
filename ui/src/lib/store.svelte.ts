// Global reactive app state (Svelte 5 runes).
import { api, put } from './api';
import { syncDockIcon } from './logo';

export type View = { kind: string; id?: string; path?: string };

export const app = $state({
  ready: false,
  error: '' as string,
  status: null as any,
  settings: null as any,
  view: { kind: 'home' } as View,
  chats: [] as any[],
  toasts: [] as { id: number; text: string }[],
  wrap: { count: 0, threshold: 5, chats: [] as any[] },
  pluginViews: [] as { id: string; title: string; icon?: string; render: (el: HTMLElement) => any }[],
  customThemes: [] as { id: string; name: string }[],
  safeMode: new URLSearchParams(location.search).has('safe'),
  showRollback: false,
  showWrapNag: false,
});

export function toast(text: string) {
  const id = Math.random();
  app.toasts.push({ id, text });
  setTimeout(() => { app.toasts = app.toasts.filter((t) => t.id !== id); }, 5000);
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

export function applyTheme() {
  const s = app.settings;
  if (!s) return;
  const dark = s.mode === 'dark' || (s.mode === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
  const root = document.documentElement;
  root.dataset.theme = s.theme || 'saudi';
  root.dataset.mode = dark ? 'dark' : 'light';
  root.lang = s.language || 'en';
  root.dir = s.language === 'ar' ? 'rtl' : 'ltr';
  requestAnimationFrame(() => syncDockIcon().catch(() => {}));
}

matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applyTheme);

export function go(v: View) {
  app.view = v;
}
