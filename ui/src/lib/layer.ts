// Loads the user's customisation layer (custom CSS, themes, home.json, plugins, HTML views) and runs
// the customise flow (ask → run → review → try / keep / discard, history, packs).
// Skipped entirely in safe mode so a broken customisation can always be undone.
//
// ── How plugin code runs (Plugin API v1) ────────────────────────────────────────────────────────
// * Plugin code never runs in the app window. Every `plugins/*.js` module and every `views/*.html`
//   page is loaded into its own <iframe sandbox="allow-scripts allow-forms"> (no allow-same-origin →
//   opaque origin). From there it cannot read the app's DOM, module state, localStorage or the Tauri
//   bridge, and the frame's own content-security policy forbids all network access (`connect-src
//   'none'`; forms can be filled in and raise `submit`, but `form-action 'none'` sends them nowhere).
//   The daemon token lives only in a private variable of lib/api.ts and is never sent to a frame.
// * The frame document is the static page public/plugin-frame.html, which runs public/plugin-frame.js
//   and carries its own strict content-security policy (see `frameSrc`; on Windows it is handed over
//   as `srcdoc`, see `viaSrcdoc`). The host then posts the plugin's source to it; the frame imports
//   it from a blob URL and calls `default(pluginApi)`.
// * The frame talks to the host only through postMessage RPC: `{id, method, args}` →
//   `{id, result | error}`. The host identifies the caller by `event.source`, never by anything the
//   message says. With `init` it hands the frame's runtime a one-off key that every later message
//   must carry; add-on code cannot read it, so a page the frame is later sent to (a reload, a data:
//   or blob: page) cannot speak for the add-on — and a frame that leaves its page is closed. (An
//   HTML view's own scripts may post the raw protocol without the key; that path only ever reads.)
//   The host checks every data method against the permissions that plugin declared in
//   `manifest.json` (`plugins["<file>.js"].permissions`: library:read, todos:read, todos:write,
//   ideas:read, chat:start). Undeclared → the call rejects and the user sees one toast naming the
//   plugin and the missing permission. Private library paths are never listed, read or searched:
//   every library call made for a plugin carries `scope=plugin`, and the DAEMON enforces private
//   folders (and, for writes, the plugin's declared `write` folders via `allow=`). The `isPrivate`
//   filter below is defence in depth only — never the source of truth.
// * v1.1 (additive, apiVersion stays 1): `storage` (one JSON document per plugin), `library:write`
//   (only inside the folders the plugin declared in `write`), `todos:write` also covers
//   setDone/edit/remove, `ideas:write` → ideas.setStatus. Permissions, `write`, `apiVersion`,
//   `description` and `unsupported` arrive resolved per plugin from `GET /api/layer` (the per-plugin
//   `plugins/<name>.json`, falling back to manifest.json). Unsupported items are never loaded.
// * While a staged change is being tried ("Try it") nothing a plugin does is saved, yet it works as it
//   will: what it saves goes to a throwaway copy that is read back to it (its storage, files it
//   writes or removes, an item's status) and is dropped when the preview ends; todo changes are
//   answered as done. Each time, a toast says what would have been saved and where. Chats and
//   questions for the assistant are not started.
// * v1.2 (additive): `read` folders narrow every library read to them (the DAEMON reads them from the
//   add-on's own declaration: each call names the add-on with `plugin=`); `todos.add(text, {list,
//   group})`; `ui.confirm`, `ui.print`; `ui.notify` (notify), `ui.saveFile` (files:save) and
//   `assistant.ask` (assistant:ask), the last three checked again by the daemon (crates/thoughtsd/src/addons.rs).
// * The app's own shortcuts (Cmd/Ctrl + key, Escape) pressed inside an add-on's page reach the app.
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
//   switched off on its own; the freeze guard below turns customisations off after a session in
//   which add-on code never gave control back.
// * What an add-on shows carries its name (toasts, its pages, its commands). It can start a chat or
//   move to another page only right after the person pressed something of its own, and chats have an
//   hourly ceiling per add-on.
import { api, inTauri, on, post, put, reloadApp, ApiError } from './api';
import { app, toast, go, refreshChats, type PendingChange, type NavConfig, type TodosConfig, type PluginView } from './store.svelte';
import { confirmDialog, promptDialog, dialogs } from './dialog.svelte';
import { saveFile as offerFile } from './download';
import { matchShortcut } from './shortcuts';
import { aboveShield } from './layer/shield';
import { setPages } from './pages.svelte';
import { errCode, errDetail, failed } from './errors';
import { t, fmtDate , lang } from './i18n';

let styleEl: HTMLStyleElement | null = null;

// ---------- freeze guard ----------
// Add-on code runs in frames that share the window's thread on macOS and Linux: code there that
// never gives control back freezes the whole window, and no timer of ours can fire to notice. What
// does survive is localStorage. So whenever the host hands a frame something that runs add-on code
// (its start, an event, a command, the answer to a call — see `deliver`) it first writes down whose
// code that is, and removes the note when that frame answers again. A note still there at the next
// start names what the app was running when it stopped:
//   * add-ons of the kept layer → customisations start switched off (safe mode), with the name;
//   * a staged add-on under test or preview → the kept layer starts as usual, and that staged change
//     is not test-loaded again: the review says so instead, and Try / Keep ask first.
// Code an add-on runs on its own (a timer, a click inside its page) is not covered by the note.
const WATCH_KEY = 'thoughts.layerBoot'; // name from 0.0.8, whose bare timestamp still reads as "never finished"
const FROZE_KEY = 'thoughts.layerFroze';
/** `names`: kept add-ons that were running; `tests`: staged ones being test-loaded; `preview`: Try it was on. */
type Watch = { names: string[]; tests: string[]; preview: boolean; sig: string };
type Froze = { sig: string; names: string[] };
const store = {
  get: (k: string) => { try { return localStorage.getItem(k); } catch { return null; } },
  set: (k: string, v: string) => { try { localStorage.setItem(k, v); } catch { /* ignore */ } },
  del: (k: string) => { try { localStorage.removeItem(k); } catch { /* ignore */ } },
};
const names = (v: any): string[] => (Array.isArray(v) ? v.filter((x) => typeof x === 'string').slice(0, 20) : []);
function readJson(k: string): any { try { const v = JSON.parse(store.get(k) || 'null'); return v && typeof v === 'object' ? v : null; } catch { return null; } }

/** What the last session left behind. Read once, before this session writes anything. */
const leftBehind: Watch | null = (() => {
  if (!store.get(WATCH_KEY)) return null;
  const v = readJson(WATCH_KEY);
  return { names: names(v?.names), tests: names(v?.tests), preview: v?.preview === true, sig: typeof v?.sig === 'string' ? v.sig : '' };
})();
const froze = {
  get(): Froze | null { const v = readJson(FROZE_KEY); return v && typeof v.sig === 'string' && v.sig ? { sig: v.sig, names: names(v.names) } : null; },
  add(sigOf: string, list: string[]) {
    if (!sigOf) return;
    const cur = froze.get();
    store.set(FROZE_KEY, JSON.stringify({ sig: sigOf, names: [...new Set([...(cur?.sig === sigOf ? cur.names : []), ...list])] }));
  },
  clear() { store.del(FROZE_KEY); },
};
// A staged change was being tested or previewed: remember it, and do not blame the kept layer.
const stagedLeft = !!leftBehind && (leftBehind.preview || leftBehind.tests.length > 0);
if (leftBehind && stagedLeft) froze.add(leftBehind.sig, leftBehind.tests.length ? leftBehind.tests : leftBehind.names);
/** True while the kept layer is suspected: its add-ons were running when the app last stopped. */
let crashed = !!leftBehind && !leftBehind.preview && (leftBehind.names.length > 0 || !leftBehind.tests.length);
if (leftBehind && !crashed) store.del(WATCH_KEY);
let firstLoad = true;

// The desktop app loads the page with `?recovered=1` after its Recovery menu undid a change or went
// back to an earlier version. The customisations that failed to start are then no longer the ones
// in place, so the marker goes. The note is taken out of the address at once, so that a later
// reload does not repeat it.
try {
  const q = new URLSearchParams(location.search);
  if (q.has('recovered')) {
    crashed = false;
    store.del(WATCH_KEY);
    q.delete('recovered');
    const rest = q.toString();
    history.replaceState(null, '', location.pathname + (rest ? '?' + rest : '') + location.hash);
  }
} catch { /* ignore */ }

/** Why customisations start switched off after a session that stopped responding, in plain words. */
export function safeCrashText(): string {
  const running = crashed ? leftBehind?.names ?? [] : [];
  if (running.length === 1) return t('lua.safeCrashNamed', { name: shortName(running[0]) });
  return t(running.length ? 'lua.safeCrash' : 'shell.safe.crash');
}
/** Forget a failed start (after a rollback, or when the user chooses to try again). */
export function clearLayerBootFlag() { crashed = false; store.del(WATCH_KEY); }
/** Leave safe mode and load customisations again. */
export function retryLayer() { clearLayerBootFlag(); reloadApp('', { leaveSafe: true }); }
/** An add-on's name as people see it: its file name without the ending. */
const shortName = (file: string) => String(file).replace(/\.(js|html)$/i, '');

// ---------- per-plugin bookkeeping ----------
type Surface = { kind: 'view' | 'widget' | 'html'; id: string };
type Plugin = {
  name: string; kind: 'plugin' | 'view'; code: string; perms: Set<string>; dead: boolean;
  disposers: (() => void)[]; frames: Set<Frame>; warned: Set<string>;
  /** Library-relative folders (each ending in "/") this plugin may create and change files in. */
  write: string[];
  /** The only folders it may read in (each ending in "/"); empty = the whole library but private folders. */
  read: string[];
  /** What the plugin says it is for; used as the accessible name of its frames. */
  description: string;
  /** Review-time test load: every data call answers with nothing and nothing is registered. */
  dry?: { done: (error: string | null) => void };
};
/** A plugin or view of the layer as the user should see it listed (Settings, Review). `error`: it did
 *  not start when test-loaded; `skipped`: why it was not test-loaded at all. */
export type LayerItem = { name: string; kind: 'plugin' | 'view'; permissions: string[]; write: string[]; read: string[]; description: string; apiVersion: number; unsupported: boolean; message: string; error?: string; skipped?: string };
type Frame = {
  el: HTMLIFrameElement; plugin: Plugin; surface: Surface | null; events: Set<string>;
  /** Handed to the frame's runtime with `init`; every message it sends afterwards must carry it. */
  key: string;
  /** The frame's document said it is ready (once: a second time means the frame went elsewhere). */
  ready: boolean;
  loads: number; closed: boolean;
  /** For a page or widget: the mount it belongs to, so the frame can be started again in place. */
  slot?: { f: Frame };
  /** How tall the frame's page last said it is (for printing a page whole). */
  height?: number;
};

let plugins: Plugin[] = [];
const frames = new Set<Frame>();
let bg: HTMLElement | null = null;

const PERMS = ['library:read', 'library:write', 'todos:read', 'todos:write', 'ideas:read', 'ideas:write', 'chat:start', 'storage', 'files:save', 'notify', 'assistant:ask'];
const VIEW_PERMS = ['library:read', 'todos:read', 'ideas:read'];
const NEEDS: Record<string, string> = {
  'library.list': 'library:read', 'library.read': 'library:read', 'library.search': 'library:read',
  'library.write': 'library:write', 'library.remove': 'library:write',
  'todos.list': 'todos:read', 'todos.add': 'todos:write', 'todos.setDone': 'todos:write', 'todos.edit': 'todos:write', 'todos.remove': 'todos:write',
  'ideas.list': 'ideas:read', 'ideas.setStatus': 'ideas:write', 'chat.start': 'chat:start',
  // A Home chip puts a request in front of the assistant, so it needs the same permission as starting a chat.
  'ui.addHomeChip': 'chat:start',
  'storage.get': 'storage', 'storage.set': 'storage',
  'ui.saveFile': 'files:save', 'ui.notify': 'notify', 'assistant.ask': 'assistant:ask',
};
/** Calls that change something. While a staged change is only being tried none of them reaches the
 *  library or the add-on's storage (see `pretend`), and chats or questions are not started. */
const WRITES = new Set(['library.write', 'library.remove', 'todos.add', 'todos.setDone', 'todos.edit', 'todos.remove', 'ideas.setStatus', 'storage.set', 'chat.start', 'assistant.ask']);
const WRITE_EXT = /\.(md|json|csv|txt)$/i;
const STATUSES = ['seed', 'active', 'parked', 'done', 'dropped'];
const STORAGE_MAX = 256 * 1024;
/** The highest `apiVersion` this app can run. */
export const API_VERSION = 1;
const READ_ONLY = new Set(['library.list', 'library.read', 'library.search', 'todos.list', 'ideas.list', 'on', 'off']);
/** Events a plugin may listen to, and the only fields it is given. */
const EVENTS: Record<string, string[]> = {
  'todos.changed': [], 'library.changed': [], 'layer.changed': [], 'settings.changed': [], 'wrap.done': [], 'chat.done': ['chatId'],
};
const VIEWS = ['home', 'chats', 'chat', 'ideas', 'todos', 'docs', 'projects', 'settings', 'help', 'plugin'];

// ---------- limits on what an add-on may do unasked ----------
// Kept per add-on NAME for the whole session, so reloading the layer does not reset them.
/** How many chats one add-on may start in an hour. */
const CHATS_PER_HOUR = 20;
const rateLog = new Map<string, number[]>();
/** When the person last ran one of this add-on's commands or item actions from the app itself. */
const pressed = new Map<string, number>();
const told = new Set<string>();
/** The times noted under `key` that are younger than `within` ms (the list is kept: push to it to note one more). */
function recent(key: string, within: number): number[] {
  const now = Date.now();
  const list = (rateLog.get(key) ?? []).filter((x) => now - x < within);
  rateLog.set(key, list);
  return list;
}
/** Refuse something an add-on tried without being asked to; the person is told once per session. */
function refused(p: Plugin, why: 'chatUnasked' | 'chatBudget'): Error {
  const k = p.name + ':' + why;
  if (!told.has(k)) {
    told.add(k);
    toast(t('lua.' + why, { name: shortName(p.name), n: CHATS_PER_HOUR }), { kind: 'error', key: 'limit:' + k, duration: 0 });
  }
  return Object.assign(new Error(why === 'chatBudget' ? `chat.start: this add-on already started ${CHATS_PER_HOUR} chats in the last hour` : 'chat.start: only right after the person pressed something in this add-on'), { code: 'limit' });
}

function destroyFrame(f: Frame) {
  f.closed = true;
  frames.delete(f);
  f.plugin.frames.delete(f);
  f.el.remove();
  landed(f);
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
    // The next step after a crash: switch that add-on off, or undo what brought it.
    action: { label: t('ldc.crash.manage'), run: () => { go({ kind: 'customise', id: 'addons' }); } },
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

// ---------- handing control to add-on code (the freeze guard's note) ----------
/** `want`: the ping sent right after the hand-over (its pong means the add-on gave control back);
 *  `pinged` / `ponged`: the last ping sent and the highest one answered; `silent`: how long the app
 *  itself has been awake since the frame last answered a ping. */
type Flight = { since: number; load: boolean; want: number; pinged: number; ponged: number; silent: number };
/** Frames that were handed something and have not answered since. */
const flights = new Map<Frame, Flight>();
/** A frame in its own process (Chromium may give it one) can hang alone: after this long without
 *  an answer while the app itself is running, it is switched off. */
const STALL_MS = 6000;
let pingSeq = 0;
let written = '';
let watchdog: ReturnType<typeof setInterval> | undefined;
let lastTick = 0;
/** Add-ons start one at a time, so the note names exactly one of them while it starts. */
const loadQueue: { f: Frame; msg: any }[] = [];
let loading: Frame | null = null;

function noteWatch() {
  if (!flights.size) {
    // Gone as soon as nothing is out: a note left by an app that was merely quit mid-call would
    // look like a freeze at the next start.
    if (written) { written = ''; store.del(WATCH_KEY); }
    return;
  }
  const kept = new Set<string>(), tests = new Set<string>();
  for (const f of flights.keys()) (f.plugin.dry ? tests : kept).add(f.plugin.name);
  const staged = app.preview || tests.size > 0;
  const note: Watch = { names: [...kept].sort(), tests: [...tests].sort(), preview: app.preview, sig: staged ? sig(app.pending) : '' };
  const body = JSON.stringify(note);
  if (body === written) return;
  written = body;
  store.set(WATCH_KEY, body);
}

/** Hand a frame something that runs add-on code. `load`: the add-on's start, which is over when the
 *  frame reports `loaded` / `failed` — or keeps answering although it has not finished. */
function deliver(f: Frame, msg: any, load = false) {
  if (f.closed) return;
  const n = ++pingSeq;
  const fl = flights.get(f);
  if (fl) { fl.want = n; fl.pinged = n; fl.load ||= load; } else flights.set(f, { since: Date.now(), load, want: n, pinged: n, ponged: 0, silent: 0 });
  noteWatch();
  postTo(f, msg);
  postTo(f, { type: 'ping', n });
  if (!watchdog) { lastTick = Date.now(); watchdog = setInterval(watchTick, 500); }
}

/** The frame answered (or is gone): its code is no longer running on our account. */
function landed(f: Frame) {
  if (flights.delete(f)) noteWatch();
  if (loading === f) { loading = null; nextLoad(); }
}

function queueLoad(f: Frame, msg: any) { loadQueue.push({ f, msg }); nextLoad(); }
function nextLoad() {
  while (!loading) {
    const job = loadQueue.shift();
    if (!job) return;
    if (job.f.closed || job.f.plugin.dead) continue;
    loading = job.f;
    deliver(job.f, job.msg, true);
  }
}

function watchTick() {
  const now = Date.now();
  // A long gap between ticks means the app itself was asleep or in the background: no verdict from that.
  const awake = Math.min(now - lastTick, 1000) * (now - lastTick > 5000 ? 0 : 1);
  lastTick = now;
  for (const [f, fl] of [...flights]) {
    if (f.closed || f.plugin.dead) { landed(f); continue; }
    // Answered the ping of the last tick: however slowly the app's timers run (a hidden window is
    // throttled), the frame is not stuck.
    const answering = fl.ponged >= fl.pinged;
    fl.silent = answering ? 0 : fl.silent + awake;
    // Still starting, but answering: not stuck, so the next add-on may start. (A test load keeps its
    // note until it is over: it runs the module twice, and the second run must be covered too.)
    if (fl.load && answering && now - fl.since > 1500 && !f.plugin.dry) { landed(f); continue; }
    if (fl.silent >= STALL_MS) { stalled(f); continue; }
    if (answering) { fl.pinged = ++pingSeq; postTo(f, { type: 'ping', n: fl.pinged }); }
  }
  if (!flights.size) { clearInterval(watchdog); watchdog = undefined; }
}

/** The app is running but this frame has stopped answering: its code is stuck. (The app did not
 *  freeze, so this is an ordinary failure, not a reason to stop testing the change.) */
function stalled(f: Frame) {
  const p = f.plugin;
  destroyFrame(f);
  if (p.dry) p.dry.done(t('lua.hang'));
  else fail(p, new Error(t('lua.hang')));
}

/** The frame's page was replaced: by the add-on (it went to another page, or reloaded itself), or
 *  because the app moved the frame on screen, which reloads it. Whatever is in the frame now gets
 *  nothing. A page or widget is started again in a new frame from our own document; anything else,
 *  or a frame that keeps leaving, switches the add-on off. */
function left(f: Frame) {
  if (f.closed) return;
  const p = f.plugin, into = f.el.parentElement, slot = f.slot;
  destroyFrame(f);
  if (p.dry) { p.dry.done(t('lua.left')); return; }
  if (p.dead) return;
  const again = recent('left:' + p.name, 10_000);
  again.push(Date.now());
  if (f.surface && slot && into && again.length <= 3) {
    try { slot.f = createFrame(p, f.surface, into); slot.f.slot = slot; return; } catch { /* fall through */ }
  }
  fail(p, new Error(t('lua.left')));
}

let lookTimer: ReturnType<typeof setTimeout> | undefined;
function relook() {
  clearTimeout(lookTimer);
  lookTimer = setTimeout(() => { if (!frames.size) return; const m = lookMsg('look'); for (const f of frames) postTo(f, m); }, 60);
}

// ---------- frames ----------
// Each frame loads the static page public/plugin-frame.html (which runs public/plugin-frame.js) from
// the app's own origin. It carries its own strict content-security policy.
const frameSrc = () => new URL('plugin-frame.html', document.baseURI).href.replace(/[?#].*$/, '');

// Windows is different. There the desktop shell puts its command bridge into EVERY frame of the
// window, sandboxed or not, and accepts a command from any frame whose address is the app's own. A
// frame loaded from plugin-frame.html has such an address, so an add-on could send the shell
// commands. On Windows the same document is therefore handed to the frame as `srcdoc`: its address
// is then `about:srcdoc`, which the shell treats as a foreign page and refuses. (A srcdoc page also
// has to satisfy the app's own policy: the runtime's hash is in tauri.conf.json for that.) Elsewhere
// the bridge exists only in the app window itself, and the frame's runtime checks that it does.
const viaSrcdoc = () => inTauri() && ((window as any).__THOUGHTS_PLATFORM__ === 'windows' || (!(window as any).__THOUGHTS_PLATFORM__ && /Windows/i.test(navigator.userAgent)));
let frameHtml: string | null = null;
/** Fetch the frame document once where it is needed as text (see `viaSrcdoc`). */
async function frameDoc(): Promise<void> {
  if (frameHtml !== null || !viaSrcdoc()) return;
  const r = await fetch(frameSrc());
  if (!r.ok) throw new Error('plugin-frame.html: ' + r.status);
  frameHtml = await r.text();
}

function frameKey(): string {
  const b = new Uint8Array(18);
  crypto.getRandomValues(b);
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
}

function createFrame(p: Plugin, surface: Surface | null, into: HTMLElement): Frame {
  if (viaSrcdoc() && frameHtml === null) throw new Error('The add-on frame is not ready yet');
  const el = document.createElement('iframe');
  // allow-forms: without it Chromium (Windows) never raises `submit`. Nothing can be sent anywhere:
  // the frame's policy says `form-action 'none'` and its runtime cancels the browser's own submit.
  el.setAttribute('sandbox', 'allow-scripts allow-forms');
  el.setAttribute('referrerpolicy', 'no-referrer');
  el.setAttribute('title', p.description || shortName(p.name));
  el.style.cssText = surface
    ? `border:0;display:block;width:100%;height:${surface.kind === 'widget' ? '80px' : '100%'};background:transparent;color-scheme:normal`
    : 'border:0;width:0;height:0;position:absolute;visibility:hidden';
  if (!surface) { el.tabIndex = -1; el.setAttribute('aria-hidden', 'true'); }
  if (frameHtml !== null) el.srcdoc = frameHtml; else el.src = frameSrc();
  const f: Frame = { el, plugin: p, surface, events: new Set(), key: frameKey(), ready: false, loads: 0, closed: false };
  // One document per frame: after our runtime said it is ready, its own page finishing is the one
  // load to expect; another means the frame was sent somewhere else. (A load before `ready`, such as
  // an engine reporting the frame's first empty page, does not count.)
  el.addEventListener('load', () => { if (f.ready && ++f.loads > 1) left(f); });
  frames.add(f);
  p.frames.add(f);
  into.appendChild(el);
  return f;
}

/** Mount one of a plugin's surfaces into a slot. Returns the cleanup that removes it. */
function mountSurface(p: Plugin, surface: Surface, el: HTMLElement): () => void {
  if (p.dead) throw new Error(t('layer.pluginOff', { name: p.name }));
  const slot = { f: createFrame(p, surface, el) };
  slot.f.slot = slot;
  return () => destroyFrame(slot.f);
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
/** Plain words for a permission (the v1.1 ones, and the widened todos:write, live in the layer3 dictionary). */
export function permText(perm: string): string {
  for (const k of ['lub.perm.' + perm, 'layer3.perm.' + perm]) { const s = t(k); if (s !== k) return s; }
  return t('layer.perm.' + perm);
}

function denied(p: Plugin, perm: string): Error {
  if (!p.warned.has(perm)) {
    p.warned.add(perm);
    toast(t('layer.permMissing', { name: p.name, perm: permText(perm) }), { kind: 'error', key: `perm:${p.name}:${perm}` });
  }
  return Object.assign(new Error(`Permission "${perm}" is not declared for ${p.name} in manifest.json`), { code: 'permission' });
}

const str = (v: any, max: number) => String(v ?? '').slice(0, max);
// Defence in depth only: the daemon enforces private folders for every `scope=plugin` call.
const isPrivate = (path: string) => { const p = String(path).toLowerCase(); return ((app.settings?.privatePaths ?? []) as string[]).some((x) => { const b = String(x).replace(/\/+$/, '').toLowerCase(); return !!b && (p === b || p.startsWith(b + '/')); }); };
function cleanPath(v: any): string {
  const p = String(v ?? '').replace(/\\/g, '/').replace(/^\.?\/+/, '');
  if (!p || p.split('/').some((s) => s === '..' || s === '.' || s === '') || /^([A-Za-z]:|~)/.test(p)) throw new Error('Invalid path');
  if (isPrivate(p)) throw new Error('That file is private');
  return p;
}
/** A path a plugin wants to create, change or remove: inside one of its declared folders, text files only. */
function writePath(p: Plugin, v: any): string {
  const path = cleanPath(v);
  if (!WRITE_EXT.test(path)) throw new Error('Add-ons can only write .md, .json, .csv and .txt files');
  if (!inFolders(p.write, path)) throw new Error(p.write.length ? `"${path}" is outside the folders this add-on declared (${p.write.join(', ')})` : 'This add-on declared no "write" folders');
  return path;
}
/** What every library call made for an add-on carries: the daemon keeps private folders closed, and
 *  reads the add-on's own declaration (its `read` folders, the permission for a status change) from
 *  the layer it runs from — the staged one while a change is tried. */
const scopeQuery = (p: Plugin) => `scope=plugin&plugin=${encodeURIComponent(p.name)}&kind=${p.kind}${app.preview ? '&preview=1' : ''}`;
// The folders go as `a,b`; when a folder name itself has a comma they go as a JSON list instead,
// which the daemon reads when the value starts with `[`.
const allowQuery = (p: Plugin) => scopeQuery(p) + '&allow=' + encodeURIComponent(p.write.some((w) => w.includes(',')) ? JSON.stringify(p.write) : p.write.join(','));
/** Is library path `path` inside one of `folders` ("a/" each)? */
const inFolders = (folders: string[], path: string) => { const low = path.toLowerCase(); return folders.some((w) => { const f = w.toLowerCase().replace(/\/+$/, ''); return low === f || low.startsWith(f + '/'); }); };
/** The `{file, line, text}` reference of a todo, from an item as todos.list() or an item action gave it. */
function todoRef(v: any): { file: string; line: number; text: string } {
  const it = v && typeof v === 'object' && v.item && typeof v.item === 'object' ? v.item : v;
  const line = Number(it?.line);
  if (!it || typeof it.file !== 'string' || !Number.isFinite(line) || typeof it.text !== 'string') throw new Error('Expected a todo item with "file", "line" and "text" (as returned by todos.list() with its list\'s "file")');
  return { file: cleanPath(it.file), line, text: it.text };
}
// The daemon keys storage by the plugin's file name (e.g. `hours.js`).
const storageName = (p: Plugin) => encodeURIComponent(p.name);
// ---------- Try it: what add-ons save is kept aside (UX-28) ----------
// While a staged change is tried, an add-on works as it will once kept — a form saves, a card moves —
// but nothing reaches the library or its storage: what it saves goes here, is read back to it, and is
// dropped when the preview ends (every reload of the customisations starts afresh).
type Pretend = { storage: Map<string, any>; files: Map<string, string | null>; status: Map<string, string>; told: Set<string> };
const freshPretend = (): Pretend => ({ storage: new Map(), files: new Map(), status: new Map(), told: new Set() });
let pretend = freshPretend();
/** Say what an add-on would have saved: one toast per add-on, replaced by the next one. */
function wouldDo(p: Plugin, what: string, vars: Record<string, string | number> = {}) {
  toast(t('lub.would.' + what, { name: shortName(p.name), ...vars }), { key: 'preview:' + p.name, detail: t('lub.would.detail') });
}
/** A chat or a question the add-on would start: not while only trying a change. */
function previewOnly(p: Plugin, what: 'chat' | 'ask'): Error {
  toast(t('lub.would.' + what, { name: shortName(p.name) }), { key: 'preview:' + p.name });
  return Object.assign(new Error(what === 'chat' ? 'Preview: no chat is started while a change is tried' : 'Preview: the assistant is not asked while a change is tried'), { code: 'preview' });
}
/** Every write the service makes for an add-on goes through here: never while only trying a change. */
function commit<T>(run: () => Promise<T>): Promise<T> {
  return app.preview ? Promise.reject(Object.assign(new Error('Preview: nothing was saved'), { code: 'preview' })) : run();
}
const ideaKey = (path: string) => (/\.md$/i.test(path) ? path : path.replace(/\/+$/, '') + '/README.md');
const statusWord = (s: string) => t('idea.status.' + s);
/** The library list as the add-on would see it with what it saved while trying. */
function pretendList(list: any[]): any[] {
  if (!pretend.files.size) return list;
  const out = list.filter((x) => !pretend.files.has(x.path));
  for (const [path, text] of pretend.files) if (text !== null) out.push({ path, size: new Blob([text]).size, modified: new Date().toISOString() });
  return out.sort((a, b) => String(a.path).localeCompare(String(b.path)));
}
/** What a test load answers: empty data in the shape the real call gives (so a correct add-on does
 *  not trip over it), and nothing is ever registered, written or shown. */
function dryAnswer(method: string): any {
  if (method === 'library.read') return '';
  if (/\.(list|search)$/.test(method)) return [];
  if (method === 'storage.get') return {};
  if (method === 'chat.start') return { chatId: '' };
  if (method === 'assistant.ask') return '';
  if (method === 'ui.confirm') return false;
  if (method === 'ui.saveFile') return { saved: false };
  if (/^(library\.(write|remove)|todos\.(add|setDone|edit|remove)|ideas\.setStatus|storage\.set)$/.test(method)) return { ok: true };
  return null;
}

const plain = (v: any) => { try { return JSON.parse(JSON.stringify(v ?? null)); } catch { return null; } };

// Two add-ons may both call something `board`, and an add-on may call a page `todos`. What the
// add-on chose is kept as `short` (the name `nav.json`, `ui.go` and `ui.remove` use); the `id` of a
// page or a command, which the sidebar, the palette and the open view are keyed by, also carries the
// add-on's file name, so it is unique in the app. Widgets, chips and item actions are keyed by add-on
// and id where they are drawn, and `home.json` places widgets by their short id.
const ownId = (p: Plugin, id: string) => `${p.name}:${id}`;
const shortOf = (x: any): string => x.short ?? x.id;
function register<T extends { id: string; plugin?: string; short?: string }>(p: Plugin, key: 'pluginViews' | 'commands' | 'homeWidgets' | 'homeChips' | 'itemActions', item: T) {
  const list = app[key] as any[];
  (app as any)[key] = [...list.filter((x) => !(x.plugin === p.name && shortOf(x) === shortOf(item))), item];
  p.disposers.push(() => unregister(p, key, shortOf(item)));
}
function unregister(p: Plugin, key: string, short: string) {
  (app as any)[key] = ((app as any)[key] as any[]).filter((x) => !(x.plugin === p.name && shortOf(x) === short));
}

/** The sidebar name of an add-on's page: `nav.json` may rename it by its full id or by its short one. */
export function viewLabel(v: PluginView): string {
  const pick = (id?: string) => { const l = id ? app.nav?.labels?.[id] : undefined; return l && (lang() === 'ar' ? l.ar || l.en : l.en || l.ar); };
  return String(pick(v.id) || (v.short && ![...VIEWS, 'me', 'inbox'].includes(v.short) && pick(v.short)) || v.title || v.short || v.id).slice(0, 60);
}
/** Whose page or command this is, in words: "Add-on · hours". */
export const addonTag = (file?: string) => (file ? t('lua.tag', { name: shortName(file) }) : t('shell.pal.plugin'));
/** The open add-on page for a view id — also an id from before pages carried their add-on's name. */
export function findView(id: string): PluginView | undefined {
  return app.pluginViews.find((x) => x.id === id) ?? app.pluginViews.find((x) => x.short === id || x.id === 'view:' + id);
}

/** Add-ons with one of their questions (`ui.confirm`) open right now. */
const confirming = new Set<string>();

/** Print an add-on's page whole (`ui.print`): for the print its frame is as tall as its page, and the
 *  app's print rules leave out everything around it. */
function printPage(f: Frame) {
  const el = f.el, html = document.documentElement;
  const was = { height: el.style.height, title: document.title };
  const done = () => { el.style.height = was.height; html.classList.remove('print-addon'); document.title = was.title; window.removeEventListener('afterprint', done); };
  window.addEventListener('afterprint', done);
  if (f.height) el.style.height = Math.max(f.height, el.clientHeight) + 'px';
  html.classList.add('print-addon');
  // The window's title is the name a saved PDF is offered under.
  const view = app.pluginViews.find((v) => v.plugin === f.plugin.name && shortOf(v) === f.surface?.id);
  if (view?.title) document.title = view.title;
  try { window.print(); } catch { done(); toast(t('docs.printFailed'), { kind: 'error' }); }
}

// ---------- the app's shortcuts from inside an add-on's page (UX-31) ----------
/** What a key pressed inside an add-on's page may do in the app: move around (pages, back and
 *  forward, search), open the command palette, zoom, show or hide the sidebar. Nothing that starts,
 *  saves, syncs, wraps, undoes or opens settings, whatever the frame says was pressed. */
const FORWARDED = /^(palette|nav\.[1-9]|back|forward|search\.focus|find|sidebar\.toggle|zoom\.(in|out|reset))$/;
/** Key events the app itself made from a key pressed inside an add-on's page. Only this module adds
 *  to it, and an add-on cannot reach an event of the app's window. */
const fromAddOn = new WeakSet<Event>();
/** Did this key event come from inside an add-on's page? The app's key handler then runs nothing
 *  but `FORWARDED` (see App.svelte `onKey`). */
export const keyFromAddOn = (e: Event): boolean => fromAddOn.has(e);
export const keyMayComeFromAddOn = (id: string): boolean => FORWARDED.test(id);

/** A key the person pressed inside an add-on's page or widget (its runtime passes on only keys a
 *  person pressed, with Cmd or Ctrl, and Escape). It counts only while that very frame has the
 *  keyboard, a few a second at most. Escape takes the keyboard out of the add-on, back to the app
 *  (never closing anything of the app's); any other key runs the app's shortcut only when it is
 *  one of `FORWARDED`. */
function pressKey(f: Frame, m: any) {
  if (document.activeElement !== f.el) return;
  const presses = recent('keys:' + f.key, 1000);
  if (presses.length >= 5) return;
  presses.push(Date.now());
  const key = str(m.key, 20), code = str(m.code, 30);
  const meta = m.meta === true, ctrl = m.ctrl === true, shift = m.shift === true, alt = m.alt === true;
  if (key === 'Escape') {
    if (meta || ctrl || alt || dialogs.current || app.showPalette || app.showReview || app.showRollback) return;
    const into = f.el.parentElement;
    f.el.blur();
    if (into) { into.tabIndex = -1; into.focus({ preventScroll: true }); }
    return;
  }
  const ev = new KeyboardEvent('keydown', { key, code, metaKey: meta, ctrlKey: ctrl, shiftKey: shift, altKey: alt, bubbles: true, cancelable: true });
  const id = matchShortcut(ev);
  if (!id || !FORWARDED.test(id)) return;
  fromAddOn.add(ev);
  window.dispatchEvent(ev);
}

/** `asked`: the person pressed or typed something in this very frame a moment ago (its runtime says
 *  so, see plugin-frame.js), or just ran one of the add-on's commands or item actions. */
async function handle(f: Frame, method: string, args: any[], asked = false): Promise<any> {
  const p = f.plugin;
  if (p.dead) throw new Error('This add-on was switched off');
  if (p.kind === 'view' && !READ_ONLY.has(method)) throw new Error(`"${method}" is not available to HTML views`);
  if (p.dry) return dryAnswer(method);
  const perm = NEEDS[method];
  if (perm && !p.perms.has(perm)) throw denied(p, perm);
  const a = Array.isArray(args) ? args : [];
  const trying = app.preview;
  asked ||= Date.now() - (pressed.get(p.name) ?? 0) < 10_000;
  /** Something that is only for right after the person pressed something of this add-on. */
  const onlyAsked = (what: string) => { if (!asked) throw Object.assign(new Error(`${what}: only right after the person pressed something in this add-on`), { code: 'limit' }); };
  // Running a command or an item action is the person asking this add-on for something.
  const invoke = (cb: any) => (...x: any[]) => { if (!p.dead) { pressed.set(p.name, Date.now()); deliver(f, { type: 'invoke', cb, args: x.map(plain) }); } };
  const scope = scopeQuery(p);
  switch (method) {
    case 'library.list': {
      const list = ((await api(`/library/tree?${scope}&exclude=private`)) as any[]).filter((x) => !isPrivate(x.path)).map(plain);
      return trying ? pretendList(list) : list;
    }
    case 'library.read': {
      const path = cleanPath(a[0]);
      if (trying && pretend.files.has(path)) {
        const text = pretend.files.get(path);
        if (text == null) throw new Error(`library.read: "${path}" does not exist`);
        return text;
      }
      const d = await api(`/library/file?${scope}&path=${encodeURIComponent(path)}`);
      return String(d.content ?? '');
    }
    case 'library.search': { const q = str(a[0], 200).trim(); return q ? ((await api(`/library/search?${scope}&q=` + encodeURIComponent(q))) as any[]).filter((x) => !isPrivate(x.path)).map(plain) : []; }
    case 'library.write': {
      const path = writePath(p, a[0]);
      if (typeof a[1] !== 'string') throw new Error('library.write(path, text): text must be a string');
      if (a[1].length > 2 * 1024 * 1024) throw new Error('library.write: the text is too large (2 MB at most)');
      if (trying) { pretend.files.set(path, a[1]); wouldDo(p, 'save', { path }); return { ok: true, path }; }
      await commit(() => put('/library/file?' + allowQuery(p), { path, content: a[1] }));
      return { ok: true, path };
    }
    case 'library.remove': {
      const path = writePath(p, a[0]);
      if (trying) { pretend.files.set(path, null); wouldDo(p, 'remove', { path }); return { ok: true, path }; }
      await commit(() => post('/library/delete?' + allowQuery(p), { path }));
      return { ok: true, path };
    }
    case 'todos.setDone': {
      const ref = todoRef(a[0]);
      const it = a[0]?.item && typeof a[0].item === 'object' ? a[0].item : a[0];
      const want = a.length < 2 ? true : !!a[1];
      // The daemon toggles; when the plugin told us the current state, do nothing if it already matches.
      if (typeof it?.done === 'boolean' && it.done === want) return { ok: true };
      if (trying) { wouldDo(p, want ? 'tick' : 'untick', { text: str(it?.title ?? ref.text, 80) }); return { ok: true }; }
      await commit(() => post(`/todos/toggle?${scope}`, ref));
      return { ok: true };
    }
    case 'todos.edit': {
      const text = str(a[1], 500).replace(/\s*\n\s*/g, ' ').trim();
      if (!text) throw new Error('todos.edit(item, text): text is empty');
      const ref = todoRef(a[0]);
      if (trying) { wouldDo(p, 'edit', { text: str(text, 80) }); return { ok: true }; }
      await commit(() => post(`/todos/edit?${scope}`, { ...ref, new_text: text }));
      return { ok: true };
    }
    case 'todos.remove': {
      const ref = todoRef(a[0]);
      if (trying) { wouldDo(p, 'delete', { text: str(a[0]?.title ?? a[0]?.item?.title ?? ref.text, 80) }); return { ok: true }; }
      await commit(() => post(`/todos/delete?${scope}`, ref));
      return { ok: true };
    }
    case 'ideas.setStatus': {
      const path = cleanPath(typeof a[0] === 'string' ? a[0] : a[0]?.item?.path ?? a[0]?.path);
      const status = str(a[1], 20).toLowerCase();
      if (!STATUSES.includes(status)) throw new Error(`ideas.setStatus: status must be one of ${STATUSES.join(', ')}`);
      if (trying) {
        // The same folders the service holds it to once kept.
        if (!inFolders(p.write, ideaKey(path))) throw new Error(`ideas.setStatus: "${path}" is outside the folders this add-on declared`);
        pretend.status.set(ideaKey(path), status);
        wouldDo(p, 'status', { path: ideaKey(path).replace(/\/README\.md$/i, ''), status: statusWord(status) });
        return { ok: true };
      }
      // The service checks again that this add-on declares ideas:write, that `path` is an item's page
      // as the Ideas list shows it, and that it lies inside the folders the add-on may write in.
      await commit(() => post(`/library/ideas/status?${allowQuery(p)}`, { path, status }));
      return { ok: true };
    }
    case 'storage.get': {
      // While trying, what it saved since the preview began (a copy: the add-on cannot change ours).
      if (trying && pretend.storage.has(p.name)) return plain(pretend.storage.get(p.name));
      // The daemon answers with the add-on's own object, exactly as it was saved (`{}` before the first save).
      try { const r = plain(await api('/layer/storage/' + storageName(p))); return r && typeof r === 'object' && !Array.isArray(r) ? r : {}; }
      catch (e) { if (e instanceof ApiError && e.status === 404) return {}; throw e; }
    }
    case 'storage.set': {
      const doc = plain(a[0]);
      if (JSON.stringify(doc).length > STORAGE_MAX) throw new Error('storage.set: the document is too large (256 KB at most)');
      if (trying) {
        pretend.storage.set(p.name, doc);
        // Its own data: said once per preview, not on every save.
        if (!pretend.told.has('storage:' + p.name)) { pretend.told.add('storage:' + p.name); wouldDo(p, 'storage'); }
        return { ok: true };
      }
      await commit(() => put('/layer/storage/' + storageName(p), doc));
      return { ok: true };
    }
    // Each item also carries its list's `file`, so it can be passed straight to setDone/edit/remove.
    case 'todos.list': return ((await api(`/todos?${scope}`)) as any[]).filter((l) => !isPrivate(l.file)).map((l) => plain({ ...l, items: (Array.isArray(l.items) ? l.items : []).map((it: any) => ({ ...it, file: l.file })) }));
    case 'todos.add': {
      const text = str(a[0], 500).replace(/\s*\n\s*/g, ' ').trim();
      if (!text) throw new Error('todos.add(text): text is empty');
      // Where (FEAT-47): a list the add-on can see that already exists, and one of its groups. No list
      // named: the service adds it to the library's general list, wherever that is kept.
      const o = a[1] && typeof a[1] === 'object' ? a[1] : {};
      const file = o.list ? cleanPath(o.list) : '';
      const group = str(o.group, 80).trim();
      if (file && !/(^|\/)(TODO|todo)\.md$/.test(file)) throw new Error(`todos.add: "${file}" is not a todo list (give the "file" of a list from todos.list())`);
      if (trying) {
        // The same checks the service makes, on the lists the add-on can see.
        // (An add-on with `read` folders adds only to lists inside them, the general list included.)
        if (file || group || p.read.length) {
          const lists = (await api(`/todos?${scope}`)) as any[];
          const target = file ? lists.find((l) => l.file === file) : lists.find((l) => l.file === 'tasks/TODO.md') ?? lists.find((l) => l.file === 'TODO.md');
          if (!target && (file || p.read.length)) throw new Error(t('lub.noList', { file: file || 'TODO.md' }));
          if (group && target && !(target.groups ?? []).includes(group)) throw new Error(t('lub.noGroup', { group }));
        }
        wouldDo(p, file ? 'addTo' : 'add', { text: str(text, 80), list: file });
        return { ok: true };
      }
      await commit(() => post(`/todos/add?${scope}`, { file, text, ...(group ? { group } : {}) }));
      return { ok: true };
    }
    case 'ideas.list': {
      const list = ((await api(`/library/ideas?${scope}`)) as any[]).filter((x) => !isPrivate(x.path)).map(plain);
      return trying && pretend.status.size ? list.map((x) => (pretend.status.has(x.path) ? { ...x, status: pretend.status.get(x.path) } : x)) : list;
    }
    case 'chat.start': {
      const prompt = str(a[0], 8000).trim();
      if (!prompt) throw new Error('chat.start(prompt): prompt is empty');
      // A chat costs the person money and attention. An add-on may start one only when they have just
      // pressed something of its own (never from a timer, or from the end of the chat before), a few
      // seconds apart, and no more than CHATS_PER_HOUR in an hour.
      if (!asked) throw refused(p, 'chatUnasked');
      if (trying) throw previewOnly(p, 'chat');
      const started = recent('chat:' + p.name, 3_600_000);
      if (started.length && Date.now() - started[started.length - 1] < 5000) throw new Error('chat.start: wait a few seconds between chats');
      if (started.length >= CHATS_PER_HOUR) throw refused(p, 'chatBudget');
      started.push(Date.now());
      // Never `byUser`: an add-on's prompt must not inherit a "bypass permissions" default.
      const c = await post('/chats', {});
      await post(`/chats/${c.id}/send`, { text: prompt });
      refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
      return { chatId: c.id };
    }
    case 'assistant.ask': {
      // FEAT-48. The answer comes back to the add-on; the assistant sees only the question (it has no
      // tools), so asking never shows an add-on more of the library than it may read. Like a chat it
      // costs money: only right after a press of the add-on's own, and the service holds each add-on
      // to one question at a time and to an hourly and a daily budget.
      const question = str(a[0], 40_000).trim();
      if (!question) throw new Error('assistant.ask(question): question is empty');
      onlyAsked('assistant.ask');
      if (trying) throw previewOnly(p, 'ask');
      const r = await post('/layer/addon/ask', { plugin: p.name, question });
      return String(r?.text ?? '');
    }
    case 'ui.toast': {
      // Always under the add-on's name, and no more than three in any ten seconds.
      const shown = recent('toast:' + p.name, 10_000);
      if (shown.length >= 3) return null;
      shown.push(Date.now());
      const k = a[1]?.kind;
      toast(t('lua.says', { name: shortName(p.name), text: str(a[0], 500) }), { kind: k === 'error' || k === 'success' ? k : 'info' });
      return null;
    }
    case 'ui.confirm': {
      // FEAT-58. Drawn by the app with the add-on's name on it; a yes or no, nothing typed. Only
      // right after a press, one at a time, and never in place of a question the app itself is asking.
      const text = str(a[0], 600).trim();
      if (!text) throw new Error('ui.confirm(text): text is empty');
      onlyAsked('ui.confirm');
      if (confirming.has(p.name) || dialogs.current) throw Object.assign(new Error('ui.confirm: another question is open'), { code: 'limit' });
      const o = a[1] && typeof a[1] === 'object' ? a[1] : {};
      confirming.add(p.name);
      try {
        return await confirmDialog({ title: t('lub.confirmTitle', { name: shortName(p.name) }), body: text, confirmLabel: str(o.confirm, 30).replace(/\s+/g, ' ').trim() || t('lub.confirmYes'), danger: o.danger === true });
      } finally { confirming.delete(p.name); }
    }
    case 'ui.notify': {
      // FEAT-58. The service checks the permission and how often, and sends the app's own notice: a
      // toast, and a system notification while the window is in the background (if they are on).
      const text = str(a[0], 200).trim();
      if (!text) throw new Error('ui.notify(text): text is empty');
      await post('/layer/addon/notify', { plugin: p.name, preview: trying, text });
      return null;
    }
    case 'ui.saveFile': {
      // FEAT-43. The person chooses where in the app's own save dialog; the service checks the
      // permission and the kind of file and makes it safe to open (no formulas, pictures or HTML
      // that could reach the internet).
      onlyAsked('ui.saveFile');
      if (typeof a[1] !== 'string') throw new Error('ui.saveFile(name, text): text must be a string');
      if (a[1].length > 5 * 1024 * 1024) throw new Error('ui.saveFile: the file is too large (5 MB at most)');
      const r = await post('/layer/addon/file', { plugin: p.name, preview: trying, name: str(a[0], 200), content: a[1] });
      const ext = String(r.name).split('.').pop()?.toLowerCase();
      const mime = ext === 'csv' ? 'text/csv' : ext === 'json' ? 'application/json' : ext === 'md' ? 'text/markdown' : 'text/plain';
      const how = await offerFile(String(r.name), String(r.content), mime);
      if (how === 'unsupported') toast(t('lub.saveUnsupported', { name: shortName(p.name) }), { kind: 'error', key: 'save:' + p.name });
      return { saved: how === 'saved' };
    }
    case 'ui.print': {
      // FEAT-43. Only the add-on's own page, whole, as the person sees it (the app's print dialog).
      onlyAsked('ui.print');
      if (f.surface?.kind !== 'view') throw new Error('ui.print: only a page of the add-on can be printed');
      printPage(f);
      return null;
    }
    case 'ui.go': {
      // Moving the person to another page is for when they asked: right after a press, once a second at most.
      if (!asked) throw Object.assign(new Error('ui.go: only right after the person pressed something in this add-on'), { code: 'limit' });
      const moved = recent('go:' + p.name, 1000);
      if (moved.length) throw Object.assign(new Error('ui.go: wait a moment between moves'), { code: 'limit' });
      moved.push(Date.now());
      // A page of this add-on first, then the app's own pages, then another add-on's page of that name.
      const name = typeof a[0] === 'string' ? a[0] : '';
      const mine = name && app.pluginViews.find((x) => x.plugin === p.name && shortOf(x) === name);
      if (mine) return await go({ kind: 'plugin', id: mine.id });
      const other = name && !VIEWS.includes(name) && findView(name);
      if (other) return await go({ kind: 'plugin', id: other.id });
      const v = name ? { kind: name } : { kind: str(a[0]?.kind, 40), id: a[0]?.id != null ? str(a[0].id, 200) : undefined, path: a[0]?.path != null ? str(a[0].path, 500) : undefined };
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
      register(p, 'pluginViews', { id: ownId(p, id), short: id, title: str(s.title, 60), icon: s.icon ? str(s.icon, 30) : undefined, plugin: p.name, render: (el: HTMLElement) => mountSurface(p, { kind: 'view', id }, el) });
      return null;
    case 'ui.addHomeWidget':
      register(p, 'homeWidgets', { id, title: str(s.title, 60), plugin: p.name, render: (el: HTMLElement) => mountSurface(p, { kind: 'widget', id }, el) });
      return null;
    case 'ui.addCommand':
      register(p, 'commands', { id: ownId(p, id), short: id, title: str(s.title, 80), icon: s.icon ? str(s.icon, 30) : undefined, plugin: p.name, run: invoke(s.cb) });
      return null;
    case 'ui.addHomeChip':
      register(p, 'homeChips', { id, label: str(s.label, 40), prompt: str(s.prompt, 4000), plugin: p.name });
      return null;
    case 'ui.addItemAction': {
      const target = s.target === 'idea' ? 'idea' : s.target === 'todo' ? 'todo' : null;
      if (!target) throw new Error('addItemAction: target must be "todo" or "idea"');
      // The callback gets the item's fields, plus `target` and `item` — so both `run(item)` and
      // `run({ target, item })` work, and the value can be handed straight to todos.setDone/edit/remove.
      const call = invoke(s.cb);
      register(p, 'itemActions', { id, target, label: str(s.label, 60), plugin: p.name, run: (item: any) => { const it = plain(item) ?? {}; call({ ...it, target, item: it }); } });
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

/** Raised on a surface frame's element (it bubbles to its slot) once the add-on's page has drawn or failed. */
export const SURFACE_DRAWN = 'thoughts-surface-drawn';

function onMessage(e: MessageEvent) {
  let f: Frame | undefined;
  for (const x of frames) if (x.el.contentWindow === e.source) { f = x; break; }
  if (!f || f.closed) return;
  const m = e.data;
  if (!m || typeof m !== 'object') return;
  const p = f.plugin, frame = f;
  if (m.type === 'ready') {
    // Our frame document says this once, before it has the key. Again means the frame holds another page.
    if (f.ready) { left(f); return; }
    f.ready = true;
    queueLoad(f, {
      ...lookMsg('init'), key: f.key, kind: p.kind, name: p.name, code: p.code,
      surface: !p.dry && f.surface && f.surface.kind !== 'html' ? f.surface : null,
      // A test load first runs the code in a worker, which can be stopped if it never yields.
      preflight: !!p.dry,
    });
    return;
  }
  const rpc = typeof m.method === 'string' && 'id' in m;
  if (m.k !== f.key) {
    // No key. The scripts of an HTML view may post the raw protocol themselves (it only reads);
    // anything else was not sent by our runtime and gets nothing.
    if (!(p.kind === 'view' && f.ready && rpc && m.k === undefined)) {
      if (rpc && f.ready) postTo(f, { id: m.id, error: 'Use the "thoughts" object the add-on was given.', code: 'key' });
      return;
    }
  } else if (m.type === 'pong') {
    const fl = flights.get(f);
    if (fl) { const n = Number(m.n) || 0; if (n > fl.ponged) fl.ponged = n; if (!fl.load && n >= fl.want) landed(f); }
    return;
  } else if (m.type === 'bye') {
    left(f);
    return;
  } else if (m.type === 'key') {
    // Only from a frame the person can be typing in; never from a test load.
    if (f.surface && !p.dry && !p.dead) pressKey(f, m);
    return;
  } else if (m.type === 'loaded' || m.type === 'failed') {
    landed(f);
    // A page or Home box has drawn (or given up): its slot shows it now, with the app's own entrance.
    if (f.surface) f.el.dispatchEvent(new Event(SURFACE_DRAWN, { bubbles: true }));
  }
  if (p.dry) {
    // A review-time test load: only listen for "it loaded" or "it failed"; calls get empty answers.
    if (m.type === 'loaded') p.dry.done(null);
    else if (m.type === 'failed') p.dry.done(m.code === 'hang' ? t('lua.hang') : str(m.error, 600) || 'failed');
    else if (rpc) reply(frame, { id: m.id, result: dryAnswer(m.method) });
    return;
  }
  if (m.type === 'failed') {
    if (m.code === 'permission' || m.code === 'limit' || m.code === 'preview') return; // the person was already told
    fail(p, new Error(m.code === 'hang' ? t('lua.hang') : str(m.error, 2000)));
  } else if (m.type === 'size') {
    f.height = Math.max(0, Math.min(100_000, Number(m.height) || 0));
    if (f.surface?.kind === 'widget') f.el.style.height = Math.max(24, Math.min(900, Number(m.height) || 0)) + 'px';
  } else if (rpc) {
    handle(frame, m.method, m.args, m.k === frame.key && m.ua === true).then(
      (result) => reply(frame, { id: m.id, result }),
      (err) => reply(frame, { id: m.id, error: String(err?.message ?? err), code: err?.code }),
    );
  }
}

/** Answer a call. The add-on's code carries on from there, so it is noted like any other hand-over. */
function reply(f: Frame, msg: any) { if (!f.closed) deliver(f, msg); }

if (typeof window !== 'undefined') {
  window.addEventListener('message', onMessage);
  // Reloading or leaving the window while a note is up is not a freeze: this still ran.
  window.addEventListener('pagehide', () => { if (written) { written = ''; store.del(WATCH_KEY); } });
  on((e) => {
    const fields = EVENTS[e?.type];
    if (!fields) return;
    const payload = Object.fromEntries(fields.map((k) => [k, e[k]]));
    for (const f of frames) if (f.events.has(e.type) && !f.plugin.dead) deliver(f, { type: 'event', event: e.type, payload });
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

/** A folder a plugin declared in `write`, normalised to "folder/". Null when it is not a plain library folder. */
function writeFolder(v: any): string | null {
  const w = String(v ?? '').replace(/\\/g, '/').replace(/^\.?\/+/, '').replace(/\/+$/, '');
  if (!w || w.split('/').some((x) => !x || x === '..' || x.startsWith('.')) || /^([A-Za-z]:|~)/.test(w)) return null;
  return w + '/';
}

/** One plugin / view of a `GET /api/layer` answer, with what it is allowed to do. The daemon sends
 *  these resolved (per-plugin file, else manifest.json); an older daemon is read from the manifest. */
function describe(src: any, kind: 'plugin' | 'view', manifest: any): LayerItem {
  const name = String(src?.name ?? '');
  const group = kind === 'view' ? 'views' : 'plugins';
  const fallback = manifest?.[group]?.[name] ?? manifest?.[group]?.[`${group}/${name}`] ?? {};
  const declared: any[] = Array.isArray(src?.permissions) ? src.permissions : [...permsOf(manifest, `${group}|${name}`, `${group}|${group}/${name}`)];
  const allowed = kind === 'view' ? VIEW_PERMS : PERMS;
  const apiVersion = Number(src?.apiVersion ?? fallback?.apiVersion ?? manifest?.apiVersion ?? 1) || 1;
  const unsupported = src?.unsupported === true || apiVersion > API_VERSION;
  const write = kind === 'view' ? [] : (Array.isArray(src?.write) ? src.write : Array.isArray(fallback?.write) ? fallback.write : []).map(writeFolder).filter((x: string | null): x is string => !!x).slice(0, 20);
  // The only folders it reads in (the daemon holds it to them whatever is said here).
  const read = (Array.isArray(src?.read) ? src.read : Array.isArray(fallback?.read) ? fallback.read : []).map(writeFolder).filter((x: string | null): x is string => !!x).slice(0, 20);
  return {
    name, kind, apiVersion, unsupported,
    permissions: declared.filter((x) => allowed.includes(x)),
    write, read,
    description: str(src?.description ?? fallback?.description, 300).trim(),
    message: unsupported ? str(src?.message, 300).trim() || t('layer3.unsupported', { version: apiVersion }) : '',
  };
}

/** What the loaded layer contains, including items that were not loaded (Settings → Customisations).
 *  Kept on the reactive app state (`app.layerItems`) because this module is plain TypeScript. */
export const layerItems = (): LayerItem[] => ((app as any).layerItems as LayerItem[] | undefined) ?? [];
const setLayerItems = (v: LayerItem[]) => { (app as any).layerItems = v; };

function unload() {
  for (const p of plugins) { p.dead = true; dispose(p); }
  plugins = [];
  // What add-ons saved while a change was tried goes with the preview.
  pretend = freshPretend();
  app.pluginViews = [];
  app.commands = [];
  app.homeWidgets = [];
  app.homeChips = [];
  app.itemActions = [];
}

const oneOf = <T extends string>(v: any, list: readonly T[]): T | undefined => (list.includes(v) ? v : undefined);
/** `nav.json` → NavConfig (null when absent or not an object). Home can never be hidden. */
function navConfig(v: any): NavConfig | null {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return null;
  const labels: NavConfig['labels'] = {};
  if (v.labels && typeof v.labels === 'object' && !Array.isArray(v.labels)) {
    for (const [id, l] of Object.entries(v.labels as Record<string, any>).slice(0, 40)) {
      if (!l || typeof l !== 'object') continue;
      const en = typeof l.en === 'string' ? l.en.trim().slice(0, 40) : '', ar = typeof l.ar === 'string' ? l.ar.trim().slice(0, 40) : '';
      if (en || ar) labels[String(id).slice(0, 120)] = { ...(en ? { en } : {}), ...(ar ? { ar } : {}) };
    }
  }
  return { hidden: strings(v.hidden).filter((x) => !['home', 'settings', 'help'].includes(x)).slice(0, 60), order: strings(v.order).slice(0, 60), labels };
}
/** `todos.json` → TodosConfig (null when absent); unknown values are dropped, not guessed. */
function todosConfig(v: any): TodosConfig | null {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return null;
  const c: TodosConfig = {};
  const g = oneOf(v.groupBy, ['list', 'due', 'tag', 'none'] as const); if (g) c.groupBy = g;
  const s = oneOf(v.sort, ['list', 'due'] as const); if (s) c.sort = s;
  const f = oneOf(v.filter, ['all', 'soon', 'overdue'] as const); if (f) c.filter = f;
  if (typeof v.showDone === 'boolean') c.showDone = v.showDone;
  if (typeof v.moveDone === 'boolean') c.moveDone = v.moveDone;
  return c;
}

const strings = (v: any) => (Array.isArray(v) ? v.filter((x) => typeof x === 'string') : []);

/** Load the kept layer, or with `preview` the staged one ("Try it"). Nothing staged is loaded any other way.
 *  A preview names the signature of the change the user chose to try (`sig`, else the one being tried
 *  now); the service refuses (`CHANGED`) when the staged files are no longer exactly that change. */
export async function loadLayer(opts: { preview?: boolean; sig?: string } = {}) {
  if (app.safeMode) { app.nav = null; app.todosCfg = null; setPages(null); setLayerItems([]); return; }
  const boot = firstLoad;
  firstLoad = false;
  if (boot && crashed) {
    // Add-ons of the kept layer were running when the app last stopped responding (see the freeze guard).
    app.safeMode = true;
    app.safeReason = 'crash';
    app.nav = null; app.todosCfg = null; setPages(null);
    return;
  }
  const want = opts.preview ? (opts.sig ?? tried) : '';
  const m = await api(opts.preview ? '/layer?preview=1&sig=' + encodeURIComponent(want) : '/layer');
  // Without the frame document (Windows only) each add-on reports that it could not start; the
  // rest of the customisations still load.
  await frameDoc().catch(() => {});
  // Reload: remove everything the previous generation of plugins registered.
  unload();
  app.preview = !!opts.preview;
  tried = want;
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
  // `order` may also name `widget:<id>` and `suggestions`; Home places them.
  app.nav = navConfig(m.nav);
  app.todosCfg = todosConfig(m.todos);
  setPages(m.pages);
  const manifest = m.manifest && typeof m.manifest === 'object' ? m.manifest : {};
  const mk = (it: LayerItem, code: string): Plugin =>
    ({ name: it.name, kind: it.kind, code: String(code ?? ''), perms: new Set(it.permissions), write: it.write, read: it.read, description: it.description, dead: false, disposers: [], frames: new Set(), warned: new Set() });
  const views: any[] = Array.isArray(m.views) ? m.views : [];
  const list: any[] = Array.isArray(m.plugins) ? m.plugins : [];
  const items: LayerItem[] = [...list.map((x) => describe(x, 'plugin', manifest)), ...views.map((x) => describe(x, 'view', manifest))];
  setLayerItems(items);
  views.forEach((v, i) => {
    const it = items[list.length + i];
    if (it.unsupported) return; // made for a newer app: listed in Settings, never run
    // Views get nothing by default; like add-ons they must declare read permissions.
    const p = mk(it, v.code);
    plugins.push(p);
    const id = 'view:' + v.name;
    register(p, 'pluginViews', { id, title: v.name.replace(/\.html$/, ''), icon: 'plugin', plugin: p.name, render: (el: HTMLElement) => mountSurface(p, { kind: 'html', id }, el) });
  });
  list.forEach((src, i) => {
    const it = items[i];
    if (it.unsupported) return;
    const p = mk(it, src.code);
    plugins.push(p);
    try { createFrame(p, null, background()); } catch (e) { fail(p, e); }
  });
  relook();
}

// ---------- customise flow: ask → run → review → try / keep / discard ----------
const sig = (p: PendingChange | null) => (p ? p.signature || JSON.stringify([p.summary, p.files, p.origin]) : '');
let dismissed = '';
// The change whose preview was switched off to show its review (`showReview`). Closing the review
// without choosing puts that preview back, so closing never leaves the change half-shown.
let resumePreview = '';
// The signature of the change being tried, sent with every preview load (see `loadLayer`).
let tried = '';
const pendingSig = () => app.pending?.signature ?? '';

/** Load the preview of the change with `signature`; when the service refuses (the change was edited,
 *  replaced or is being written again) the kept customisations load instead and the waiting change is
 *  read again, so the choice is offered for what is staged now. */
async function previewOr(signature: string) {
  try { await loadLayer({ preview: true, sig: signature }); }
  catch (e) { await loadLayer().catch(() => {}); await checkPending(false); throw e; }
}

/** Reload after the customisations changed (`layer.changed`): a change being tried stays on screen
 *  only while it is still exactly what the user chose to try. */
export async function reloadLayer() {
  if (!app.preview) return loadLayer();
  try { await previewOr(tried); }
  catch (e) { if (errCode(e) !== 'CHANGED') failed(t('layer.tryFailed'), e); }
}
const missing = (e: any) => e instanceof ApiError && (e.status === 404 || e.status === 405);

/** Ask the daemon whether a staged change is waiting. Opens the review panel when `open` (unless the
 *  user already closed the panel for this very change). Quietly does nothing on an older service. */
export async function checkPending(open = true): Promise<PendingChange | null> {
  let p: PendingChange;
  try { p = await api('/layer/pending'); } catch { return app.pending; }
  if (!p?.exists) {
    app.pending = null;
    app.showReview = false;
    resumePreview = '';
    froze.clear();
    if (app.preview) await loadLayer().catch(() => {});
    return null;
  }
  app.pending = p;
  // A version the user went back to the chat about waits for their next message, not for a decision.
  if (open && !p.running && !p.continued && sig(p) !== dismissed) await showReview();
  return p;
}

/** Put the review on screen. The staged change is never loaded into the app while it is decided on:
 *  a preview is stopped first, so staged CSS cannot draw over the review or relabel its buttons. */
async function showReview() {
  if (app.preview) { resumePreview = pendingSig(); await loadLayer().catch(() => {}); }
  app.showReview = true;
}

export function openReview() {
  dismissed = '';
  // "Review it again" on a version set aside for more conversation: back to the review as it is.
  if (app.pending?.continued) { post('/layer/pending/review-again').then(() => checkPending(true), (e) => failed(t('shell.layerFailed'), e)); return; }
  if (app.pending) showReview(); else checkPending(true);
}
/** Close the review without choosing ("Later", ×, Escape, a click outside). The change keeps waiting
 *  (the shell shows a "waiting for your choice" strip) and, if it was being tried when the review
 *  opened, its preview comes back on. */
export function closeReview() {
  dismissed = sig(app.pending);
  app.showReview = false;
  const back = resumePreview && resumePreview === pendingSig() && !app.pending?.running && !app.safeMode && froze.get()?.sig !== sig(app.pending) ? resumePreview : '';
  resumePreview = '';
  if (back && !app.preview) previewOr(back).catch((e) => { if (errCode(e) !== 'CHANGED') failed(t('layer.tryFailed'), e); });
}

/** The staged change froze the app when it was last tested or tried: ask before running it again. */
async function sureDespiteFreeze(what: 'try' | 'keep'): Promise<boolean> {
  const fz = froze.get();
  if (!fz || fz.sig !== sig(app.pending)) return true;
  return aboveShield(() => confirmDialog({ title: t('lua.frozeTitle'), body: t(what === 'try' ? 'lua.frozeTry' : 'lua.frozeKeep'), confirmLabel: t(what === 'try' ? 'lua.frozeTryAnyway' : 'lua.frozeKeepAnyway'), danger: true }));
}

/** New customisation: open an empty customisation chat (an empty one already there is opened again),
 *  where the person describes the change. Its first message starts the assistant on a staged copy. */
let opening = false;
export async function newCustomise(): Promise<void> {
  if (opening) return;
  opening = true;
  try {
    const c = await post('/chats', { kind: 'customize', byUser: true });
    dismissed = '';
    await refreshChats().catch(() => {});
    if (c?.id) go({ kind: 'chat', id: c.id });
  } catch (e) { failed(t('r10-cust-start.failed'), e); }
  finally { opening = false; }
}

/** Start a customise run with a request the app prepared (a Home suggestion the person accepted),
 *  in a customisation chat named `title`. Nothing changes until the review. */
export async function startCustomise(prompt: string, title = ''): Promise<string | null> {
  const text = prompt.trim();
  if (!text) return null;
  const r = await post('/layer/customize', { prompt: text, ...(title.trim() ? { title: title.trim() } : {}) });
  dismissed = '';
  refreshChats().catch(() => {});
  if (r?.chatId) go({ kind: 'chat', id: r.chatId });
  return r?.chatId ?? null;
}

/** "Try it": load the staged layer into the running app. False when the person decided not to. */
export async function tryPending(): Promise<boolean> {
  if (!(await sureDespiteFreeze('try'))) return false;
  // Close the review before anything staged is loaded (see `showReview`).
  resumePreview = '';
  closeReview();
  try { await loadLayer({ preview: true, sig: pendingSig() }); }
  catch (e) { dismissed = ''; if (errCode(e) === 'CHANGED') await checkPending(false); app.showReview = !!app.pending; throw e; }
  return true;
}

export async function stopPreview() { await loadLayer(); }

/** "Keep": make the staged change part of the app. The signature of what the user reviewed goes
 *  along; if the staged change was replaced meanwhile (`CHANGED::`) nothing is applied, the review is
 *  reloaded for the user to look at again. */
export async function keepPending(opts: { approveWorkflows?: boolean } = {}): Promise<void> {
  if (app.pending?.problems?.length) { openReview(); throw new Error(t('layer3.problemsBlock')); }
  if (!(await sureDespiteFreeze('keep'))) return;
  let r: any;
  try {
    r = await post('/layer/pending/apply', { ...(app.pending?.signature ? { signature: app.pending.signature } : {}), ...(opts.approveWorkflows ? { approveWorkflows: true } : {}) });
  } catch (e) {
    if (errCode(e) !== 'CHANGED') throw e;
    dismissed = '';
    await checkPending(true);
    toast(t('layer3.changedReload'), { kind: 'error', key: 'layer' });
    return;
  }
  app.pending = null;
  app.showReview = false;
  resumePreview = '';
  froze.clear();
  await loadLayer().catch((e) => failed(t('shell.layerFailed'), e));
  toast(t('layer.kept'), {
    kind: 'success', key: 'layer',
    action: r?.hash ? { label: t('undo'), run: () => { revertChange(r.hash).then(() => toast(t('layer.undone'), { kind: 'success', key: 'layer' }), (e) => failed(t('layer.undoFailed'), e)); } } : undefined,
  });
}

/** "Continue chatting": the person tried this version and wants changes. Like Discard, the app is back
 *  to how it was at once (the preview ends first); unlike it, the version stays for the assistant to
 *  build on, and the person lands in its chat with the cursor in the message box. */
export async function continueChatting() {
  resumePreview = '';
  if (app.preview) await loadLayer().catch(() => {});
  app.showReview = false;
  let r: any;
  // Refused (the chat was deleted, the assistant is still writing): the review comes back, unchanged.
  try { r = await post('/layer/pending/continue'); } catch (e) { dismissed = ''; await checkPending(true); throw e; }
  await checkPending(false);
  const id = r?.chatId;
  if (!id) return;
  go({ kind: 'chat', id });
  toast(t('r10-cust-continue.toast'), { key: 'layer' });
  // An open chat view is told to put the cursor in its box; one that opens now does that by itself.
  setTimeout(() => window.dispatchEvent(new CustomEvent('thoughts:compose', { detail: id })), 0);
}

/** "Discard": throw the staged change away. */
export async function discardPending() {
  await post('/layer/pending/discard');
  app.pending = null;
  app.showReview = false;
  resumePreview = '';
  froze.clear();
  if (app.preview) await loadLayer().catch(() => {});
}

/** Undo one change from the history (the rest of the customisations stay). */
export async function revertChange(hash: string) {
  await post('/layer/revert', { hash });
  if (!app.preview) await loadLayer().catch(() => {});
}

/** What the staged layer's add-ons and pages are allowed to do — read as data, nothing is run in the app. */
export async function stagedItems(): Promise<LayerItem[]> {
  try {
    const m = await api('/layer?preview=1&sig=' + encodeURIComponent(pendingSig()));
    const manifest = m.manifest && typeof m.manifest === 'object' ? m.manifest : {};
    return [...(m.plugins || []).map((x: any) => describe(x, 'plugin', manifest)), ...(m.views || []).map((x: any) => describe(x, 'view', manifest))];
  } catch { return []; }
}

/** Test-load every staged add-on and page in a hidden sandbox frame that is given no data and can
 *  change nothing, and report the ones that fail to start. Unsupported items are not run.
 *
 *  Nothing is run when customisations are off (safe mode must load nothing, staged or kept), nor when
 *  this very change froze the app the last time it was tested or tried: such items come back with
 *  `skipped` (why) and, in the second case, `error`. One item is tested at a time. */
export async function dryLoadStaged(): Promise<LayerItem[]> {
  let m: any;
  try { m = await api('/layer?preview=1&sig=' + encodeURIComponent(pendingSig())); } catch { return []; }
  const manifest = m.manifest && typeof m.manifest === 'object' ? m.manifest : {};
  const sources: [any, 'plugin' | 'view'][] = [...(m.plugins || []).map((x: any) => [x, 'plugin'] as [any, 'plugin']), ...(m.views || []).map((x: any) => [x, 'view'] as [any, 'view'])];
  const items = sources.map(([src, kind]) => describe(src, kind, manifest));
  if (app.safeMode) return items.map((it) => (it.unsupported ? it : { ...it, skipped: t('lua.notTestedSafe') }));
  const fz = froze.get();
  if (fz && fz.sig === sig(app.pending)) {
    const blame = (it: LayerItem) => !fz.names.length || fz.names.includes(it.name);
    return items.map((it) => (it.unsupported ? it : { ...it, skipped: t('lua.notTestedFroze'), ...(blame(it) ? { error: t('lua.frozeItem') } : {}) }));
  }
  try { await frameDoc(); } catch { return items.map((it) => (it.unsupported ? it : { ...it, skipped: t('lua.notTested') })); }
  const out: LayerItem[] = [];
  for (let i = 0; i < items.length; i++) {
    const it = items[i], [src, kind] = sources[i];
    if (it.unsupported) { out.push(it); continue; }
    const error = await new Promise<string | null>((resolve) => {
      let f: Frame | null = null;
      const timer = setTimeout(() => finish(t('layer3.dryTimeout')), 8000);
      function finish(err: string | null) { clearTimeout(timer); if (f) destroyFrame(f); f = null; resolve(err); }
      const p: Plugin = { name: it.name, kind, code: String(src.code ?? ''), perms: new Set(), write: [], read: [], description: '', dead: false, disposers: [], frames: new Set(), warned: new Set(), dry: { done: finish } };
      try { f = createFrame(p, null, background()); } catch (e: any) { finish(String(e?.message ?? e)); }
    });
    out.push(error ? { ...it, error } : it);
  }
  return out;
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
  if (trigger.every === 'weekdays') {
    // `days` (0 = Sunday) narrows or changes the default Monday–Friday.
    const days: number[] = Array.isArray(trigger.days) ? [...new Set<number>(trigger.days.map(Number).filter((d: number) => d >= 0 && d <= 6))].sort() : [];
    if (!days.length || days.join() === '1,2,3,4,5') return t('layer3.wf.weekdays', { time });
    const names = new Intl.ListFormat(lang() === 'ar' ? 'ar' : 'en', { type: 'conjunction' }).format(days.map((d) => fmtDate(new Date(2023, 0, 1 + d, 12), { weekday: 'long' })));
    return t('ui.wf.days', { days: names, time });
  }
  if (trigger.every === 'month') {
    if (trigger.day === 'last') return t('layer3.wf.monthlyLast', { time });
    const d = Math.min(28, Math.max(1, Number(trigger.day) || 1));
    return t('layer3.wf.monthly', { day: d, time });
  }
  return t('layer.wf.daily', { time });
}

// ---------- packs ----------
/** A pack's id: the same for every export under one name from this computer, different for anyone
 *  else's pack of that name. On import the pack's CSS block is keyed by it, so a newer export replaces
 *  its own earlier block and never another pack's. */
export function packId(name: string): string {
  const key = name.trim().toLowerCase();
  let ids: Record<string, string> = {};
  try { const v = JSON.parse(store.get('thoughts.packIds') || '{}'); if (v && typeof v === 'object' && !Array.isArray(v)) ids = v; } catch { /* start again */ }
  if (!/^[a-z0-9]{12}$/.test(ids[key] ?? '')) {
    const b = new Uint8Array(12);
    crypto.getRandomValues(b);
    ids[key] = Array.from(b, (x) => 'abcdefghijklmnopqrstuvwxyz0123456789'[x % 36]).join('');
    store.set('thoughts.packIds', JSON.stringify(Object.fromEntries(Object.entries(ids).slice(-200))));
  }
  return ids[key];
}

/** Save the user's customisations as a pack file. The person names it first (and may add a line
 *  about what it does): the name is what whoever imports it sees, and tells packs apart.
 *  Resolves 'saved', 'copied' (clipboard fallback) or 'cancelled'. */
export async function exportPack(paths?: string[]): Promise<'saved' | 'copied' | 'cancelled'> {
  const asked = await promptDialog({ title: t('lua.packNameTitle'), body: t('lua.packNameBody'), placeholder: t('lua.packNameHint'), confirmLabel: t('next') });
  if (asked === null) return 'cancelled';
  const title = str(asked, 80).replace(/\s+/g, ' ').trim();
  if (!title) { toast(t('lua.packNameNeeded'), { kind: 'error', key: 'pack-name' }); return 'cancelled'; }
  const about = await promptDialog({ title: t('lua.packAboutTitle'), body: t('lua.packAboutBody'), placeholder: t('lua.packAboutHint'), confirmLabel: t('lua.packSave') });
  if (about === null) return 'cancelled';
  const q = new URLSearchParams({ name: title, description: str(about, 400).replace(/\s+/g, ' ').trim() });
  if (paths?.length) q.set('paths', paths.join(','));
  const pack = await api('/layer/export?' + q.toString());
  if (pack?.format !== 'thoughts-pack') throw new Error(t('layer.packInvalid'));
  pack.id = packId(title);
  const text = JSON.stringify(pack, null, 2);
  const d = new Date();
  // The file is named after the pack, in whatever script the person wrote it; only characters a file name cannot hold are dropped.
  const slug = title.replace(/[\\/:*?"<>|\u0000-\u001f]+/g, ' ').replace(/\s+/g, '-').replace(/^[-.]+|[-.]+$/g, '').slice(0, 24).replace(/-+$/, '');
  const name = `thoughts-${paths?.length ? 'feature-' : ''}pack-${slug ? slug + '-' : ''}${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}.json`;
  if ((window as any).__TAURI_INTERNALS__) {
    // The native window has no download bar: the shell opens the save dialog itself and writes the file.
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      return (await invoke('save_pack', { name: name.replace(/\.json$/, ''), contents: text })) ? 'saved' : 'cancelled';
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

/** The files of the kept layer that can go into a feature pack, grouped for the "choose items" list. */
export async function packChoices(): Promise<{ path: string; kind: 'plugin' | 'view' | 'workflow' | 'look' | 'layout'; label: string }[]> {
  const m = await api('/layer');
  const out: { path: string; kind: 'plugin' | 'view' | 'workflow' | 'look' | 'layout'; label: string }[] = [];
  for (const p of m.plugins || []) out.push({ path: 'plugins/' + p.name, kind: 'plugin', label: str(p.description, 80) || String(p.name).replace(/\.js$/, '') });
  for (const v of m.views || []) out.push({ path: 'views/' + v.name, kind: 'view', label: str(v.description, 80) || String(v.name).replace(/\.html$/, '') });
  try { for (const w of (await api('/workflows')) as any[]) if (w?.file || w?.name) out.push({ path: 'workflows/' + (w.file ?? `${w.name}.json`), kind: 'workflow', label: String(w.name) }); } catch { /* optional */ }
  if (String(m.css ?? '').trim()) out.push({ path: 'custom.css', kind: 'look', label: 'custom.css' });
  if (Array.isArray(m.themes) && m.themes.length) out.push({ path: 'themes.json', kind: 'look', label: 'themes.json' });
  if (m.home) out.push({ path: 'home.json', kind: 'layout', label: 'home.json' });
  if (m.nav) out.push({ path: 'nav.json', kind: 'layout', label: 'nav.json' });
  if (m.todos) out.push({ path: 'todos.json', kind: 'layout', label: 'todos.json' });
  if (m.pages) out.push({ path: 'pages.json', kind: 'layout', label: 'pages.json' });
  return out;
}

/** Read a pack file without staging anything, so the user can see what it is first. */
export async function readPack(file: File): Promise<{ pack: any; name: string; description: string; kind: 'full' | 'feature'; apiVersion: number; unsupported: boolean; files: string[] }> {
  if (file.size > 5 * 1024 * 1024) throw new Error(t('layer.packTooBig'));
  let pack: any;
  try { pack = JSON.parse(await file.text()); } catch { throw new Error(t('layer.packInvalid')); }
  if (pack?.format !== 'thoughts-pack' || !pack.files || typeof pack.files !== 'object') throw new Error(t('layer.packInvalid'));
  const apiVersion = Number(pack.apiVersion ?? 1) || 1;
  return { pack, name: str(pack.name, 80), description: str(pack.description, 400), kind: pack.kind === 'feature' ? 'feature' : 'full', apiVersion, unsupported: apiVersion > API_VERSION, files: Object.keys(pack.files).slice(0, 200) };
}

/** Stage a pack (already read with `readPack`) for review. It is never applied directly. */
export async function stagePack(pack: any) {
  await post('/layer/import', { pack });
  dismissed = '';
  await checkPending(true);
}

/** Stage a pack file for review. It is never applied directly. */
export async function importPack(file: File) {
  const r = await readPack(file);
  if (r.unsupported) throw new Error(t('layer3.packNewer'));
  await stagePack(r.pack);
}

export { errDetail as layerErrorText };
export { missing as isMissingEndpoint };
