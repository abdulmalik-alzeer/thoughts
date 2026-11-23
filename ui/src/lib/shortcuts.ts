// The app's keyboard shortcuts now live in one registry, lib/keys/registry.ts (every key, its
// defaults, the user's own keys from the layer's `keys.json`, and the keys that stay fixed). This
// file keeps the old names working for the code that imports them.
//
// Command ids (also the `detail.id` of the window CustomEvent `thoughts:command`):
//   nav.<view>            home | chats | ideas | todos | docs | projects | me
//   chat.new              start a new chat            chat.open   { chatId }
//   doc.open { path }     open a library file         search.focus (the sidebar search box)
//   find                  find in the open note or chat (falls back to search.focus)
//   wrap.start            sync            rollback    settings    help    tour    print
//   palette               sidebar.toggle  theme.<id>  mode.<light|dark|system>
//   back / forward        through the views visited this session   history (notes history)
//   zoom.in / zoom.out / zoom.reset   size of everything in the window (Settings › Accessibility)
//   chat.composer.expand  the open chat's message box made tall, or small again (views/Chat.svelte)
//   slides.present        present the open deck (lib/slides/)
//   plugin:<command id>   a command registered by a plugin was run
import { matchKey, keys as keysOf, iso } from './keys/registry';

export { isMac, isTyping, KEYS, matchKey, effective, label, conflicts, recording } from './keys/registry';

/** The command id for a key press anywhere in the window, or null (`matchKey(e, 'app')`). */
export function matchShortcut(e: KeyboardEvent): string | null { return matchKey(e, 'app'); }

/** Human-readable keys for a command, e.g. "⌘N" on a Mac and "Ctrl+N" elsewhere. Empty when it has none. */
export function keys(id: string): string { return keysOf(id); }

/** The keys for a command, isolated so they read correctly inside Arabic text. Empty when it has none. */
export function isolatedKeys(id: string): string { return iso(keysOf(id)); }

/** "Label (⌘N)" for tooltips. The keys are isolated so they read correctly inside Arabic text. */
export function tip(label: string, id: string): string {
  const k = keysOf(id);
  return k ? `${label} (${iso(k)})` : label;
}
