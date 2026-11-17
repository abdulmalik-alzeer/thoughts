// Which assistant program the app starts its own runs on (Settings › Assistant: Claude Code or
// Codex), and whether that one is ready to work. A library's waiting setup runs on this program
// (`assistant::for_run` in the service), so this is what "connected" means for it.

export type Program = 'claude' | 'codex';

/** The program chosen in Settings (anything else, or nothing yet, is Claude Code). */
export function primaryOf(settings: any): Program {
  return settings?.ai?.primary === 'codex' ? 'codex' : 'claude';
}

/** The status of `program`, as `/status` reports it. */
export function statusOf(status: any, program: Program): any {
  return (program === 'codex' ? status?.codex : status?.claude) ?? {};
}

/** The program is installed and can answer: signed in, or (Claude Code) paid with a key of its own.
 *  Codex that cannot tell whether it is signed in is let through, as the service does before a run. */
export function isReady(st: any, program: Program): boolean {
  if (!st?.installed) return false;
  if (program === 'codex') return !!st.loggedIn || st.known === false;
  return !!st.loggedIn || st.provider === 'anthropicKey' || st.provider === 'service';
}

/** Installed but signed out: the app's own sign-in banner already says what to do (App.svelte's rule). */
export function isSignedOut(st: any, program: Program): boolean {
  return st?.installed === true && st.loggedIn === false && (program !== 'codex' || st.known !== false);
}

/** The name of the program as people know it (the same in both languages). */
export const programName = (p: Program) => (p === 'codex' ? 'Codex' : 'Claude');
