// The user's yes, proven by the desktop app itself (src-tauri/src/consent.rs, crates/thoughtsd/src/consent.rs).
//
// Switching a chat to "bypass permissions", making that the default, and trusting a folder are only
// done by the service with an approval from the desktop shell, made with a key this window never
// sees. Ask for one right after the user agreed to the warning, once per request: each approval is
// good for that one action, once, for two minutes. Outside the desktop app (the browser preview)
// there is none, and the service refuses those changes.

export type ConsentAction = 'chat-bypass' | 'settings-bypass' | 'trust';

export async function approval(action: ConsentAction, subject = ''): Promise<string | undefined> {
  if (typeof (window as any).__TAURI_INTERNALS__ === 'undefined') return undefined;
  const { invoke } = await import('@tauri-apps/api/core');
  try { return await invoke<string>('approve_action', { action, subject }); }
  catch (e: any) { throw new Error(String(e?.message ?? e)); }
}
