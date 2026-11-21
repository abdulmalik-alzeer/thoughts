<script lang="ts">
  // The small marks on a node or an edge: what is pending on it, and how many comments it has.
  import { t, tn } from '../../../i18n';
  import { ICON } from '../icons';
  import type { Change, Marks } from '../model';
  let { state, was, marks, withNode = false }: { state: Change; was?: string; marks: Marks | null; withNode?: boolean } = $props();
  const Comment = ICON.comment;
</script>

{#if state !== 'same' && !withNode}
  <span class="mark st-{state}" title={state === 'changed' && was ? t('mado-graph.was', { name: was }) : t(`mado-graph.pending.${state}.tip`)}>{t(`mado-graph.pending.${state}`)}</span>
{/if}
{#if marks}
  <span class="mark notes" class:open={marks.open > 0} title={marks.open ? tn('mado-graph.comments.open', marks.open) : tn('mado-graph.comments', marks.total)}>
    <Comment size={11} strokeWidth={2.2} aria-hidden="true" /><span>{marks.total}</span>
  </span>
{/if}

<style>
  .mark {
    display: inline-flex; align-items: center; gap: 3px; height: 18px; padding: 0 7px; border-radius: 999px; font-size: 11px; font-weight: 600; line-height: 1;
    white-space: nowrap; background: var(--panel); color: var(--muted); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow);
  }
  .st-added { color: var(--accent-fg); background: var(--accent); border-color: var(--accent); }
  .st-changed { color: var(--on-tint); background: color-mix(in srgb, var(--accent) 18%, var(--panel)); border-color: color-mix(in srgb, var(--accent) 50%, transparent); }
  .st-removed { color: var(--danger); border-color: color-mix(in srgb, var(--danger) 55%, transparent); }
  .notes.open { color: var(--accent-fg); background: var(--accent); border-color: var(--accent); }
  :global([data-theme="contrast"]) .mark { border-color: var(--border); }
</style>
