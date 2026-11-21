<script lang="ts">
  // The small bar over what is selected: comment, rename, the fields of a table, delete or put back.
  import { t } from '../../../i18n';
  import { useGraph, type Target } from '../ctx';
  import { ICON } from '../icons';
  import type { Change } from '../model';

  let { target, state, hasId, withNode = false, entity = false }: { target: Target; state: Change; hasId: boolean; withNode?: boolean; entity?: boolean } = $props();
  const g = useGraph();
  const Comment = ICON.comment, Rename = ICON.rename, Label = ICON.label, Fields = ICON.fields, Remove = ICON.remove, Restore = ICON.restore;
  const node = $derived(target.kind === 'node');
</script>

<div class="bar nodrag nopan" dir={g.dir} role="toolbar" aria-label={t(node ? 'mado-graph.bar.node' : 'mado-graph.bar.edge')}>
  {#if state === 'removed'}
    {#if withNode}
      <span class="note">{t('mado-graph.bar.withNode')}</span>
    {:else}
      <span class="note">{t('mado-graph.bar.removed')}</span>
      {#if g.editable}<button type="button" onclick={() => g.restore(target)}><Restore size={14} aria-hidden="true" /> {t('mado-graph.restore')}</button>{/if}
    {/if}
  {:else}
    {#if hasId}
      <button type="button" onclick={() => g.comment(target)} title={t('mado-graph.comment.tip')}><Comment size={14} aria-hidden="true" /> {t('mado-graph.comment')}</button>
    {:else}
      <span class="note" title={t('mado-graph.bar.newTip')}>{t('mado-graph.bar.new')}</span>
    {/if}
    {#if g.editable}
      {#if node}
        <button type="button" onclick={() => g.rename(target)} title={t('mado-graph.rename.tip')}><Rename size={14} aria-hidden="true" /> {t('mado-graph.rename')}</button>
        {#if entity}<button type="button" aria-expanded={g.fieldsFor === target.key} onclick={() => g.fields(g.fieldsFor === target.key ? null : target.key)}><Fields size={14} aria-hidden="true" /> {t('mado-graph.fields')}</button>{/if}
      {:else}
        <button type="button" onclick={() => g.rename(target)}><Label size={14} aria-hidden="true" /> {t('mado-graph.relabel')}</button>
      {/if}
      <button type="button" class="danger" onclick={() => g.remove(target)} title={t('mado-graph.delete.tip')}><Remove size={14} aria-hidden="true" /> {t('delete')}</button>
    {/if}
  {/if}
</div>

<style>
  .bar {
    display: inline-flex; align-items: center; gap: 2px; padding: 3px; background: var(--panel); color: var(--fg); border: var(--border-w) solid var(--hairline);
    border-radius: 10px; box-shadow: var(--shadow-lg); font-size: 12.5px; white-space: nowrap; cursor: default;
  }
  button {
    font: inherit; font-weight: 500; display: inline-flex; align-items: center; gap: 5px; padding: 5px 8px; border: 0; border-radius: 7px; background: transparent; color: var(--fg); cursor: pointer;
  }
  button:hover { background: var(--fill-2); }
  button[aria-expanded="true"] { background: var(--tint); color: var(--on-tint); }
  .danger { color: var(--danger); }
  .note { padding: 0 8px; color: var(--muted); }
  :global([data-theme="contrast"]) .bar { border-color: var(--border); }
</style>
