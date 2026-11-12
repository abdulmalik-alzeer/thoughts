// Settings › Maintenance, outside the Settings page: the check-up run when the app opens, and the
// "a newer version exists" notice.
import { api, daemonStale, on } from './api';
import { app, go, toast } from './store.svelte';
import { confirmDialog } from './dialog.svelte';
import { t, tn } from './i18n';

export type Issue = { id: string; fix?: string; n?: number };

/** What the check-up finds. An older background service kept because it was busy is known only here. */
export async function healthIssues(): Promise<Issue[]> {
  const r = await api('/health');
  const list: Issue[] = Array.isArray(r?.issues) ? r.issues : [];
  if (daemonStale()) list.push({ id: 'old-service', fix: 'restart-service' });
  return list;
}

/** Open a release page. Only the app's own download page on GitHub is ever opened from here. */
export async function openDownload(url: string) {
  if (!/^https:\/\/github\.com\/[\w.-]+\/[\w.-]+\/releases\//.test(url)) return;
  try { const { openUrl } = await import('@tauri-apps/plugin-opener'); await openUrl(url); }
  catch { window.open(url, '_blank', 'noopener'); }
}

/** A newer version exists and this copy cannot install it: offer its download page. */
export async function offerDownload(version: string, url: string) {
  const ok = await confirmDialog({ title: t('care.upd.available', { version }), body: t('care.upd.body'), confirmLabel: t('care.upd.open'), cancelLabel: t('layer.later') });
  if (ok) await openDownload(url);
}

const seenKey = 'thoughts.newerSeen';
/** Say once per version that a newer one exists (from the weekly check or an earlier manual one). */
function announceNewer(v: { version?: string; url?: string } | null | undefined) {
  if (!v?.version || !v.url) return;
  let seen: string | null = null;
  try { seen = localStorage.getItem(seenKey); } catch { /* private window */ }
  if (seen === v.version) return;
  try { localStorage.setItem(seenKey, v.version); } catch { /* ignore */ }
  const url = v.url;
  toast(t('care.upd.toast', { version: v.version }), { key: 'newer', duration: 15000, action: { label: t('care.upd.open'), run: () => { openDownload(url); } } });
}

let started = false;
/** Run once the app is up: the check-up (one notice for everything found) and the newer-version notice. */
export async function startupChecks() {
  if (started) return;
  started = true;
  on((e) => { if (e?.type === 'release.newer') announceNewer(e); });
  announceNewer(app.status?.newerVersion);
  try {
    // A missing library already has a screen of its own.
    const list = (await healthIssues()).filter((i) => i.id !== 'library-missing');
    if (list.length) {
      toast(tn('care.health.toast', list.length), {
        kind: 'error', key: 'health', duration: 0,
        action: { label: t('care.health.review'), run: () => { go({ kind: 'settings', id: 'maintenance' }); } },
      });
    }
  } catch { /* an older background service without the check-up */ }
}
