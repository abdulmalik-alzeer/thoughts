// Signing the assistant in (Claude Code or Codex), from wherever the app notices it is signed out.
import { api, post } from './api';
import { app, toast } from './store.svelte';
import { t } from './i18n';

export type Program = 'claude' | 'codex';

/** Open the program's browser sign-in (Claude Code unless Codex is named). The service says
 *  `auth.changed` when it is over. */
export async function signIn(program: Program | unknown = 'claude') {
  const codex = program === 'codex';
  try { await post(codex ? '/ai/codex/login' : '/ai/claude/login'); toast(t(codex ? 'codex.set.signInToast' : 'set.loginToast'), { key: 'auth' }); }
  catch (e: any) { toast(t(codex ? 'codex.set.signInFailed' : 'ui.set.loginFailed'), { kind: 'error', key: 'auth', detail: e?.message }); }
}

/** Ask the program afresh, forgetting what the last failed run said. Returns whether it is signed in now. */
export async function recheckSignIn(program: Program = 'claude'): Promise<boolean> {
  const now = await post(program === 'codex' ? '/ai/codex/recheck' : '/ai/claude/recheck');
  if (app.status) app.status = { ...app.status, [program]: now };
  else app.status = await api('/status');
  return !!now?.loggedIn;
}
