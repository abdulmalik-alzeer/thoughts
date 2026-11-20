// The add-on host's half of the machine bridge (crates/thoughtsd/src/machine.rs), for pages and
// add-ons while full access is on (lib/fullAccess.ts). The host only forwards: the service refuses
// every request flatly while full access is off, whatever is sent from here. Documented for add-on
// authors in docs/CUSTOMISING.md ("Full access: the machine bridge").
//
// In the frame (`thoughts.machine` / the plugin API's `machine`):
//   run(program, args?, {cwd, stdin, env, timeoutSecs}?) → {code, stdout, stderr, timedOut}
//   stream(program, args?, opts?, onOutput(stream, data)) → Promise<{code, timedOut}>; .id, .kill()
//   kill(id) · read(path, {base64}?) → text · write(path, content, {base64, append}?) · list(path) · open(target, app?)

import { on, post } from '../api';

type Deliver = (msg: any) => void;
/** Streamed commands: which frame started each one. */
const owners = new Map<string, Deliver>();
let listening = false;

function listen() {
  if (listening) return;
  listening = true;
  on((e) => {
    if (e?.type !== 'machine.output' && e?.type !== 'machine.exit') return;
    const to = owners.get(String(e.id));
    if (!to) return;
    to({ type: 'event', event: e.type, payload: { id: e.id, stream: e.stream, data: e.data, code: e.code ?? null, timedOut: !!e.timedOut } });
    if (e.type === 'machine.exit') owners.delete(String(e.id));
  });
}

const s = (v: any) => String(v ?? '');
const list = (v: any) => (Array.isArray(v) ? v.map(s) : []);
const opts = (v: any) => (v && typeof v === 'object' ? v : {});

/** Forget the commands a frame that is gone had started (they go on; `kill` still stops them). */
export function forgetFrame(deliver: Deliver) { for (const [id, d] of owners) if (d === deliver) owners.delete(id); }

/** One `machine.*` call from a frame. */
export async function machineHandle(method: string, a: any[], deliver: Deliver): Promise<any> {
  const o = opts(a[2]);
  const body = { program: s(a[0]), args: list(a[1]), cwd: o.cwd ? s(o.cwd) : undefined, stdin: o.stdin == null ? undefined : s(o.stdin),
    env: o.env && typeof o.env === 'object' ? Object.fromEntries(Object.entries(o.env).map(([k, v]) => [k, s(v)])) : undefined,
    timeoutSecs: Number.isFinite(Number(o.timeoutSecs)) && Number(o.timeoutSecs) > 0 ? Number(o.timeoutSecs) : undefined };
  switch (method) {
    case 'machine.run': return post('/machine/run', body);
    case 'machine.stream': {
      listen();
      const r = await post<{ id: string }>('/machine/run', { ...body, stream: true });
      owners.set(r.id, deliver);
      return r;
    }
    case 'machine.kill': return post('/machine/kill', { id: s(a[0]) });
    case 'machine.read': { const r = await post('/machine/read', { path: s(a[0]), base64: !!opts(a[1]).base64 }); return r.content; }
    case 'machine.write': return post('/machine/write', { path: s(a[0]), content: s(a[1]), base64: !!opts(a[2]).base64, append: !!opts(a[2]).append });
    case 'machine.list': return (await post('/machine/list', { path: s(a[0]) })).entries;
    case 'machine.open': return post('/machine/open', { target: s(a[0]), app: a[1] == null ? undefined : s(a[1]) });
    default: throw new Error(`"${method}" is not part of the machine bridge`);
  }
}

/** The web-platform features a frame is allowed while full access is on (the system still asks). */
export const FULL_ACCESS_ALLOW = 'camera; microphone; display-capture; geolocation; bluetooth; serial; usb; hid; clipboard-read; clipboard-write; fullscreen; midi';
