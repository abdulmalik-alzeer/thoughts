// Unsaved work in this window, reported to the desktop app so that quitting (or reloading the
// window from its Recovery menu) can ask first.
//
// The desktop app is told as things change and never has to ask the page at the last moment: a
// page that has stopped responding can therefore never hold a quit back. In the browser preview
// there is no desktop app to tell, and the browser's own "leave this page?" question does the job.
import { inTauri } from './api';

type Item = { what: string; kept: boolean };
const items = new Map<string, Item>();
let sent = '[]';
let queued = false;

/** Say that view `id` holds unsaved work, or with `what = null` that it no longer does.
 *  `what`: a short name the person would recognise (a note's name, "Settings").
 *  `kept`: true when the work survives a quit as a draft that is offered again next time. */
export function reportUnsaved(id: string, what: string | null, kept = false) {
  if (what) items.set(id, { what, kept });
  else items.delete(id);
  if (!inTauri() || queued) return;
  queued = true;
  queueMicrotask(async () => {
    queued = false;
    const list = [...items.values()];
    const next = JSON.stringify(list);
    if (next === sent) return;
    sent = next;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('set_unsaved', { items: list });
    } catch { sent = ''; /* not told: try again on the next change */ }
  });
}

/** What a reload of this window would lose: the unsaved work that is not kept as a draft. */
export function unsavedLost(): string[] {
  return [...items.values()].filter((x) => !x.kept).map((x) => x.what);
}
