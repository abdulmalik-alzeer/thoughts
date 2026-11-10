// Every keyboard shortcut in one place. App.svelte listens for keys, matches them here and runs
// the command; tooltips and the Help page read the same table so they can never drift apart.
//
// Command ids (also the `detail.id` of the window CustomEvent `thoughts:command`):
//   nav.<view>            home | chats | ideas | todos | docs | projects | me
//   chat.new              start a new chat            chat.open   { chatId }
//   doc.open { path }     open a library file         search.focus
//   wrap.start            sync            rollback    settings    help    tour    print
//   palette               sidebar.toggle  theme.<id>  mode.<light|dark|system>
//   plugin:<command id>   a command registered by a plugin was run

export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);

export type Shortcut = {
  /** Command id, or `nav.N` (N = position in the sidebar) / `escape`. */
  id: string;
  /** i18n key describing it (Help page). */
  label: string;
  /** Physical key (layout-independent, so it also works on an Arabic keyboard). */
  code: string;
  /** Character shown to the user. */
  key: string;
  mod?: boolean;
  shift?: boolean;
  /** Only when the user is not typing in a field. */
  plain?: boolean;
  /** Listed in Help? */
  hidden?: boolean;
};

export const shortcuts: Shortcut[] = [
  { id: 'palette', label: 'shell.sc.palette', code: 'KeyK', key: 'K', mod: true },
  { id: 'chat.new', label: 'shell.sc.newChat', code: 'KeyN', key: 'N', mod: true },
  { id: 'search.focus', label: 'shell.sc.search', code: 'KeyF', key: 'F', mod: true },
  { id: 'search.focus', label: 'shell.sc.searchSlash', code: 'Slash', key: '/', plain: true },
  ...[1, 2, 3, 4, 5, 6].map((n) => ({ id: 'nav.' + n, label: 'shell.sc.nav', code: 'Digit' + n, key: String(n), mod: true, hidden: n > 1 })),
  { id: 'wrap.start', label: 'shell.sc.wrap', code: 'KeyW', key: 'W', mod: true, shift: true },
  { id: 'sidebar.toggle', label: 'shell.sc.sidebar', code: 'Backslash', key: '\\', mod: true },
  { id: 'settings', label: 'shell.sc.settings', code: 'Comma', key: ',', mod: true },
  { id: 'escape', label: 'shell.sc.escape', code: 'Escape', key: 'Esc' },
];

export function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el?.closest?.('input, textarea, select, [contenteditable=""], [contenteditable="true"]');
}

/** The command id for a key press, or null. */
export function matchShortcut(e: KeyboardEvent): string | null {
  if (e.isComposing) return null;
  const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  for (const s of shortcuts) {
    if (e.code !== s.code && !(e.code === '' && e.key.toUpperCase() === s.key.toUpperCase())) continue;
    if (s.id === 'escape') return e.metaKey || e.ctrlKey || e.altKey ? null : 'escape';
    if (e.altKey) continue;
    if (s.plain) { if (!mod && !e.metaKey && !e.ctrlKey && !e.shiftKey && !isTyping(e.target)) return s.id; continue; }
    if (!!s.mod === mod && !!s.shift === e.shiftKey) return s.id;
  }
  return null;
}

/** Human-readable keys for a command, e.g. "⌘N" on a Mac and "Ctrl+N" elsewhere. Empty when it has none. */
export function keys(idOrShortcut: string | Shortcut): string {
  const s = typeof idOrShortcut === 'string' ? shortcuts.find((x) => x.id === idOrShortcut && !x.plain) : idOrShortcut;
  if (!s) return '';
  if (isMac) return (s.mod ? '⌘' : '') + (s.shift ? '⇧' : '') + s.key;
  return [s.mod ? 'Ctrl' : '', s.shift ? 'Shift' : '', s.key].filter(Boolean).join('+');
}

/** "Label (⌘N)" for tooltips. The keys are isolated so they read correctly inside Arabic text. */
export function tip(label: string, id: string): string {
  const k = keys(id);
  return k ? `${label} (⁦${k}⁩)` : label;
}
