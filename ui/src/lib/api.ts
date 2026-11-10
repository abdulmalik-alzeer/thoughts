// Client for the thoughtsd daemon (HTTP + WebSocket on 127.0.0.1).
//
// The daemon token lives ONLY in the module-private `info` below. It is never written to
// localStorage / sessionStorage / `window.*` / the DOM, and in browser-preview mode `?token=` is
// removed from the address bar as soon as it has been read. Nothing in this module returns it.

type Info = { port: number; token: string };
let info: Info | null = null;
const listeners = new Set<(e: any) => void>();
let ws: WebSocket | null = null;

export const inTauri = () => typeof (window as any).__TAURI_INTERNALS__ !== 'undefined';

export async function connect(): Promise<void> {
  if (inTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    const i = await invoke<Info & { stale?: boolean }>('daemon_info');
    info = { port: i.port, token: i.token };
    if (i.stale !== undefined && i.stale !== stale) { stale = !!i.stale; listeners.forEach((l) => l({ type: 'daemon.stale', stale })); }
  } else {
    // Browser preview mode: http://localhost:1420/?port=…&token=… (read once, then stripped from the URL).
    const q = new URLSearchParams(location.search);
    try { localStorage.removeItem('thoughts.dev'); } catch { /* older builds kept the token here */ }
    if (q.get('port') && q.get('token')) {
      info = { port: +q.get('port')!, token: q.get('token')! };
      q.delete('port'); q.delete('token');
      const rest = q.toString();
      try { history.replaceState(null, '', location.pathname + (rest ? '?' + rest : '') + location.hash); } catch { /* ignore */ }
    }
    if (!info) throw new Error('Open with ?port=&token= from ~/.thoughts/daemon.json');
  }
  openSocket();
}

/** True when an older background service was kept running because it was busy (it switches over later). */
let stale = false;
export const daemonStale = () => stale;

/** Restart the background service. The shell refuses with `BUSY::{json}` while a save or a chat is
 *  running unless `force` — see `withBusyPrompt` in lib/shell.ts, which asks the user. */
export async function reconnect(force = false) {
  if (inTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    try {
      const i = await invoke<Info>('restart_daemon', { force });
      info = { port: i.port, token: i.token };
      if (stale) { stale = false; listeners.forEach((l) => l({ type: 'daemon.stale', stale })); }
    } catch (e: any) {
      const msg = String(e?.message ?? e);
      if (/^BUSY::\s*\{/.test(msg)) throw new ApiError(msg, 409, 'BUSY');
      throw new Error(await friendly(msg));
    }
    openSocket();
  }
}

/** Reload the window (`query` e.g. `?safe=1`). In browser-preview mode the connection details ride
 *  along in the URL for the reload only — they are stripped again on boot. */
export function reloadApp(query = '') {
  const q = new URLSearchParams(query);
  if (!inTauri() && info) { q.set('port', String(info.port)); q.set('token', info.token); }
  const s = q.toString();
  location.replace(location.pathname + (s ? '?' + s : ''));
}

/** Machine-readable daemon errors that have one plain-language meaning everywhere. */
async function friendly(msg: string): Promise<string> {
  const m = /(?:^|\s)BUSY::(.*)$/s.exec(msg);
  if (m) { const { t } = await import('./i18n'); return m[1].trim() || t('layer.busy'); }
  return msg;
}

/** The machine-readable prefix of a daemon error (`BUSY`, `CONFLICT`, `NO_HISTORY`, …), or ''. */
export function codeOf(msg: string): string {
  return /(?:^|\s)([A-Z][A-Z_]{2,})::/.exec(msg)?.[1] ?? '';
}

async function authed(path: string): Promise<Response> {
  if (!info) await connect();
  return fetch(`http://127.0.0.1:${info!.port}/api${path}`, { headers: { Authorization: `Bearer ${info!.token}` } });
}

/** Fetch any daemon path as a blob (PDF previews, the calendar file). Throws an ApiError when it fails. */
export async function apiBlob(path: string): Promise<Blob> {
  let r: Response;
  try { r = await authed(path); }
  catch { const { t } = await import('./i18n'); throw new ApiError(t('shell.unreachable'), 0); }
  if (!r.ok) {
    let msg = r.statusText;
    try { msg = JSON.parse(await r.text())?.error || msg; } catch { /* not JSON */ }
    throw new ApiError(await friendly(msg), r.status, codeOf(msg));
  }
  return r.blob();
}

/** Fetch a library file's bytes (`GET /api/library/raw?path=`). Null when missing or unsupported. */
export async function libraryBlob(path: string): Promise<Blob | null> {
  try {
    if (!info) await connect();
    const r = await fetch(`http://127.0.0.1:${info!.port}/api/library/raw?path=${encodeURIComponent(path)}`, { headers: { Authorization: `Bearer ${info!.token}` } });
    return r.ok ? await r.blob() : null;
  } catch { return null; }
}

let online = true;
let everOpen = false;
let retry: ReturnType<typeof setTimeout> | null = null;

/** Tell listeners when the background service drops or comes back: `{ type: 'connection', online }`. */
function setOnline(v: boolean) {
  if (online === v) return;
  online = v;
  listeners.forEach((l) => l({ type: 'connection', online: v }));
}

function openSocket() {
  if (!info) return;
  if (ws) { ws.onclose = null; ws.onopen = null; ws.onmessage = null; ws.close(); }
  if (retry) { clearTimeout(retry); retry = null; }
  const sock = (ws = new WebSocket(`ws://127.0.0.1:${info.port}/api/events?token=${info.token}`));
  sock.onopen = () => {
    setOnline(true);
    // Any socket after the first means events may have been missed: ask views to refresh.
    if (everOpen) listeners.forEach((l) => l({ type: 'resync' }));
    everOpen = true;
  };
  sock.onmessage = (m) => {
    let e: any;
    try { e = JSON.parse(m.data); } catch { return; }
    listeners.forEach((l) => { try { l(e); } catch (err) { console.error(err); } });
  };
  sock.onclose = () => {
    if (ws !== sock) return;
    setOnline(false);
    retry = setTimeout(async () => {
      retry = null;
      // connect() opens a fresh socket; if the service is still down its close schedules the next try.
      try { await connect(); } catch { retry = setTimeout(openSocket, 3000); }
    }, 1500);
  };
}

/** Listen to daemon events (plus the local `connection` and `resync` events). Returns a function that stops listening. */
export function on(fn: (e: any) => void): () => void {
  listeners.add(fn);
  return () => { listeners.delete(fn); };
}

/** Error from the daemon. `status` is the HTTP status (0 when the service could not be reached). */
export class ApiError extends Error {
  status: number;
  /** Stable machine string: the daemon's `code`, or the `XXX::` prefix of the message ('' when none). */
  code: string;
  constructor(message: string, status: number, code = '') { super(message); this.status = status; this.code = code; }
}

export async function api<T = any>(path: string, opts: { method?: string; body?: any } = {}): Promise<T> {
  if (!info) await connect();
  let r: Response;
  try {
    r = await fetch(`http://127.0.0.1:${info!.port}/api${path}`, {
      method: opts.method || (opts.body !== undefined ? 'POST' : 'GET'),
      headers: { Authorization: `Bearer ${info!.token}`, 'Content-Type': 'application/json' },
      body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
    });
  } catch {
    // The request never reached the background service (it stopped or is restarting).
    if (ws && ws.readyState !== WebSocket.OPEN && ws.readyState !== WebSocket.CONNECTING) setOnline(false);
    const { t } = await import('./i18n');
    throw new ApiError(t('shell.unreachable'), 0);
  }
  const text = await r.text();
  let data: any = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = { error: text.slice(0, 300) }; }
  if (!r.ok) {
    const msg: string = data?.error || r.statusText;
    // The daemon refuses to work in a library that can run programs until the user trusts it.
    const m = msg.match(/UNTRUSTED::(.*?)::(.*)/s);
    if (m && path !== '/library/trust') {
      const { confirmDialog } = await import('./dialog.svelte');
      const { t } = await import('./i18n');
      const ok = await confirmDialog({ title: t('trustTitle'), body: t('trustBody'), list: m[2].split('; '), confirmLabel: t('trustConfirm'), danger: true });
      if (!ok) throw new ApiError(t('shell.libraryNotOpened'), r.status);
      await api('/library/trust', { method: 'POST', body: { path: m[1] } });
      return api<T>(path, opts);
    }
    throw new ApiError(await friendly(msg), r.status, codeOf(msg) || String(data?.code ?? ''));
  }
  return data as T;
}

export const post = <T = any>(path: string, body: any = {}) => api<T>(path, { method: 'POST', body });
export const put = <T = any>(path: string, body: any) => api<T>(path, { method: 'PUT', body });
export const del = <T = any>(path: string) => api<T>(path, { method: 'DELETE' });
export const patch = <T = any>(path: string, body: any) => api<T>(path, { method: 'PATCH', body });
