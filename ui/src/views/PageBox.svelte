<script lang="ts">
  // One add-on box on one of the app's own pages (`pages.json`: `boxes`, or a box among the page's
  // sections). The add-on draws inside its own sealed frame; the heading and the add-on's name
  // beside it are the app's, so a box never passes for part of the app.
  import { addonTag } from '../lib/layer';
  import { surfaceIn } from '../lib/surfaceIn';
  import type { HomeWidget } from '../lib/store.svelte';
  let { w }: { w: HomeWidget } = $props();
  function plug(el: HTMLElement, w: { render: (el: HTMLElement) => any }) {
    let cleanup: any;
    try { cleanup = w.render(el); } catch (e) { console.error(e); }
    return { destroy() { try { if (typeof cleanup === 'function') cleanup(); } catch { /* frame already gone */ } } };
  }
</script>

<section class="surface box" aria-label={w.title} use:surfaceIn>
  <div class="bh"><h2 dir="auto">{w.title}</h2><span class="from muted">{addonTag(w.plugin)}</span></div>
  <div class="slot" use:plug={w}></div>
</section>

<style>
  .box { padding: 6px; min-width: 0; overflow: hidden; }
  /* A box rises like the page's own sections, once its add-on has drawn it (lib/surfaceIn.ts). */
  .box:global([data-drawn='no']) { opacity: 0; }
  .box:global([data-drawn='yes']) { animation: rise .5s var(--ease) backwards; }
  @media (prefers-reduced-motion: reduce) { .box:global([data-drawn='no']) { opacity: 1; } }
  :global(html[data-motion='reduce']) .box:global([data-drawn='no']) { opacity: 1; }
  @media print { .box:global([data-drawn='no']) { opacity: 1; } }
  .bh { display: flex; align-items: baseline; gap: 10px; padding: 8px 10px 6px; }
  .bh h2 { margin: 0; font-size: 15px; flex: 1; min-width: 0; }
  .from { font-size: var(--fs-xs); flex: none; }
  .slot { position: relative; padding: 4px 10px 10px; }
</style>
