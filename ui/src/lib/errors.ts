// Turning a failed call into a toast: the headline is always a plain, localised sentence about what
// the user was doing; whatever the background service said (already in the app language) is the detail.
import { ApiError, codeOf } from './api';
import { go, toast, type ToastOpts } from './store.svelte';
import { t } from './i18n';

const msgOf = (e: unknown) => (e instanceof Error ? e.message : String(e ?? ''));

/** `BUSY`, `CONFLICT`, `NO_HISTORY`, `WRAP_INTERRUPTED`, `CHANGED`, `NEWER_SETTINGS`… or ''. */
export function errCode(e: unknown): string {
  return (e instanceof ApiError && e.code) || codeOf(msgOf(e));
}

/** The message without its machine prefix (`CONFLICT::a;b` → `a;b`). */
export function errDetail(e: unknown): string {
  return msgOf(e).replace(/(^|\s)[A-Z][A-Z_]{2,}::/, '$1').trim();
}

/** Show an error toast for a failed action. `headline` is `t('…')` text for that action.
 *  A few errors mean the same thing everywhere and get their own explanation (and a way forward). */
export function failed(headline: string, e: unknown, opts: ToastOpts = {}): void {
  const code = errCode(e);
  const detail = errDetail(e);
  if (code === 'WRAP_INTERRUPTED') {
    const chatId = detail.split(/\s/)[0];
    toast(t('err.wrapInterrupted'), { kind: 'error', key: 'wrap-interrupted', duration: 0, action: chatId ? { label: t('err.openWrap'), run: () => go({ kind: 'chat', id: chatId }) } : undefined });
    return;
  }
  if (code === 'NO_HISTORY') {
    toast(t('err.noHistory'), { kind: 'error', key: 'no-history', action: { label: t('settings'), run: () => go({ kind: 'settings', id: 'git' }) } });
    return;
  }
  if (code === 'NEWER_SETTINGS') { toast(t('err.newerSettings'), { kind: 'error', key: 'newer-settings' }); return; }
  toast(headline, { kind: 'error', detail: detail && detail !== headline ? detail : undefined, ...opts });
}
