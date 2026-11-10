<script lang="ts">
  // Actions that plugins added to todos / ideas (`ui.addItemAction`), as a small menu on each item.
  import Icon from './Icon.svelte';
  import { app } from './store.svelte';
  import { t } from './i18n';

  let { target, item, name = '' }: { target: 'todo' | 'idea'; item: any; name?: string } = $props();
  const actions = $derived(app.itemActions.filter((a) => a.target === target));
  let open = $state(false);
  let root = $state<HTMLElement>();

  function run(a: (typeof actions)[number]) {
    open = false;
    a.run(item);
  }
  function onKey(e: KeyboardEvent) { if (e.key === 'Escape' && open) { e.stopPropagation(); open = false; (root?.querySelector('button') as HTMLElement | null)?.focus(); } }
</script>

<svelte:window onclick={(e) => { if (open && root && !root.contains(e.target as Node)) open = false; }} />

{#if actions.length}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <span class="ia" bind:this={root} onkeydown={onKey}>
    <button type="button" class="ib" aria-haspopup="menu" aria-expanded={open} aria-label={t('layer.itemActions', { name })} title={t('layer.itemActionsTip')}
      onclick={(e) => { e.stopPropagation(); e.preventDefault(); open = !open; }}><Icon name="plugin" size={14} /></button>
    {#if open}
      <span class="menu" role="menu">
        {#each actions as a (a.plugin + ':' + a.id)}
          <button type="button" role="menuitem" class="mi" dir="auto" onclick={(e) => { e.stopPropagation(); e.preventDefault(); run(a); }}>{a.label}</button>
        {/each}
      </span>
    {/if}
  </span>
{/if}

<style>
  .ia { position: relative; display: inline-flex; }
  .ib { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 5px; cursor: pointer; display: grid; place-items: center; }
  .ib:hover, .ib[aria-expanded="true"] { color: var(--fg); background: var(--fill); }
  .menu { position: absolute; inset-block-start: calc(100% + 4px); inset-inline-end: 0; z-index: 20; min-width: 180px; max-width: 280px; padding: 4px; display: grid; gap: 1px;
    background: var(--panel); border: var(--border-w) solid var(--hairline); border-radius: var(--radius); box-shadow: var(--shadow-lg); }
  .mi { font: inherit; font-size: 13px; color: var(--fg); background: none; border: 0; text-align: start; padding: 6px 9px; border-radius: 6px; cursor: pointer; overflow-wrap: anywhere; }
  .mi:hover, .mi:focus-visible { background: var(--fill); }
</style>
