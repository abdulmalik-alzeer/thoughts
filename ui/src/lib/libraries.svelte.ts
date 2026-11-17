// Several libraries (FEAT-32): the list the switcher shows, and switching, naming, adding and
// taking libraries off the list. Switching reloads the window, so nothing of the library left
// behind (an open document, a cached list, a search) stays on screen; work still going on there
// carries on in that library.
import { api, del, on, patch, post, reloadApp, setLibraryId } from './api';
import { app, go, toast } from './store.svelte';
import { t, tn } from './i18n';
import { confirmDialog, promptDialog } from './dialog.svelte';
import { failed } from './errors';
import { unsavedLost } from './unsaved';
import type { PaletteItem } from './CommandPalette.svelte';

export type Lib = { id: string; name: string; displayName: string; path: string; open: boolean; missing: boolean; running: number; waiting: number };

export const libs = $state({ list: [] as Lib[], openId: '', loaded: false, adding: false, busy: false });

/** What the window says once it has reloaded into another library (kept for that one reload). */
const NOTE_KEY = 'thoughts.libs.note';
const ss = {
  get(k: string) { try { return sessionStorage.getItem(k); } catch { return null; } },
  set(k: string, v: string) { try { sessionStorage.setItem(k, v); } catch { /* private mode */ } },
  drop(k: string) { try { sessionStorage.removeItem(k); } catch { /* private mode */ } },
};

export const openLib = () => libs.list.find((l) => l.open);

export async function loadLibraries() {
  try {
    const r = await api<{ libraries: Lib[]; openId: string | null }>('/libraries');
    // Another window opened another library and this one waits to reload (see `switchedElsewhere`):
    // it keeps naming the library it shows, so its changes are refused, never written to the new one.
    if (away) return;
    libs.list = Array.isArray(r?.libraries) ? r.libraries : [];
    libs.openId = r?.openId ?? '';
    setLibraryId(libs.openId);
    libs.loaded = true;
  } catch { /* an older background service, or offline: the switcher keeps what it had */ }
}

let started = false;
let reloadTimer: ReturnType<typeof setTimeout> | undefined;
let away = false;
let awayTimer: ReturnType<typeof setInterval> | undefined;

/** Another window opened another library. With nothing unsaved here this window follows at once.
 *  Unsaved Settings edits or a typed key cannot be saved any more (they belong to the library left
 *  behind, and the service refuses them), and a reload would throw them away without a word: the
 *  window says so and waits for "Reload now", or reloads by itself once that work is saved elsewhere,
 *  copied and discarded. Note drafts are kept through a reload and never hold it back. */
export function switchedElsewhere(id?: string) {
  const lost = unsavedLost();
  const name = (id && libs.list.find((l) => l.id === id)?.displayName) || '';
  if (!lost.length) { stopAway(); toast(t('libs.switched')); reloadApp(); return; }
  away = true;
  clearTimeout(reloadTimer);
  toast(t(name ? 'ui-guards.switchedNamed' : 'ui-guards.switched', { name, what: lost.join(', ') }), {
    key: 'libs', stay: true, action: { label: t('ui-guards.reloadNow'), run: () => { stopAway(); reloadApp(); } },
  });
  if (!awayTimer) awayTimer = setInterval(() => { if (!unsavedLost().length) { stopAway(); reloadApp(); } }, 1000);
}
function stopAway() { clearInterval(awayTimer); awayTimer = undefined; }
/** Load the list, say what happened in the reload that brought this window here, and follow
 *  the background service: another window (or a setup) opening another library reloads this one. */
export function startLibraries() {
  if (started) return;
  started = true;
  // Known before the first change is sent: a window that shows a library says which one.
  if (app.status?.libraryId) setLibraryId(String(app.status.libraryId));
  loadLibraries().then(afterReload);
  on((e) => {
    if (e?.type === 'library.switched') {
      // This window started it (and reloads itself), or it already shows that library.
      if (libs.busy || libs.adding) return;
      if (!e.stale && e.id && e.id === libs.openId) return;
      switchedElsewhere(e.id ? String(e.id) : undefined);
      return;
    }
    if (['libraries.changed', 'settings.changed', 'chat.done', 'chat.updated', 'chat.removed', 'resync'].includes(e?.type)) {
      // Running and waiting counts follow the chats; a burst of events is one look.
      if (away) return;
      clearTimeout(reloadTimer);
      reloadTimer = setTimeout(loadLibraries, 400);
    }
  });
}

type Note = { name?: string; left?: string; running?: number; chatId?: string };
function afterReload() {
  const raw = ss.get(NOTE_KEY);
  if (!raw) return;
  ss.drop(NOTE_KEY);
  let n: Note = {};
  try { n = JSON.parse(raw) ?? {}; } catch { return; }
  if (n.chatId) go({ kind: 'chat', id: n.chatId });
  const name = openLib()?.displayName ?? n.name ?? '';
  if (!name) return;
  const left = n.left && n.running ? ' ' + tn('libs.leftWorking', n.running, { name: n.left }) : '';
  toast(t('libs.nowIn', { name }) + left, { key: 'libs' });
}

/** Open library `id`. Unsaved work in the view is asked about first, like leaving the view, and
 *  work still going on in the library open now is named before it is left to finish there. */
export async function switchLibrary(id: string) {
  if (libs.busy || id === libs.openId) return;
  const to = libs.list.find((l) => l.id === id);
  if (!to) return;
  if (to.missing) {
    toast(t('libs.missingToast', { name: to.displayName }), { kind: 'error', key: 'libs', action: { label: t('libs.removeConfirm'), run: () => removeLibrary(to) } });
    return;
  }
  if (!(await go({ kind: 'home' }))) return;
  await loadLibraries();
  const here = openLib();
  if (here && here.running > 0) {
    const ok = await confirmDialog({
      title: t('libs.workingTitle', { name: here.displayName }),
      body: tn('libs.workingBody', here.running, { name: here.displayName, other: to.displayName }),
      confirmLabel: t('libs.switchAnyway', { other: to.displayName }),
    });
    if (!ok) return;
  }
  libs.busy = true;
  toast(t('libs.switching', { name: to.displayName }), { key: 'libs' });
  try {
    const r = await post<{ left?: { name?: string; running?: number } }>('/libraries/switch', { id });
    ss.set(NOTE_KEY, JSON.stringify({ name: to.displayName, left: r?.left?.name ?? here?.displayName, running: r?.left?.running ?? 0 }));
    reloadApp();
  } catch (e) {
    libs.busy = false;
    failed(t('libs.switchFailed'), e);
  }
}

/** A library was just added (setup in "add" mode): reload into it, opening its setup chat. */
export function addedLibrary(setupChatId: string | null, name: string) {
  ss.set(NOTE_KEY, JSON.stringify({ name, chatId: setupChatId ?? undefined }));
  reloadApp();
}

export async function renameLibrary(l: Lib | undefined = openLib()) {
  if (!l) return;
  const v = await promptDialog({ title: t('libs.renameTitle'), body: t('libs.renameBody'), value: l.name, placeholder: l.path.split(/[\\/]/).filter(Boolean).pop() ?? '', confirmLabel: t('save') });
  if (v === null) return;
  try { await patch(`/libraries/${encodeURIComponent(l.id)}`, { name: v.trim() }); await loadLibraries(); }
  catch (e) { failed(t('libs.renameFailed'), e); }
}

export async function removeLibrary(l: Lib) {
  if (l.open) return;
  const ok = await confirmDialog({ title: t('libs.removeTitle', { name: l.displayName }), body: t('libs.removeBody'), confirmLabel: t('libs.removeConfirm'), danger: true });
  if (!ok) return;
  try { await del(`/libraries/${encodeURIComponent(l.id)}`); await loadLibraries(); toast(t('libs.removed', { name: l.displayName }), { key: 'libs' }); }
  catch (e) { failed(t('libs.removeFailed'), e); }
}

/** "Add it back": put the library folder of a chat (or a task) on the list again, without opening it. */
export async function addBack(path: string) {
  try {
    const l = await post<Lib>('/libraries/add', { path });
    await loadLibraries();
    toast(t('libs.addedBack', { name: l?.displayName ?? path }), { key: 'libs', action: l?.id ? { label: t('libs.openIt'), run: () => switchLibrary(l.id) } : undefined });
  } catch (e) { failed(t('libs.addBackFailed'), e); }
}

export function startAdding() { libs.adding = true; }

/** The switcher's commands, for the command palette. */
export function libraryPaletteItems(group: string): PaletteItem[] {
  const items: PaletteItem[] = [];
  for (const l of libs.list) {
    if (l.open) continue;
    items.push({ id: 'lib:' + l.id, title: t('libs.palOpen', { name: l.displayName }), group, icon: 'book', keywords: 'library switch مكتبة', run: () => { switchLibrary(l.id); } });
  }
  items.push({ id: 'libs.add', title: t('libs.palAdd'), group, icon: 'plus', keywords: 'library add new open مكتبة إضافة', run: () => startAdding() });
  if (openLib()) items.push({ id: 'libs.rename', title: t('libs.palRename'), group, icon: 'edit', keywords: 'library name rename مكتبة اسم', run: () => { renameLibrary(); } });
  return items;
}
