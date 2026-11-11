// Signing the assistant in to Claude, from wherever the app notices it is signed out.
import { api, post } from './api';
import { app, toast } from './store.svelte';
import { t } from './i18n';

/** Open Claude Code's browser sign-in. The service says `auth.changed` when it is over. */
export async function signIn() {
  try { await post('/ai/claude/login'); toast(t('set.loginToast'), { key: 'auth' }); }
  catch (e: any) { toast(t('ui.set.loginFailed'), { kind: 'error', key: 'auth', detail: e?.message }); }
}

/** Ask the CLI afresh, forgetting what the last failed run said. Returns whether it is signed in now. */
export async function recheckSignIn(): Promise<boolean> {
  const claude = await post('/ai/claude/recheck');
  if (app.status) app.status = { ...app.status, claude };
  else app.status = await api('/status');
  return !!claude?.loggedIn;
}
