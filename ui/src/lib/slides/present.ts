// Present a deck: Present.svelte (and reveal.js with it) is loaded only now, and shown over the
// whole window. One deck at a time.
import { parseDeck, type Deck } from './deck';

let open: (() => void) | null = null;

/** Show `deck` (or the markdown of one) full screen. `path`: the note it came from, for its pictures. */
export async function present(deck: Deck | string, path = ''): Promise<void> {
  open?.();
  const d = typeof deck === 'string' ? parseDeck(deck) : deck;
  const [{ mount, unmount }, { default: Present }] = await Promise.all([import('svelte'), import('./Present.svelte')]);
  const host = document.createElement('div');
  document.body.append(host);
  const before = document.activeElement as HTMLElement | null;
  let comp: Record<string, any> | null = null;
  const close = () => {
    if (open !== close) return;
    open = null;
    if (comp) void unmount(comp);
    host.remove();
    if (before?.isConnected) before.focus({ preventScroll: true });
  };
  open = close;
  comp = mount(Present, { target: host, props: { deck: d, path, onclose: close } });
}

/** Is a deck being shown? */
export const presenting = () => !!open;
