// The main backup location holds a copy of the library that this computer's copy was not made from
// (the library folder was copied here without its history). A sync changes nothing then: the person
// chooses, once, how to put the two together (lib/SyncJoin.svelte, mounted once in App). Whichever
// way, nothing is deleted and the background service keeps a backup of this computer's copy first.
import { api, post } from './api';
import { toast } from './store.svelte';
import { syncDone, syncFailed } from './backup.svelte';
import { t, tn } from './i18n';

export type How = 'combine' | 'theirs';

export const join = $state<{ open: boolean; name: string; how: How; busy: boolean }>({ open: false, name: '', how: 'combine', busy: false });

let finished: (() => void) | null = null;

/** The name of the main location, for the dialog's sentences. */
async function mainName(): Promise<string> {
  try {
    const g = await api('/git');
    const rs: any[] = Array.isArray(g?.remotes) ? g.remotes : [];
    return String((rs.find((r) => r?.main) ?? rs[0])?.name ?? '');
  } catch { return ''; }
}

function close(): void {
  join.open = false;
  join.busy = false;
  const f = finished;
  finished = null;
  f?.();
}

/** Ask how to put the two copies together, and do it. Resolves once the dialog is closed. */
export async function settleJoin(name = ''): Promise<void> {
  if (join.open) return;
  const where = name || (await mainName());
  return new Promise((done) => {
    finished = done;
    join.name = where;
    join.how = 'combine';
    join.busy = false;
    join.open = true;
  });
}

/** "Not now": nothing changes, and the next Sync asks again. */
export function cancelJoin(): void {
  if (join.busy) return;
  close();
  toast(t('data-sync.join.cancelled'), { key: 'sync' });
}

/** Put the two copies together the chosen way, then report it like a Sync. */
export async function confirmJoin(): Promise<void> {
  if (join.busy) return;
  join.busy = true;
  const name = join.name;
  try {
    const r = await post('/git/join', { how: join.how });
    close();
    syncDone(r, t('data-sync.join.done', { name }));
    const copies = Array.isArray(r?.copies) ? r.copies.length : 0;
    if (copies) toast(tn('data-sync.join.copies', copies), { key: 'sync-join' });
  } catch (e) {
    close();
    await syncFailed(e, t('data-sync.join.failed'));
  }
}
