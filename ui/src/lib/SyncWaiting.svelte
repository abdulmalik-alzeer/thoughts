<script lang="ts">
  // "3 changes waiting", beside Sync: what the background check found at the main backup location.
  // Pressing it syncs, exactly like the Sync button. Nothing shows when nothing is waiting.
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { tn } from './i18n';
  import { incoming, watchIncoming } from './incoming.svelte';

  let { onsync, disabled = false }: { onsync: () => void; disabled?: boolean } = $props();
  onMount(watchIncoming);
  const tip = $derived(tn('libf-sync.waitingTip', incoming.waiting));
</script>

{#if incoming.on && incoming.waiting > 0}
  <button type="button" class="waiting" {disabled} onclick={onsync} title={tip} aria-label={tip}>
    <Icon name="sync" size={13} />
    <span class="grow" aria-hidden="true">{tn('libf-sync.waiting', incoming.waiting)}</span>
  </button>
{/if}

<style>
  .waiting {
    display: flex; align-items: center; gap: 6px; width: 100%; padding: 5px 8px;
    font: inherit; font-size: var(--fs-xs); font-weight: 600; text-align: start; color: var(--on-tint);
    background: var(--tint); border: var(--border-w) solid color-mix(in srgb, var(--accent) 35%, transparent);
    border-radius: var(--radius); cursor: pointer;
  }
  .waiting:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 24%, transparent); }
  .waiting:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .waiting:disabled { opacity: .6; cursor: default; }
  .grow { flex: 1; min-width: 0; }
</style>
