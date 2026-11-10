// Loads the user's customisation layer (custom CSS, themes, home.json, plugins, HTML views) and runs
// the customise flow (ask → run → review → try / keep / discard, history, packs).
// Skipped entirely in safe mode so a broken customisation can always be undone.
//
// ── How plugin code runs (Plugin API v1) ────────────────────────────────────────────────────────
// * Plugin code never runs in the app window. Every `plugins/*.js` module and every `views/*.html`
//   page is loaded into its own <iframe sandbox="allow-scripts"> (no allow-same-origin → opaque
//   origin). From there it cannot read the app's DOM, module state, localStorage or the Tauri bridge,
//   and the frame's own content-security policy forbids all network access (`connect-src 'none'`).
//   The daemon token lives only in a private variable of lib/api.ts and is never sent to a frame.
// * The frame document is the static page public/plugin-frame.html, which runs public/plugin-frame.js
//   and carries its own strict content-security policy (see `frameSrc`). The host then posts the plugin's
//   source to it; the frame imports it from a blob URL and calls `default(pluginApi)`.
// * The frame talks to the host only through postMessage RPC: `{id, method, args}` →
//   `{id, result | error}`. The host identifies the caller by `event.source`, never by anything the
//   message says, and checks every data method against the permissions that plugin declared in
//   `manifest.json` (`plugins["<file>.js"].permissions`: library:read, todos:read, todos:write,
//   ideas:read, chat:start). Undeclared → the call rejects and the user sees one toast naming the
//   plugin and the missing permission. Private library paths are never listed, read or searched.
// * One hidden "background" frame per plugin runs for the whole session: it registers views,
//   widgets, chips, commands and item actions and receives events. Callbacks (`run`) are invoked
//   in that frame by handle. `render(el)` callbacks cannot draw into the app, so each visible
//   surface (a view page, a Home widget) gets its own frame of the same plugin, mounted into the
//   slot and told which surface to render; `el` is that frame's root element. A surface frame is a
//   fresh run of the plugin: it shares no JavaScript state with the background frame.
// * HTML views get only the read-only subset (`library.*`, `todos.list`, `ideas.list`, `on`), through
//   the raw protocol or `window.thoughts`. Per the layer contract (LAYER.md) they need no manifest
//   entry: a view is a static page with no network and no write access, reviewed before it is kept.
// * The current theme's CSS variables are copied into every frame (and re-sent when the theme,
//   mode or language changes) so plugin UI matches the app.
// * Everything a plugin registered is disposed when the layer reloads; a plugin that throws is
//   switched off on its own; the crash-loop guard below turns customisations off after a start
//   that never finished.
import { api, on, post, reloadApp, ApiError } from './api';
import { app, toast, go, refreshChats, type PendingChange } from './store.svelte';
import { t, fmtDate } from './i18n';

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
export function retryLayer() { flag.clear(); reloadApp(); }

// ---------- per-plugin bookkeeping ----------
type Surface = { kind: 'view' | 'widget' | 'html'; id: string };
type Plugin = {
  name: string; kind: 'plugin' | 'view'; code: string; perms: Set<string>; dead: boolean;
  disposers: (() => void)[]; frames: Set<Frame>; warned: Set<string>; lastChat: number;
};
type Frame = { el: HTMLIFrameElement; plugin: Plugin; surface: Surface | null; events: Set<string> };

let plugins: Plugin[] = [];
const frames = new Set<Frame>();
let bg: HTMLElement | null = null;

const PERMS = ['library:read', 'todos:read', 'todos:write', 'ideas:read', 'chat:start'];
const NEEDS: Record<string, string> = {
  'library.list': 'library:read', 'library.read': 'library:read', 'library.search': 'library:read',
  'todos.list': 'todos:read', 'todos.add': 'todos:write', 'ideas.list': 'ideas:read', 'chat.start': 'chat:start',
};
const READ_ONLY = new Set(['library.list', 'library.read', 'library.search', 'todos.list', 'ideas.list', 'on', 'off']);
/** Events a plugin may listen to, and the only fields it is given. */
const EVENTS: Record<string, string[]> = {
  'todos.changed': [], 'library.changed': [], 'layer.changed': [], 'settings.changed': [], 'wrap.done': [], 'chat.done': ['chatId'],
};
const VIEWS = ['home', 'chats', 'chat', 'ideas', 'todos', 'docs', 'projects', 'settings', 'help', 'plugin'];

function destroyFrame(f: Frame) {
  frames.delete(f);
  f.plugin.frames.delete(f);
  f.el.remove();
}

function dispose(p: Plugin) {
  for (const d of p.disposers.splice(0)) { try { d(); } catch { /* already gone */ } }
  for (const f of [...p.frames]) destroyFrame(f);
}

/** A plugin threw: tell the user once, then switch that plugin off (its frames, commands, views, widgets). */
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

// ---------- theme → frames ----------
let varNames: string[] | null = null;

function themeCss(): string {
  if (!varNames) {
    const names = new Set<string>();
    const walk = (rules: CSSRuleList) => {
      for (const r of Array.from(rules) as any[]) {
        if (r.style) for (const n of Array.from(r.style as CSSStyleDeclaration)) if (n.startsWith('--')) names.add(n);
        if (r.cssRules) walk(r.cssRules);
      }
    };
    for (const sheet of Array.from(document.styleSheets)) { try { walk(sheet.cssRules); } catch { /* cross-origin sheet */ } }
    varNames = [...names];
  }
  const cs = getComputedStyle(document.documentElement);
  const vars = varNames.map((n) => { const v = cs.getPropertyValue(n).trim(); return v ? `${n}:${v};` : ''; }).join('');
  const font = getComputedStyle(document.body).fontFamily;
  return `:root{${vars}}
html,body{margin:0;padding:0;background:transparent;color:var(--fg);font:13.5px/1.5 ${font};}
*{box-sizing:border-box}
a{color:var(--accent)}
button,input,select,textarea{font:inherit;color:inherit}
button,.btn{background:var(--fill);border:1px solid var(--hairline);border-radius:var(--radius);padding:5px 12px;cursor:pointer}
button:hover,.btn:hover{background:var(--fill-2)}
button.primary,.btn.primary{background:var(--accent);color:var(--accent-fg);border-color:transparent}
input,select,textarea{background:var(--fill);border:1px solid var(--hairline);border-radius:var(--radius);padding:5px 9px}
:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
.muted{color:var(--muted)}
.card{background:var(--panel);border:1px solid var(--hairline);border-radius:var(--radius-lg);padding:12px 14px}
code,pre{font-family:var(--mono);font-size:12.5px}
table{border-collapse:collapse}td,th{border-bottom:1px solid var(--hairline);padding:5px 8px;text-align:start}`;
}

function lookMsg(type: 'init' | 'look') {
  const r = document.documentElement;
  return { type, css: themeCss(), lang: r.lang, dir: r.dir || 'ltr', theme: r.dataset.theme, mode: r.dataset.mode };
}

function postTo(f: Frame, msg: any) { try { f.el.contentWindow?.postMessage(msg, '*'); } catch { /* frame gone */ } }

let lookTimer: ReturnType<typeof setTimeout> | undefined;
function relook() {
  clearTimeout(lookTimer);
  lookTimer = setTimeout(() => { if (!frames.size) return; const m = lookMsg('look'); for (const f of frames) postTo(f, m); }, 60);
}

// ---------- frames ----------
// Each frame loads the static page public/plugin-frame.html (which runs public/plugin-frame.js) from
// the app's own origin. Because it is a real document rather than srcdoc, it carries its own strict
// content-security policy instead of inheriting the app's, and it works under the desktop app's policy.
const frameSrc = () => new URL('plugin-frame.html', document.baseURI).href.replace(/[?#].*$/, '');

function createFrame(p: Plugin, surface: Surface | null, into: HTMLElement): Frame {
  const el = document.createElement('iframe');
  el.setAttribute('sandbox', 'allow-scripts');
  el.setAttribute('referrerpolicy', 'no-referrer');
  el.setAttribute('title', p.name.replace(/\.(js|html)$/, ''));
  el.style.cssText = surface
    ? `border:0;display:block;width:100%;height:${surface.kind === 'widget' ? '80px' : '100%'};background:transparent;color-scheme:normal`
    : 'border:0;width:0;height:0;position:absolute;visibility:hidden';
  if (!surface) { el.tabIndex = -1; el.setAttribute('aria-hidden', 'true'); }
  el.src = frameSrc();
  const f: Frame = { el, plugin: p, surface, events: new Set() };
  frames.add(f);
  p.frames.add(f);
  into.appendChild(el);
  return f;
}

/** Mount one of a plugin's surfaces into a slot. Returns the cleanup that removes it. */
function mountSurface(p: Plugin, surface: Surface, el: HTMLElement): () => void {
  if (p.dead) throw new Error(t('layer.pluginOff', { name: p.name }));
  const f = createFrame(p, surface, el);
  return () => destroyFrame(f);
}

function background(): HTMLElement {
  if (!bg) {
    bg = document.createElement('div');
    bg.setAttribute('aria-hidden', 'true');
    bg.style.cssText = 'position:fixed;width:0;height:0;overflow:hidden;inset-block-start:0;inset-inline-start:0';
    document.body.appendChild(bg);
  }
  return bg;
}

// ---------- host side of the RPC ----------
function denied(p: Plugin, perm: string): Error {
  if (!p.warned.has(perm)) {
    p.warned.add(perm);
    toast(t('layer.permMissing', { name: p.name, perm: t('layer.perm.' + perm) }), { kind: 'error', key: `perm:${p.name}:${perm}` });
  }
  return Object.assign(new Error(`Permission "${perm}" is not declared for ${p.name} in manifest.json`), { code: 'permission' });
}

const str = (v: any, max: number) => String(v ?? '').slice(0, max);
const isPrivate = (path: string) => ((app.settings?.privatePaths ?? []) as string[]).some((x) => { const b = String(x).replace(/\/+$/, ''); return !!b && (path === b || path.startsWith(b + '/')); });
function cleanPath(v: any): string {
  const p = String(v ?? '').replace(/\\/g, '/').replace(/^\.?\/+/, '');
  if (!p || p.split('/').some((s) => s === '..' || s === '') || /^([A-Za-z]:|~)/.test(p)) throw new Error('Invalid path');
  if (isPrivate(p)) throw new Error('That file is private');
  return p;
}
const plain = (v: any) => { try { return JSON.parse(JSON.stringify(v ?? null)); } catch { return null; } };

function register<T extends { id: string; plugin?: string }>(p: Plugin, key: 'pluginViews' | 'commands' | 'homeWidgets' | 'homeChips' | 'itemActions', item: T) {
  const list = app[key] as any[];
  (app as any)[key] = [...list.filter((x) => !(x.id === item.id && x.plugin === p.name)), item];
  p.disposers.push(() => unregister(p, key, item.id));
}
function unregister(p: Plugin, key: string, id: string) {
  (app as any)[key] = ((app as any)[key] as any[]).filter((x) => !(x.id === id && x.plugin === p.name));
}

async function handle(f: Frame, method: string, args: any[]): Promise<any> {
  const p = f.plugin;
  if (p.dead) throw new Error('This add-on was switched off');
  if (p.kind === 'view' && !READ_ONLY.has(method)) throw new Error(`"${method}" is not available to HTML views`);
  const perm = NEEDS[method];
  if (perm && !p.perms.has(perm)) throw denied(p, perm);
  const a = Array.isArray(args) ? args : [];
  const invoke = (cb: any) => (...x: any[]) => { if (!p.dead) postTo(f, { type: 'invoke', cb, args: x.map(plain) }); };
  switch (method) {
    case 'library.list': return ((await api('/library/tree?exclude=private')) as any[]).filter((x) => !isPrivate(x.path)).map(plain);
    case 'library.read': { const d = await api(`/library/file?path=${encodeURIComponent(cleanPath(a[0]))}`); return String(d.content ?? ''); }
    case 'library.search': { const q = str(a[0], 200).trim(); return q ? ((await api('/library/search?q=' + encodeURIComponent(q))) as any[]).filter((x) => !isPrivate(x.path)).map(plain) : []; }
    case 'todos.list': return ((await api('/todos')) as any[]).filter((l) => !isPrivate(l.file)).map(plain);
    case 'todos.add': {
      const text = str(a[0], 500).replace(/\s*\n\s*/g, ' ').trim();
      if (!text) throw new Error('todos.add(text): text is empty');
      await post('/todos/add', { file: 'TODO.md', text });
      return { ok: true };
    }
    case 'ideas.list': return ((await api('/library/ideas')) as any[]).filter((x) => !isPrivate(x.path)).map(plain);
    case 'chat.start': {
      const prompt = str(a[0], 8000).trim();
      if (!prompt) throw new Error('chat.start(prompt): prompt is empty');
      if (Date.now() - p.lastChat < 5000) throw new Error('chat.start: wait a few seconds between chats');
      p.lastChat = Date.now();
      const c = await post('/chats', {});
      await post(`/chats/${c.id}/send`, { text: prompt });
      refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
      return { chatId: c.id };
    }
    case 'ui.toast': { const k = a[1]?.kind; toast(str(a[0], 500), { kind: k === 'error' || k === 'success' ? k : 'info' }); return null; }
    case 'ui.go': {
      const own = typeof a[0] === 'string' && app.pluginViews.find((x) => x.id === a[0] || x.id === 'view:' + a[0]);
      if (own) return await go({ kind: 'plugin', id: own.id });
      const v = typeof a[0] === 'string' ? { kind: a[0] } : { kind: str(a[0]?.kind, 40), id: a[0]?.id != null ? str(a[0].id, 200) : undefined, path: a[0]?.path != null ? str(a[0].path, 500) : undefined };
      if (!VIEWS.includes(v.kind)) throw new Error(`ui.go: unknown view "${v.kind}"`);
      return await go(v);
    }
    case 'on': { const ev = str(a[0], 60); if (!(ev in EVENTS)) throw new Error(`on: "${ev}" is not an event add-ons can listen to (${Object.keys(EVENTS).join(', ')})`); f.events.add(ev); return null; }
    case 'off': f.events.delete(str(a[0], 60)); return null;
  }
  // Registration is only taken from the plugin's background frame.
  if (f.surface) return null;
  const s = a[0] ?? {};
  const id = str(s.id, 80);
  if (method !== 'ui.remove' && !id) throw new Error(`${method}: "id" is missing`);
  switch (method) {
    case 'ui.addView':
      register(p, 'pluginViews', { id, title: str(s.title, 60), icon: s.icon ? str(s.icon, 30) : undefined, plugin: p.name, render: (el: HTMLElement) => mountSurface(p, { kind: 'view', id }, el) });
      return null;
    case 'ui.addHomeWidget':
      register(p, 'homeWidgets', { id, title: str(s.title, 60), plugin: p.name, render: (el: HTMLElement) => mountSurface(p, { kind: 'widget', id }, el) });
      return null;
    case 'ui.addCommand':
      register(p, 'commands', { id, title: str(s.title, 80), icon: s.icon ? str(s.icon, 30) : undefined, plugin: p.name, run: invoke(s.cb) });
      return null;
    case 'ui.addHomeChip':
      register(p, 'homeChips', { id, label: str(s.label, 40), prompt: str(s.prompt, 4000), plugin: p.name });
      return null;
    case 'ui.addItemAction': {
      const target = s.target === 'idea' ? 'idea' : s.target === 'todo' ? 'todo' : null;
      if (!target) throw new Error('addItemAction: target must be "todo" or "idea"');
      register(p, 'itemActions', { id, target, label: str(s.label, 60), plugin: p.name, run: invoke(s.cb) });
      return null;
    }
    case 'ui.remove': {
      const key = ({ view: 'pluginViews', widget: 'homeWidgets', command: 'commands', chip: 'homeChips', action: 'itemActions' } as Record<string, string>)[str(a[0], 20)];
      if (key) unregister(p, key, str(a[1], 80));
      return null;
    }
  }
  throw new Error(`Unknown method "${method}"`);
}

function onMessage(e: MessageEvent) {
  let f: Frame | undefined;
  for (const x of frames) if (x.el.contentWindow === e.source) { f = x; break; }
  if (!f) return;
  const m = e.data;
  if (!m || typeof m !== 'object') return;
  const p = f.plugin;
  if (m.type === 'ready') {
    postTo(f, { ...lookMsg('init'), kind: p.kind, name: p.name, code: p.code, surface: f.surface && f.surface.kind !== 'html' ? f.surface : null });
  } else if (m.type === 'failed') {
    if (m.code === 'permission') return; // the user was already told which permission is missing
    fail(p, new Error(str(m.error, 2000)));
  } else if (m.type === 'size') {
    if (f.surface?.kind === 'widget') f.el.style.height = Math.max(24, Math.min(900, Number(m.height) || 0)) + 'px';
  } else if (typeof m.method === 'string' && 'id' in m) {
    const frame = f;
    handle(frame, m.method, m.args).then(
      (result) => postTo(frame, { id: m.id, result }),
      (err) => postTo(frame, { id: m.id, error: String(err?.message ?? err), code: err?.code }),
    );
  }
}

if (typeof window !== 'undefined') {
  window.addEventListener('message', onMessage);
  on((e) => {
    const fields = EVENTS[e?.type];
    if (!fields) return;
    const payload = Object.fromEntries(fields.map((k) => [k, e[k]]));
    for (const f of frames) if (f.events.has(e.type) && !f.plugin.dead) postTo(f, { type: 'event', event: e.type, payload });
  });
  new MutationObserver(relook).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-mode', 'dir', 'lang', 'class'] });
}

// ---------- loading ----------
function permsOf(manifest: any, ...keys: string[]): Set<string> {
  for (const [group, key] of keys.map((k) => k.split('|'))) {
    const list = manifest?.[group]?.[key]?.permissions;
    if (Array.isArray(list)) return new Set(list.filter((x: any) => PERMS.includes(x)));
  }
  return new Set();
}

function unload() {
  for (const p of plugins) { p.dead = true; dispose(p); }
  plugins = [];
  app.pluginViews = [];
  app.commands = [];
  app.homeWidgets = [];
  app.homeChips = [];
  app.itemActions = [];
}

const strings = (v: any) => (Array.isArray(v) ? v.filter((x) => typeof x === 'string') : []);

/** Load the kept layer, or with `preview` the staged one ("Try it"). Nothing staged is loaded any other way. */
export async function loadLayer(opts: { preview?: boolean } = {}) {
  if (app.safeMode) return;
  const boot = firstLoad;
  firstLoad = false;
  if (boot && flag.get()) {
    app.safeMode = true;
    app.safeReason = 'crash';
    return;
  }
  const m = await api(opts.preview ? '/layer?preview=1' : '/layer');
  // Reload: remove everything the previous generation of plugins registered.
  unload();
  app.preview = !!opts.preview;
  styleEl ??= document.head.appendChild(document.createElement('style'));
  styleEl.textContent = m.css || '';
  varNames = null;
  app.customThemes = Array.isArray(m.themes) ? m.themes : [];
  const h = m.home && typeof m.home === 'object' ? m.home : {};
  app.home = {
    chips: (Array.isArray(h.chips) ? h.chips : []).filter((c: any) => c && typeof c.label === 'string' && typeof c.prompt === 'string' && c.label.trim()).slice(0, 8).map((c: any) => ({ label: str(c.label, 40), prompt: str(c.prompt, 4000) })),
    hidden: strings(h.hidden),
    order: strings(h.order),
  };
  const manifest = m.manifest && typeof m.manifest === 'object' ? m.manifest : {};
  const mk = (name: string, kind: Plugin['kind'], code: string, perms: Set<string>): Plugin =>
    ({ name, kind, code: String(code ?? ''), perms, dead: false, disposers: [], frames: new Set(), warned: new Set(), lastChat: 0 });
  const views: any[] = m.views || [];
  const list: any[] = m.plugins || [];
  if (boot && (list.length || views.length)) flag.set();
  for (const v of views) {
    // Views get nothing by default; like add-ons they must declare read permissions in manifest.json ("views").
    const READ = ['library:read', 'todos:read', 'ideas:read'];
    const p = mk(v.name, 'view', v.code, new Set([...permsOf(manifest, `views|${v.name}`, `views|views/${v.name}`)].filter((x) => READ.includes(x))));
    plugins.push(p);
    const id = 'view:' + v.name;
    register(p, 'pluginViews', { id, title: v.name.replace(/\.html$/, ''), icon: 'plugin', plugin: p.name, render: (el: HTMLElement) => mountSurface(p, { kind: 'html', id }, el) });
  }
  for (const src of list) {
    const p = mk(src.name, 'plugin', src.code, permsOf(manifest, `plugins|${src.name}`, `plugins|plugins/${src.name}`));
    plugins.push(p);
    try { createFrame(p, null, background()); } catch (e) { fail(p, e); }
  }
  relook();
}

// ---------- customise flow: ask → run → review → try / keep / discard ----------
const sig = (p: PendingChange | null) => (p ? JSON.stringify([p.summary, p.files, p.origin]) : '');
let dismissed = '';
const missing = (e: any) => e instanceof ApiError && (e.status === 404 || e.status === 405);

/** Ask the daemon whether a staged change is waiting. Opens the review panel when `open` (unless the
 *  user already closed the panel for this very change). Quietly does nothing on an older service. */
export async function checkPending(open = true): Promise<PendingChange | null> {
  let p: PendingChange;
  try { p = await api('/layer/pending'); } catch { return app.pending; }
  if (!p?.exists) {
    app.pending = null;
    app.showReview = false;
    if (app.preview) await loadLayer().catch(() => {});
    return null;
  }
  app.pending = p;
  if (open && !p.running && sig(p) !== dismissed) app.showReview = true;
  return p;
}

export function openReview() { dismissed = ''; if (app.pending) app.showReview = true; else checkPending(true); }
export function closeReview() { dismissed = sig(app.pending); app.showReview = false; }

/** Start a customise run. The assistant works on a staged copy; nothing changes until the review. */
export async function startCustomise(prompt: string): Promise<string | null> {
  const text = prompt.trim();
  if (!text) return null;
  const r = await post('/layer/customize', { prompt: text, text });
  dismissed = '';
  refreshChats().catch(() => {});
  if (r?.chatId) go({ kind: 'chat', id: r.chatId });
  return r?.chatId ?? null;
}

/** "Try it": load the staged layer into the running app. */
export async function tryPending() {
  await loadLayer({ preview: true });
  closeReview();
}

export async function stopPreview() { await loadLayer(); }

/** "Keep": make the staged change part of the app. */
export async function keepPending(opts: { approveWorkflows?: boolean } = {}) {
  const r = await post('/layer/pending/apply', opts.approveWorkflows ? { approveWorkflows: true } : {});
  app.pending = null;
  app.showReview = false;
  await loadLayer().catch((e) => toast(t('shell.layerFailed'), { kind: 'error', detail: e.message }));
  toast(t('layer.kept'), {
    kind: 'success', key: 'layer',
    action: r?.hash ? { label: t('undo'), run: () => { revertChange(r.hash).then(() => toast(t('layer.undone'), { kind: 'success', key: 'layer' }), (e) => toast(t('layer.undoFailed'), { kind: 'error', detail: e.message })); } } : undefined,
  });
}

/** "Discard": throw the staged change away. */
export async function discardPending() {
  await post('/layer/pending/discard');
  app.pending = null;
  app.showReview = false;
  if (app.preview) await loadLayer().catch(() => {});
}

/** Undo one change from the history (the rest of the customisations stay). */
export async function revertChange(hash: string) {
  await post('/layer/revert', { hash });
  if (!app.preview) await loadLayer().catch(() => {});
}

/** Permissions declared by the staged layer's plugins — read as data for the review panel, nothing is run. */
export async function stagedPermissions(): Promise<{ name: string; permissions: string[] }[]> {
  try {
    const m = await api('/layer?preview=1');
    // Same lookup the runtime uses (permsOf), so what the review shows is exactly what will be granted.
    const READ = ['library:read', 'todos:read', 'ideas:read'];
    return [
      ...(m.plugins || []).map((p: any) => ({ name: p.name, permissions: [...permsOf(m.manifest, `plugins|${p.name}`, `plugins|plugins/${p.name}`)] })),
      ...(m.views || []).map((v: any) => ({ name: v.name, permissions: [...permsOf(m.manifest, `views|${v.name}`, `views|views/${v.name}`)].filter((x) => READ.includes(x)) })),
    ];
  } catch { return []; }
}

/** When a workflow runs, in plain words ("Every Friday at 4:00 PM", "After each wrap"). */
export function whenText(trigger: any): string {
  if (trigger?.type === 'event') return t(trigger.on === 'app.start' ? 'layer.wf.onStart' : trigger.on === 'wrap.done' ? 'layer.wf.onWrap' : 'layer.wf.unknown');
  if (trigger?.type !== 'schedule') return t('layer.wf.unknown');
  const [h, m] = String(trigger.at ?? '').split(':').map((x) => parseInt(x, 10));
  const time = Number.isFinite(h) ? fmtDate(new Date(2023, 0, 1, h, m || 0), { timeStyle: 'short' }) : String(trigger.at ?? '');
  if (trigger.every === 'week') {
    // 1 January 2023 was a Sunday; weekday 0 = Sunday.
    const day = fmtDate(new Date(2023, 0, 1 + (Number(trigger.weekday) || 0), 12), { weekday: 'long' });
    return t('layer.wf.weekly', { day, time });
  }
  return t('layer.wf.daily', { time });
}

// ---------- packs ----------
/** Save the user's customisations as a pack file. Resolves 'saved', 'copied' (clipboard fallback) or 'cancelled'. */
export async function exportPack(): Promise<'saved' | 'copied' | 'cancelled'> {
  const pack = await api('/layer/export');
  if (pack?.format !== 'thoughts-pack') throw new Error(t('layer.packInvalid'));
  const text = JSON.stringify(pack, null, 2);
  const d = new Date();
  const name = `thoughts-pack-${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}.json`;
  if ((window as any).__TAURI_INTERNALS__) {
    // The native window has no download bar: ask where to save, then let the shell write the file.
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const path = await save({ defaultPath: name, filters: [{ name: 'Thoughts pack', extensions: ['json'] }] });
      if (!path) return 'cancelled';
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('save_text_file', { path, contents: text });
      return 'saved';
    } catch {
      try { await navigator.clipboard.writeText(text); return 'copied'; } catch { /* fall through to a download */ }
    }
  }
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url; a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
  return 'saved';
}

/** Stage a pack for review. It is never applied directly. */
export async function importPack(file: File) {
  if (file.size > 5 * 1024 * 1024) throw new Error(t('layer.packTooBig'));
  let pack: any;
  try { pack = JSON.parse(await file.text()); } catch { throw new Error(t('layer.packInvalid')); }
  if (pack?.format !== 'thoughts-pack' || typeof pack.files !== 'object') throw new Error(t('layer.packInvalid'));
  await post('/layer/import', { pack });
  dismissed = '';
  await checkPending(true);
}

export { missing as isMissingEndpoint };
