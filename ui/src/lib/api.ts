// Client for the thoughtsd daemon (HTTP + WebSocket on 127.0.0.1).

type Info = { port: number; token: string };
let info: Info | null = null;
const listeners = new Set<(e: any) => void>();
let ws: WebSocket | null = null;

export const inTauri = () => typeof (window as any).__TAURI_INTERNALS__ !== 'undefined';

export async function connect(): Promise<Info> {
  if (inTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    info = await invoke<Info>('daemon_info');
  } else {
    // Browser dev mode: http://localhost:1420/?port=…&token=…
    const q = new URLSearchParams(location.search);
    const saved = JSON.parse(localStorage.getItem('thoughts.dev') || 'null');
    info = q.get('port') ? { port: +q.get('port')!, token: q.get('token')! } : saved;
    if (!info) throw new Error('Open with ?port=&token= from ~/.thoughts/daemon.json');
    localStorage.setItem('thoughts.dev', JSON.stringify(info));
  }
  openSocket();
  return info;
}

export async function reconnect() {
  if (inTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    info = await invoke<Info>('restart_daemon');
    openSocket();
  }
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
  constructor(message: string, status: number) { super(message); this.status = status; }
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
    throw new ApiError(msg, r.status);
  }
  return data as T;
}

export const post = <T = any>(path: string, body: any = {}) => api<T>(path, { method: 'POST', body });
export const put = <T = any>(path: string, body: any) => api<T>(path, { method: 'PUT', body });
export const del = <T = any>(path: string) => api<T>(path, { method: 'DELETE' });
export const patch = <T = any>(path: string, body: any) => api<T>(path, { method: 'PATCH', body });
