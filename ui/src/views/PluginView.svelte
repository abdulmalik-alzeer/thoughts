<script lang="ts">
  // One page of an add-on. The add-on draws inside its own sandboxed frame; what is outside the frame
  // (the strip saying whose page this is, the "not available" notice) is drawn by the app.
  import Icon from '../lib/Icon.svelte';
  import { app } from '../lib/store.svelte';
  import { findView } from '../lib/layer';
  import { t } from '../lib/i18n';
  let { id }: { id: string } = $props();
  let el = $state<HTMLElement>();
  let problem = $state('');
  // The page as it is registered right now. Reloading the customisations (Try it, Keep, Stop preview,
  // an undo) removes every add-on's frames and registers the pages again under the same ids: the
  // frame is then mounted again here, instead of the page staying empty until it is reopened.
  const view = $derived(findView(id));
  $effect(() => {
    const v = view, into = el;
    if (!v || !into) return;
    problem = '';
    let cleanup: any;
    try { cleanup = v.render(into); } catch (e: any) { problem = e?.message ?? String(e); }
    return () => { try { if (typeof cleanup === 'function') cleanup(); } catch { /* plugin cleanup failed */ } };
  });
  // While the customisations reload the page is gone for a moment: only say so when it stays gone.
  let gone = $state(false);
  $effect(() => {
    if (view) { gone = false; return; }
    const timer = setTimeout(() => (gone = true), 1500);
    return () => clearTimeout(timer);
  });
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
  <p class="from no-print"><Icon name="plugin" size={13} /> <span>{t('lua.pageFrom')} <bdi>{from}</bdi></span></p>
{/if}
<div class="pv" class:hide={gone || !!problem} bind:this={el}></div>
<style>
  .from { display: flex; align-items: center; gap: 6px; height: 24px; margin: 0; padding: 0 28px; font-size: var(--fs-xs); color: var(--muted); }
  .from :global(.icon) { flex: none; }
  .pv { height: calc(100vh - 26px - 24px); }
  .hide { display: none; }
</style>
