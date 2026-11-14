<script lang="ts">
  // An add-on page shown in place of one of the app's own (`replace` in `pages.json`). The app's own
  // page is always one press away, and it comes back by itself when the add-on page cannot be shown.
  import PluginView from './PluginView.svelte';
  import { t } from '../lib/i18n';
  import { pageSwap, showOriginal } from '../lib/pages.svelte';
  let { page }: { page: string } = $props();
  const view = $derived(pageSwap(page));
</script>

{#if view}
  {#key view.id}
    <PluginView id={view.id} onfail={() => showOriginal(page)}>
      <span class="sep" aria-hidden="true">·</span>
      <button class="link" onclick={() => showOriginal(page)}>{t('r10-cust-pages.showOriginal')}</button>
    </PluginView>
  {/key}
{/if}

<style>
  .sep { margin-inline: 2px; }
  .link { background: none; border: 0; padding: 0; color: var(--accent); font: inherit; cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .link:hover { color: var(--fg); }
</style>
