// One window edits a note at a time. Every app window (the main one and the "Open in new window"
// ones) shares localStorage, and with it Docs' crash-safety draft of each note: two windows editing
// the same note would silently overwrite each other's draft. So a window that opens a note in the
// editor marks it in localStorage ("being edited by window W, last seen at T") and keeps the mark
// fresh; another window seeing a fresh mark says so and does not edit or touch the draft.
//
// A window that closes or reloads takes its marks away (`pagehide`); one that crashed stops renewing
// them, and a mark older than TTL counts as gone, so it never blocks for long. The saved file itself
// is guarded separately: each save carries the version it was loaded from, and the service refuses a
// stale one (`baseHash` → `STALE::`, shown as the "two versions" comparison).

export const LOCK_PREFIX = 'thoughts.docedit:';
/** How often a window renews its marks. */
export const HEARTBEAT = 5_000;
/** A mark not renewed for this long belongs to a window that is gone (crashed, or closed without
 *  a chance to say so). A window in the background whose timers were slowed down so much that its
 *  mark lapses is told when it comes back (`lost`), and its saves stay guarded by the version check. */
export const TTL = 45_000;

export type Lock = { win: string; at: number };

/** The minimum of Storage this needs (tests pass a Map-backed one). */
export type Store = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

export const lockKey = (lib: string, path: string) => `${LOCK_PREFIX}${lib}\n${path}`;

/** The live mark on `key`, or null (none, unreadable or stale). */
export function readLock(store: Store, key: string, now: number): Lock | null {
  let l: any = null;
  try { l = JSON.parse(store.getItem(key) || 'null'); } catch { return null; }
  if (!l || typeof l.win !== 'string' || typeof l.at !== 'number') return null;
  // A mark from the future (a clock moved back) is renewed by its owner soon enough; trust it for one TTL.
  if (now - l.at > TTL || l.at - now > TTL) return null;
  return l;
}

/** Another live window is editing this note. */
export function heldByOther(store: Store, key: string, me: string, now: number): boolean {
  const l = readLock(store, key, now);
  return !!l && l.win !== me;
}

/** Take or renew the mark. False (and nothing written) while another live window holds it. */
export function claim(store: Store, key: string, me: string, now: number): boolean {
  if (heldByOther(store, key, me, now)) return false;
  try { store.setItem(key, JSON.stringify({ win: me, at: now } satisfies Lock)); } catch { return true; /* storage unavailable: nothing to share either */ }
  // Two windows claiming at the same instant: the one whose write landed last wins; the other sees it.
  return !heldByOther(store, key, me, now);
}

/** Drop the mark, only if it is this window's. */
export function release(store: Store, key: string, me: string) {
  try {
    const l = JSON.parse(store.getItem(key) || 'null');
    if (l?.win === me) store.removeItem(key);
  } catch { /* unreadable: leave it, it goes stale */ }
}

export type Presence = {
  /** This window's id. */
  me: string;
  /** Mark `path` as edited here while the returned stop function is not called. `lost` is told
   *  whenever another window holds it after all (it keeps trying, and takes it back once free). */
  hold(lib: string, path: string, lost?: (yes: boolean) => void): () => void;
  /** Another live window is editing `path`. */
  elsewhere(lib: string, path: string): boolean;
  /** Called when marks may have changed (another window's write, or time passing). */
  subscribe(fn: () => void): () => void;
};

type Env = { store: Store; me?: string; now?: () => number; win?: Pick<Window, 'addEventListener' | 'removeEventListener'> | null };

export function makePresence({ store, me = newId(), now = Date.now, win = typeof window === 'undefined' ? null : window }: Env): Presence {
  const held = new Map<string, { n: number; lost?: (yes: boolean) => void }>();
  const subs = new Set<() => void>();
  let timer: ReturnType<typeof setInterval> | undefined;
  const ping = () => { for (const f of [...subs]) { try { f(); } catch { /* one listener's error stays its own */ } } };
  function renew() {
    for (const [key, h] of held) { const got = claim(store, key, me, now()); h.lost?.(!got); }
  }
  function tick() { renew(); ping(); }
  function sync() {
    const want = held.size > 0 || subs.size > 0;
    if (want && !timer) timer = setInterval(tick, HEARTBEAT);
    else if (!want && timer) { clearInterval(timer); timer = undefined; }
  }
  const onStorage = (e: StorageEvent) => { if (e.key === null || e.key.startsWith(LOCK_PREFIX)) { renew(); ping(); } };
  const onHide = () => { for (const key of held.keys()) release(store, key, me); };
  // Back in front after a long sleep: renew at once rather than at the next (slowed) tick.
  const onShow = () => tick();
  win?.addEventListener('storage', onStorage as EventListener);
  win?.addEventListener('pagehide', onHide);
  // Closing a window may skip pagehide; if the close is then cancelled, the next heartbeat takes it back.
  win?.addEventListener('beforeunload', onHide);
  win?.addEventListener('pageshow', onShow);
  win?.addEventListener('focus', onShow);
  return {
    me,
    hold(lib, path, lost) {
      const key = lockKey(lib, path);
      const h = held.get(key) ?? { n: 0 };
      h.n++; h.lost = lost;
      held.set(key, h);
      const got = claim(store, key, me, now());
      lost?.(!got);
      sync();
      let done = false;
      return () => {
        if (done) return;
        done = true;
        const cur = held.get(key);
        if (cur && --cur.n <= 0) { held.delete(key); release(store, key, me); }
        sync();
      };
    },
    elsewhere: (lib, path) => heldByOther(store, lockKey(lib, path), me, now()),
    subscribe(fn) { subs.add(fn); sync(); return () => { subs.delete(fn); sync(); }; },
  };
}

function newId(): string {
  try { return crypto.randomUUID(); } catch { return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`; }
}

/** Storage that may be missing (private mode, a test page): a do-nothing stand-in then. */
function pageStore(): Store {
  try { if (typeof localStorage !== 'undefined') { localStorage.getItem(LOCK_PREFIX); return localStorage; } } catch { /* unavailable */ }
  return { getItem: () => null, setItem: () => {}, removeItem: () => {} };
}

let shared: Presence | null = null;
/** This window's presence (one per page). */
export function presence(): Presence {
  return (shared ??= makePresence({ store: pageStore() }));
}
