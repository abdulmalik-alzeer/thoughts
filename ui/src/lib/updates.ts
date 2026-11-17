// App updates (the native updater in the shell) and updates to the shipped customisation defaults.
import { api, inTauri, on, post } from './api';
import { app, go, toast } from './store.svelte';
import { choiceDialog } from './dialog.svelte';
import { t, tn } from './i18n';
import { checkPending } from './layer';
import { withBusyPrompt } from './shell';
import { offerDownload, openDownload } from './maintenance';
import { download } from './updateProgress.svelte';

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
    // A check that could not tell (no connection, a release that is not public, the GitHub
    // fallback not ready): said only when the person asked.
    if (r.problem) { if (manual) sayProblem(String(r.problem), typeof r.detail === 'string' ? r.detail : ''); return; }
    if (!r.available) { if (manual) toast(t('layer.upd.none'), { kind: 'success', key: 'upd' }); return; }
    // The dialog speaks in the app's own words only. The notes in the update's description are
    // not signed and nothing has been checked yet, so they are never shown here: the new version
    // shows its own (Help › What's new) once it runs, and its release page opens in the browser,
    // as GitHub's page, for anyone who wants to read about it first.
    const page = typeof r.page === 'string' ? r.page : '';
    for (;;) {
      const choice = await choiceDialog({
        title: t('layer.upd.available', { version: r.version ?? '' }),
        body: [t('layer.upd.body'), t('sec-shell-updater.whatsNew'), page ? t('sec-shell-updater.pageHint') : ''].filter(Boolean).join('\n\n'),
        note: t('layer.upd.note'), cancelLabel: t('layer.later'),
        choices: [
          ...(page ? [{ id: 'page', label: t('sec-shell-updater.page') }] : []),
          { id: 'install', label: t('layer.upd.install'), primary: true },
        ],
      });
      if (choice === 'page') { await openDownload(page); continue; }
      if (choice !== 'install') return;
      break;
    }
    // The download shows as one bar in Settings › Maintenance, which is opened for it.
    app.toasts = app.toasts.filter((x) => x.key !== 'upd');
    Object.assign(download, { active: true, got: 0, total: 0 });
    go({ kind: 'settings', id: 'maintenance' });
    const stopProgress = await showProgress();
    try {
      const done = await withBusyPrompt((force) => invoke('update_install', { force }), 'update');
      if (!done) toast(t('ui.busy.waiting'), { key: 'upd' });
    } catch (e: any) {
      const said = String(e?.message ?? e);
      const problem = /^PROBLEM::([a-z-]+)::([\s\S]*)$/.exec(said);
      if (problem) sayProblem(problem[1], problem[2]);
      else toast(t('layer.upd.installFailed'), { kind: 'error', key: 'upd', detail: said });
    } finally { stopProgress(); download.active = false; }
  } finally { checking = false; }
}

/** One plain sentence for a check or an install that could not tell or could not finish. The
 *  codes are the shell's (`find_update` in main.rs and `ghfeed::Problem`). */
function sayProblem(code: string, detail = '') {
  const key = `care.upd.p.${code}`;
  const sentence = t(key);
  // Nothing published, or nothing published yet: news, not a failure.
  const calm = code === 'unpublished' || code === 'gh-no-release';
  toast(sentence === key ? t('layer.upd.checkFailed') : sentence, { key: 'upd', kind: calm ? undefined : 'error', detail: detail || undefined });
}

/** While an update downloads, keep how far it is (the shell reports each piece as it arrives) for
 *  the bar in Settings › Maintenance. Returns what stops the reports. */
async function showProgress(): Promise<() => void> {
  try {
    const { listen } = await import('@tauri-apps/api/event');
    return await listen<{ downloaded?: number; total?: number | null }>('update-progress', (e) => {
      const got = Number(e.payload?.downloaded ?? 0), total = Number(e.payload?.total ?? 0);
      if (got > 0) download.got = got;
      if (total > 0) download.total = total;
    });
  } catch { return () => {}; }
}

const DAY = 24 * 3600_000;
const lastAuto = {
  get: () => { try { return Number(localStorage.getItem('thoughts.updAutoAt')) || 0; } catch { return 0; } },
  set: (at: number) => { try { localStorage.setItem('thoughts.updAutoAt', String(at)); } catch { /* ignore */ } },
};

/** Is an automatic check due? At most one a day, however often the app is opened; a clock that
 *  went back does not stop them for good. */
export function autoCheckDue(last: number, now: number): boolean {
  return !last || now - last >= DAY || last > now;
}

let auto: ReturnType<typeof setInterval> | null = null;
/** Checks nobody asked for happen only while "Check for new versions automatically" is on
 *  (Settings › Maintenance, off unless switched on), and then at most once a day: the first a
 *  couple of minutes after the app opened, if a day has passed since the last one. With the
 *  setting off the app never asks about updates by itself. The setting is read each time, so
 *  switching it takes effect without a restart. */
export function startAutoUpdateChecks() {
  if (auto) return;
  const look = () => {
    if (!app.settings?.releaseCheck || !autoCheckDue(lastAuto.get(), Date.now())) return;
    lastAuto.set(Date.now());
    checkAppUpdate(false);
  };
  setTimeout(look, 2 * 60_000);
  auto = setInterval(look, 3600_000);
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
    toast(tn('ldb.upd.toast', open.length), { key: 'upd-choose', duration: 20000, action: { label: t('ldb.upd.choose'), run: () => go({ kind: 'customise', id: 'updates' }) } });
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
