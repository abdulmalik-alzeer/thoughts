// Svelte action: pictures inside rendered content (an answer, a note, a Word page…) open in the
// picture viewer when clicked, or with Enter or Space once focused. Put it on the container
// (`use:zoomImages`), or on a single <img>. Pictures inside a link stay links. The container's
// content may be replaced at any time ({@html}, a streaming answer): new pictures are picked up.
import { openImage } from './imageOpen.svelte';
import { nameFromSrc } from './imageView';
import { t } from './i18n';

const MARK = 'data-zoom';

function eligible(img: HTMLImageElement): boolean {
  return !img.closest('a, button, [contenteditable=""], [contenteditable="true"], [data-no-zoom]');
}

function label(img: HTMLImageElement, name?: string): string {
  const what = (name || img.getAttribute('alt') || '').trim() || nameFromSrc(img.getAttribute('src') || '');
  return what ? t('chatfViewer.openNamed', { name: what }) : t('chatfViewer.open');
}

function mark(img: HTMLImageElement, name?: string) {
  if (img.hasAttribute(MARK) || !eligible(img)) return;
  img.setAttribute(MARK, '');
  img.tabIndex = 0;
  img.setAttribute('role', 'button');
  img.setAttribute('aria-label', label(img, name));
}

/** Take the viewer's marks off a copy of rendered content (for export or the clipboard). */
export function unmarkImages(root: ParentNode): void {
  for (const img of Array.from(root.querySelectorAll<HTMLElement>(`img[${MARK}]`))) {
    img.removeAttribute(MARK); img.removeAttribute('tabindex'); img.removeAttribute('role'); img.removeAttribute('aria-label');
  }
}

export function zoomImages(node: HTMLElement, opts: { name?: string } = {}) {
  const single = node instanceof HTMLImageElement;
  let queued = 0;
  const scan = () => {
    queued = 0;
    if (single) { mark(node as HTMLImageElement, opts.name); return; }
    for (const img of Array.from(node.querySelectorAll('img'))) mark(img, undefined);
  };
  scan();
  const obs = single ? null : new MutationObserver(() => { if (!queued) queued = requestAnimationFrame(scan); });
  obs?.observe(node, { childList: true, subtree: true });

  const target = (e: Event): HTMLImageElement | null => {
    const el = e.target as HTMLElement | null;
    if (!el || el.tagName !== 'IMG' || !el.hasAttribute(MARK) || !node.contains(el)) return null;
    return eligible(el as HTMLImageElement) ? (el as HTMLImageElement) : null;
  };
  const open = (img: HTMLImageElement) => {
    const src = img.currentSrc || img.getAttribute('src') || '';
    if (!src) return false; // not loaded yet (a note's picture still on its way)
    const alt = img.getAttribute('alt') || '';
    openImage({ src, alt, name: opts.name || alt || nameFromSrc(img.getAttribute('src') || ''), from: img });
    return true;
  };
  function onClick(e: MouseEvent) {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const img = target(e);
    if (img && open(img)) e.preventDefault();
  }
  function onKey(e: KeyboardEvent) {
    if ((e.key !== 'Enter' && e.key !== ' ') || e.isComposing || e.repeat) return;
    const img = target(e);
    if (img && open(img)) { e.preventDefault(); e.stopPropagation(); }
  }
  node.addEventListener('click', onClick);
  node.addEventListener('keydown', onKey);
  return {
    update(o: { name?: string } = {}) { opts = o; },
    destroy() {
      obs?.disconnect();
      cancelAnimationFrame(queued);
      node.removeEventListener('click', onClick);
      node.removeEventListener('keydown', onKey);
    },
  };
}

// Rendered markdown is injected HTML (no component scope), so the pictures' look lives here.
if (typeof document !== 'undefined' && !document.getElementById('zoom-images')) {
  const s = document.createElement('style');
  s.id = 'zoom-images';
  s.textContent = `
img[data-zoom] { cursor: zoom-in; }
img[data-zoom]:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 2px; }
@media print { img[data-zoom] { cursor: default; outline: none; } }
`;
  document.head.appendChild(s);
}
