// Which chats are waiting for the user's OK right now (an approval card nobody answered yet), so the
// sidebar can mark them "needs you", the Dock icon can carry a count while the window is in the
// background, and the "waiting for your OK" toast stays until the question is settled.
//
// The service tells everything that matters: `approval.asked` and the card itself (a `chat.message`
// with role `approval` and status `waiting`) when a question is put; the same card again with another
// status (`once`, `always`, `denied`, `timeout`, `ended`) when it is answered, runs out of time or the
// run ends; `chat.done` / `chat.removed` when the run or the chat goes. After a lost connection the
// list is read again from the service (`GET /chats/:id/approvals` for the chats that are working).
import { untrack } from 'svelte';
import { api, inTauri, on } from './api';
import { app, dismissToast } from './store.svelte';

/** chat id → approval id → when the question runs out (ms since epoch). */
const waiting = $state<Record<string, Record<string, number>>>({});

/** A safety net only: the service settles a card when its time is up and says so. This removes a
 *  card whose end was never heard of (a missed event), a little after the service's own limit. */
const FALLBACK_MS = 5 * 60_000 + 15_000;
const timers = new Map<string, ReturnType<typeof setTimeout>>();

/** Is chat `id` waiting for the user's OK? */
export function needsYou(id: string | null | undefined): boolean {
  return !!id && !!waiting[id] && Object.keys(waiting[id]).length > 0;
}

/** How many questions wait across all chats. */
function total(): number {
  let n = 0;
  for (const c of Object.values(waiting)) n += Object.keys(c).length;
  return n;
}

function add(chat: string, id: string, until?: number) {
  const at = until && Number.isFinite(until) && until > Date.now() ? until + 2_000 : Date.now() + FALLBACK_MS;
  const fresh = !waiting[chat]?.[id];
  waiting[chat] = { ...(waiting[chat] ?? {}), [id]: at };
  clearTimeout(timers.get(id));
  timers.set(id, setTimeout(() => settle(chat, id), Math.max(1_000, at - Date.now())));
  if (fresh) dock(true);
}

function settle(chat: string, id: string) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  if (!waiting[chat]?.[id]) return;
  const { [id]: _gone, ...rest } = waiting[chat];
  if (Object.keys(rest).length) waiting[chat] = rest;
  else { delete waiting[chat]; closeToast(chat); }
  dock(false);
}

function clearChat(chat: string) {
  for (const id of Object.keys(waiting[chat] ?? {})) settle(chat, id);
  closeToast(chat);
}

/** The "waiting for your OK" toast of `chat` (App.svelte, key `approval:<chat>`) goes once nothing waits there. */
function closeToast(chat: string) {
  for (const x of app.toasts) if (x.key === 'approval:' + chat) dismissToast(x.id);
}

// ---------- the Dock ----------

let last = -1;
/** Tell the desktop app how many questions wait; `ask` = a new one came, so it may bounce the Dock
 *  icon once when the window is in the background. The app itself shows the count only while the
 *  window is not in front, so this is called again whenever the window gains or loses focus. */
function dock(ask: boolean) {
  if (!inTauri()) return;
  const n = total();
  if (!ask && n === last) return;
  last = n;
  import('@tauri-apps/api/core').then(({ invoke }) => invoke('set_attention', { waiting: n, ask })).catch(() => { /* an older desktop app */ });
}
function refocus() { last = -1; dock(false); }

// ---------- keeping the list ----------

async function reread() {
  const running = app.chats.filter((c: any) => c.running).map((c: any) => c.id as string);
  for (const chat of Object.keys(waiting)) if (!running.includes(chat)) clearChat(chat);
  for (const chat of running) {
    try {
      const r = await api(`/chats/${encodeURIComponent(chat)}/approvals`);
      const ids: string[] = Array.isArray(r?.waiting) ? r.waiting.map(String) : [];
      for (const id of Object.keys(waiting[chat] ?? {})) if (!ids.includes(id)) settle(chat, id);
      for (const id of ids) if (!waiting[chat]?.[id]) add(chat, id);
    } catch { /* the chat went meanwhile, or the service is away: the next resync tries again */ }
  }
}

let started = false;
/** Start following approvals (once; lives as long as the window). */
export function startAttention(): void {
  if (started) return;
  started = true;
  on((e) => {
    if (e.type === 'approval.asked' && e.chatId && e.id) add(String(e.chatId), String(e.id));
    else if (e.type === 'chat.message' && e.chatId && e.message?.role === 'approval' && e.message.id) {
      const chat = String(e.chatId), id = String(e.message.id);
      if (e.message.tool?.status === 'waiting') add(chat, id, Date.parse(e.message.tool?.expiresAt ?? ''));
      else settle(chat, id);
    } else if ((e.type === 'chat.done' || e.type === 'chat.removed') && e.chatId) clearChat(String(e.chatId));
    else if (e.type === 'resync' && app.ready) reread();
  });
  $effect.root(() => {
    // Once the window is set up (and after a library switch), read what already waits.
    $effect(() => { if (app.ready) untrack(() => { reread(); }); });
    // Looking at the chat is answering the toast's question "where?": its card is right there.
    $effect(() => {
      const v = app.view;
      if (v.kind === 'chat' && v.id) untrack(() => closeToast(v.id!));
    });
  });
  window.addEventListener('focus', refocus);
  window.addEventListener('blur', refocus);
}
