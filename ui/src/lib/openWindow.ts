// The "Open in new window" action of Docs and Chat (lib/windows.ts does the work).
import { toast } from './store.svelte';
import { t } from './i18n';
import { openInNewWindow, type Route } from './windows';

export async function openWindow(v: Route): Promise<void> {
  try { await openInNewWindow(v); }
  catch (e: any) { toast(t('chatfWindow.failed'), { kind: 'error', detail: String(e?.message ?? e) }); }
}
