// App updates (the native updater in the shell) and updates to the shipped customisation defaults.
import { api, inTauri, on, post } from './api';
import { go, toast } from './store.svelte';
import { confirmDialog } from './dialog.svelte';
import { t, tn } from './i18n';
import { checkPending } from './layer';
import { withBusyPrompt } from './shell';
import { offerDownload } from './maintenance';

let checking = false;

/** Ask the shell whether a new version exists. `manual` (menu / button) also reports "nothing new". */
export async function checkAppUpdate(manual = false) {
  // Without the built-in updater (a browser preview, a build without an updater key, a Linux
  // package) a check asked for by the user looks up the latest published version instead.
  if (!inTauri()) { if (manual) await checkRelease('layer.upd.desktopOnly'); return; }
  if (checking) return;
  checking = true;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    let r: any;
    try { r = await invoke('update_check'); }
    catch (e: any) { if (manual) toast(t('layer.upd.checkFailed'), { kind: 'error', detail: String(e?.message ?? e) }); return; }
    if (!r?.configured) { if (manual) await checkRelease(r?.reason === 'deb' ? 'ui.upd.deb' : r?.reason === 'build' ? 'ui.upd.build' : 'layer.upd.notConfigured'); return; }
    if (!r.available) { if (manual) toast(t('layer.upd.none'), { kind: 'success', key: 'upd' }); return; }
    const ok = await confirmDialog({
      title: t('layer.upd.available', { version: r.version ?? '' }),
      body: (typeof r.notes === 'string' && r.notes.trim()) || t('layer.upd.body'),
      note: t('layer.upd.note'), confirmLabel: t('layer.upd.install'), cancelLabel: t('layer.later'),
    });
    if (!ok) return;
    toast(t('layer.upd.installing'), { key: 'upd', duration: 0 });
    try {
      const done = await withBusyPrompt((force) => invoke('update_install', { force }), 'update');
      if (!done) toast(t('ui.busy.waiting'), { key: 'upd' });
    } catch (e: any) { toast(t('layer.upd.installFailed'), { kind: 'error', key: 'upd', detail: String(e?.message ?? e) }); }
  } finally { checking = false; }
}

/** Ask the background service for the latest published version (one request to GitHub, see
 *  docs/PRIVACY.md). When it cannot tell, say what the build says about updates (`fallback`). */
async function checkRelease(fallback: string) {
  let r: any = null;
  try { r = await post('/release-check'); } catch { /* older background service, or no connection */ }
  if (r?.state === 'newer' && r.version && r.url) { await offerDownload(r.version, r.url); return; }
  if (r?.state === 'current') { toast(t('layer.upd.none'), { kind: 'success', key: 'upd' }); return; }
  toast(r?.state === 'unknown' ? `${t(fallback)} ${t('care.upd.unknown')}` : t(fallback), { key: 'upd' });
}

const seen = {
  get: () => { try { return localStorage.getItem('thoughts.updSeen'); } catch { return null; } },
  set: (v: string) => { try { localStorage.setItem('thoughts.updSeen', v); } catch { /* ignore */ } },
};

/** After an app update: say what was refreshed automatically, and point to the choices left about
 *  files changed on both sides. Those are made in Settings › Customisations › App updates
 *  (UpdateChoices.svelte), all on one screen, where each says what the new version changes and
 *  "Keep mine" can be revisited. */
export async function checkLayerUpdates() {
  let s: any;
  try { s = await api('/updates/status'); } catch { return; } // older background service
  const applied: string[] = Array.isArray(s?.applied) ? s.applied : [];
  const key = `${s?.from ?? ''}>${s?.to ?? ''}`;
  if (seen.get() !== key) {
    // An older app version over newer defaults changes nothing; defaults dropped upstream are kept as the user's own.
    if (s?.downgrade) toast(t('ui.upd.downgrade'), { key: 'upd-down' });
    const removed: string[] = Array.isArray(s?.removed) ? s.removed : [];
    if (removed.length) toast(t('ui.upd.removed'), { key: 'upd-removed', detail: removed.join('\n') });
    if ((s?.downgrade || removed.length) && !applied.length) seen.set(key);
  }
  if (applied.length && seen.get() !== key) {
    seen.set(key);
    toast(tn('layer.upd.applied', applied.length), { key: 'upd-applied', detail: applied.join('\n') });
  }
  // A conflict whose merge is being prepared or waits for review is answered by keeping that review.
  const open = (Array.isArray(s?.conflicts) ? s.conflicts : []).filter((c: any) => c && typeof c.path === 'string' && !c.merging);
  if (open.length) {
    toast(tn('ldb.upd.toast', open.length), { key: 'upd-choose', duration: 20000, action: { label: t('ldb.upd.choose'), run: () => go({ kind: 'settings', id: 'appLayer' }) } });
  }
  emitUpdates(s);
}

// ---------- the choices left by an app update (UpdateChoices.svelte) ----------

export type UpdateQuestion = { path: string; from?: string; to?: string; merging?: boolean; changes?: { area: string; op: string; [k: string]: any }[] | null };
type Listener = (s: any) => void;
const listeners = new Set<Listener>();
function emitUpdates(s: any) { for (const l of listeners) l(s); }
/** Be told whenever the update status was read again. */
export function onUpdates(l: Listener): () => void { listeners.add(l); return () => { listeners.delete(l); }; }

/** The current status: `{conflicts, kept, …}` (each with what the new version changes). */
export async function layerUpdateStatus(): Promise<any> {
  const s = await api('/updates/status');
  emitUpdates(s);
  return s;
}

/** Answer one question: keep mine, use the new version, or let the assistant merge (a review follows). */
export async function resolveLayerUpdate(path: string, choice: 'keep' | 'new' | 'merge'): Promise<void> {
  const r = await post('/updates/resolve', { path, choice });
  if (r?.status) emitUpdates(r.status);
  if (choice !== 'merge') return;
  toast(t('layer.upd.merging'), { key: 'upd', action: r?.chatId ? { label: t('layer.watch'), run: () => go({ kind: 'chat', id: r.chatId }) } : undefined });
  checkPending(false);
  // The question stays open until the merged version is kept. If the assistant ends without
  // leaving anything to review, the question is simply open again: read the status anew.
  if (r?.chatId) {
    const off = on((e) => {
      if (e.type !== 'chat.done' || e.chatId !== r.chatId) return;
      off();
      setTimeout(() => { layerUpdateStatus().catch(() => {}); }, 1500);
    });
  }
}
