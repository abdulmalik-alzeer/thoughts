// A failed change to a todo, told in plain words. When the service refuses because the todo is no
// longer what the window showed (changed or moved elsewhere meanwhile), the user hears that nothing
// was changed — and an edit's typed text is shown in the toast, with its Copy button, so it is not lost.
import { errCode, failed } from './errors';
import { toast } from './store.svelte';
import { t } from './i18n';

/** The service's "this todo changed in the meantime" refusal (`stale.todo`). */
export const isStaleTodo = (e: unknown) => errCode(e).toLowerCase() === 'stale';

export function todoFailed(headline: string, e: unknown, kept?: string): void {
  if (isStaleTodo(e)) {
    toast(t(kept ? 'ui-todos.staleKept' : 'ui-todos.stale'), { kind: 'error', key: 'todo-stale', detail: kept || undefined });
    return;
  }
  failed(headline, e);
}

/** An open edit box closed because its todo changed or disappeared elsewhere: keep the typed text visible. */
export function editLost(kept: string): void {
  toast(t('ui-todos.gone'), { kind: 'error', key: 'todo-stale', detail: kept });
}
