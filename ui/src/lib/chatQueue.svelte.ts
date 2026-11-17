// A message written while the assistant is still answering waits here, one per chat, and is sent
// as soon as that answer has finished — whichever view is on screen at the time. It is visible and
// can be changed or cancelled until then. An answer that does not finish (stopped, failed) does
// not send it: it keeps waiting, and the chat offers to send it now.
//
// The files' bytes live only in memory. What else was waiting is also kept in the browser's
// storage, so that a message that never went (the app was closed first) is put back into that
// chat's message box the next time the app starts, rather than lost.
import { ApiError, api, on, post } from './api';
import type { Chat } from './types';
import { readyForQueued } from '../views/chat/merge';
import { failed } from './errors';
import { t } from './i18n';

export type QueuedFile = { name: string; data: string; size: number; image: boolean };
/** A library file or folder named in the message: its path, and the words "@…" it shows as. */
export type Mention = { path: string; label: string };
export type Queued = { text: string; files: QueuedFile[]; mentions: Mention[] };

const KEY = 'thoughts.chatqueue:';
const DRAFT = 'thoughts.chatdraft:';

/** What waits, by chat id. */
export const waiting = $state<Record<string, Queued>>({});
/** Chats whose message was sent as soon as their answer had finished: "stop and send now". */
const sendNow = new Set<string>();
const sending = new Set<string>();
/** Chats whose waiting message came back into the message box at start-up (the view says so once). */
export const restored = new Set<string>();

function keep(id: string) {
  try {
    const q = waiting[id];
    if (q) localStorage.setItem(KEY + id, JSON.stringify({ text: q.text, mentions: q.mentions, files: q.files.map(({ name, size, image }) => ({ name, size, image })) }));
    else localStorage.removeItem(KEY + id);
  } catch { /* storage full or unavailable */ }
}

const join = (a: Queued, b: Queued): Queued => ({
  text: [a.text, b.text].filter((x) => x.trim()).join('\n\n'),
  files: [...a.files, ...b.files],
  mentions: [...a.mentions, ...b.mentions.filter((m) => !a.mentions.some((x) => x.path === m.path))],
});
const copy = (q: Queued): Queued => ({ text: q.text, files: [...q.files], mentions: q.mentions.map((m) => ({ ...m })) });

/** Add to what waits in this chat (a second message joins the first). */
export function enqueue(id: string, q: Queued) {
  const had = waiting[id];
  waiting[id] = had ? join(copy(had), q) : copy(q);
  keep(id);
}

/** Take the waiting message back out (to edit it, or to drop it). */
export function take(id: string): Queued | null {
  const q = waiting[id] ?? null;
  delete waiting[id];
  sendNow.delete(id);
  keep(id);
  return q ? copy(q) : null;
}

/** Send it now: at once when the chat is at rest, else right after stopping the answer. */
export async function sendNowOrAfterStop(id: string, running: boolean) {
  if (!waiting[id]) return;
  if (!running) return dispatch(id);
  sendNow.add(id);
  try { await post(`/chats/${id}/stop`); } catch (e) { sendNow.delete(id); failed(t('chat3.stopFailed'), e); }
}

async function dispatch(id: string) {
  const q = waiting[id];
  if (!q || sending.has(id)) return;
  sending.add(id);
  sendNow.delete(id);
  // Out of the queue while it is on its way; anything written meanwhile waits for the next answer.
  const out = copy(q);
  delete waiting[id];
  keep(id);
  try {
    await post(`/chats/${id}/send`, { text: out.text, attachments: out.files.map(({ name, data }) => ({ name, data })), mentions: out.mentions.map((m) => m.path) });
  } catch (e) {
    // Not sent: it waits again, ahead of anything added meanwhile. (Refused because another answer
    // started first, it goes when that one has finished.)
    const later = waiting[id];
    waiting[id] = later ? join(out, later) : out;
    keep(id);
    if (!(e instanceof ApiError && e.status === 409 && e.code === 'busy')) failed(t('chatc.queue.failed'), e);
  } finally { sending.delete(id); }
}

on((e) => {
  // Back after the connection dropped: an answer may have ended unseen (its chat.done was missed).
  if (e?.type === 'resync') { afterReconnect(); return; }
  if (e?.type !== 'chat.done' || typeof e.chatId !== 'string' || !waiting[e.chatId]) return;
  // A finished answer sends what waits; one that was stopped or failed only when that was asked for.
  if (e.ok || sendNow.has(e.chatId)) dispatch(e.chatId);
});

/** Ask the service how each chat with a waiting message stands, and send it where the answer has
 *  finished meanwhile (by the same rule as a live end). */
function afterReconnect() {
  for (const id of Object.keys(waiting)) {
    if (sending.has(id)) continue;
    api<Chat>(`/chats/${id}`).then((c) => {
      if (waiting[id] && readyForQueued(c, sendNow.has(id))) dispatch(id);
    }).catch(() => { /* gone or unreachable: it keeps waiting, and the chat offers to send it */ });
  }
}

// At start-up: a message left waiting when the app was closed goes back into its chat's message box.
try {
  for (let i = localStorage.length - 1; i >= 0; i--) {
    const k = localStorage.key(i);
    if (!k?.startsWith(KEY)) continue;
    const id = k.slice(KEY.length);
    let q: any = null;
    try { q = JSON.parse(localStorage.getItem(k) || 'null'); } catch { /* unreadable: dropped */ }
    localStorage.removeItem(k);
    if (!q || typeof q !== 'object') continue;
    let d: any = null;
    try { d = JSON.parse(localStorage.getItem(DRAFT + id) || 'null'); } catch { /* no draft */ }
    if (!d || typeof d !== 'object') d = {};
    const text = [String(q.text ?? ''), String(d.text ?? '')].filter((x) => x.trim()).join('\n\n');
    const mentions = [...new Set([...(Array.isArray(q.mentions) ? q.mentions : []), ...(Array.isArray(d.mentions) ? d.mentions : [])])];
    const files = [...(Array.isArray(q.files) ? q.files : []), ...(Array.isArray(d.files) ? d.files : [])];
    localStorage.setItem(DRAFT + id, JSON.stringify({ text, mentions, files }));
    restored.add(id);
  }
} catch { /* storage unavailable */ }
