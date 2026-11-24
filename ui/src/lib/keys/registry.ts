// Every keyboard shortcut of the app in one table (round 0.5.0). The app-wide key handler
// (App.svelte), the add-on frame relay (lib/layer.ts) and every view's own key handler ask
// `matchKey(e, scope)` which command a key press is; tooltips, Help and Settings › Keyboard read the
// same table, so a key the user changed shows correctly everywhere.
//
// A key is written one way everywhere ("Binding"): modifiers in the order Mod, Control, Alt, Shift,
// Super, then the physical key as `KeyboardEvent.code` gives it ("Mod+Shift+KeyW", "Slash", "F3").
// `Mod` is ⌘ on a Mac and Ctrl elsewhere, so a set moves between computers; `Control` is ⌃ on a Mac
// (the same as Mod elsewhere) and `Super` the Windows key (⌘, the same as Mod, on a Mac). Keys follow
// the physical key, so an Arabic keyboard works and shows the Latin key cap.
//
// What the user changed lives in the layer's `keys.json` ({format: 1, bindings: {id: [Binding]}}),
// which replaces an id's defaults (`[]` = no key). The service checks it (crates/thoughtsd/src/keys.rs)
// against the same reserved list (`reserved.json`). Keys that stay fixed are listed with a reason.
import { t } from '../i18n';
import { app } from '../store.svelte';
import { keysLabel, os as osNow, type Os } from '../quickKeys';
import { keyState } from './state.svelte';
import RESERVED_JSON from './reserved.json';

export { isMac } from './platform';

export type Scope = 'app' | 'native' | 'global' | 'docs' | 'docs.editor' | 'find' | 'graph' | 'mado-graph' | 'mado-design' | 'mado-fields'
  | 'image' | 'comments' | 'sidebar-edit' | 'fixed';
export type Binding = string;
export interface KeyDef {
  id: string;
  /** i18n key of what it does; `args` fill it (a value starting with `@` is itself an i18n key). */
  label: string;
  args?: Record<string, string | number>;
  /** Literal words instead of `label` (a command an add-on registered: its own title). */
  title?: string;
  /** i18n key of its group on the Keyboard page. */
  group: string;
  scope: Scope;
  defaults: Binding[];
  editable: boolean;
  native?: 'menu' | 'global';
  /** A native menu key the page also handles itself (Settings ⌘,). */
  web?: boolean;
  /** May be a key with no modifier (app-wide only outside text fields). */
  plain?: boolean;
  /** Why it cannot be changed (i18n key). */
  fixedWhy?: string;
  /** Defaults on a Mac / on Windows / on Linux when they differ from `defaults`. */
  mac?: Binding[];
  windows?: Binding[];
  linux?: Binding[];
  /** Not listed in Help (the nav keys after the first). */
  hidden?: boolean;
}

const G = {
  general: 'kb.g.general', nav: 'kb.g.nav', chats: 'kb.g.chats', editor: 'kb.g.editor', find: 'kb.g.find', graph: 'kb.g.graph',
  mado: 'kb.g.mado', pictures: 'kb.g.pictures', window: 'kb.g.window', anywhere: 'kb.g.anywhere', fixed: 'kb.g.fixed', addons: 'kb.g.addons',
};

const app_ = (id: string, label: string, group: string, defaults: Binding[] = [], extra: Partial<KeyDef> = {}): KeyDef =>
  ({ id, label, group, scope: 'app', defaults, editable: true, ...extra });
const local = (scope: Scope, group: string, id: string, label: string, defaults: Binding[], extra: Partial<KeyDef> = {}): KeyDef =>
  ({ id, label, group, scope, defaults, editable: true, ...extra });
const fixed = (id: string, label: string, defaults: Binding[], fixedWhy: string, extra: Partial<KeyDef> = {}): KeyDef =>
  ({ id, label, group: G.fixed, scope: 'fixed', defaults, editable: false, fixedWhy, ...extra });

const ZOOM_IN = ['Equal', 'Shift+Equal', 'NumpadAdd'];
const ZOOM_OUT = ['Minus', 'Shift+Minus', 'NumpadSubtract'];
const FIT = ['Digit0', 'Numpad0'];

export const KEYS: KeyDef[] = [
  // ---- app-wide ----
  app_('palette', 'shell.sc.palette', G.general, ['Mod+KeyK']),
  app_('search.focus', 'srch.sc.search', G.general, ['Mod+Shift+KeyF', 'Slash'], { plain: true }),
  app_('find', 'srch.sc.find', G.find, ['Mod+KeyF']),
  app_('wrap.start', 'shell.sc.wrap', G.chats, ['Mod+Shift+KeyW']),
  app_('chat.new', 'shell.sc.newChat', G.chats, ['Mod+KeyN']),
  app_('chat.composer.expand', 'chatg.sc.expand', G.chats, ['Mod+Shift+KeyE']),
  ...[1, 2, 3, 4, 5, 6].map((n) => app_('nav.' + n, 'kb.k.nav', G.nav, ['Mod+Digit' + n], { args: { n }, hidden: n > 1 })),
  app_('back', 'ui.sc.back', G.nav, ['Mod+BracketLeft']),
  app_('forward', 'ui.sc.forward', G.nav, ['Mod+BracketRight']),
  app_('sidebar.toggle', 'shell.sc.sidebar', G.nav, ['Mod+Backslash']),
  app_('zoom.in', 'a11y.sc.zoomIn', G.general, ['Mod+Equal', 'Mod+Shift+Equal', 'Mod+NumpadAdd']),
  app_('zoom.out', 'a11y.sc.zoomOut', G.general, ['Mod+Minus', 'Mod+NumpadSubtract']),
  app_('zoom.reset', 'a11y.sc.zoomReset', G.general, ['Mod+Digit0']),
  // Commands of the command palette: no key until the user gives them one.
  app_('print', 'shell.pal.print', G.general),
  app_('help', 'shell.help', G.nav),
  app_('tour', 'shell.pal.tour', G.general),
  app_('history', 'ui.hist.title', G.nav),
  app_('rollback', 'ui.pal.appHistory', G.general),
  app_('sync', 'shell.pal.sync', G.general),
  app_('customise', 'layer.customise', G.general),
  app_('customise.new', 'r10-cust-start.new', G.general),
  app_('customise.automate', 'auto-ui.suggest', G.general),
  app_('review', 'layer.reviewTitle', G.general),
  app_('nav.todos.calendar', 'ui.cal.title', G.nav),
  app_('updates.check', 'layer.upd.check', G.general),
  app_('sidebar.edit', 'sbe.open', G.nav),
  app_('settings.accessibility', 'a11y.open', G.nav),
  app_('slides.present', 'kb.k.slides.present', G.general),
  ...(['saudi', 'contrast', 'tech'] as const).map((th) => app_('theme.' + th, 'shell.pal.theme', G.general, [], { args: { name: '@' + th } })),
  ...(['light', 'dark', 'system'] as const).map((m) => app_('mode.' + m, 'shell.pal.mode', G.general, [], { args: { name: '@' + m } })),
  // ---- the native menu (the shell builds it from `keys.json`, crates/thoughtsd/src/keys.rs `native`) ----
  { id: 'settings', label: 'shell.sc.settings', group: G.window, scope: 'native', native: 'menu', web: true, defaults: ['Mod+Comma'], editable: true },
  { id: 'window.close', label: 'kb.k.window.close', group: G.window, scope: 'native', native: 'menu', defaults: ['Mod+KeyW'], editable: true },
  { id: 'app.quit', label: 'kb.k.app.quit', group: G.window, scope: 'native', native: 'menu', defaults: ['Mod+KeyQ'], windows: [], editable: true },
  { id: 'window.fullscreen', label: 'kb.k.window.fullscreen', group: G.window, scope: 'native', native: 'menu', defaults: ['F11'], mac: ['Mod+Control+KeyF'], editable: true },
  // ---- anywhere on the computer: stored in settings.capture, changed with QuickCapturePrefs ----
  { id: 'capture.global', label: 'kb.k.capture.global', group: G.anywhere, scope: 'global', native: 'global', defaults: ['Control+Alt+Space'], mac: ['Mod+Control+Alt+Space'], editable: true },
  // ---- one view or part at a time ----
  local('docs', G.editor, 'docs.save', 'kb.k.docs.save', ['Mod+KeyS']),
  local('docs.editor', G.editor, 'docs.editor.bold', 'kb.k.docs.editor.bold', ['Mod+KeyB']),
  local('docs.editor', G.editor, 'docs.editor.italic', 'kb.k.docs.editor.italic', ['Mod+KeyI']),
  local('docs.editor', G.editor, 'docs.editor.link', 'kb.k.docs.editor.link', ['Mod+KeyK']),
  ...[0, 1, 2, 3].map((n) => local('docs.editor', G.editor, 'docs.editor.heading' + n, n ? 'kb.k.docs.editor.heading' : 'kb.k.docs.editor.heading0', ['Mod+Alt+Digit' + n, 'Mod+Alt+Numpad' + n], { args: { n } })),
  local('docs.editor', G.editor, 'docs.editor.list.number', 'kb.k.docs.editor.list.number', ['Mod+Shift+Digit7']),
  local('docs.editor', G.editor, 'docs.editor.list.bullet', 'kb.k.docs.editor.list.bullet', ['Mod+Shift+Digit8']),
  local('docs.editor', G.editor, 'docs.editor.list.task', 'kb.k.docs.editor.list.task', ['Mod+Shift+Digit9']),
  local('find', G.find, 'find.next', 'kb.k.find.next', ['F3', 'Mod+KeyG'], { plain: true }),
  local('find', G.find, 'find.prev', 'kb.k.find.prev', ['Shift+F3', 'Mod+Shift+KeyG'], { plain: true }),
  local('graph', G.graph, 'graph.zoomIn', 'kb.k.zoomIn', ZOOM_IN, { plain: true }),
  local('graph', G.graph, 'graph.zoomOut', 'kb.k.zoomOut', ZOOM_OUT, { plain: true }),
  local('graph', G.graph, 'graph.fit', 'kb.k.fit', FIT, { plain: true }),
  local('graph', G.graph, 'graph.home', 'kb.k.graph.home', ['Home'], { plain: true }),
  local('mado-graph', G.mado, 'mado-graph.undo', 'kb.k.undo', ['Mod+KeyZ']),
  local('mado-graph', G.mado, 'mado-graph.redo', 'kb.k.redo', ['Mod+Shift+KeyZ', 'Control+KeyY']),
  local('mado-graph', G.mado, 'mado-graph.rename', 'kb.k.mado.rename', ['F2'], { plain: true }),
  local('mado-graph', G.mado, 'mado-graph.delete', 'kb.k.mado.delete', ['Delete', 'Backspace'], { plain: true }),
  local('mado-graph', G.mado, 'mado-graph.comment', 'kb.k.mado.comment', ['KeyC'], { plain: true }),
  local('mado-graph', G.mado, 'mado-graph.zoomIn', 'kb.k.zoomIn', ZOOM_IN, { plain: true }),
  local('mado-graph', G.mado, 'mado-graph.zoomOut', 'kb.k.zoomOut', ZOOM_OUT, { plain: true }),
  local('mado-graph', G.mado, 'mado-graph.fit', 'kb.k.fit', FIT, { plain: true }),
  local('mado-fields', G.mado, 'mado-fields.apply', 'kb.k.mado.fields', ['Mod+Enter']),
  local('mado-design', G.mado, 'mado-design.undo', 'kb.k.mado.designUndo', ['Mod+KeyZ']),
  local('mado-design', G.mado, 'mado-design.redo', 'kb.k.mado.designRedo', ['Mod+Shift+KeyZ']),
  local('comments', G.mado, 'comments.send', 'kb.k.comments.send', ['Mod+Enter']),
  local('image', G.pictures, 'image.zoomIn', 'kb.k.zoomIn', ZOOM_IN, { plain: true }),
  local('image', G.pictures, 'image.zoomOut', 'kb.k.zoomOut', ZOOM_OUT, { plain: true }),
  local('image', G.pictures, 'image.fit', 'kb.k.fit', FIT, { plain: true }),
  // Editing the sidebar in place (views/SidebarEdit.svelte): the row that has the focus.
  local('sidebar-edit', G.nav, 'sidebar-edit.up', 'sb72.k.up', ['Alt+ArrowUp'], { plain: true }),
  local('sidebar-edit', G.nav, 'sidebar-edit.down', 'sb72.k.down', ['Alt+ArrowDown'], { plain: true }),
  local('sidebar-edit', G.nav, 'sidebar-edit.rename', 'sb72.k.rename', ['F2'], { plain: true }),
  local('sidebar-edit', G.nav, 'sidebar-edit.undo', 'sb72.k.undo', ['Mod+KeyZ']),
  // ---- shown, but fixed ----
  fixed('escape', 'shell.sc.escape', ['Escape'], 'kb.why.escape'),
  fixed('fixed.tab', 'kb.k.fixed.tab', ['Tab', 'Shift+Tab'], 'kb.why.a11y'),
  fixed('fixed.arrows', 'kb.k.fixed.arrows', ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End'], 'kb.why.a11y'),
  fixed('fixed.activate', 'kb.k.fixed.activate', ['Enter', 'Space'], 'kb.why.a11y'),
  fixed('fixed.message', 'kb.k.fixed.message', ['Enter', 'Shift+Enter', 'Mod+Enter'], 'kb.why.message'),
  fixed('fixed.palette', 'kb.k.fixed.palette', ['ArrowUp', 'ArrowDown', 'Enter'], 'kb.why.a11y'),
  fixed('fixed.findEnter', 'kb.k.fixed.findEnter', ['Enter', 'Shift+Enter', 'ArrowDown', 'ArrowUp'], 'kb.why.a11y'),
  fixed('fixed.madoArrows', 'kb.k.fixed.madoArrows', ['ArrowUp', 'Alt+ArrowUp', 'Alt+Shift+ArrowUp', 'Enter'], 'kb.why.a11y'),
  fixed('fixed.edit', 'kb.k.fixed.edit', ['Mod+KeyZ', 'Mod+Shift+KeyZ', 'Mod+KeyX', 'Mod+KeyC', 'Mod+KeyV', 'Mod+KeyA'], 'kb.why.edit'),
  fixed('fixed.system', 'kb.k.fixed.system', ['Mod+KeyH', 'Mod+Alt+KeyH', 'Mod+KeyM'], 'kb.why.system', { mac: ['Mod+KeyH', 'Mod+Alt+KeyH', 'Mod+KeyM'], windows: ['Alt+F4'], linux: [] }),
  fixed('fixed.recovery', 'kb.k.fixed.recovery', ['Mod+KeyR'], 'kb.why.recovery'),
  fixed('fixed.capture', 'kb.k.fixed.capture', ['Enter', 'Escape', 'Mod+KeyZ'], 'kb.why.capture'),
  fixed('fixed.slides', 'kb.k.fixed.slides', ['ArrowRight', 'ArrowLeft', 'Space', 'PageDown', 'PageUp', 'Home', 'End', 'KeyO', 'KeyS', 'Escape'], 'kb.why.slides'),
];

// ---------- the user's own keys ----------

const RESERVED = RESERVED_JSON as { all: string[]; macos: string[]; windows: string[]; linux: string[]; recovery: string[]; keptDefaults: Record<string, string[]> };

/** The keys.json the layer loaded (null in safe mode / when there is none). Returns true when it
 *  differs from what was loaded before, so the caller can rebuild the native menu. */
export function setOverrides(file: unknown): boolean {
  const b = file && typeof file === 'object' && (file as any).bindings && typeof (file as any).bindings === 'object' ? (file as any).bindings : {};
  const clean: Record<string, string[]> = {};
  for (const [id, list] of Object.entries(b)) if (Array.isArray(list) && list.every((x) => typeof x === 'string')) clean[id] = (list as string[]).slice(0, 3);
  const text = JSON.stringify(clean);
  const changed = text !== keyState.loaded;
  keyState.loaded = text;
  keyState.overrides = clean;
  return changed;
}

/** Commands added by add-ons and the user's own themes, bindable like the app's own. */
function dynamicDefs(): KeyDef[] {
  const out: KeyDef[] = [];
  for (const c of app.commands ?? []) out.push({ id: 'plugin:' + c.id, label: '', title: String(c.title ?? c.id), group: G.addons, scope: 'app', defaults: [], editable: true });
  for (const th of app.customThemes ?? []) out.push({ id: 'theme.' + th.id, label: 'shell.pal.theme', args: { name: String(th.name) }, group: G.general, scope: 'app', defaults: [], editable: true });
  return out;
}

/** Every shortcut the app knows right now (the table plus add-on commands and custom themes). */
export function allKeys(): KeyDef[] {
  const seen = new Set(KEYS.map((k) => k.id));
  return [...KEYS, ...dynamicDefs().filter((k) => !seen.has(k.id) && (seen.add(k.id), true))];
}

export const find = (id: string): KeyDef | undefined => allKeys().find((k) => k.id === id);

export function currentOs(): Os { return osNow(); }

/** The defaults of `def` on this (or that) computer. */
export function defaultsOf(def: KeyDef, on: Os = currentOs()): Binding[] {
  return (on === 'macos' ? def.mac : on === 'windows' ? def.windows : def.linux) ?? def.defaults;
}

/** The keys a command has now: what the user set (`keys.json`), else its defaults. */
export function effective(id: string, on: Os = currentOs()): Binding[] {
  const def = find(id);
  if (!def) return [];
  const o = keyState.overrides[id];
  if (def.editable && def.native !== 'global' && Array.isArray(o)) return o.filter((b) => parse(b));
  return defaultsOf(def, on);
}

export const isChanged = (id: string): boolean => Array.isArray(keyState.overrides[id]) && !!find(id)?.editable;

// ---------- bindings ----------

const MODS = ['Mod', 'Control', 'Alt', 'Shift', 'Super'] as const;
type Mod = (typeof MODS)[number];
/** The `KeyboardEvent.code`s a binding may end in (the service accepts the same, keys.rs `CODE`). */
export const CODE = /^(Key[A-Z]|Digit[0-9]|F([1-9]|1[0-9]|2[0-4])|Numpad([0-9]|Add|Subtract|Multiply|Divide|Decimal|Enter|Equal)|Arrow(Up|Down|Left|Right)|Enter|Space|Backspace|Delete|Insert|Home|End|PageUp|PageDown|Escape|Tab|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash|Backquote|IntlBackslash)$/;

/** `{mods, code}` of a binding, or null when it is not one (a modifier twice, out of order, unknown key). */
export function parse(b: string): { mods: Mod[]; code: string } | null {
  if (typeof b !== 'string' || b.length > 60) return null;
  const parts = b.split('+');
  const code = parts.pop() ?? '';
  if (!CODE.test(code)) return null;
  let last = -1;
  for (const p of parts) {
    const i = MODS.indexOf(p as Mod);
    if (i <= last) return null;
    last = i;
  }
  return { mods: parts as Mod[], code };
}

/** The same binding written for comparison on `on`: on a Mac ⌘ is Mod (Super), elsewhere Ctrl is Mod (Control). */
export function norm(b: string, on: Os = currentOs()): string {
  const p = parse(b);
  if (!p) return '';
  const mods = new Set<Mod>(p.mods.map((m) => (on === 'macos' && m === 'Super' ? 'Mod' : on !== 'macos' && m === 'Control' ? 'Mod' : m)));
  return [...MODS.filter((m) => mods.has(m)), p.code].join('+');
}

const PUNCT: Record<string, string> = {
  ',': 'Comma', '.': 'Period', '/': 'Slash', ';': 'Semicolon', "'": 'Quote', '[': 'BracketLeft', ']': 'BracketRight', '\\': 'Backslash',
  '`': 'Backquote', '-': 'Minus', '=': 'Equal',
};
const MODIFIER_KEY = /^(Control|Shift|Alt|Meta|OS|Super|Hyper|Fn|AltGraph)(Left|Right)?$/;

/** The binding a key press makes, on `on`; null while only modifier keys are held. */
export function fromEvent(e: KeyboardEvent, on: Os = currentOs()): Binding | null {
  let code = e.code;
  if (!code) {
    const k = e.key ?? '';
    // (Some synthetic events carry no code: take it from the character, as the old table did.)
    code = /^[a-z]$/i.test(k) ? 'Key' + k.toUpperCase() : /^[0-9]$/.test(k) ? 'Digit' + k : k === ' ' ? 'Space' : PUNCT[k] ?? k;
  }
  if (!code || MODIFIER_KEY.test(code) || MODIFIER_KEY.test(e.key ?? '')) return null;
  const mac = on === 'macos';
  const mods: Mod[] = [];
  if (mac ? e.metaKey : e.ctrlKey) mods.push('Mod');
  if (mac && e.ctrlKey) mods.push('Control');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (!mac && e.metaKey) mods.push('Super');
  return [...mods, code].join('+');
}

/** No Mod, Control or Super (Shift and Alt alone type characters). */
export function bare(b: string): boolean {
  const p = parse(b);
  return !!p && !p.mods.some((m) => m === 'Mod' || m === 'Control' || m === 'Super');
}
const fkey = (b: string) => /^F\d+$/.test(parse(b)?.code ?? '');

export function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el?.closest?.('input, textarea, select, [contenteditable=""], [contenteditable="true"]');
}

/** While Settings › Keyboard records a key, no shortcut runs. */
export const recording = { on: false };

const overlaps = (a: Scope, b: Scope) => a === 'app' || a === 'native' || b === 'app' || b === 'native' || a === b || a === 'global' || b === 'global';
const inScope = (def: KeyDef, scope: Scope) => def.scope === scope || (scope === 'app' && def.scope === 'native' && !!def.web);

/** Which command a key press is in `scope` (`app` = the window as a whole), or null. App-wide keys with no
 *  modifier run only outside text fields. Esc is the fixed `escape` (closes what is on top). */
export function matchKey(e: KeyboardEvent, scope: Scope, on: Os = currentOs()): string | null {
  if (recording.on || e.isComposing || e.keyCode === 229) return null;
  const pressed = fromEvent(e, on);
  if (!pressed) return null;
  if (scope === 'app' && e.code === 'Escape' || scope === 'app' && !e.code && e.key === 'Escape') return e.metaKey || e.ctrlKey || e.altKey ? null : 'escape';
  const want = norm(pressed, on);
  for (const def of allKeys()) {
    if (!def.editable || !inScope(def, scope)) continue;
    for (const b of effective(def.id, on)) {
      if (norm(b, on) !== want) continue;
      if (scope === 'app' && bare(b) && !fkey(b) && isTyping(e.target)) continue;
      return def.id;
    }
  }
  return null;
}

// ---------- showing keys ----------

const NAMES: Record<string, [string, string]> = {
  NumpadAdd: ['Num +', 'Num +'], NumpadSubtract: ['Num -', 'Num -'], NumpadMultiply: ['Num *', 'Num *'], NumpadDivide: ['Num /', 'Num /'],
  NumpadDecimal: ['Num .', 'Num .'], NumpadEnter: ['Num ↩', 'Num Enter'], NumpadEqual: ['Num =', 'Num ='],
  Escape: ['Esc', 'Esc'], Tab: ['⇥', 'Tab'], Backspace: ['⌫', 'Backspace'], Delete: ['⌦', 'Delete'], Insert: ['Insert', 'Insert'],
  Home: ['Home', 'Home'], End: ['End', 'End'], IntlBackslash: ['§', '\\'],
};

/** "⌃⌥⇧⌘K" on a Mac, "Ctrl+Alt+Shift+K" elsewhere (quickKeys' way of writing keys). */
export function label(b: Binding, on: Os = currentOs()): string {
  const p = parse(b);
  if (!p) return '';
  const mods = new Set<string>();
  for (const m of p.mods) {
    if (m === 'Mod') mods.add(on === 'macos' ? 'Super' : 'Control');
    else mods.add(m);
  }
  const name = NAMES[p.code];
  const code = name ? '__' : p.code;
  let s = keysLabel([...mods, code].join('+'), on);
  if (name) s = s.replace('__', name[on === 'macos' ? 0 : 1]);
  return s;
}

/** Wrapped so the keys read in the right order inside Arabic text. */
export const iso = (s: string) => (s ? `⁦${s}⁩` : '');

/** The first key of a command, for tooltips and the palette ("⌘N"). Empty when it has none. */
export function keys(id: string): string {
  const def = find(id);
  if (!def) return '';
  const list = effective(id);
  const b = list.find((x) => !bare(x) || fkey(x)) ?? list[0];
  return b ? label(b) : '';
}

/** What `def` does, in the UI language. */
export function labelOf(def: KeyDef): string {
  if (def.title) return def.title;
  const args: Record<string, string | number> = {};
  for (const [k, v] of Object.entries(def.args ?? {})) args[k] = typeof v === 'string' && v.startsWith('@') ? t(v.slice(1)) : v;
  return t(def.label, args);
}

// ---------- checking a key the user picks ----------

const reservedOn = (on: Os): string[] => [...RESERVED.all, ...(on === 'macos' ? RESERVED.macos : on === 'windows' ? RESERVED.windows : RESERVED.linux)];

/** Why `b` cannot be given to `id`, as an i18n key, or '' when it can. The service refuses the same
 *  reserved keys (keys.rs); "needs a modifier" is the window's own rule (the service does not know the table). */
export function refusal(id: string, b: Binding, on: Os = currentOs()): string {
  const p = parse(b);
  if (!p) return 'kb.no.unknown';
  const n = norm(b, on);
  const kept = (RESERVED.keptDefaults[id] ?? []).some((k) => norm(k, on) === n);
  if (!kept) {
    if (RESERVED.recovery.some((r) => norm(r, on) === n)) return 'kb.no.recovery';
    if (reservedOn(on).some((r) => r === 'Super+*' ? p.mods.includes('Super') : norm(r, on) === n)) return 'kb.no.system';
  }
  const def = find(id);
  if (def && !def.plain && bare(b) && !fkey(b)) return 'kb.no.mod';
  return '';
}

/** Other commands that already use `b` where `id` could also be active. */
export function conflicts(id: string, b: Binding, on: Os = currentOs()): { id: string; label: string }[] {
  const def = find(id);
  if (!def) return [];
  const n = norm(b, on);
  const out: { id: string; label: string }[] = [];
  for (const other of allKeys()) {
    if (other.id === id || other.scope === 'fixed' || other.native === 'global' || !overlaps(def.scope, other.scope)) continue;
    if (effective(other.id, on).some((x) => norm(x, on) === n)) out.push({ id: other.id, label: labelOf(other) });
  }
  return out;
}

/** `keys.json`'s `bindings` with `id` set to `list`: an entry equal to the defaults is dropped. */
export function withBinding(id: string, list: Binding[] | null): Record<string, Binding[]> {
  const next: Record<string, Binding[]> = { ...$snapshotOverrides() };
  const def = find(id);
  if (!list || (def && same(list, defaultsOf(def)))) delete next[id];
  else next[id] = list;
  return next;
}
const same = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i]);
function $snapshotOverrides(): Record<string, Binding[]> {
  const o: Record<string, Binding[]> = {};
  for (const [k, v] of Object.entries(keyState.overrides)) o[k] = [...v];
  return o;
}

/** The ways a person may type a binding into a search box, each flattened (lower case, no spaces or
 *  `+`): "⌘k", "ctrlk", "cmdk", "commandk", "modkeyk"… A query matches a key when it flattens to one of them. */
export function searchText(b: Binding): string[] {
  const p = parse(b);
  if (!p) return [];
  const key = p.code.replace(/^(Key|Digit)/, '').toLowerCase();
  const names: Record<string, string[]> = { Mod: ['cmd', 'command', 'ctrl', 'control', '⌘'], Control: ['ctrl', 'control', '⌃'], Alt: ['alt', 'option', 'opt', '⌥'], Shift: ['shift', '⇧'], Super: ['win', 'super', 'cmd', '⌘'] };
  let forms = [''];
  for (const m of p.mods) forms = forms.flatMap((f) => names[m].map((n) => f + n));
  const flat = (x: string) => x.toLowerCase().replace(/[\s+\u2066-\u2069]/g, '');
  return [...new Set([...forms.map((f) => f + key), flat(label(b, 'macos')), flat(label(b, 'windows')), flat(label(b, 'linux')), flat(b)])];
}
export const flatQuery = (q: string) => q.toLowerCase().replace(/[\s+\u2066-\u2069]/g, '');

/** `aria-keyshortcuts` for a command's first key ("Meta+B"), or undefined when it has none. */
export function ariaKeys(id: string, on: Os = currentOs()): string | undefined {
  const p = parse(effective(id, on)[0] ?? '');
  if (!p) return undefined;
  const mods = p.mods.map((m) => (m === 'Mod' ? (on === 'macos' ? 'Meta' : 'Control') : m === 'Super' ? 'Meta' : m));
  const key = p.code.replace(/^(Key|Digit)/, '');
  return [...mods, key].join('+');
}
