// Search by meaning in the window: where it stands (GET /api/meaning), the bar while the model is
// downloaded or the notes are made ready (`meaning.progress` events), and the suggestions an
// assistant's answer made (`meaning.suggest`). Off until the user presses the button in Settings ›
// Features; nothing here ever switches it on by itself.
import { api, on, post } from './api';
import type { MeaningInfo, MeaningResult } from './types';

/** The line an assistant ends an answer with to suggest it (the service takes it out of the saved message). */
export const MARKER = '[[thoughts:meaning-search]]';

export const meaning = $state<{
  info: MeaningInfo | null;
  /** Chat id → the answer a suggestion is shown under. */
  offers: Record<string, string>;
}>({ info: null, offers: {} });

let listening = false;
let timer: ReturnType<typeof setTimeout> | undefined;

/** Ask again soon (several events in a row make one request). */
function soon() { clearTimeout(timer); timer = setTimeout(() => { void refresh(); }, 150); }

/** Start following it (once per window); every view that shows it calls this. */
export function watchMeaning() {
  if (listening) return;
  listening = true;
  on((e) => {
    if (e.type === 'meaning.progress') {
      const i = meaning.info;
      if (!i) { soon(); return; }
      if (e.phase === 'download') i.download = { got: Number(e.got) || 0, total: Number(e.total) || 0 };
      else if (e.phase === 'index') i.index = { ...(i.index ?? { files: 0, chunks: 0 }), done: Number(e.done) || 0, total: Number(e.total) || 0 };
    } else if (e.type === 'meaning.suggest') {
      if (typeof e.chatId === 'string' && typeof e.messageId === 'string') meaning.offers[e.chatId] = e.messageId;
      soon();
    } else if (e.type === 'meaning.state' || e.type === 'resync' || e.type === 'library.switched') {
      soon();
    }
  });
  void refresh();
}

export async function refresh(): Promise<MeaningInfo | null> {
  try {
    const i = await api<MeaningInfo>('/meaning');
    meaning.info = i;
    const offers: Record<string, string> = {};
    for (const o of i.offers ?? []) offers[o.chatId] = o.messageId;
    meaning.offers = offers;
    return i;
  } catch { return meaning.info; }
}

async function act(path: string, body: any = {}) {
  try { return await post(path, body); } finally { await refresh(); }
}
export const install = () => act('/meaning/install');
export const pause = () => act('/meaning/pause');
export const rebuild = () => act('/meaning/rebuild');
/** Delete the model and every index, and switch it off. → bytes freed. */
export async function remove(): Promise<number> { const r = await act('/meaning/remove'); return Number(r?.freed) || 0; }
/** "Not now" (30 days) or "Don't ask again". Either hides every open suggestion at once. */
export async function decline(op: 'later' | 'never', chatId?: string) {
  meaning.offers = {};
  if (meaning.info) meaning.info.quiet = true;
  await act('/meaning/offer', chatId ? { op, chatId } : { op });
}

export const isReady = () => meaning.info?.state === 'ready' || (meaning.info?.state === 'indexing' && !!meaning.info.index?.updated);
/** May it be suggested now (off, and not declined or put off)? */
export const mayOffer = () => meaning.info?.state === 'off' && !meaning.info.quiet;

export async function searchByMeaning(q: string, limit = 10): Promise<MeaningResult[]> {
  const r = await api<{ results: MeaningResult[] }>(`/meaning/search?q=${encodeURIComponent(q)}&limit=${limit}`);
  return Array.isArray(r?.results) ? r.results : [];
}

/** Text shown while an answer streams in: without the marker line, nor the start of one at its end. */
export function hideMarker(text: string): string {
  if (!text.includes('[[')) return text;
  let lines = text.split('\n').filter((l) => l.trim() !== MARKER);
  const last = lines[lines.length - 1]?.trim() ?? '';
  if (last.length >= 2 && MARKER.startsWith(last)) lines = lines.slice(0, -1);
  return lines.join('\n');
}

/** Megabytes the way the update bar shows them. */
export const mb = (n: number) => (n / 1048576).toFixed(n >= 10 * 1048576 ? 0 : 1);
