<script lang="ts">
  // The details of one plugin (Customise › Plugins): everything it contains, and for each of its
  // add-ons and pages what it may do (with "Take away"), what it saved and the earlier copies of
  // that. The add-on rows are the ones Customise › Add-ons always had (views/AddOns.svelte).
  import Icon from '../lib/Icon.svelte';
  import { t } from '../lib/i18n';
  import { kindIcon, partText, type PluginItem } from '../lib/plugins';
  import AddOns from './AddOns.svelte';

  let { plugin, open }: { plugin: PluginItem; open: (tab: string) => void } = $props();

  const paths = $derived(plugin.contains.filter((c) => (c.kind === 'addon' || c.kind === 'page') && c.path).map((c) => c.path as string));
</script>

<div class="details">
  {#if plugin.contains.length}
    <p class="muted sm">{t('plg.contains')}</p>
    <ul class="parts">
      {#each plugin.contains as c, i (i)}
        <li class="row"><Icon name={kindIcon(c.kind)} size={14} /> <span dir="auto">{partText(c)}</span></li>
      {/each}
    </ul>
  {/if}
  {#if paths.length}
    <AddOns {open} only={paths} whole={!plugin.named} />
  {/if}
</div>

<style>
  .details { border-top: 1px solid var(--hairline); padding-top: 8px; display: grid; gap: 6px; }
  .sm { font-size: var(--fs-xs); }
  .details p { margin: 0; }
  .parts { list-style: none; margin: 0; padding: 0; display: grid; gap: 3px; font-size: var(--fs-xs); }
  .parts li { gap: 6px; align-items: center; }
  .parts :global(.icon) { color: var(--muted); flex: none; }
</style>
