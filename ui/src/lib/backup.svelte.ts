// What the app knows about the library's online backup, and how the outcome of a Sync is said.
// A Sync is only ever reported as done when the service says every online location has the
// library; anything else is shown as a failure that names the location and the reason.
import { api, ApiError } from './api';
import { go, toast } from './store.svelte';
import { errCode, errDetail, errTech, failed } from './errors';
import { ago, fmtDate, lang, t } from './i18n';

type Last = { at: string; location: string } | null;
/** One location as the service sees it (`git::backup_health`). */
export type LocationHealth = { name: string; kind: 'online' | 'ssh' | 'folder'; main: boolean; at: string | null; tried: string | null;
  whyKey: string | null; why: string | null; unsent: number; present: boolean | null; state: string };
export type Health = { state: 'ok' | 'waiting' | 'stale' | 'failed' | 'none'; staleDays: number; locations: LocationHealth[] };

/** The last time the library really reached a location (`null`: never, or not known yet), and how
 *  the backup stands per location (`null`: not known, or the library is not sent anywhere). */
export const backup = $state<{ last: Last; health: Health | null }>({ last: null, health: null });

/** The locations that took the library, quoted and listed the way the app language does it. */
const names = (r: any): string =>
  (Array.isArray(r?.locations) ? r.locations : []).filter((l: any) => l?.sent)
    .map((l: any) => (lang() === 'ar' ? `«${l.name}»` : `“${l.name}”`)).join(lang() === 'ar' ? '، ' : ', ');

/** Ask the service where things stand (after start, and whenever the backup settings change). */
export async function refreshBackup(): Promise<void> {
  try { const g = await api('/git'); backup.last = g?.lastBackup ?? null; backup.health = g?.health ?? null; } catch { /* shown where it matters */ }
}

/** A warning about the backup worth showing on the Sync button, or '' when all is well. */
export function healthWarning(h: Health | null = backup.health): string {
  if (!h) return '';
  if (h.state === 'failed') return t('histb.tip.failed');
  if (h.state === 'none') return t('histb.tip.never');
  if (h.state === 'stale') return h.locations.some((l) => !l.at) ? t('histb.tip.never') : t('histb.tip.stale', { days: h.staleDays });
  return '';
}

/** Where a failed backup is put right: the sign-in of a location, a server to check, or the list. */
function fixFor(h: Health | null): { label: string; path: string } {
  const why = (h?.locations ?? []).map((l) => l.whyKey);
  if (why.includes('sync.why.sign_in')) return { label: t('histb.toast.updateSignIn'), path: 'signin' };
  if (why.includes('sync.why.host_key')) return { label: t('histb.toast.checkServer'), path: 'hostkey' };
  return { label: t('histb.toast.locations'), path: 'locations' };
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
  refreshBackup();
  const lines = [r?.received ? t('hista.sync.received') : '', names(r) ? t('hista.sync.sent', { names: names(r) }) : ''].filter(Boolean);
  toast(headline, { kind: 'success', key: 'sync', detail: lines.join('\n') || undefined });
}

/** A Sync that did not finish: nowhere to send the library, or a location that did not take it. */
export async function syncFailed(e: unknown, headline: string): Promise<void> {
  if (errCode(e) === 'no_location' || errCode(e) === 'sync_failed' || errCode(e) === 'sign_in') {
    // What happened at each location (and what did arrive) is worth knowing; it also says where
    // the problem is put right — the toast's button goes straight there.
    if (e instanceof ApiError) await refreshBackup();
    const fix = errCode(e) === 'no_location' ? { label: t('settings'), path: 'locations' } : errCode(e) === 'sign_in' ? { label: t('histb.toast.updateSignIn'), path: 'signin' } : fixFor(backup.health);
    toast(headline, { kind: 'error', key: 'sync', detail: errDetail(e) || undefined, tech: errTech(e), duration: 0, action: { label: fix.label, run: () => go({ kind: 'settings', id: 'git', path: fix.path }) } });
    return;
  }
  failed(headline, e, { key: 'sync' });
}

/** A short name for a drive location: the folder's own name where it can be one ("My-Passport"),
 *  else "drive", "drive-2"… (names are plain letters, digits, dots and dashes). */
export function driveName(path: string, taken: Set<string>): string {
  const base = (path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? '').replace(/\s+/g, '-').replace(/[^A-Za-z0-9._-]/g, '').replace(/^[-.]+/, '').slice(0, 30);
  if (base && !taken.has(base)) return base;
  for (let n = 1; ; n++) { const name = n === 1 ? 'drive' : `drive-${n}`; if (!taken.has(name)) return name; }
}

