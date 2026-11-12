// What the app knows about the library's online backup, and how the outcome of a Sync is said.
// A Sync is only ever reported as done when the service says every online location has the
// library; anything else is shown as a failure that names the location and the reason.
import { api, ApiError } from './api';
import { go, toast } from './store.svelte';
import { errCode, errDetail, failed } from './errors';
import { ago, fmtDate, lang, t } from './i18n';

type Last = { at: string; location: string } | null;

/** The last time the library really reached an online location (`null`: never, or not known yet). */
export const backup = $state<{ last: Last }>({ last: null });

/** The locations that took the library, quoted and listed the way the app language does it. */
const names = (r: any): string =>
  (Array.isArray(r?.locations) ? r.locations : []).filter((l: any) => l?.sent)
    .map((l: any) => (lang() === 'ar' ? `«${l.name}»` : `“${l.name}”`)).join(lang() === 'ar' ? '، ' : ', ');

/** Ask the service where things stand (after start, and whenever the backup settings change). */
export async function refreshBackup(): Promise<void> {
  try { backup.last = (await api('/git'))?.lastBackup ?? null; } catch { /* shown where it matters */ }
}

/** “Last backed up 2 hours ago to “origin””, or '' when the library has never been backed up. */
export function lastBackupText(last: Last = backup.last): string {
  if (!last?.at) return '';
  const old = Date.now() - new Date(last.at).getTime() > 6 * 24 * 3600 * 1000;
  return t('hista.sync.last', { when: old ? fmtDate(last.at, { dateStyle: 'medium' }) : ago(last.at), name: last.location });
}

/** The answer of a Sync (or of settling a clash) that reached every location. */
export function syncDone(r: any, headline: string): void {
  if (r?.lastBackup) backup.last = r.lastBackup;
  const lines = [r?.received ? t('hista.sync.received') : '', names(r) ? t('hista.sync.sent', { names: names(r) }) : ''].filter(Boolean);
  toast(headline, { kind: 'success', key: 'sync', detail: lines.join('\n') || undefined });
}

/** A Sync that did not finish: nowhere to send the library, or a location that did not take it. */
export function syncFailed(e: unknown, headline: string): void {
  const toSettings = { label: t('settings'), run: () => go({ kind: 'settings', id: 'git' }) };
  if (errCode(e) === 'no_location' || errCode(e) === 'sync_failed' || errCode(e) === 'sign_in') {
    toast(headline, { kind: 'error', key: 'sync', detail: errDetail(e) || undefined, duration: 0, action: toSettings });
    // What did arrive is still worth knowing about.
    if (e instanceof ApiError && errCode(e) === 'sync_failed') refreshBackup();
    return;
  }
  failed(headline, e, { key: 'sync' });
}
