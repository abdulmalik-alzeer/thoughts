<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '../lib/store.svelte';
  let { id }: { id: string } = $props();
  let el: HTMLElement;
  onMount(() => {
    const v = app.pluginViews.find((x) => x.id === id);
    let cleanup: any;
    try { cleanup = v?.render(el); } catch (e: any) { el.textContent = 'This view crashed: ' + e.message; }
    return () => typeof cleanup === 'function' && cleanup();
  });
</script>

<div class="pv" bind:this={el}></div>
<style>.pv { height: calc(100vh - 26px); }</style>
