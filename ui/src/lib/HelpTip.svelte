<script lang="ts">
  // A small "?" button that shows explanatory text in a popover (the "toggletip" pattern). Click or
  // Enter/Space opens it; clicking again, Escape, a click outside or the focus moving away closes it.
  // Use for "how it works" prose; keep warnings, errors, costs, privacy and status visible.
  // `label` is the button's accessible name: say what it explains ("About ready-made packs"). Without
  // one, a tip inside a heading or a field label is named after it ("About Automatic tasks").
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import { t } from './i18n';

  let { text = '', label = '', children, size = 14 }: {
    text?: string; label?: string; children?: Snippet; size?: number;
  } = $props();

  let open = $state(false);
  let root: HTMLSpanElement | undefined = $state();
  let btn: HTMLButtonElement | undefined = $state();
  let pop: HTMLSpanElement | undefined = $state();
  const id = `helptip-${Math.random().toString(36).slice(2, 9)}`;

  let fromHeading = $state('');
  $effect(() => {
    if (label || !root) return;
    const h = root.closest('h1, h2, h3, h4, h5, h6, legend, label, .lbl');
    const what = (h?.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 80);
    fromHeading = what ? t('czh.about', { what }) : '';
  });
  const name = $derived(label || fromHeading || t('helpTip.label'));

  const close = (refocus = false) => { open = false; if (refocus) btn?.focus(); };

  /** The popover is `position: fixed`, placed from the button, so a card's or a dialog's scrolling
   *  box never clips it: below the button, else above it; kept 8px inside the window on every side;
   *  starting at the button's start edge (its right edge in Arabic). */
  function place() {
    if (!btn || !pop) return;
    const r = btn.getBoundingClientRect();
    const m = 8, vw = window.innerWidth, vh = window.innerHeight;
    const w = pop.offsetWidth, h = pop.offsetHeight;
    const rtl = getComputedStyle(btn).direction === 'rtl';
    const left = Math.max(m, Math.min(rtl ? r.right - w + 6 : r.left - 6, vw - w - m));
    const top = r.bottom + 6 + h > vh - m && r.top - 6 - h >= m ? r.top - 6 - h : r.bottom + 6;
    pop.style.left = `${left}px`; pop.style.top = `${top}px`;
    // Inside a transformed box (a dialog during its entrance) `fixed` is measured from that box:
    // see where it landed and move it back by the difference.
    const got = pop.getBoundingClientRect();
    if (got.left !== left || got.top !== top) { pop.style.left = `${2 * left - got.left}px`; pop.style.top = `${2 * top - got.top}px`; }
  }

  $effect(() => {
    if (!open || !pop) return;
    place();
    const again = () => place();
    window.addEventListener('resize', again);
    window.addEventListener('scroll', again, true);
    return () => { window.removeEventListener('resize', again); window.removeEventListener('scroll', again, true); };
  });

  function onDoc(e: PointerEvent) { if (open && root && !root.contains(e.target as Node)) close(); }
  function onKey(e: KeyboardEvent) { if (open && e.key === 'Escape') { e.stopPropagation(); e.preventDefault(); close(true); } }
  // Also a listener of its own on the tip (Svelte's `onkeydown` is handled at the document, after a
  // dialog around it has seen the key): Escape closes the tip and not also that dialog.
  $effect(() => {
    const el = root;
    if (!el) return;
    el.addEventListener('keydown', onKey);
    return () => el.removeEventListener('keydown', onKey);
  });
  function onOut(e: FocusEvent) {
    const to = e.relatedTarget as Node | null;
    if (open && to && root && !root.contains(to)) close();
  }
</script>

<svelte:document onpointerdown={onDoc} onkeydown={onKey} />

<span class="helptip" bind:this={root} onfocusout={onOut} role="presentation">
  <button
    bind:this={btn}
    type="button"
    class="helptip-btn"
    aria-label={name}
    aria-expanded={open}
    aria-controls={id}
    onclick={(e) => { e.stopPropagation(); e.preventDefault(); open = !open; }}
  ><Icon name="help" {size} /></button>
  <!-- Always in the page, so a screen reader reads the text out as it appears (and finds it right
       after the button when reading on). -->
  <span class="helptip-live" aria-live="polite">
    {#if open}
      <span class="helptip-pop" bind:this={pop} {id} role="note" aria-label={name}>
        {#if text}<span class="helptip-text">{text}</span>{/if}
        {#if children}{@render children()}{/if}
      </span>
    {/if}
  </span>
</span>

<style>
  .helptip { position: relative; display: inline-flex; vertical-align: middle; margin-inline-start: 4px; }
  .helptip-btn {
    display: inline-flex; align-items: center; justify-content: center; padding: 2px; border: 0;
    background: transparent; color: var(--muted); border-radius: 999px; cursor: pointer; line-height: 0;
  }
  .helptip-btn:hover, .helptip-btn[aria-expanded="true"] { color: var(--fg); background: var(--fill); }
  .helptip-btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .helptip-pop {
    position: fixed; top: 0; left: 0; z-index: 1000;
    width: max-content; max-width: min(320px, calc(100vw - 16px)); max-height: calc(100vh - 16px); overflow: auto;
    padding: 10px 12px; background: var(--panel); color: var(--fg);
    border: var(--border-w) solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg);
    font-size: var(--fs-sm); font-weight: 400; line-height: 1.5; text-align: start; white-space: normal;
    text-transform: none; letter-spacing: normal; display: flex; flex-direction: column; gap: 6px;
    animation: helptip-in .12s ease-out;
  }
  /* Lists and paragraphs passed in as children. */
  .helptip-pop :global(ol), .helptip-pop :global(ul) { margin: 0; padding-inline-start: 18px; display: grid; gap: 3px; }
  .helptip-pop :global(p) { margin: 0; }
  /* Opacity only: a moving entrance would make the box the containing block of what opens from it. */
  @keyframes helptip-in { from { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .helptip-pop { animation: none; } }
  /* On paper the button does nothing; a tip left open prints in place, as ordinary text. */
  @media print {
    .helptip-btn { display: none; }
    .helptip-pop { position: static; box-shadow: none; max-height: none; overflow: visible; animation: none; }
  }
</style>
