// Changes waiting at the library's main backup location, from the user's other computers. The
// service only looks (when "Check for changes from your other computers" is on); this keeps what it
// last saw for the chip beside Sync and the line under the switch in Settings.
import { api, on } from './api';

export type Incoming = { on: boolean; waiting: number; state: 'off' | 'unknown' | 'ok' | 'failed'; why: string | null; checkedAt: string | null };

export const incoming = $state<Incoming>({ on: false, waiting: 0, state: 'off', why: null, checkedAt: null });

export async function refreshIncoming(): Promise<void> {
  try {
    const r = await api('/git/incoming');
    incoming.on = !!r?.on;
    incoming.waiting = Math.max(0, Number(r?.waiting) || 0);
    incoming.state = r?.state ?? 'off';
    incoming.why = r?.why ?? null;
    incoming.checkedAt = r?.checkedAt ?? null;
  } catch { /* an older service, or no library yet: nothing to show */ }
}

let watching = false;
/** Keep it fresh (once; lives as long as the window). */
export function watchIncoming(): void {
  if (watching) return;
  watching = true;
  refreshIncoming();
  on((e) => { if (['incoming.changed', 'settings.changed', 'library.switched', 'resync'].includes(e?.type)) refreshIncoming(); });
}
