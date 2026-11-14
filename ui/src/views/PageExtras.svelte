<script lang="ts">
  // What a customisation adds to one of the app's own pages (`pages.json`): at the top, the line
  // under the heading and the add-on boxes placed there; at the bottom, the boxes placed there.
  // When an add-on page normally replaces this page and the person chose to see the app's own,
  // the top also offers to switch back. Every box is drawn by its add-on in its own sealed frame,
  // with the add-on's name beside it so it never passes for part of the app.
  import Icon from '../lib/Icon.svelte';
  import { t } from '../lib/i18n';
  import { addonTag } from '../lib/layer';
  import { surfaceIn } from '../lib/surfaceIn';
  import { pageBoxes, pageIntro, showingOriginal, swapFor, useSwap } from '../lib/pages.svelte';

  let { page, place = 'top', intro = true }: { page: string; place?: 'top' | 'bottom'; intro?: boolean } = $props();

  const boxes = $derived(pageBoxes(page, place));
  const line = $derived(place === 'top' && intro ? pageIntro(page) : '');
  const swap = $derived(place === 'top' && showingOriginal(page) ? swapFor(page) : undefined);

  function plug(el: HTMLElement, w: { render: (el: HTMLElement) => any }) {
    let cleanup: any;
    try { cleanup = w.render(el); } catch (e) { console.error(e); }
    return { destroy() { try { if (typeof cleanup === 'function') cleanup(); } catch { /* frame already gone */ } } };
  }
</script>

{#if swap}
  <p class="swap no-print" role="status">
    <Icon name="plugin" size={13} />
    <span class="grow">{t('r10-cust-pages.original', { name: swap.title || swap.short || '' })}</span>
    <button class="btn ghost sm" onclick={() => useSwap(page)}>{t('r10-cust-pages.useSwap')}</button>
  </p>
{/if}
{#if line}<p class="intro muted" dir="auto">{line}</p>{/if}
{#if boxes.length}
  <div class="boxes {place}">
    {#each boxes as w (w.plugin + ':' + w.id)}
      <section class="surface box" aria-label={w.title} use:surfaceIn>
        <div class="bh"><h2 dir="auto">{w.title}</h2><span class="from muted">{addonTag(w.plugin)}</span></div>
        <div class="slot" use:plug={w}></div>
      </section>
    {/each}
  </div>
{/if}

<style>
  .intro { margin: -4px 0 16px; max-width: 70ch; }
  .boxes { display: grid; gap: 14px; }
  .boxes.top { margin-bottom: 18px; }
  .boxes.bottom { margin-top: 24px; }
  .box { padding: 6px; min-width: 0; overflow: hidden; }
  /* A box rises like the page's own sections, once its add-on has drawn it (lib/surfaceIn.ts). */
  .box:global([data-drawn='no']) { opacity: 0; }
  .box:global([data-drawn='yes']) { animation: rise .5s var(--ease) both; }
  @media (prefers-reduced-motion: reduce) { .box:global([data-drawn='no']) { opacity: 1; } }
  :global(html[data-motion='reduce']) .box:global([data-drawn='no']) { opacity: 1; }
  @media print { .box:global([data-drawn='no']) { opacity: 1; } }
  .bh { display: flex; align-items: baseline; gap: 10px; padding: 8px 10px 6px; }
  .bh h2 { margin: 0; font-size: 15px; flex: 1; min-width: 0; }
  .from { font-size: var(--fs-xs); flex: none; }
  .slot { position: relative; padding: 4px 10px 10px; }
  .swap { display: flex; align-items: center; gap: 8px; margin: 0 0 14px; padding: 6px 10px; border: var(--border-w, 1px) solid var(--border); border-radius: var(--radius); color: var(--muted); font-size: var(--fs-sm, 13px); }
  .swap :global(.icon) { flex: none; }
</style>
