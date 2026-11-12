// A sync that found the same documents changed here and on another device: the person chooses,
// per document, to keep theirs, the other device's, or both (lib/SyncConflict.svelte, mounted once
// in App). A backup of this computer's version is always kept by the background service.
import { post } from './api';
import { toast } from './store.svelte';
import { syncDone, syncFailed } from './backup.svelte';
import { t, tn } from './i18n';

export type Choice = 'mine' | 'theirs' | 'both';

export const clash = $state<{ files: string[]; choices: Record<string, Choice>; answer: ((v: Record<string, Choice> | null) => void) | null }>({
  files: [], choices: {}, answer: null,
});

/** Show the dialog for `files` (earlier answers kept). Resolves the choices, or null when cancelled. */
export function askClash(files: string[], earlier: Record<string, Choice> = {}): Promise<Record<string, Choice> | null> {
  closeClash(null);
  return new Promise((answer) => {
    clash.files = files;
    clash.choices = Object.fromEntries(files.map((f) => [f, earlier[f] ?? 'mine']));
    clash.answer = answer;
  });
}

export function closeClash(v: Record<string, Choice> | null): void {
  const a = clash.answer;
  clash.answer = null;
  clash.files = [];
  a?.(v);
}

const listOf = (detail: string) => detail.split(';').map((f) => f.trim()).filter(Boolean);

/** Ask about each clashing document, then finish the sync with those choices. When more documents
 *  clash than were first reported, the person is asked again about all of them (answers kept). */
export async function settleClash(files: string[]): Promise<void> {
  let earlier: Record<string, Choice> = {};
  for (let round = 0; round < 4; round++) {
    const choices = await askClash(files, earlier);
    if (!choices) { toast(t('shell.conflict.cancelled')); return; }
    try {
      const r = await post('/git/resolve', { choices });
      const picked = Object.values(choices);
      const copies = Array.isArray(r?.settled) ? r.settled.filter((s: any) => s?.copy).length : 0;
      const headline = copies ? tn('histc.clash.doneCopies', copies)
        : picked.every((c) => c === 'mine') ? t('shell.conflict.keptMine')
        : picked.every((c) => c === 'theirs') ? t('shell.conflict.usedTheirs')
        : t('histc.clash.done');
      syncDone(r, headline);
      return;
    } catch (e: any) {
      const m = /CONFLICT::(.*)/s.exec(e?.message || '');
      if (m && listOf(m[1]).length) { files = listOf(m[1]); earlier = choices; continue; }
      syncFailed(e, t('shell.conflict.failed'));
      return;
    }
  }
  toast(t('shell.conflict.failed'), { kind: 'error' });
}
