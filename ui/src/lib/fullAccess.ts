// Full access (Settings › Privacy & cost › Full access; crates/thoughtsd/src/full_access.rs).
//
// While it is on the app adds no limits of its own: every assistant run bypasses permissions, the
// customisation layer is not guarded, add-ons get every permission and pages get the machine
// bridge (`machine.*` in lib/layer.ts, crates/thoughtsd/src/machine.rs). Everything that matters is
// checked by the service on the saved settings; what is here only decides what the page shows and
// what the add-on host hands on.
//
// Switching it ON is the one guarded step: the desktop shell asks in its own two-step native
// warning (`approve_action('full-access')`) and only its approval lets the service switch it on.
// Switching it OFF is one request (`?fullAccess=off`) and needs nothing.

import { app, saveSettings } from './store.svelte';
import { approval } from './consent';

/** Is full access on (as the saved settings say)? */
export const fullAccessOn = (): boolean => !!app.settings?.fullAccess?.on;

export type SwitchResult = 'on' | 'off' | 'declined' | 'unavailable';

/** Switch full access on (after the shell's warning) or off. */
export async function setFullAccess(on: boolean): Promise<SwitchResult> {
  if (!on) {
    await saveSettings({}, '?fullAccess=off');
    return 'off';
  }
  const a = await approval('full-access');
  if (a === null) return 'declined';
  if (a === undefined) return 'unavailable';
  await saveSettings({}, `?confirm=full-access&approval=${encodeURIComponent(a)}`);
  return fullAccessOn() ? 'on' : 'declined';
}

/** Name the folder of the app's own source code (empty: none). */
export async function setSourceDir(dir: string): Promise<void> {
  await saveSettings((now) => ({ ...now, fullAccess: { ...(now?.fullAccess ?? {}), sourceDir: dir.trim() || null } }));
}
