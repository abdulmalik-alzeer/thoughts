// Looking at the built product's screens (docs/mado-spec.md, round 0.4.0). The desktop shell opens each
// address in a hidden window of its own that has no access to the app (src-tauri/src/screen_check.rs)
// and hands back what each screen showed. Everything in the answer is untrusted text from the project:
// show it only as text, never as markup.
//
// `null` means nobody can look here: the browser preview (no desktop shell), or a second app window
// (only the main window may ask the shell). Errors thrown carry a code as their message:
// `screen-check-bad-url` (not http://localhost:<port> or http://127.0.0.1:<port>, or one of the app's own
// ports), `screen-check-busy` (another check is running), `screen-check-too-many` (over 200 screens).
import type { ScreenResult } from './types';

export async function screenCheck(url: string, screens: { id: string; path: string }[]): Promise<ScreenResult[] | null> {
  if (typeof (window as any).__TAURI_INTERNALS__ === 'undefined') return null;
  const { isSecondary } = await import('../windows');
  if (isSecondary()) return null;
  const { invoke } = await import('@tauri-apps/api/core');
  const plain = screens.map((s) => ({ id: String(s.id), path: String(s.path) }));
  try {
    const out = await invoke<ScreenResult[]>('mado_screen_check', { url, screens: plain });
    return Array.isArray(out) ? out : [];
  } catch (e: any) {
    throw new Error(String(e?.message ?? e));
  }
}
