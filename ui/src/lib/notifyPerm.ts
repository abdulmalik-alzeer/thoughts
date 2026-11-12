// Whether the computer lets Thoughts show notifications — the Settings switch can be on while the
// system says no, and reminders then stay silent. Asking is only ever done from a click.
import { inTauri } from './api';

/** `granted`: notifications appear · `blocked`: the system does not allow them (or was never asked)
 *  · `unknown`: this window cannot tell (browser preview, no notification support). */
export type NotifState = 'granted' | 'blocked' | 'unknown';

export async function notifState(): Promise<NotifState> {
  try {
    if (inTauri()) {
      const n = await import('@tauri-apps/plugin-notification');
      return (await n.isPermissionGranted()) ? 'granted' : 'blocked';
    }
    if (typeof Notification !== 'undefined') return Notification.permission === 'granted' ? 'granted' : Notification.permission === 'denied' ? 'blocked' : 'unknown';
  } catch { /* not available */ }
  return 'unknown';
}

/** Ask the system (shows its prompt the first time; afterwards it answers from its own settings). */
export async function askNotify(): Promise<NotifState> {
  try {
    if (inTauri()) {
      const n = await import('@tauri-apps/plugin-notification');
      if (await n.isPermissionGranted()) return 'granted';
      return (await n.requestPermission()) === 'granted' ? 'granted' : 'blocked';
    }
    if (typeof Notification !== 'undefined') {
      const p = Notification.permission === 'default' ? await Notification.requestPermission() : Notification.permission;
      return p === 'granted' ? 'granted' : p === 'denied' ? 'blocked' : 'unknown';
    }
  } catch { /* not available */ }
  return 'unknown';
}

/** The system's own notification settings, where Thoughts can be allowed: on macOS and Windows,
 *  in the desktop app. The desktop app lets the window open exactly these two addresses. */
export function notifySettingsUrl(): string | null {
  if (!inTauri()) return null;
  const os = (window as any).__THOUGHTS_PLATFORM__;
  return os === 'macos' ? 'x-apple.systempreferences:com.apple.preference.notifications' : os === 'windows' ? 'ms-settings:notifications' : null;
}

/** Open the system's notification settings. Resolves false when that could not be done. */
export async function openNotifySettings(): Promise<boolean> {
  const url = notifySettingsUrl();
  if (!url) return false;
  try { await (await import('@tauri-apps/plugin-opener')).openUrl(url); return true; } catch { return false; }
}

/** Send one notification now (asking first if needed). Resolves the permission it found. */
export async function testNotify(title: string, body: string): Promise<NotifState> {
  const st = await askNotify();
  if (st !== 'granted') return st;
  try {
    if (inTauri()) { (await import('@tauri-apps/plugin-notification')).sendNotification({ title, body }); }
    else new Notification(title, { body });
  } catch { return 'unknown'; }
  return 'granted';
}
