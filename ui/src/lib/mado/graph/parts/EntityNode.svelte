<script lang="ts">
  // One table of the data model: its name, then its fields with their types and key marks.
  import { Handle, type NodeProps, type Position } from '@xyflow/svelte';
  import { t, tn } from '../../../i18n';
  import { useGraph } from '../ctx';
  import type { NodeData } from '../flow';
  import { ICON, lookIcon } from '../icons';
  import { ERD_ROWS_SHOWN } from '../layout';
  import { LABEL_MAX } from '../ops';
  import Marks from './Marks.svelte';
  import NameInput from './NameInput.svelte';

  let { id, data, selected, sourcePosition, targetPosition }: NodeProps & { data: NodeData } = $props();
  const g = useGraph();
  const v = $derived(data.v);
  const Ico = lookIcon('entity');
  const Pk = ICON.pk, Fk = ICON.fk;
  const editing = $derived(g.editing?.kind === 'node' && g.editing.key === id);
  const rows = $derived(v.fields.slice(0, ERD_ROWS_SHOWN));
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="table st-{v.state}" class:sel={selected} class:rtl={g.dir === 'rtl'} ondblclick={(e) => { if (g.editable) { e.stopPropagation(); g.rename({ kind: 'node', key: id }); } }}>
  <div class="head">
    <span class="ico"><Ico size={14} strokeWidth={2} aria-hidden="true" /></span>
    {#if editing}
      <NameInput value={v.label} max={LABEL_MAX} label={t('mado-graph.name')} oncommit={(text) => g.commit({ kind: 'node', key: id }, text)} oncancel={g.cancel} />
    {:else}
      <span class="lbl" dir="auto" title={v.label}>{v.label}</span>
    {/if}
  </div>
  <ul class="rows">
    {#each rows as f, i (i)}
      <li title={f.note || undefined}>
        <span class="key" class:pk={f.key === 'pk'} class:fk={f.key === 'fk'}>
          {#if f.key === 'pk'}<Pk size={11} strokeWidth={2.2} aria-hidden="true" /><span title={t('mado-graph.key.pk.tip')}>{t('mado-graph.key.pk')}</span>
          {:else if f.key === 'fk'}<Fk size={11} strokeWidth={2.2} aria-hidden="true" /><span title={t('mado-graph.key.fk.tip')}>{t('mado-graph.key.fk')}</span>{/if}
        </span>
        <span class="fname" dir="auto">{f.name}</span>
        <span class="ftype" dir="ltr">{f.type}</span>
      </li>
    {:else}
      <li class="none">{t('mado-graph.fields.none')}</li>
    {/each}
    {#if v.fields.length > ERD_ROWS_SHOWN}<li class="none">{tn('mado-graph.fields.more', v.fields.length - ERD_ROWS_SHOWN)}</li>{/if}
  </ul>
  <span class="marks"><Marks state={v.state} was={v.was} marks={data.marks} /></span>
  <Handle type="target" position={targetPosition as Position} />
  <Handle type="source" position={sourcePosition as Position} />
</div>

<style>
  .table {
    position: relative; width: 100%; height: 100%; box-sizing: border-box; display: flex; flex-direction: column; color: var(--fg); background: var(--panel);
    border: var(--border-w) solid color-mix(in srgb, var(--accent) 38%, var(--border)); border-radius: 12px; box-shadow: var(--shadow);
    font-size: 13px; line-height: 1.3; transition: box-shadow .15s, border-color .15s;
  }
  .table:hover { border-color: color-mix(in srgb, var(--accent) 70%, var(--border)); }
  .table.sel { border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 30%, transparent), var(--shadow); }
  .head {
    flex: none; height: 42px; box-sizing: border-box; display: flex; align-items: center; gap: 8px; padding: 0 12px; border-radius: 11px 11px 0 0;
    background: color-mix(in srgb, var(--accent) 10%, var(--panel)); border-bottom: 1px solid color-mix(in srgb, var(--accent) 28%, var(--hairline));
  }
  .ico { flex: none; display: grid; place-items: center; color: color-mix(in srgb, var(--accent) 72%, var(--fg)); }
  .lbl { flex: 1; min-width: 0; font-weight: 650; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; text-align: left; }
  .rtl .head, .rtl .rows li { flex-direction: row-reverse; }
  .rtl .lbl, .rtl .fname { text-align: right; }
  .rtl .rows li { padding: 0 8px 0 12px; }
  .rows { list-style: none; margin: 0; padding: 4px 0; flex: 1; min-height: 0; overflow: hidden; }
  .rows li { height: 26px; box-sizing: border-box; display: flex; align-items: center; gap: 6px; padding: 0 12px 0 8px; }
  .rows li + li { border-top: 1px solid color-mix(in srgb, var(--fg) 5%, transparent); }
  .key { flex: none; width: 34px; display: inline-flex; align-items: center; gap: 2px; font-size: 9.5px; font-weight: 700; color: var(--muted); }
  .key.pk { color: color-mix(in srgb, var(--accent-2) 80%, var(--fg)); }
  .key.fk { color: color-mix(in srgb, var(--accent) 75%, var(--fg)); }
  .fname { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; text-align: left; }
  .ftype { flex: none; max-width: 45%; font-family: var(--mono); font-size: 11px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .none { color: var(--muted); font-size: 12px; padding-inline: 12px !important; }
  .marks { position: absolute; top: -10px; right: 10px; display: flex; gap: 4px; pointer-events: none; }
  .st-added { border-style: dashed; border-color: var(--accent); }
  .st-removed { opacity: .5; border-style: dashed; border-color: color-mix(in srgb, var(--danger) 60%, var(--border)); }
  .st-removed .lbl { text-decoration: line-through; }
  .table :global(.svelte-flow__handle) {
    width: 11px; height: 11px; border-radius: 50%; background: var(--panel); border: 2px solid var(--accent); opacity: 0; transition: opacity .12s; top: 21px;
  }
  .table:hover :global(.svelte-flow__handle.connectionindicator), .table.sel :global(.svelte-flow__handle.connectionindicator),
  .table :global(.svelte-flow__handle.connectingto), .table :global(.svelte-flow__handle.connectingfrom) { opacity: 1; }
  .table :global(.svelte-flow__handle)::after { content: ''; position: absolute; inset: -9px; border-radius: 50%; }
  :global([data-motion="reduce"]) .table { transition: none; }
  @media (prefers-reduced-motion: reduce) { .table { transition: none; } }
  :global([data-theme="contrast"]) .table { border-color: var(--border); }
  :global([data-theme="contrast"]) .table.sel { border-color: var(--accent); outline: 3px solid var(--accent); }
</style>
