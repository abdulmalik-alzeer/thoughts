<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  let { id }: { id: string } = $props();
  let el: HTMLElement;
  let problem = $state('');
  const gone = $derived(!app.pluginViews.some((x) => x.id === id));
  onMount(() => {
    const v = app.pluginViews.find((x) => x.id === id);
    if (!v) return;
    let cleanup: any;
    try { cleanup = v.render(el); } catch (e: any) { problem = e?.message ?? String(e); }
    return () => { try { if (typeof cleanup === 'function') cleanup(); } catch { /* plugin cleanup failed */ } };
  });
</script>

{#if gone || problem}
  <div class="page" role="alert">
    <h1>{t('shell.pv.title')}</h1>
    <p class="muted">{t('shell.pv.body')}</p>
    {#if problem}<pre dir="auto">{problem}</pre>{/if}
    <button class="btn" onclick={() => (app.showRollback = true)}>{t('shell.openRollback')}</button>
  </div>
{/if}
<div class="pv" class:hide={gone || !!problem} bind:this={el}></div>
<style>.pv { height: calc(100vh - 26px); } .hide { display: none; }</style>
