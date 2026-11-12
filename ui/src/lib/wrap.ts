// What a person can do with a Wrap outside its plan: finish, undo or keep one that was cut off while
// saving (offered by the banner and on the Wrap's own page), and undo one that was saved.
import { api, post } from './api';
import { confirmDialog } from './dialog.svelte';
import { errCode, failed } from './errors';
import { go, refreshChats, refreshWrap, toast } from './store.svelte';
import { lang, t, tn } from './i18n';

const names = (paths: string[]) => paths.join(lang() === 'ar' ? '، ' : ', ');
const refresh = () => { refreshChats().catch(() => {}); refreshWrap().catch(() => {}); };

/** Let the assistant save the rest of a cut-off Wrap, and show it working. */
export async function finishWrap(id: string): Promise<boolean> {
  try { await post(`/wrap/${id}/apply`, { note: '' }); refresh(); go({ kind: 'chat', id }); return true; }
  catch (e) { failed(t('wrapa.finishFailed'), e); return false; }
}

/** Undo the part of a cut-off Wrap that reached the library — after showing exactly what goes back,
 *  and what is left alone because it changed afterwards. */
export async function undoCutOffWrap(id: string): Promise<boolean> {
  let p: any;
  try { p = await api(`/wrap/${id}/undo-preview`); }
  catch (e) { failed(t('wrapa.undoFailed'), e); return false; }
  const files: { path: string; change: string }[] = Array.isArray(p?.files) ? p.files : [];
  const kept: string[] = Array.isArray(p?.kept) ? p.kept : [];
  const sure = !!p?.known && !!p?.precise;
  const body = !p?.known ? t('wrapa.undo.bodyUnknown') : !p.precise ? t('wrapa.undo.bodyAll') : files.length ? t('wrapa.undo.body') : t('wrapa.undo.bodyNone');
  const ok = await confirmDialog({
    title: t('wrapa.undo.title'), body,
    list: files.map((f) => t('wrapa.undo.' + (['added', 'deleted'].includes(f.change) ? f.change : 'edited'), { path: f.path })),
    note: kept.length ? tn('wrapa.undo.kept', kept.length, { names: names(kept) }) : undefined,
    confirmLabel: t('wrapa.undo.confirm'), danger: !sure,
  });
  if (!ok) return false;
  try {
    const r = await post(`/wrap/${id}/rollback`);
    const left = Array.isArray(r?.kept) ? r.kept.length : 0;
    toast(left ? tn('wrapa.undoneKept', left) : t('wrapa.undone'), { kind: 'success', key: 'wrap' });
    refresh();
    return true;
  } catch (e) { failed(t('wrapa.undoFailed'), e); return false; }
}

/** Leave the library as the cut-off Wrap left it. Its chats stay unsaved. */
export async function keepCutOffWrap(id: string): Promise<boolean> {
  if (!(await confirmDialog({ title: t('wrapa.keep.title'), body: t('wrapa.keep.body'), confirmLabel: t('wrapa.keep.confirm') }))) return false;
  try { await post(`/wrap/${id}/keep`); toast(t('wrapa.kept'), { kind: 'success', key: 'wrap' }); refresh(); return true; }
  catch (e) { failed(t('wrapa.keepFailed'), e); return false; }
}

/** Drop a plan that was not approved. */
export async function discardPlan(id: string): Promise<boolean> {
  try { await post(`/wrap/${id}/discard`); toast(t('wrapa.discarded'), { key: 'wrap' }); refresh(); return true; }
  catch (e) { failed(t('wrapa.discardFailed'), e); return false; }
}

/** Undo a Wrap that was saved: its documents go back (a new history entry) and its chats wait again. */
export async function undoSavedWrap(id: string): Promise<boolean> {
  if (!(await confirmDialog({ title: t('wrapa.undoWrap.title'), body: t('wrapa.undoWrap.body'), confirmLabel: t('wrapa.undo.confirm') }))) return false;
  try { await post(`/wrap/${id}/undo`); toast(t('wrapa.wrapUndone'), { kind: 'success', key: 'wrap' }); refresh(); return true; }
  catch (e) {
    // Later changes build on it: say so, and point at the way that still works.
    if (errCode(e) === 'CONFLICT') toast(t('wrapa.undoClash'), { kind: 'error', key: 'wrap', action: { label: t('ui.hist.title'), run: () => go({ kind: 'history' }) } });
    else failed(t('wrapa.undoWrapFailed'), e);
    return false;
  }
}
