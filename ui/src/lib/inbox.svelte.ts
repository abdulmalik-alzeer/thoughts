// How many captures wait in the library's inbox (Home, the sidebar and the Inbox page share it).
// Captures from the phone arrive through the cloud folder without the app being told, so the count
// is read again when the window comes back to the front, once a minute, and whenever the library changes.
import { api, on } from './api';

export const inbox = $state({ count: 0 });

export async function refreshInbox(): Promise<void> {
  try {
    const r = await api('/inbox?count=1');
    inbox.count = Math.max(0, Number(r?.count) || 0);
  } catch { /* an older service, or no library yet: nothing to show */ }
}

let watching = false;
/** Start keeping the count fresh (once; lives as long as the window). */
export function watchInbox(): void {
  if (watching) return;
  watching = true;
  refreshInbox();
  on((e) => { if (e.type === 'library.changed' || e.type === 'resync') refreshInbox(); });
  window.addEventListener('focus', () => { refreshInbox(); });
  setInterval(() => { if (!document.hidden) refreshInbox(); }, 60_000);
}
