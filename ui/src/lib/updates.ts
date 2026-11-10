// App updates (the native updater in the shell) and updates to the shipped customisation defaults.
import { api, inTauri, post } from './api';
import { app, go, toast } from './store.svelte';
import { choiceDialog, confirmDialog } from './dialog.svelte';
import { t, tn } from './i18n';
import { checkPending } from './layer';
import { withBusyPrompt } from './shell';

let checking = false;

/** Ask the shell whether a new version exists. `manual` (menu / button) also reports "nothing new". */
export async function checkAppUpdate(manual = false) {
  if (!inTauri()) { if (manual) toast(t('layer.upd.desktopOnly')); return; }
  if (checking) return;
  checking = true;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    let r: any;
    try { r = await invoke('update_check'); }
    catch (e: any) { if (manual) toast(t('layer.upd.checkFailed'), { kind: 'error', detail: String(e?.message ?? e) }); return; }
    if (!r?.configured) { if (manual) toast(t(r?.reason === 'deb' ? 'ui.upd.deb' : r?.reason === 'build' ? 'ui.upd.build' : 'layer.upd.notConfigured'), { key: 'upd' }); return; }
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

const seen = {
  get: () => { try { return localStorage.getItem('thoughts.updSeen'); } catch { return null; } },
  set: (v: string) => { try { localStorage.setItem('thoughts.updSeen', v); } catch { /* ignore */ } },
};

/** After an app update: say what was refreshed automatically, and ask about files changed on both sides. */
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
  for (const c of (Array.isArray(s?.conflicts) ? s.conflicts : []) as { path: string }[]) {
    const choice = await choiceDialog({
      title: t('layer.upd.conflictTitle'), body: t('layer.upd.conflictBody'), list: [c.path], note: t('layer.upd.conflictNote'),
      choices: [
        { id: 'merge', label: t('layer.upd.merge') },
        { id: 'new', label: t('layer.upd.useNew') },
        { id: 'keep', label: t('layer.upd.keepMine'), primary: true },
      ],
    });
    if (!choice) return; // asked again next start
    try {
      const r = await post('/updates/resolve', { path: c.path, choice });
      if (choice === 'merge') {
        toast(t('layer.upd.merging'), { key: 'upd', action: r?.chatId ? { label: t('layer.watch'), run: () => go({ kind: 'chat', id: r.chatId }) } : undefined });
        checkPending(false);
      }
    } catch (e: any) { toast(t('layer.upd.resolveFailed'), { kind: 'error', detail: e?.message ?? String(e) }); return; }
  }
  void app;
}
