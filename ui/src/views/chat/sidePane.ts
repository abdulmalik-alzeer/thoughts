// The side pane beside a chat (SidePane.svelte): which clicks open it, where a link in an answer
// points in the library, and how wide the pane may be. Pure: no DOM, no requests.

/** Narrowest the pane gets. */
export const PANE_MIN = 300;
/** Widest the pane gets, however wide the window. */
export const PANE_MAX = 900;
/** Width the first time it opens. */
export const PANE_DEFAULT = 440;
/** Room the conversation keeps next to the pane; with less, the pane lies over the chat instead. */
export const CHAT_MIN = 420;
/** One arrow-key press on the handle; with Shift, a bigger step. */
export const KEY_STEP = 16;
export const KEY_STEP_BIG = 64;

const WIDTH_KEY = 'thoughts.chatPaneWidth';

/** Is there room for the pane beside the chat, or does it lie over it? */
export function isOverlay(avail: number): boolean {
  return avail > 0 && avail < PANE_MIN + CHAT_MIN;
}

/** The widest the pane may be in a space `avail` pixels wide. */
export function maxWidth(avail: number): number {
  if (!(avail > 0)) return PANE_MAX;
  if (isOverlay(avail)) return Math.max(Math.min(PANE_MAX, avail), 0);
  return Math.max(PANE_MIN, Math.min(PANE_MAX, avail - CHAT_MIN));
}

/** The smallest the pane may be (in an overlay narrower than PANE_MIN, the whole space). */
export function minWidth(avail: number): number {
  return avail > 0 ? Math.min(PANE_MIN, maxWidth(avail)) : PANE_MIN;
}

/** A wanted width kept inside what the space allows. */
export function clampWidth(w: number, avail: number): number {
  const n = Number.isFinite(w) ? Math.round(w) : PANE_DEFAULT;
  return Math.min(maxWidth(avail), Math.max(minWidth(avail), n));
}

/** The width while dragging the handle from `startX` to `x`. The pane sits at the line's end, so its
 *  handle is on its start edge: towards the start makes it wider (left in English, right in Arabic). */
export function dragWidth(start: number, startX: number, x: number, rtl: boolean): number {
  return start + (rtl ? x - startX : startX - x);
}

/** The width after a key on the focused handle, or null when the key is not one of the handle's.
 *  The arrow keys move the handle the way they point, as with the mouse. */
export function keyWidth(key: string, w: number, avail: number, rtl: boolean, big = false): number | null {
  const step = big ? KEY_STEP_BIG : KEY_STEP;
  let next: number;
  if (key === 'ArrowLeft') next = w + (rtl ? -step : step);
  else if (key === 'ArrowRight') next = w + (rtl ? step : -step);
  else if (key === 'Home') next = minWidth(avail);
  else if (key === 'End') next = maxWidth(avail);
  else return null;
  return clampWidth(next, avail);
}

type Store = { getItem(k: string): string | null; setItem(k: string, v: string): void };
const storage = (): Store | null => { try { return typeof localStorage === 'undefined' ? null : localStorage; } catch { return null; } };

/** The width the user last left the pane at (only on this computer). */
export function loadWidth(s: Store | null = storage()): number {
  try {
    const n = Number(s?.getItem(WIDTH_KEY));
    return n >= PANE_MIN && n <= PANE_MAX ? Math.round(n) : PANE_DEFAULT;
  } catch { return PANE_DEFAULT; }
}
export function saveWidth(w: number, s: Store | null = storage()): void {
  try { s?.setItem(WIDTH_KEY, String(Math.round(w))); } catch { /* private mode, storage full */ }
}

type Click = { button?: number; metaKey?: boolean; ctrlKey?: boolean; shiftKey?: boolean; altKey?: boolean };

/** What a click on a file in a chat does: a plain click reads it in the side pane; with ⌘ or Ctrl
 *  (or Shift), or with the middle button, it opens in Docs as before. Null: not a click we act on. */
export function clickIntent(e: Click): 'pane' | 'docs' | null {
  const b = e.button ?? 0;
  if (b === 1) return 'docs';
  if (b !== 0) return null;
  if (e.metaKey || e.ctrlKey || e.shiftKey) return 'docs';
  if (e.altKey) return null;
  return 'pane';
}

/** The library file a link in a chat message points to, or null for web addresses, mail, anchors
 *  and anything else that is not a file of the library. Answers are written from the top of the
 *  library, so a relative link starts there; a full path inside the library (`/Users/…/Thoughts/x.md`
 *  or `file://…`) is made relative to it. */
export function linkTarget(href: string | null | undefined, libRoot = ''): string | null {
  let h = String(href ?? '').trim();
  if (!h || h.startsWith('#') || h.startsWith('//')) return null;
  const root = libRoot.replace(/\\/g, '/').replace(/\/+$/, '');
  const fileUrl = /^file:/i.test(h);
  if (fileUrl) h = h.replace(/^file:(\/\/[^/]*)?/i, '');
  if (/^[a-z][a-z0-9+.-]*:/i.test(h)) return null;
  const dec = (s: string) => { try { return decodeURIComponent(s); } catch { return s; } };
  const full = h.split('#')[0].split('?')[0].split('/').map(dec).join('/');
  let rel: string;
  if (root && (full === root || full.startsWith(root + '/'))) rel = full.slice(root.length);
  else if (fileUrl) return null; // a file somewhere else on the computer
  else rel = full;
  const out: string[] = [];
  for (const part of rel.split('/')) {
    if (part === '..') { if (!out.length) return null; out.pop(); }
    else if (part && part !== '.') out.push(part);
  }
  return out.length ? out.join('/') : null;
}

/** Notes are rendered as formatted text; every other file goes to the shared preview. */
export const isNote = (p: string) => /\.(md|markdown|mdown)$/i.test(p);

/** A path with no extension on its last part: probably a folder (an idea) shown by its front page. */
export const looksLikeFolder = (p: string) => !/\.[a-z0-9]{1,8}$/i.test(p.split('/').pop() || '');
