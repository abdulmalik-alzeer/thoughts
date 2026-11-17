// The one picture viewer (lib/ImageViewer.svelte, mounted once in App.svelte): any view opens a
// picture in it with `openImage`. It closes only with Esc or its close button, and gives keyboard
// focus back to `from` (the picture or button that opened it).

export type ImageToOpen = {
  /** The picture's address (a blob:, data: or https: address the page can already show). */
  src: string;
  /** Its description, read out by screen readers. */
  alt?: string;
  /** The name shown at the top (a file name), when known. */
  name?: string;
  /** What to give focus back to on close; the focused element when left out. */
  from?: HTMLElement | null;
};

export const imageViewer = $state<{ open: (ImageToOpen & { seq: number }) | null }>({ open: null });
let seq = 0;

/** Show a picture in the viewer. */
export function openImage(img: ImageToOpen): void {
  if (!img?.src) return;
  const from = img.from ?? (typeof document !== 'undefined' ? (document.activeElement as HTMLElement | null) : null);
  imageViewer.open = { ...img, from, seq: ++seq };
}

export function closeImage(): void {
  imageViewer.open = null;
}
