// The user's yes, asked and proven by the desktop app itself (src-tauri/src/consent.rs,
// crates/thoughtsd/src/consent.rs).
//
// Switching a chat to "bypass permissions", making that the default, and trusting a folder are only
// done by the service with an approval from the desktop shell, made with a key this window never
// sees. The shell asks the user in a native dialog of its own before it makes one, so that dialog IS
// the confirmation: do not ask "are you sure?" in the page as well. Ask once per request: each
// approval is good for that one action, once, for two minutes. Outside the desktop app (the browser
// preview) there is none, and the service refuses those changes.

import { isSecondary, showInMain } from './windows';

export type ConsentAction = 'chat-bypass' | 'settings-bypass' | 'trust';

/** The approval, `null` when the user said no in the shell's dialog (or one was already open: say
 *  nothing then either), `undefined` outside the desktop app. Throws only for a real problem. */
export async function approval(action: ConsentAction, subject = ''): Promise<string | null | undefined> {
  if (typeof (window as any).__TAURI_INTERNALS__ === 'undefined') return undefined;
  // The desktop app asks only in the main window (src-tauri/src/windows.rs): a second window brings
  // it forward, at the chat in question, and says so.
  if (isSecondary()) {
    await showInMain(action === 'chat-bypass' && subject ? { kind: 'chat', id: subject } : undefined);
    const { t } = await import('./i18n');
    throw new Error(t(action === 'trust' ? 'chatfWindow.trustInMain' : 'chatfWindow.confirmInMain'));
  }
  const { invoke } = await import('@tauri-apps/api/core');
  try { return await invoke<string>('approve_action', { action, subject }); }
  catch (e: any) {
    const msg = String(e?.message ?? e);
    if (msg.startsWith('DECLINED::')) return null;
    throw new Error(msg);
  }
}
