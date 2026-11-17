// More app windows (src-tauri/src/windows.rs): a note or a chat opened in a window of its own.
//
// A second window is the whole app at the place it was opened for (`?open=docs&path=…`,
// `?open=chat&id=…`), with its own connection to the background service, so it hears every change
// the main window hears. What belongs to the app as a whole stays with the main window: the Dock,
// notifications and the toasts that announce something from elsewhere, thoughts:// links, updates,
// the native menus, voice, the tour, reviewing a customisation, and the places that need those
// (Settings, Customise), to which a second window sends the person (`showInMain`).
//
// Windows also tell each other which chat they show (a BroadcastChannel between pages of the app),
// so that an empty chat is never tidied away while another window has it open.

import { inTauri } from './api';

export type Route = { kind: string; id?: string; path?: string };

/** The labels of second windows (`view-<n>`); never `main-…`, which is the main window's. */
export const isViewLabel = (label: string) => /^view-\d{1,6}$/.test(label);

/** This window's label in the desktop app ('' in a browser). */
export function windowLabel(): string {
  try { return String((window as any).__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? ''); } catch { return ''; }
}

/** Is this a second window (not the main one)? */
export const isSecondary = () => isViewLabel(windowLabel());

/** Can this window open more windows? Only in the desktop app. */
export const canOpenWindows = () => inTauri();

/** Places a second window shows only by sending the person to the main window. */
export const MAIN_ONLY = ['settings', 'customise'];

const plainId = (s: string) => /^[A-Za-z0-9_-]{1,64}$/.test(s);
const chatId = (s: string) => /^[A-Za-z0-9-]{1,64}$/.test(s);
function plainPath(p: string): boolean {
  // eslint-disable-next-line no-control-regex
  return !!p && [...p].length <= 1024 && !/^[/\\?]/.test(p) && !/[\u0000-\u001f\u007f-\u009f]/.test(p) && !p.split(/[/\\]/).includes('..');
}

/** The route a window may be opened at: a note in Docs or a chat (the shell checks the same). */
export function windowRoute(v: Route): Route | null {
  if (v.kind === 'chat' && v.id && chatId(v.id) && !v.path) return { kind: 'chat', id: v.id };
  if (v.kind === 'docs' && v.path && plainPath(v.path) && !v.id) return { kind: 'docs', path: v.path };
  return null;
}

/** A route the main window is sent to from a second window: one of the above, Settings or Customise. */
export function mainRoute(v: Route): Route | null {
  if (MAIN_ONLY.includes(v.kind) && !v.path && (!v.id || plainId(v.id))) return v.id ? { kind: v.kind, id: v.id } : { kind: v.kind };
  return windowRoute(v);
}

/** Where a second window opens: read from its address (`?open=…`). Null for anything else. */
export function initialRoute(search: string): Route | null {
  const q = new URLSearchParams(search);
  const kind = q.get('open');
  if (!kind) return null;
  return windowRoute({ kind, id: q.get('id') ?? undefined, path: q.get('path') ?? undefined });
}

/** The address a second window is opened at (what the shell builds; kept here for the tests). */
export function windowQuery(v: Route, safe = false): string {
  const q = new URLSearchParams({ open: v.kind });
  if (v.id) q.set('id', v.id);
  if (v.path) q.set('path', v.path);
  if (safe) q.set('safe', '1');
  return '?' + q.toString();
}

/** The title of a second window: what it shows, then the app's name. */
export function windowTitle(name: string): string {
  const n = String(name ?? '').replace(/[\u0000-\u001f\u007f]+/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 80);
  return n ? `${n} — Thoughts` : 'Thoughts';
}

async function call(cmd: string, args: Record<string, unknown>) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke(cmd, args);
}

/** Open `v` (a note or a chat) in a new window. Throws with a sentence when it cannot. */
export async function openInNewWindow(v: Route): Promise<void> {
  const route = windowRoute(v);
  if (!route || !inTauri()) throw new Error('cannot open a window here');
  await call('open_window', { route });
}

/** Bring the main window forward, at `v` when given (from a second window). */
export async function showInMain(v?: Route): Promise<void> {
  if (!inTauri()) return;
  const route = v ? mainRoute(v) : null;
  try { await call('focus_main', { route }); } catch { /* an older shell */ }
}

let titled = '';
/** Name this second window after what it shows. */
export function setWindowTitle(name: string) {
  const title = windowTitle(name);
  if (!isSecondary() || title === titled) return;
  titled = title;
  import('@tauri-apps/api/window').then(({ getCurrentWindow }) => getCurrentWindow().setTitle(title)).catch(() => { titled = ''; });
}

// ---------- what the other windows show ----------

type Peer = { chat: string | null };
const peers = new Map<string, Peer>();
let chan: BroadcastChannel | null = null;
let mine: Peer = { chat: null };

function post(m: any) { try { chan?.postMessage(m); } catch { /* closed */ } }

/** Tell the other windows which chat this one shows (null: none). Starts listening the first time. */
export function announceChat(chat: string | null) {
  mine = { chat };
  if (!chan && inTauri() && typeof BroadcastChannel !== 'undefined') {
    try {
      chan = new BroadcastChannel('thoughts.windows');
      chan.onmessage = (e) => {
        const m = e.data ?? {};
        const from = String(m.from ?? '');
        if (!from || from === windowLabel()) return;
        if (m.type === 'hello') post({ type: 'view', from: windowLabel(), chat: mine.chat });
        if (m.type === 'hello' || m.type === 'view') peers.set(from, { chat: typeof m.chat === 'string' ? m.chat : null });
        if (m.type === 'bye') peers.delete(from);
      };
      window.addEventListener('pagehide', () => post({ type: 'bye', from: windowLabel() }));
      post({ type: 'hello', from: windowLabel(), chat });
      return;
    } catch { chan = null; }
  }
  post({ type: 'view', from: windowLabel(), chat });
}

/** Does another window show chat `id`? */
export function chatOpenElsewhere(id: string): boolean {
  for (const p of peers.values()) if (p.chat === id) return true;
  return false;
}
