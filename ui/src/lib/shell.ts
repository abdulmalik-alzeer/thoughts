// Things the native shell refuses to do while work is in progress (restart the background service,
// install an update): it answers `BUSY::{"saving":bool,"chats":[title,…]}` and we ask the user.
import { choiceDialog } from './dialog.svelte';
import { reconnect } from './api';
import { toast } from './store.svelte';
import { t } from './i18n';

export type Work = { saving?: boolean; chats?: string[] };

/** Parses `BUSY::{json}` out of an error; null when the error is something else. */
export function busyWork(e: unknown): Work | null {
  const m = /BUSY::\s*(\{.*\})\s*$/s.exec(e instanceof Error ? e.message : String(e ?? ''));
  if (!m) return null;
  try { const w = JSON.parse(m[1]); return { saving: !!w.saving, chats: Array.isArray(w.chats) ? w.chats.map(String).slice(0, 8) : [] }; }
  catch { return {}; }
}

/** Says what is still running and asks: wait, or do it anyway? Resolves true for "do it anyway". */
export async function askBusy(work: Work, action: 'restart' | 'update'): Promise<boolean> {
  const list = [...(work.saving ? [t('ui.busy.saving')] : []), ...(work.chats ?? []).map((c) => t('ui.busy.chat', { title: c }))];
  const choice = await choiceDialog({
    title: t('ui.busy.title'), body: t(action === 'update' ? 'ui.busy.bodyUpdate' : 'ui.busy.bodyRestart'),
    list: list.length ? list : undefined, note: t('ui.busy.note'), cancelLabel: t('ui.busy.wait'),
    choices: [{ id: 'force', label: t('ui.busy.anyway'), danger: true }],
  });
  return choice === 'force';
}

/** Runs `fn(false)`; when the shell says it is busy, asks the user and — only if they insist — runs `fn(true)`.
 *  Resolves false when the user chose to wait. Other errors are rethrown. */
export async function withBusyPrompt(fn: (force: boolean) => Promise<unknown>, action: 'restart' | 'update'): Promise<boolean> {
  try { await fn(false); return true; }
  catch (e) {
    const work = busyWork(e);
    if (!work) throw e;
    if (!(await askBusy(work, action))) return false;
    await fn(true);
    return true;
  }
}

/** Restart the background service, asking first when something is still running. */
export async function restartService(): Promise<boolean> {
  try {
    const done = await withBusyPrompt((force) => reconnect(force), 'restart');
    if (done) toast(t('set.restarted'), { kind: 'success', key: 'restart' });
    return done;
  } catch (e: any) { toast(t('ui.restartFailed'), { kind: 'error', detail: e?.message ?? String(e) }); return false; }
}
