// The content area's own scroll (r10-scroll). Only the page beside the sidebar scrolls; the sidebar
// and the window's top bar stay put. A new page opens at its top; Back and Forward return to where
// the page was left, once its content has loaded far enough to get there.

let el: HTMLElement | null = null;
let key = '';
let back = false;
let job = 0;
const pos = new Map<string, number>();

/** Svelte action for the content area's scrolling box. */
export function contentScroller(node: HTMLElement) {
  el = node;
  // The user scrolling, clicking or typing ends any return that is still waiting for content.
  const stop = () => { job++; };
  node.addEventListener('wheel', stop, { passive: true });
  node.addEventListener('touchstart', stop, { passive: true });
  node.addEventListener('pointerdown', stop);
  node.addEventListener('keydown', stop);
  return {
    destroy() {
      node.removeEventListener('wheel', stop);
      node.removeEventListener('touchstart', stop);
      node.removeEventListener('pointerdown', stop);
      node.removeEventListener('keydown', stop);
      if (el === node) el = null;
    },
  };
}

/** The next page change is Back or Forward: return to where that page was left. */
export function returningNext() { back = true; }

/** Call before the page changes (`$effect.pre`): remembers where the old page was, then shows the
 *  new one from its top, or from where it was left when coming back to it. */
export function pageChanging(next: string) {
  const returning = back;
  back = false;
  if (next === key) return;
  if (el && key) {
    pos.delete(key);
    pos.set(key, el.scrollTop);
    if (pos.size > 60) pos.delete(pos.keys().next().value!);
  }
  key = next;
  if (!el) return;
  const box = el;
  const n = ++job;
  box.scrollTop = 0;
  const want = returning ? pos.get(next) ?? 0 : 0;
  if (want <= 0) return;
  // The page fills in as its data arrives: try for a little while, then settle for as far as it goes.
  const until = performance.now() + 1500;
  const step = () => {
    if (n !== job || el !== box) return;
    const room = box.scrollHeight - box.clientHeight;
    box.scrollTop = Math.min(want, room);
    if (room >= want || performance.now() > until) return;
    setTimeout(step, 30);
  };
  setTimeout(step, 0);
}
