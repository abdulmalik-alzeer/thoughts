// What a key does in the message box. Compact (the usual box): Enter sends, Shift+Enter makes a
// new line, ⌘/Ctrl+Enter also sends. Expanded (the tall box, for writing at length): Enter makes a
// new line and only ⌘/Ctrl+Enter sends; Esc makes the box small again, the text staying.
// Never while an input method is composing a word (lib/ime.ts).
import { imeKey } from '../ime';
import { isMac } from '../shortcuts';

export type ComposerAction = 'send' | 'newline' | 'collapse' | null;

/** ⌘ on a Mac, Ctrl elsewhere, held alone (no Alt). */
export const sendMod = (e: KeyboardEvent, mac = isMac): boolean => (mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey) && !e.altKey;

export function composerKey(e: KeyboardEvent, expanded: boolean, mac = isMac): ComposerAction {
  if (imeKey(e)) return null;
  if (e.key === 'Escape') return expanded && !e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey ? 'collapse' : null;
  if (e.key !== 'Enter') return null;
  if (sendMod(e, mac) && !e.shiftKey) return 'send';
  if (expanded) return 'newline';
  return e.shiftKey ? 'newline' : 'send';
}

/** The send keys as shown in the tall box's hint: "⌘↩" on a Mac, "Ctrl+Enter" elsewhere. */
export const sendKeys = (mac = isMac): string => (mac ? '⌘↩' : 'Ctrl+Enter');
