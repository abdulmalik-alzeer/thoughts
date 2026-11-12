// Two things Settings → Privacy offers beyond the list of private folders: closing the library
// folder to other accounts on this computer, and taking a newly private folder's items out of the index.
import { api, post } from './api';
import { confirmDialog } from './dialog.svelte';
import { failed } from './errors';
import { fmtNum, t, tn } from './i18n';
import { toast } from './store.svelte';

/** Can other accounts on this computer open the library folder? null when that cannot be told (or does not apply). */
export async function libraryOpenToOthers(): Promise<boolean | null> {
  try { const r = await api('/library/access'); return typeof r?.openToOthers === 'boolean' ? r.openToOthers : null; }
  catch { return null; }
}

/** Close the library folder to every account but this one. Resolves what is true afterwards. */
export async function keepLibraryToMe(): Promise<boolean | null> {
  try {
    const r = await post('/library/access');
    if (r?.openToOthers === false) toast(t('seca.access.done'));
    return typeof r?.openToOthers === 'boolean' ? r.openToOthers : null;
  } catch (e) { failed(t('seca.access.failed'), e); return true; }
}

/** After a folder was marked private (and saved): if the index still has lines about its items,
 *  say so and offer to remove them. The assistant reads the index in most chats. */
export async function offerIndexCleanup(folder: string): Promise<void> {
  let found: { count: number; titles: string[] };
  try { found = await api(`/library/private-index?path=${encodeURIComponent(folder)}`); } catch { return; }
  if (!found?.count) return;
  const more = found.count - found.titles.length;
  const yes = await confirmDialog({
    title: t('seca.index.title'), body: tn('seca.index.body', found.count),
    list: more > 0 ? [...found.titles, t('seca.index.more', { n: fmtNum(more) })] : found.titles,
    note: t('seca.index.note'), confirmLabel: t('seca.index.remove'), cancelLabel: t('seca.index.keep'),
  });
  if (!yes) return;
  try { const r = await post('/library/private-index', { path: folder }); toast(tn('seca.index.done', r?.removed ?? 0)); }
  catch (e) { failed(t('seca.index.failed'), e); }
}
