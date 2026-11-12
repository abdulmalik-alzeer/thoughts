// Changes in progress besides the one open for review. Only one change is open at a time (the one
// the review shows and Keep applies); a new request, an imported pack or an update merge sets the
// open one aside instead of replacing it, and the user can come back to any of them. The service
// keeps them (`/api/layer/drafts`); this module keeps the list fresh and says when one was set aside.
//
// Imported by Review.svelte, which App.svelte always imports: so the listener below is in place from
// the start, whichever screen started the request that set a change aside.
import { api, on, post } from './api';
import { app, go, toast } from './store.svelte';
import { t } from './i18n';
import { checkPending, openReview, stopPreview } from './layer';
import { checkLayerUpdates } from './updates';

export type Draft = { id: string; title: string; origin: string; setAside: string; created: string; chatId?: string | null; resolves?: string };

export const drafts = $state({ list: [] as Draft[], loaded: false });

let seq = 0;
/** Read the list again (quietly keeps the old one when the service cannot be reached). */
export async function refreshDrafts(): Promise<Draft[]> {
  const mine = ++seq;
  try {
    const r = await api('/layer/drafts');
    if (mine === seq) { drafts.list = Array.isArray(r) ? r.filter((x: any) => x && typeof x.id === 'string') : []; drafts.loaded = true; }
  } catch { /* an older service, or no connection */ }
  return drafts.list;
}

/** A plain title for a waiting change: its own, or where it came from. */
export function draftTitle(d: Draft): string {
  return d.title?.trim() || t(d.origin === 'import' ? 'ldb.untitled.import' : d.origin === 'update' ? 'ldb.untitled.update' : d.origin === 'manual' ? 'lda.origin.manual' : 'ldb.untitled');
}

/** Open a waiting change for review. The one that was open waits in its place. */
export async function openDraft(id: string) {
  await post('/layer/drafts/open', { id });
  // What was being tried belonged to the change that is now set aside.
  if (app.preview) await stopPreview().catch(() => {});
  await checkPending(false);
  openReview();
  await refreshDrafts();
}

/** Throw away a waiting change without opening it (the caller has asked the user first). */
export async function discardDraft(id: string) {
  const r = await post('/layer/drafts/discard', { id });
  await refreshDrafts();
  // A discarded merge of an app update leaves its question open.
  if (r?.origin === 'update') checkLayerUpdates();
}

const settings = () => go({ kind: 'settings', id: 'appLayer' });

on((e) => {
  if (e?.type === 'layer.pending' || e?.type === 'layer.changed') refreshDrafts();
  if (e?.type === 'layer.drafts' && typeof e.id === 'string') {
    refreshDrafts();
    const title = typeof e.setAside === 'string' && e.setAside.trim() ? e.setAside.trim() : t('ldb.untitled');
    toast(t('ldb.setAside', { title }), { key: 'ldb-aside', action: { label: t('ldb.setAsideShow'), run: settings } });
  }
});
