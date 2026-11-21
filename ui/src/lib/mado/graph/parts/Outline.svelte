<script lang="ts">
  // The same diagram as a list, for the keyboard and for screen readers: every node with what it leads to and what
  // leads to it, and the same actions as on the map.
  import { t, tn } from '../../../i18n';
  import { useGraph, type Target } from '../ctx';
  import type { Selection } from '../flow';
  import { ICON, lookIcon } from '../icons';
  import { cardinality, type Marks, type VEdge, type VNode, type View } from '../model';
  import { EDGE_LABEL_MAX, LABEL_MAX } from '../ops';
  import NameInput from './NameInput.svelte';

  let { view, order, marks, sel, onpick }: {
    view: View; order: string[]; marks: { nodes: Map<string, Marks>; edges: Map<string, Marks> }; sel: Selection; onpick: (t: Target) => void;
  } = $props();
  const g = useGraph();
  const STEP = 60;
  let max = $state(STEP);
  const by = $derived(new Map(view.nodes.map((n) => [n.key, n])));
  const out = $derived.by(() => { const m = new Map<string, VEdge[]>(); for (const e of view.edges) m.set(e.from, [...(m.get(e.from) ?? []), e]); return m; });
  const inn = $derived.by(() => { const m = new Map<string, VEdge[]>(); for (const e of view.edges) m.set(e.to, [...(m.get(e.to) ?? []), e]); return m; });
  const kindName = (n: VNode) => (n.look === 'other' ? (n.kind === 'other' ? t('mado-graph.kind.other') : n.kind) : t(`mado-graph.kind.${n.look}`));
  const ends = (e: VEdge) => { const c = view.kind === 'erd' ? cardinality(e.kind) : null; return c ? t(`mado-graph.rel.${c.from}-${c.to}`) : ''; };
  const Comment = ICON.comment, Rename = ICON.rename, Remove = ICON.remove, Restore = ICON.restore, Label = ICON.label;
  const isEditing = (kind: 'node' | 'edge', key: string) => g.editing?.kind === kind && g.editing.key === key;
</script>

{#snippet edgeRow(e: VEdge, other: string)}
  {@const m = e.id ? marks.edges.get(e.id) : null}
  <li class="e st-{e.state}" class:on={sel?.kind === 'edge' && sel.key === e.key}>
    <button type="button" class="lnk" dir="auto" onclick={() => onpick({ kind: 'node', key: other })}>{by.get(other)?.label ?? other}</button>
    {#if isEditing('edge', e.key)}
      <span class="edit"><NameInput value={e.label} max={EDGE_LABEL_MAX} label={t('mado-graph.edgeLabel')} oncommit={(text) => g.commit({ kind: 'edge', key: e.key }, text)} oncancel={g.cancel} /></span>
    {:else if e.label}<span class="elabel" dir="auto">“{e.label}”</span>{/if}
    {#if ends(e)}<span class="muted">{ends(e)}</span>{/if}
    {#if e.state !== 'same'}<span class="tag st-{e.state}">{t(`mado-graph.pending.${e.state}`)}</span>{/if}
    {#if m}<span class="tag" class:open={m.open > 0}>{tn('mado-graph.comments', m.total)}</span>{/if}
    <span class="acts">
      {#if e.state === 'removed'}
        {#if g.editable && !e.withNode}<button type="button" class="ib" onclick={() => g.restore({ kind: 'edge', key: e.key })} aria-label={t('mado-graph.restore')} title={t('mado-graph.restore')}><Restore size={13} aria-hidden="true" /></button>{/if}
      {:else}
        {#if e.id}<button type="button" class="ib" onclick={() => g.comment({ kind: 'edge', key: e.key })} aria-label={t('mado-graph.comment')} title={t('mado-graph.comment')}><Comment size={13} aria-hidden="true" /></button>{/if}
        {#if g.editable}
          <button type="button" class="ib" onclick={() => g.rename({ kind: 'edge', key: e.key })} aria-label={t('mado-graph.relabel')} title={t('mado-graph.relabel')}><Label size={13} aria-hidden="true" /></button>
          <button type="button" class="ib danger" onclick={() => g.remove({ kind: 'edge', key: e.key })} aria-label={t('delete')} title={t('delete')}><Remove size={13} aria-hidden="true" /></button>
        {/if}
      {/if}
    </span>
  </li>
{/snippet}

<section class="outline" dir={g.dir} aria-label={t('mado-graph.list.label')}>
  <p class="muted count">{tn('mado-graph.list.count', view.nodes.length)}</p>
  <ul class="nodes">
    {#each order.slice(0, max) as key (key)}
      {@const n = by.get(key)!}
      {@const Ico = lookIcon(n.look)}
      {@const m = n.id ? marks.nodes.get(n.id) : null}
      <li class="n st-{n.state}" class:on={sel?.kind === 'node' && sel.key === key} data-key={key}>
        <div class="row">
          <span class="ico"><Ico size={15} aria-hidden="true" /></span>
          {#if isEditing('node', key)}
            <span class="edit"><NameInput value={n.label} max={LABEL_MAX} label={t('mado-graph.name')} oncommit={(text) => g.commit({ kind: 'node', key }, text)} oncancel={g.cancel} /></span>
          {:else}
            <button type="button" class="name" dir="auto" aria-current={sel?.kind === 'node' && sel.key === key ? 'true' : undefined} onclick={() => onpick({ kind: 'node', key })}>{n.label}</button>
          {/if}
          <span class="muted" dir="auto">{kindName(n)}</span>
          {#if n.state !== 'same'}<span class="tag st-{n.state}">{t(`mado-graph.pending.${n.state}`)}</span>{/if}
          {#if m}<span class="tag" class:open={m.open > 0}>{tn('mado-graph.comments', m.total)}</span>{/if}
          <span class="acts">
            {#if n.state === 'removed'}
              {#if g.editable}<button type="button" class="ib" onclick={() => g.restore({ kind: 'node', key })} aria-label={t('mado-graph.restore')} title={t('mado-graph.restore')}><Restore size={14} aria-hidden="true" /></button>{/if}
            {:else}
              {#if n.id}<button type="button" class="ib" onclick={() => g.comment({ kind: 'node', key })} aria-label={t('mado-graph.comment')} title={t('mado-graph.comment')}><Comment size={14} aria-hidden="true" /></button>{/if}
              {#if g.editable}
                <button type="button" class="ib" onclick={() => g.rename({ kind: 'node', key })} aria-label={t('mado-graph.rename')} title={t('mado-graph.rename')}><Rename size={14} aria-hidden="true" /></button>
                <button type="button" class="ib danger" onclick={() => g.remove({ kind: 'node', key })} aria-label={t('delete')} title={t('delete')}><Remove size={14} aria-hidden="true" /></button>
              {/if}
            {/if}
          </span>
        </div>
        {#if view.kind === 'erd' && n.fields.length}
          <p class="fields"><span class="muted">{t('mado-graph.fields')}{t('mado-graph.list.colon')}</span>
            {#each n.fields as f, i (i)}<span class="f"><span dir="auto">{f.name}</span>{#if f.type} <code dir="ltr">{f.type}</code>{/if}{#if f.key} <b>{t(`mado-graph.key.${f.key}`)}</b>{/if}</span>{/each}
          </p>
        {/if}
        {#if out.get(key)?.length}
          <div class="links"><span class="muted">{t(`mado-graph.list.to.${view.kind}`)}</span><ul>{#each out.get(key) ?? [] as e (e.key)}{@render edgeRow(e, e.to)}{/each}</ul></div>
        {/if}
        {#if inn.get(key)?.length}
          <div class="links"><span class="muted">{t(`mado-graph.list.from.${view.kind}`)}</span><ul>{#each inn.get(key) ?? [] as e (e.key)}
            <li class="e st-{e.state}"><button type="button" class="lnk" dir="auto" onclick={() => onpick({ kind: 'node', key: e.from })}>{by.get(e.from)?.label ?? e.from}</button>{#if e.label}<span class="elabel" dir="auto">“{e.label}”</span>{/if}</li>
          {/each}</ul></div>
        {/if}
      </li>
    {/each}
  </ul>
  {#if order.length > max}<button type="button" class="btn" onclick={() => (max += STEP * 2)}>{tn('mado-graph.list.more', Math.min(STEP * 2, order.length - max))}</button>{/if}
</section>

<style>
  .outline { height: 100%; overflow: auto; padding: 52px 14px 14px; box-sizing: border-box; font-size: 13px; text-align: start; }
  .count { margin: 0 0 8px; }
  .muted { color: var(--muted); }
  ul { list-style: none; margin: 0; padding: 0; }
  .nodes { display: grid; gap: 6px; max-width: 860px; }
  .n { padding: 8px 10px; border-radius: var(--radius); border: var(--border-w) solid var(--hairline); background: var(--panel); }
  .n.on { border-color: var(--accent); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 25%, transparent); }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .ico { display: grid; place-items: center; color: var(--muted); }
  .name { font: inherit; font-weight: 600; color: var(--fg); background: none; border: 0; padding: 2px 4px; border-radius: 5px; cursor: pointer; text-align: start; overflow-wrap: anywhere; }
  .name:hover, .lnk:hover { background: var(--fill); }
  .edit { flex: 1; min-width: 140px; max-width: 320px; }
  .acts { margin-inline-start: auto; display: inline-flex; gap: 2px; }
  .ib { display: grid; place-items: center; width: 26px; height: 26px; border: 0; border-radius: 6px; background: transparent; color: var(--muted); cursor: pointer; }
  .ib:hover { background: var(--fill-2); color: var(--fg); }
  .ib.danger:hover { color: var(--danger); }
  .tag { font-size: 11px; font-weight: 600; padding: 1px 7px; border-radius: 999px; background: var(--fill); color: var(--muted); }
  .tag.st-added, .tag.open { background: var(--accent); color: var(--accent-fg); }
  .tag.st-changed { background: var(--tint); color: var(--on-tint); }
  .tag.st-removed { color: var(--danger); }
  .st-removed > .row .name, .e.st-removed .lnk { text-decoration: line-through; color: var(--muted); }
  .fields { margin: 6px 0 0; display: flex; flex-wrap: wrap; gap: 4px 10px; padding-inline-start: 23px; }
  .f code { font-size: 11px; color: var(--muted); background: none; padding: 0; }
  .f b { font-size: 10px; color: var(--on-tint); }
  .links { display: flex; gap: 8px; align-items: baseline; margin-top: 4px; padding-inline-start: 23px; }
  .links ul { display: grid; gap: 2px; flex: 1; min-width: 0; }
  .e { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; border-radius: 6px; }
  .e.on { background: var(--tint); }
  .lnk { font: inherit; color: var(--accent); background: none; border: 0; padding: 1px 4px; border-radius: 5px; cursor: pointer; text-align: start; overflow-wrap: anywhere; }
  .elabel { color: var(--fg); }
  :global([data-theme="contrast"]) .lnk { text-decoration: underline; }
</style>
