// Key combinations for the quick capture shortcut. The desktop app stores them one way
// ("Control+Alt+Super+Space": modifiers in that order, then the key's physical name, as
// `KeyboardEvent.code` gives it) and these helpers turn that into what the user sees.
import { t } from './i18n';

export type Os = 'macos' | 'windows' | 'linux';

/** Which computer the window runs on (the desktop app tells the page before it starts). */
export function os(): Os {
  const p = (window as any).__THOUGHTS_PLATFORM__;
  if (p === 'macos' || p === 'windows' || p === 'linux') return p;
  const n = navigator.platform || navigator.userAgent;
  return /Mac/i.test(n) ? 'macos' : /Win/i.test(n) ? 'windows' : 'linux';
}

const ORDER = ['Control', 'Alt', 'Shift', 'Super'] as const;
const MAC: Record<string, string> = { Control: '⌃', Alt: '⌥', Shift: '⇧', Super: '⌘' };
const SYMBOLS: Record<string, string> = {
  Backquote: '`', Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']', Backslash: '\\', Semicolon: ';', Quote: "'",
  Comma: ',', Period: '.', Slash: '/', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→', PageUp: 'Page Up', PageDown: 'Page Down',
};

/** The key's name as printed on it. */
export function keyLabel(code: string, on: Os = os()): string {
  if (code === 'Space') return t('nativeb.key.space');
  if (code === 'Enter') return on === 'macos' ? '↩' : 'Enter';
  const m = /^(?:Key([A-Z])|Digit(\d))$/.exec(code);
  if (m) return m[1] ?? m[2];
  const n = /^Numpad(\d)$/.exec(code);
  if (n) return 'Num ' + n[1];
  return SYMBOLS[code] ?? code;
}

/** "⌃⌥⌘Space" on a Mac, "Ctrl+Alt+Space" elsewhere. */
export function keysLabel(keys: string, on: Os = os()): string {
  const parts = keys.split('+');
  const key = parts.pop() ?? '';
  const mods = ORDER.filter((m) => parts.includes(m));
  if (on === 'macos') return mods.map((m) => MAC[m]).join('') + keyLabel(key, on);
  const names: Record<string, string> = { Control: 'Ctrl', Alt: 'Alt', Shift: 'Shift', Super: on === 'windows' ? 'Win' : 'Super' };
  return [...mods.map((m) => names[m]), keyLabel(key, on)].join('+');
}

/** Wrapped so the keys read in the right order inside Arabic text. */
export const isolated = (s: string) => `⁦${s}⁩`;

/** The combination a key press makes, written the way the desktop app stores it; null while only
 *  modifier keys are held. */
export function keysFromEvent(e: KeyboardEvent): string | null {
  if (!e.code || /^(Control|Shift|Alt|Meta|OS|Super|Hyper|Fn)(Left|Right)?$/.test(e.code) || ['Control', 'Shift', 'Alt', 'Meta', 'OS', 'AltGraph', 'Fn'].includes(e.key)) return null;
  const mods = [e.ctrlKey && 'Control', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super'].filter(Boolean) as string[];
  return [...mods, e.code].join('+');
}

/** The modifier keys a shortcut needs two of, named for this computer. */
export const modsText = (on: Os = os()) => t(on === 'macos' ? 'nativeb.mods.mac' : on === 'windows' ? 'nativeb.mods.windows' : 'nativeb.mods.linux');
