<script lang="ts">
  // One page of an add-on. The add-on draws inside its own sandboxed frame; what is outside the frame
  // (the strip saying whose page this is, the "not available" notice) is drawn by the app.
  import Icon from '../lib/Icon.svelte';
  import { app } from '../lib/store.svelte';
  import { findView, SURFACE_DRAWN } from '../lib/layer';
  import { t } from '../lib/i18n';
  import type { Snippet } from 'svelte';
  // `onfail`: called when the page cannot be shown (an add-on page standing in for one of the app's
  // own then gives way to it); `children`: more in the strip saying whose page this is.
  let { id, onfail, children }: { id: string; onfail?: () => void; children?: Snippet } = $props();
  let el = $state<HTMLElement>();
  let problem = $state('');
  // The page as it is registered right now. Reloading the customisations (Try it, Keep, Stop preview,
  // an undo) removes every add-on's frames and registers the pages again under the same ids: the
  // frame is then mounted again here, instead of the page staying empty until it is reopened.
  const view = $derived(findView(id));
  // The add-on draws in its own frame a moment after the page opens. Until then the frame is held back,
  // and it then comes in the way a section of the app's own pages does (theme.css `rise`), instead of
  // popping in after the page has already settled. A page that is slow or broken is shown anyway after
  // a short wait: nothing here holds up moving to another page.
  let drawn = $state(false);
  $effect(() => {
    const v = view, into = el;
    if (!v || !into) return;
    problem = '';
    drawn = false;
    const show = () => (drawn = true);
    into.addEventListener(SURFACE_DRAWN, show);
    const timer = setTimeout(show, 600);
    let cleanup: any;
    try { cleanup = v.render(into); } catch (e: any) { problem = e?.message ?? String(e); show(); }
    return () => {
      clearTimeout(timer);
      into.removeEventListener(SURFACE_DRAWN, show);
      try { if (typeof cleanup === 'function') cleanup(); } catch { /* plugin cleanup failed */ }
    };
  });
  // While the customisations reload the page is gone for a moment: only say so when it stays gone.
  let gone = $state(false);
  $effect(() => {
    if (view) { gone = false; return; }
    const timer = setTimeout(() => (gone = true), 1500);
    return () => clearTimeout(timer);
  });
  $effect(() => { if ((gone || problem) && onfail) onfail(); });
  const from = $derived(view?.plugin ? String(view.plugin).replace(/\.(js|html)$/i, '') : '');
</script>

{#if gone || problem}
  <div class="page" role="alert">
    <h1>{t('shell.pv.title')}</h1>
    <p class="muted">{t('shell.pv.body')}</p>
    {#if problem}<pre dir="auto">{problem}</pre>{/if}
    <button class="btn" onclick={() => (app.showRollback = true)}>{t('shell.openRollback')}</button>
  </div>
{:else if view}
  <!-- Whose page this is: drawn by the app, outside the add-on's frame, so the add-on cannot remove it. -->
  <p class="from no-print"><Icon name="plugin" size={13} /> <span>{t('lua.pageFrom')} <bdi>{from}</bdi></span>{@render children?.()}</p>
{/if}
<div class="pv" class:hide={gone || !!problem} class:wait={!drawn} class:in={drawn} bind:this={el}></div>
<style>
  .from { display: flex; align-items: center; gap: 6px; height: 24px; margin: 0; padding: 0 28px; font-size: var(--fs-xs); color: var(--muted); }
  .from :global(.icon) { flex: none; }
  .pv { height: calc(100vh - 26px - 24px); }
  /* The same entrance as a built-in page: the strip is its first section, the add-on's page the second. */
  .from { animation: rise .5s var(--ease) backwards; }
  .pv.wait { opacity: 0; }
  .pv.in { animation: rise .5s var(--ease) 50ms backwards; }
  /* Less motion: nothing is held back or slid in. */
  @media (prefers-reduced-motion: reduce) { .pv.wait { opacity: 1; } .from, .pv.in { animation: none; } }
  :global(html[data-motion='reduce']) .pv.wait { opacity: 1; }
  :global(html[data-motion='reduce']) .from, :global(html[data-motion='reduce']) .pv.in { animation: none; }
  .hide { display: none; }
  /* `ui.print` (lib/layer.ts): the page is printed whole; its frame is made as tall as its page. */
  @media print { :global(html.print-addon) .pv { height: auto; } .pv { opacity: 1; animation: none; } }
</style>
