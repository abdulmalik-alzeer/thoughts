// Loads the user's app layer (custom CSS, themes, plugins, views).
// Skipped entirely in safe mode so a broken customisation can always be undone.
import { api, on, post } from './api';
import { app, toast, go, type PluginCommand, type PluginView } from './store.svelte';
import { t } from './i18n';

let styleEl: HTMLStyleElement | null = null;

// ---------- crash-loop guard ----------
// Before running any plugin on start-up we leave a marker; the shell removes it once the app
// has finished booting. A marker still present on the next start means the last start never
// got that far, so we come up with customisations off and let the user roll back or retry.
const BOOT_KEY = 'thoughts.layerBoot';
const flag = {
  get: () => { try { return localStorage.getItem(BOOT_KEY); } catch { return null; } },
  set: () => { try { localStorage.setItem(BOOT_KEY, String(Date.now())); } catch { /* ignore */ } },
  clear: () => { try { localStorage.removeItem(BOOT_KEY); } catch { /* ignore */ } },
};
let firstLoad = true;

/** Called by the shell when the app has finished booting with the layer loaded. */
export function layerBooted() { if (app.safeReason !== 'crash') flag.clear(); }
/** Forget a failed start (after a rollback, or when the user chooses to try again). */
export function clearLayerBootFlag() { flag.clear(); }
/** Leave safe mode and load customisations again. */
export function retryLayer() { flag.clear(); location.replace(location.pathname); }

// ---------- per-plugin bookkeeping ----------
type Plugin = { name: string; dead: boolean; disposers: (() => void)[] };
let plugins: Plugin[] = [];

function dispose(p: Plugin) {
  for (const d of p.disposers.splice(0)) { try { d(); } catch { /* already gone */ } }
}

/** A plugin threw: tell the user once, then switch that plugin off (its listeners, commands and views). */
function fail(p: Plugin, e: any) {
  if (p.dead) return;
  p.dead = true;
  console.error(`[plugin ${p.name}]`, e);
  dispose(p);
  toast(t('shell.pluginCrashed', { name: p.name }), {
    kind: 'error', key: 'plugin:' + p.name, detail: String(e?.message ?? e),
    action: { label: t('shell.openRollback'), run: () => (app.showRollback = true) },
  });
}

function guard<F extends (...a: any[]) => any>(p: Plugin, fn: F): F {
  return ((...a: any[]) => {
    if (p.dead) return;
    try {
      const r = fn(...a);
      if (r && typeof r.then === 'function') return r.catch((e: any) => fail(p, e));
      return r;
    } catch (e) { fail(p, e); }
  }) as F;
}

/** The API handed to one plugin. Everything it registers is tracked so a reload can remove it. */
function makeApi(p: Plugin) {
  return {
    api,
    post,
    toast,
    go,
    /** Listen to daemon events. Returns a function that stops listening. */
    on(fn: (e: any) => void): () => void {
      const off = on(guard(p, fn));
      p.disposers.push(off);
      return off;
    },
    /** Add a page to the sidebar. Returns a function that removes it. */
    addView(v: Omit<PluginView, 'plugin'>): () => void {
      const view: PluginView = { ...v, plugin: p.name, render: guard(p, v.render) };
      app.pluginViews = [...app.pluginViews.filter((x) => x.id !== v.id), view];
      const off = () => { app.pluginViews = app.pluginViews.filter((x) => !(x.id === v.id && x.plugin === p.name)); };
      p.disposers.push(off);
      return off;
    },
    /** Add a command to the command palette. Returns a function that removes it. */
    addCommand(c: Omit<PluginCommand, 'plugin'>): () => void {
      const cmd: PluginCommand = { ...c, plugin: p.name, run: guard(p, c.run) };
      app.commands = [...app.commands.filter((x) => x.id !== c.id), cmd];
      const off = () => { app.commands = app.commands.filter((x) => !(x.id === c.id && x.plugin === p.name)); };
      p.disposers.push(off);
      return off;
    },
    get commands() { return app.commands; },
  };
}

export async function loadLayer() {
  if (app.safeMode) return;
  const boot = firstLoad;
  firstLoad = false;
  if (boot && flag.get()) {
    app.safeMode = true;
    app.safeReason = 'crash';
    return;
  }
  const m = await api('/layer');
  // Reload: remove everything the previous generation of plugins registered.
  for (const p of plugins) { p.dead = true; dispose(p); }
  plugins = [];
  styleEl ??= document.head.appendChild(document.createElement('style'));
  styleEl.textContent = m.css || '';
  app.customThemes = Array.isArray(m.themes) ? m.themes : [];
  app.pluginViews = [];
  app.commands = [];
  for (const v of m.views || []) {
    const html = v.code;
    app.pluginViews.push({
      id: 'view:' + v.name, title: v.name.replace(/\.html$/, ''), icon: 'plugin',
      render(el: HTMLElement) {
        const f = document.createElement('iframe');
        f.setAttribute('sandbox', 'allow-scripts');
        f.setAttribute('title', v.name.replace(/\.html$/, ''));
        f.style.cssText = 'border:0;width:100%;height:100%';
        f.srcdoc = html;
        el.appendChild(f);
      },
    });
  }
  const list = m.plugins || [];
  if (boot && list.length) flag.set();
  for (const src of list) {
    const p: Plugin = { name: src.name, dead: false, disposers: [] };
    plugins.push(p);
    const url = URL.createObjectURL(new Blob([src.code], { type: 'text/javascript' }));
    try {
      const mod = await import(/* @vite-ignore */ url);
      await mod.default?.(makeApi(p));
    } catch (e: any) {
      fail(p, e);
    } finally {
      URL.revokeObjectURL(url);
    }
  }
}

// Console / inline-script access. Not tied to a plugin file, so it is never disposed on reload.
(window as any).thoughts = makeApi({ name: 'console', dead: false, disposers: [] });
