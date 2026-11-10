// Loads the user's app layer (custom CSS, themes, plugins, views).
// Skipped entirely in safe mode so a broken customisation can always be undone.
import { api, on, post } from './api';
import { app, toast, go } from './store.svelte';

let styleEl: HTMLStyleElement | null = null;
const commands: { id: string; title: string; run: () => void }[] = [];

const pluginApi = {
  api,
  post,
  on,
  toast,
  go,
  addView(v: { id: string; title: string; icon?: string; render: (el: HTMLElement) => any }) {
    app.pluginViews = [...app.pluginViews.filter((x) => x.id !== v.id), v];
  },
  addCommand(c: { id: string; title: string; run: () => void }) {
    commands.push(c);
  },
  commands,
};

export async function loadLayer() {
  if (app.safeMode) return;
  const m = await api('/layer');
  styleEl ??= document.head.appendChild(document.createElement('style'));
  styleEl.textContent = m.css || '';
  app.customThemes = Array.isArray(m.themes) ? m.themes : [];
  app.pluginViews = [];
  for (const v of m.views || []) {
    const html = v.code;
    app.pluginViews.push({
      id: 'view:' + v.name, title: v.name.replace(/\.html$/, ''), icon: 'plugin',
      render(el: HTMLElement) {
        const f = document.createElement('iframe');
        f.setAttribute('sandbox', 'allow-scripts');
        f.style.cssText = 'border:0;width:100%;height:100%';
        f.srcdoc = html;
        el.appendChild(f);
      },
    });
  }
  for (const p of m.plugins || []) {
    try {
      const url = URL.createObjectURL(new Blob([p.code], { type: 'text/javascript' }));
      const mod = await import(/* @vite-ignore */ url);
      await mod.default?.(pluginApi);
    } catch (e: any) {
      toast(`Plugin ${p.name} failed: ${e.message}. Use Recovery → Rollback if the app misbehaves.`);
    }
  }
}

(window as any).thoughts = pluginApi;
