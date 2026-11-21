<script lang="ts">
  // One node of a flow, an architecture or a tasks diagram. The look follows the kind; the label is always text.
  import { Handle, type NodeProps, type Position } from '@xyflow/svelte';
  import { t } from '../../../i18n';
  import { useGraph } from '../ctx';
  import type { NodeData } from '../flow';
  import { lookIcon } from '../icons';
  import { LABEL_MAX, cleanText } from '../ops';
  import Marks from './Marks.svelte';
  import NameInput from './NameInput.svelte';

  let { id, data, selected, sourcePosition, targetPosition }: NodeProps & { data: NodeData } = $props();
  const g = useGraph();
  const v = $derived(data.v);
  const Ico = $derived(lookIcon(v.look));
  const editing = $derived(g.editing?.kind === 'node' && g.editing.key === id);
  const kindName = $derived(v.look === 'other' ? (v.kind === 'other' ? t('mado-graph.kind.other') : v.kind) : t(`mado-graph.kind.${v.look}`));
  const feature = $derived(data.graph === 'tasks' ? cleanText(v.props.feature, 8) : '');
  const lane = $derived(data.graph === 'tasks' ? cleanText(v.props.lane, 16) : '');
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="card g-{data.graph} look-{v.look} st-{v.state}" class:sel={selected} class:rtl={g.dir === 'rtl'} class:dim={g.spot !== null && g.spot !== v.look}
  ondblclick={(e) => { if (g.editable) { e.stopPropagation(); g.rename({ kind: 'node', key: id }); } }}>
  {#if v.look === 'decision'}
    <svg class="shape" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true"><polygon points="50,1.5 98.5,50 50,98.5 1.5,50" vector-effect="non-scaling-stroke" /></svg>
  {:else if v.look === 'store'}
    <svg class="shape" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
      <path d="M1.5,14 C1.5,-2.5 98.5,-2.5 98.5,14 L98.5,86 C98.5,102.5 1.5,102.5 1.5,86 Z" vector-effect="non-scaling-stroke" />
      <path class="line" d="M1.5,14 C1.5,30.5 98.5,30.5 98.5,14" vector-effect="non-scaling-stroke" />
    </svg>
  {:else if v.look === 'screen' || v.look === 'client'}
    <span class="chrome" aria-hidden="true"><i></i><i></i><i></i></span>
  {/if}
  <div class="body">
    {#if v.look !== 'decision'}<span class="ico"><Ico size={16} strokeWidth={1.9} aria-hidden="true" /></span>{/if}
    <div class="txt">
      {#if v.look === 'decision'}<span class="ico small"><Ico size={13} strokeWidth={2} aria-hidden="true" /></span>{/if}
      {#if editing}
        <NameInput value={v.label} max={LABEL_MAX} label={t('mado-graph.name')} oncommit={(text) => g.commit({ kind: 'node', key: id }, text)} oncancel={g.cancel} />
      {:else}
        <span class="lbl" dir="auto" title={v.label}>{v.label}</span>
      {/if}
      {#if data.graph === 'tasks'}
        <span class="chips">
          {#if feature}<span class="chip strong" dir="ltr" title={t('mado-graph.feature.tip', { id: feature })}>{feature}</span>{/if}
          {#if lane}<span class="chip" title={t('mado-graph.track.tip')}>{t('mado-graph.track', { id: lane })}</span>{/if}
          {#if v.id}<span class="chip mono id" dir="ltr" title={v.id}>{v.id}</span>{/if}
        </span>
      {:else if v.look !== 'decision' && v.look !== 'start' && v.look !== 'end'}
        <span class="kind" dir="auto">{kindName}</span>
      {/if}
    </div>
  </div>
  <span class="marks"><Marks state={v.state} was={v.was} marks={data.marks} /></span>
  <Handle type="target" position={targetPosition as Position} />
  <Handle type="source" position={sourcePosition as Position} />
</div>

<style>
  .card {
    --k: var(--accent);
    position: relative; width: 100%; height: 100%; box-sizing: border-box; display: flex; align-items: center; color: var(--fg);
    background: var(--panel); border: var(--border-w) solid color-mix(in srgb, var(--k) 42%, var(--border)); border-radius: 12px;
    box-shadow: var(--shadow); transition: box-shadow .15s, border-color .15s, opacity .15s; font-size: 13px; line-height: 1.3;
  }
  .card:hover { border-color: color-mix(in srgb, var(--k) 75%, var(--border)); }
  .card.sel { border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 30%, transparent), var(--shadow); }
  .card.dim { opacity: .28; }
  .look-decision, .look-queue { --k: var(--accent-2); }
  .look-action, .look-service { --k: color-mix(in srgb, var(--accent) 50%, var(--accent-2)); }
  .look-start, .look-store { --k: var(--ok); }
  .look-end { --k: var(--fg); }
  .look-external, .look-other { --k: var(--muted); }

  .body { position: relative; display: flex; align-items: center; gap: 10px; padding: 8px 12px; min-width: 0; width: 100%; }
  .ico {
    flex: none; display: grid; place-items: center; width: 30px; height: 30px; border-radius: 9px;
    background: color-mix(in srgb, var(--k) 14%, transparent); color: color-mix(in srgb, var(--k) 72%, var(--fg));
  }
  .ico.small { width: auto; height: auto; background: none; margin: 0 auto 2px; }
  .txt { min-width: 0; flex: 1; display: grid; gap: 2px; text-align: left; }
  /* The canvas is laid out left to right in every language; inside a card, text and icon follow the app's language. */
  .rtl .body { flex-direction: row-reverse; }
  .rtl .txt { text-align: right; }
  .rtl .chips { flex-direction: row-reverse; }
  .lbl { font-weight: 600; overflow: hidden; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow-wrap: anywhere; }
  .kind { font-size: 11px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .marks { position: absolute; top: -10px; right: 10px; display: flex; gap: 4px; pointer-events: none; }

  /* Screens and apps: a small window. */
  .look-screen, .look-client { padding-top: 14px; }
  .chrome { position: absolute; top: 0; left: 0; right: 0; height: 14px; display: flex; align-items: center; gap: 3px; padding: 0 8px;
    border-bottom: 1px solid color-mix(in srgb, var(--k) 22%, var(--hairline)); background: color-mix(in srgb, var(--k) 8%, transparent); border-radius: 11px 11px 0 0; }
  .chrome i { width: 5px; height: 5px; border-radius: 50%; background: color-mix(in srgb, var(--k) 50%, var(--muted)); }

  /* Decisions: a diamond. Data stores: a cylinder. Both are drawn, so the edges stay sharp at any zoom. */
  .look-decision, .look-store { background: none; border-color: transparent; box-shadow: none; }
  .look-decision:hover, .look-store:hover, .look-decision.sel, .look-store.sel { border-color: transparent; box-shadow: none; }
  .shape { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; filter: drop-shadow(0 1px 2px color-mix(in srgb, var(--fg) 10%, transparent)); }
  .shape polygon, .shape path { fill: var(--panel); stroke: color-mix(in srgb, var(--k) 48%, var(--border)); stroke-width: 1.25; }
  .shape .line { fill: none; }
  .sel .shape polygon, .sel .shape path { stroke: var(--accent); stroke-width: 2.5; }
  .look-decision .body { padding: 8px 34px; }
  .look-decision .txt { text-align: center; }
  .look-decision .lbl { font-size: 12.5px; }
  .look-store .body { padding-top: 22px; }

  /* Actions, starts and ends: pills. */
  .look-action, .look-start, .look-end, .look-queue { border-radius: 999px; }
  .look-action .body, .look-queue .body { padding-inline: 14px; }
  .look-start .body, .look-end .body { padding: 6px 10px; gap: 8px; }
  .look-start .ico, .look-end .ico { width: 26px; height: 26px; border-radius: 50%; }
  .look-start .ico { background: var(--k); color: var(--panel); }
  .look-end { border-width: calc(var(--border-w) + 1.5px); }
  .look-service { border-inline-start-width: 4px; }
  .look-external { border-style: double; border-width: 4px; }
  .look-queue::after { content: ''; position: absolute; top: 12px; bottom: 12px; right: 16px; width: 14px; pointer-events: none;
    background: repeating-linear-gradient(90deg, color-mix(in srgb, var(--k) 45%, transparent) 0 2px, transparent 2px 6px); }
  .look-queue .body, .look-queue.rtl .body { padding-right: 38px; }

  /* Tasks. */
  .g-tasks { align-items: stretch; }
  .g-tasks .body { align-items: flex-start; padding: 10px 12px; }
  .chips { display: flex; gap: 4px; margin-top: 4px; min-width: 0; overflow: hidden; }
  .chip { flex: none; font-size: 11px; line-height: 1.2; padding: 3px 6px; border-radius: 6px; background: var(--fill); color: var(--muted); white-space: nowrap; }
  .chip.id { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .chip.mono { font-family: var(--mono); }
  .chip.strong { background: var(--tint); color: var(--on-tint); font-weight: 600; }

  /* Pending edits. */
  .st-added { border-style: dashed; border-color: var(--accent); background: color-mix(in srgb, var(--accent) 6%, var(--panel)); }
  .st-added .shape polygon, .st-added .shape path { stroke: var(--accent); stroke-dasharray: 5 4; }
  .st-removed { opacity: .5; border-style: dashed; border-color: color-mix(in srgb, var(--danger) 60%, var(--border)); }
  .st-removed .lbl { text-decoration: line-through; }
  .st-removed .shape polygon, .st-removed .shape path { stroke: var(--danger); stroke-dasharray: 5 4; }
  .st-removed .marks { opacity: 1; }

  .card :global(.svelte-flow__handle) {
    width: 11px; height: 11px; border-radius: 50%; background: var(--panel); border: 2px solid var(--accent); opacity: 0; transition: opacity .12s;
  }
  .card:hover :global(.svelte-flow__handle.connectionindicator), .card.sel :global(.svelte-flow__handle.connectionindicator),
  .card :global(.svelte-flow__handle.connectingto), .card :global(.svelte-flow__handle.connectingfrom) { opacity: 1; }
  .card :global(.svelte-flow__handle)::after { content: ''; position: absolute; inset: -9px; border-radius: 50%; }
  .card :global(.svelte-flow__handle.valid) { background: var(--accent); }
  :global([data-motion="reduce"]) .card, :global([data-motion="reduce"]) .card :global(.svelte-flow__handle) { transition: none; }
  @media (prefers-reduced-motion: reduce) { .card, .card :global(.svelte-flow__handle) { transition: none; } }
  :global([data-theme="contrast"]) .card { border-color: var(--border); }
  :global([data-theme="contrast"]) .card.sel { border-color: var(--accent); outline: 3px solid var(--accent); }
</style>
