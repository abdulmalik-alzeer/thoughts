<script lang="ts" module>
  export type MenuItem = { id: string; label: string; icon?: string; checked?: boolean; disabled?: boolean; sep?: boolean };
</script>

<script lang="ts">
  // The small menu of a sidebar entry (right-click, or its "more" button). Opens at a point, kept
  // inside the window on every side; the arrow keys walk it, Enter or a click picks, and Escape, a
  // click elsewhere or the focus leaving closes it. `sep` draws a line above an item.
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';

  let { x, y, label, items, pick, close }: { x: number; y: number; label: string; items: MenuItem[]; pick: (id: string) => void; close: () => void } = $props();
  let el: HTMLDivElement | undefined = $state();
  let left = $state(0), top = $state(0), ready = $state(false);

  const buttons = () => [...(el?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
  onMount(() => {
    if (!el) return;
    // In Arabic the menu opens toward the start side of the pointer, like every menu there.
    const m = 8, w = el.offsetWidth, h = el.offsetHeight, rtl = getComputedStyle(el).direction === 'rtl';
    left = Math.max(m, Math.min(rtl ? x - w : x, window.innerWidth - w - m));
    top = Math.max(m, Math.min(y, window.innerHeight - h - m));
    ready = true;
    buttons()[0]?.focus();
    const out = (e: PointerEvent) => { if (el && !el.contains(e.target as Node)) close(); };
    window.addEventListener('pointerdown', out, true);
    window.addEventListener('resize', close);
    window.addEventListener('blur', close);
    return () => { window.removeEventListener('pointerdown', out, true); window.removeEventListener('resize', close); window.removeEventListener('blur', close); };
  });
  /** Closing first gives the focus back to what opened the menu; the choice may then move it on
   *  (renaming puts it in the name field). `pick` is read before closing, while the menu still is. */
  function choose(id: string) { const run = pick; close(); run(id); }
  function key(e: KeyboardEvent) {
    const b = buttons(), at = b.indexOf(document.activeElement as HTMLButtonElement);
    const go = (i: number) => { e.preventDefault(); b[(i + b.length) % b.length]?.focus(); };
    if (e.key === 'ArrowDown') go(at + 1);
    else if (e.key === 'ArrowUp') go(at - 1);
    else if (e.key === 'Home') go(0);
    else if (e.key === 'End') go(b.length - 1);
    else if (e.key === 'Escape' || e.key === 'Tab') { e.preventDefault(); e.stopPropagation(); close(); }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="navmenu no-print" class:ready bind:this={el} role="menu" aria-label={label} tabindex="-1" style:left={left + 'px'} style:top={top + 'px'}
  onkeydown={key} oncontextmenu={(e) => e.preventDefault()}>
  {#each items as it (it.id)}
    {#if it.sep}<div class="sep" role="separator"></div>{/if}
    <button role={it.checked === undefined ? 'menuitem' : 'menuitemcheckbox'} aria-checked={it.checked} disabled={it.disabled} onclick={() => choose(it.id)}>
      <span class="mi">{#if it.checked || it.icon}<Icon name={it.checked ? 'check' : it.icon ?? ''} size={14} />{/if}</span><span class="grow" dir="auto">{it.label}</span>
    </button>
  {/each}
</div>

<style>
  .navmenu { position: fixed; z-index: 120; min-width: 190px; max-width: min(280px, calc(100vw - 16px)); max-height: calc(100vh - 16px); overflow-y: auto; padding: 5px; visibility: hidden;
    background: var(--panel); color: var(--fg); border: var(--border-w) solid var(--hairline); border-radius: var(--radius-lg); box-shadow: var(--shadow-lg); font-size: 13px; }
  .navmenu.ready { visibility: visible; animation: pop .12s cubic-bezier(.33, 1, .68, 1); }
  @keyframes pop { from { opacity: 0; transform: translateY(-3px) scale(.98); } }
  button { font: inherit; color: inherit; background: none; border: 0; width: 100%; text-align: start; display: flex; align-items: center; gap: 8px; padding: 5px 8px; border-radius: 6px; cursor: pointer; }
  button:hover:not(:disabled), button:focus-visible { background: var(--tint); color: var(--on-tint); outline: none; }
  button:disabled { opacity: .45; cursor: default; }
  .mi { flex: none; width: 14px; height: 14px; display: grid; place-items: center; color: var(--muted); }
  button:hover:not(:disabled) .mi, button:focus-visible .mi { color: inherit; }
  .mi :global(.icon) { display: block; }
  .sep { height: 1px; margin: 4px 6px; background: var(--hairline); }
  @media (prefers-reduced-motion: reduce) { .navmenu.ready { animation: none; } }
  :global([data-motion="reduce"]) .navmenu.ready { animation: none; }
</style>
