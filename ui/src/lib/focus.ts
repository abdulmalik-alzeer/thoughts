// Svelte action: keep keyboard focus inside a dialog, restore it on close, and close on Escape.
export function trapFocus(node: HTMLElement, opts: { onEscape?: () => void } = {}) {
  const prev = document.activeElement as HTMLElement | null;
  const sel = 'a[href],button:not([disabled]),input:not([disabled]),textarea:not([disabled]),select:not([disabled]),[tabindex]:not([tabindex="-1"])';
  const items = () => Array.from(node.querySelectorAll<HTMLElement>(sel)).filter((el) => el.offsetParent !== null);
  queueMicrotask(() => (node.querySelector<HTMLElement>('[autofocus],[data-autofocus]') ?? items()[0] ?? node).focus());
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.stopPropagation(); opts.onEscape?.(); return; }
    if (e.key !== 'Tab') return;
    const list = items();
    if (!list.length) { e.preventDefault(); return; }
    if (!node.contains(document.activeElement)) { e.preventDefault(); list[0].focus(); return; }
    const first = list[0], last = list[list.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  }
  node.addEventListener('keydown', onKey);
  return {
    update(o: { onEscape?: () => void }) { opts = o; },
    destroy() { node.removeEventListener('keydown', onKey); prev?.focus?.(); },
  };
}
