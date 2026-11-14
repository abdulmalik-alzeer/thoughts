// The entrance of an add-on box (a Home widget, or a box `pages.json` places on one of the app's own
// pages). The add-on draws in its own frame a moment after the page opens; until it has drawn (or given
// up) the box is held back, then it comes in the way the app's own sections do (theme.css `rise`).
// The box sets `data-drawn="no"` / `"yes"`; its component styles the two states. A box that is slow
// or never draws is shown anyway after a short wait, and nothing here holds up moving to another page.
import { SURFACE_DRAWN } from './layer';

/** The longest a box is held back before it is shown, drawn or not (the same as an add-on page). */
const WAIT_MS = 600;

export function surfaceIn(node: HTMLElement) {
  node.dataset.drawn = 'no';
  const show = () => {
    clearTimeout(timer);
    node.removeEventListener(SURFACE_DRAWN, show);
    node.dataset.drawn = 'yes';
  };
  node.addEventListener(SURFACE_DRAWN, show);
  const timer = setTimeout(show, WAIT_MS);
  return {
    destroy() {
      clearTimeout(timer);
      node.removeEventListener(SURFACE_DRAWN, show);
    },
  };
}
