// Showing and hiding the sidebar (r10-scroll). The sidebar's column narrows to the icon rail (CSS
// transition on the shell's columns, same length and curve as here); the full sidebar is cut back to
// the narrowing edge as its labels fade and drift toward it, and the rail's icons fade in, or the
// other way round. Reduce motion (the app's own setting or the system's) makes it instant.
import type { TransitionConfig } from 'svelte/transition';
import { a11y } from './store.svelte';

/** Keep in step with `.shell`'s transition in App.svelte. */
export const SIDEBAR_MS = 220;
const easeOut = (t: number) => 1 - Math.pow(1 - t, 3); // = cubic-bezier(.33, 1, .68, 1)
const RAIL_W = 50;

function still(): boolean {
  try { return a11y().reduceMotion || matchMedia('(prefers-reduced-motion: reduce)').matches; } catch { return false; }
}

/** The full sidebar: clipped to the column's width as it changes, its contents fading and sliding. */
export function sidebarSlide(node: HTMLElement): TransitionConfig {
  if (still()) return { duration: 0 };
  const w = node.offsetWidth;
  const hide = Math.max(0, w - RAIL_W);
  const rtl = getComputedStyle(node).direction === 'rtl';
  return {
    duration: SIDEBAR_MS,
    easing: easeOut,
    css: (t) => {
      const cut = (1 - t) * hide;
      const shift = (1 - t) * -8 * (rtl ? -1 : 1);
      return `clip-path: ${rtl ? `inset(0 0 0 ${cut}px)` : `inset(0 ${cut}px 0 0)`}; opacity: ${t}; transform: translateX(${shift}px);`;
    },
  };
}

/** The icon rail: fades, coming in a little after the sidebar starts to go. */
export function railFade(_node: HTMLElement, { into = false }: { into?: boolean } = {}): TransitionConfig {
  if (still()) return { duration: 0 };
  return into
    ? { delay: SIDEBAR_MS * 0.2, duration: SIDEBAR_MS * 0.7, css: (t) => `opacity: ${t}` }
    : { duration: SIDEBAR_MS * 0.45, css: (t) => `opacity: ${t}` };
}
