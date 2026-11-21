<script lang="ts">
  // One diagram of a step. The drawing itself is MadoGraph (lib/mado/graph, its own worker's):
  // this pane only hands it the diagram, the comments on it and the edits not sent yet, and keeps
  // what it reports — edits stay in the store until "Send changes", positions are saved directly.
  import { Waypoints, Undo2 } from '@lucide/svelte';
  import MadoGraph from '../../lib/mado/graph/MadoGraph.svelte';
  import { t, tn } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import type { MadoStore } from '../../lib/mado/store.svelte';
  import type { CommentTarget, GraphName, Pos } from '../../lib/mado/types';
  import { commentsOnGraph, graphTarget } from '../../lib/mado/comments/batch';

  let { store, name, canEdit, canComment, oncomment }: {
    store: MadoStore; name: GraphName; canEdit: boolean; canComment: boolean;
    oncomment: (target: CommentTarget, label: string) => void;
  } = $props();

  const doc = $derived(store.graphs[name]);
  const comments = $derived(commentsOnGraph(store.comments, name).filter((c) => c.status === 'open' || c.status === 'sent'));
  const ops = $derived(store.opsOf(name));
  const editable = $derived(canEdit && name !== 'tasks');

  function positions(p: Record<string, Pos>) { store.savePositions(name, p).catch((e) => failed(t('mado.graph.posFailed'), e)); }
  function comment(on: { kind: 'node'; nodeId: string } | { kind: 'edge'; edgeId: string }) {
    if (!canComment || !doc) return;
    const label = on.kind === 'node' ? doc.nodes.find((n) => n.id === on.nodeId)?.label ?? on.nodeId : '';
    oncomment(graphTarget(name, on), label);
  }
</script>

{#if doc === undefined}
  <div class="state muted" role="status">{t('mado.loading')}</div>
{:else if doc === null || !doc.nodes?.length}
  <div class="empty">
    <Waypoints size={26} strokeWidth={1.5} aria-hidden="true" />
    <b>{t(`mado.emptyGraph.${name}.title`)}</b>
    <span class="muted">{t(`mado.emptyGraph.${name}.body`)}</span>
  </div>
{:else}
  <div class="bar">
    <span class="muted hint grow">{t(name === 'tasks' ? 'mado.graph.tasksHint' : editable ? 'mado.graph.editHint' : 'mado.graph.lookHint')}</span>
    {#if ops.length}
      <span class="pill accent" role="status">{tn('mado.graph.pending', ops.length)}</span>
      <button type="button" class="btn ghost" onclick={() => store.setOps(name, [])}><Undo2 size={14} aria-hidden="true" /> {t('mado.graph.undoAll')}</button>
    {/if}
  </div>
  <div class="stage">
    <MadoGraph {doc} {comments} {editable} pendingOps={ops} onops={(list) => store.setOps(name, list)} onpositions={positions} oncomment={comment} />
  </div>
{/if}

<style>
  .bar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 14px; min-height: 32px; }
  .hint { font-size: var(--fs-sm); }
  .stage { position: relative; height: max(420px, calc(100vh - 360px)); border-radius: var(--radius-lg); border: var(--border-w) solid var(--hairline); background: var(--bg); overflow: hidden; }
  .state { padding: 40px 0; text-align: center; }
  .empty { display: grid; justify-items: center; gap: 6px; text-align: center; padding: 56px 24px; border: 1px dashed var(--border); border-radius: var(--radius-lg); color: var(--muted); }
  .empty b { color: var(--fg); font-size: 15px; }
  .empty span { max-width: 46ch; }
</style>
