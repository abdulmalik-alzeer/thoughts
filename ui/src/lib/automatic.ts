// Automatic tasks outside their settings card: telling the user when one fails or is skipped, and
// starting one by hand.
import { post } from './api';
import { app, go, toast } from './store.svelte';
import { t } from './i18n';
import { failed } from './errors';

/** Start a task once, now, as a chat the user can watch, and open that chat. `hash` is the content they were shown. */
export async function runTask(w: { file: string; hash: string }): Promise<boolean> {
  try {
    const r = await post<{ chatId: string }>('/workflows/run', { file: w.file, hash: w.hash });
    if (r?.chatId) go({ kind: 'chat', id: r.chatId });
    return true;
  } catch (e) {
    failed(t('auto.runFailed'), e);
    return false;
  }
}

const NOTICE: Record<string, string> = {
  failed: 'auto.notice.failed', timeLimit: 'auto.notice.timeLimit', spendLimit: 'auto.notice.spendLimit',
  interrupted: 'auto.notice.interrupted', notStarted: 'auto.notice.notStarted', skipped: 'auto.notice.skipped',
};

/** The daemon's `workflow.result` event: a task the user did not start by hand failed, hit a limit, or was skipped. */
export function workflowNotice(e: any, notify: (title: string, body: string) => void) {
  const key = NOTICE[e?.outcome];
  if (!key) return;
  const retry = !!e.retry && (e.outcome === 'failed' || e.outcome === 'notStarted');
  const text = t(retry ? key + 'Retry' : key, { name: String(e.name ?? '') });
  const action = e.outcome === 'skipped'
    ? { label: t('auto.runNow'), run: () => { runTask({ file: e.file, hash: e.hash }); } }
    : e.chatId
      ? { label: t('auto.openRun'), run: () => go({ kind: 'chat', id: e.chatId }) }
      : { label: t('auto.details'), run: () => go({ kind: 'settings', id: 'appLayer' }) };
  toast(text, { kind: e.outcome === 'skipped' ? 'info' : 'error', key: 'wf:' + e.file, duration: 0, action });
  if (app.settings?.notifications && !document.hasFocus()) notify(t('auto.notice.title'), text);
}
