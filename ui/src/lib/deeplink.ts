// thoughts:// links (FEAT-38). The desktop app checks every link itself (src-tauri/src/links.rs);
// once it is sure, it brings the window forward and leaves what to open here (`take_deep_link`).
// It is opened the way the app's own buttons open it, so a view with unsaved work is asked first.
import { api } from './api';
import { app, command, go, refreshChats, toast } from './store.svelte';
import { t } from './i18n';

type Route = { kind: 'note'; path: string } | { kind: 'chat'; id: string } | { kind: 'view'; view: string };
const VIEWS = ['home', 'chats', 'ideas', 'todos', 'docs'];

let polling = false;

/** Open what a link asked for. A link can be what started the app, so this waits for the window
 *  to be set up first; before setup is finished the desktop app does not hand links over. */
async function pull() {
  if (!app.ready) {
    if (!polling) { polling = true; setTimeout(() => { polling = false; pull(); }, 400); }
    return;
  }
  if (!app.settings?.onboarded) return;
  let r: Route | null = null;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    r = await invoke<Route | null>('take_deep_link');
  } catch { return; }
  if (!r) return;
  if (r.kind === 'note' && r.path) command('doc.open', { path: r.path });
  else if (r.kind === 'chat' && r.id) {
    const id = r.id;
    const known = () => app.chats.some((c: any) => c.id === id);
    if (!known()) await refreshChats().catch(() => {});
    let found = known();
    if (!found) { try { await api(`/chats/${encodeURIComponent(id)}`); found = true; } catch { /* not there */ } }
    if (found) command('chat.open', { chatId: id });
    else toast(t('nativeb.link.chatGone'), { kind: 'error' });
  } else if (r.kind === 'view' && VIEWS.includes(r.view)) go({ kind: r.view });
}

/** In the desktop app, once at start-up: open links as they arrive, and the one the app started with. */
export async function watchDeepLinks() {
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen('deep-link', () => { pull(); });
  } catch { return; }
  pull();
}

/** The link that opens a library file in Thoughts (each part of the path encoded on its own). */
export const noteLink = (path: string) => 'thoughts://note/' + path.split('/').map(encodeURIComponent).join('/');
