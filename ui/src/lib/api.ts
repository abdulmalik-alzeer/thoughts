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

function openSocket() {
  if (!info) return;
  ws?.close();
  ws = new WebSocket(`ws://127.0.0.1:${info.port}/api/events?token=${info.token}`);
  ws.onmessage = (m) => {
    const e = JSON.parse(m.data);
    listeners.forEach((l) => l(e));
  };
  ws.onclose = () => setTimeout(async () => {
    try { await connect(); listeners.forEach((l) => l({ type: 'resync' })); } catch { /* retry next close */ }
  }, 1500);
}

export function on(fn: (e: any) => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export async function api<T = any>(path: string, opts: { method?: string; body?: any } = {}): Promise<T> {
  if (!info) await connect();
  const r = await fetch(`http://127.0.0.1:${info!.port}/api${path}`, {
    method: opts.method || (opts.body !== undefined ? 'POST' : 'GET'),
    headers: { Authorization: `Bearer ${info!.token}`, 'Content-Type': 'application/json' },
    body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
  });
  const text = await r.text();
  const data = text ? JSON.parse(text) : null;
  if (!r.ok) {
    const msg: string = data?.error || r.statusText;
    // The daemon refuses to work in a library that can run programs until the user trusts it.
    const m = msg.match(/UNTRUSTED::(.*?)::(.*)/s);
    if (m && path !== '/library/trust') {
      const ok = confirm(`This library contains settings that can run programs on your computer:\n\n• ${m[2].split('; ').join('\n• ')}\n\nOnly continue if you made this library or trust whoever did.`);
      if (!ok) throw new Error('Library not opened.');
      await api('/library/trust', { method: 'POST', body: { path: m[1] } });
      return api<T>(path, opts);
    }
    throw new Error(msg);
  }
  return data as T;
}

export const post = <T = any>(path: string, body: any = {}) => api<T>(path, { method: 'POST', body });
export const put = <T = any>(path: string, body: any) => api<T>(path, { method: 'PUT', body });
export const del = <T = any>(path: string) => api<T>(path, { method: 'DELETE' });
export const patch = <T = any>(path: string, body: any) => api<T>(path, { method: 'PATCH', body });
