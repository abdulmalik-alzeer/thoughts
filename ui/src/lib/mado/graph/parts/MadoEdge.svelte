<script lang="ts">
  // One edge: a curve with an arrow head (or, in the data model, the marks that say "one" or "many" at each end),
  // a label that is always text, and the marks of what is pending on it.
  import { EdgeLabel, EdgeToolbar, getBezierPath, getSmoothStepPath, type EdgeProps } from '@xyflow/svelte';
  import { t } from '../../../i18n';
  import { useGraph } from '../ctx';
  import type { EdgeData } from '../flow';
  import { cardinality } from '../model';
  import { EDGE_LABEL_MAX } from '../ops';
  import Marks from './Marks.svelte';
  import NameInput from './NameInput.svelte';
  import SelBar from './SelBar.svelte';

  let { id, data, selected, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition }: EdgeProps & { data?: EdgeData } = $props();
  const g = useGraph();
  const v = $derived(data!.v);
  const erd = $derived(data!.graph === 'erd');
  const loop = $derived(v.from === v.to);
  const geo = $derived.by(() => {
    if (loop) {
      // An entity related to itself: a small loop out of its side and back in.
      const r = 46;
      return [`M${sourceX},${sourceY} C${sourceX + r},${sourceY - r * 1.6} ${targetX - r},${targetY - r * 1.6} ${targetX},${targetY}`, (sourceX + targetX) / 2, Math.min(sourceY, targetY) - r * 1.2] as const;
    }
    const p = { sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition };
    const [d, x, y] = erd ? getSmoothStepPath({ ...p, borderRadius: 10 }) : getBezierPath(p);
    return [d, x, y] as const;
  });
  const ANGLE: Record<string, number> = { right: 0, bottom: 90, left: 180, top: 270 };
  const ends = $derived(erd ? cardinality(v.kind) : null);
  const editing = $derived(g.editing?.kind === 'edge' && g.editing.key === id);
  const showLabel = $derived(!!v.label || editing || !!data!.marks || (v.state !== 'same' && !v.withNode));
</script>

<g class="edge st-{v.state}" class:sel={selected} class:with-node={v.withNode}>
  <path class="line" d={geo[0]} fill="none" />
  {#if ends}
    {#each [[sourceX, sourceY, sourcePosition, ends.from], [targetX, targetY, targetPosition, ends.to]] as [x, y, pos, how], i (i)}
      <g transform="translate({x},{y}) rotate({ANGLE[pos as string] ?? 0})" class="end">
        {#if how === 'many'}<path d="M13,0 L0,-7 M13,0 L0,7 M13,0 L0,0 M17,-6 L17,6" />{:else}<path d="M9,-6 L9,6 M14,-6 L14,6" />{/if}
      </g>
    {/each}
  {:else if !erd}
    <path class="head" d="M10,-5 L0,0 L10,5 Z" transform="translate({targetX},{targetY}) rotate({ANGLE[targetPosition as string] ?? 180})" />
  {/if}
  <path class="svelte-flow__edge-interaction" d={geo[0]} fill="none" stroke-opacity="0" stroke-width="22" />
</g>

{#if showLabel}
  <EdgeLabel x={geo[1]} y={geo[2]} selectEdgeOnClick transparent class="mado-edge-label">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <span class="pill st-{v.state}" class:sel={selected} class:bare={!v.label && !editing} ondblclick={(e) => { if (g.editable) { e.stopPropagation(); g.rename({ kind: 'edge', key: id }); } }}>
      {#if editing}
        <NameInput value={v.label} max={EDGE_LABEL_MAX} label={t('mado-graph.edgeLabel')} oncommit={(text) => g.commit({ kind: 'edge', key: id }, text)} oncancel={g.cancel} />
      {:else if v.label}
        <span class="text" dir="auto" title={v.label}>{v.label}</span>
      {/if}
      <Marks state={v.state} was={v.was} marks={data!.marks} withNode={v.withNode} />
    </span>
  </EdgeLabel>
{/if}

{#if selected && !editing}
  <EdgeToolbar x={geo[1]} y={geo[2] - 18} alignY="bottom" isVisible>
    <SelBar target={{ kind: 'edge', key: id }} state={v.state} hasId={!!v.id} withNode={v.withNode} />
  </EdgeToolbar>
{/if}

<style>
  .line { stroke: color-mix(in srgb, var(--fg) 42%, var(--bg)); stroke-width: 1.6; transition: stroke .12s; }
  .head { fill: color-mix(in srgb, var(--fg) 42%, var(--bg)); stroke: none; }
  .end path { fill: none; stroke: color-mix(in srgb, var(--fg) 55%, var(--bg)); stroke-width: 1.6; stroke-linecap: round; }
  .edge:hover .line { stroke: color-mix(in srgb, var(--fg) 70%, var(--bg)); stroke-width: 2; }
  .edge:hover .head { fill: color-mix(in srgb, var(--fg) 70%, var(--bg)); }
  .sel .line { stroke: var(--accent); stroke-width: 2.4; }
  .sel .head { fill: var(--accent); }
  .sel .end path { stroke: var(--accent); stroke-width: 2; }
  .st-added .line { stroke: var(--accent); stroke-dasharray: 6 5; }
  .st-added .head { fill: var(--accent); }
  .st-changed .line { stroke: color-mix(in srgb, var(--accent) 60%, var(--fg)); }
  .st-removed .line { stroke: var(--danger); stroke-dasharray: 3 5; opacity: .6; }
  .st-removed .head, .st-removed .end { opacity: .45; }
  .st-removed.with-node .line { stroke: color-mix(in srgb, var(--fg) 40%, var(--bg)); opacity: .4; }

  .pill {
    display: inline-flex; align-items: center; gap: 4px; max-width: 220px; padding: 2px 8px; border-radius: 999px; font-size: 12px; line-height: 1.35;
    background: var(--panel); color: var(--fg); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow);
  }
  .pill.bare { padding: 0; background: none; border: 0; box-shadow: none; }
  .pill.sel:not(.bare) { border-color: var(--accent); }
  .text { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; unicode-bidi: plaintext; }
  .pill.st-removed .text { text-decoration: line-through; color: var(--muted); }
  :global([data-motion="reduce"]) .line { transition: none; }
  @media (prefers-reduced-motion: reduce) { .line { transition: none; } }
  :global([data-theme="contrast"]) .pill:not(.bare) { border-color: var(--border); }
</style>
