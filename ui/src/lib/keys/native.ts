// The keyboard shortcuts and the desktop shell: saving the user's set (`PUT /api/layer/keys`), and
// asking the shell to rebuild its menu whenever the set changed (a save, undo, rollback, a kept
// change), since the menu's keys (Settings, Close, Quit, Full Screen) come from the same `keys.json`
// (src-tauri `build_menu` through crates/thoughtsd/src/keys.rs `native`).
import { put, inTauri } from '../api';
import { setOverrides, type Binding } from './registry';

async function invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  if (!inTauri()) return undefined;
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke(cmd, args);
}

/** The layer loaded (`m.keys`, or null in safe mode): take its keys, and rebuild the menu if they changed. */
export function keysLoaded(file: unknown) {
  if (setOverrides(file)) invoke('refresh_menu').catch(() => {});
}

/** Save the whole set (an empty one removes `keys.json`: "Reset all"). One History entry. */
export async function saveKeys(bindings: Record<string, Binding[]>): Promise<{ hash?: string }> {
  const r = await put<{ hash?: string }>('/layer/keys', { bindings });
  invoke('refresh_menu').catch(() => {});
  return r ?? {};
}

/** While a key is being recorded the menu has no keys of its own, so the combination reaches the page;
 *  the shell puts them back by itself after a minute. The quick capture key is paused the same way. */
export function bareMenu(on: boolean) {
  invoke('refresh_menu', { bare: on }).catch(() => {});
  invoke('pause_capture_shortcut', { paused: on }).catch(() => {});
}
