<script lang="ts">
  // The sidebar's Plans group (under the chats): every project whose plan was started, with where
  // it stands in plain words and a small mark while something is at work or it waits for the user.
  // Rows look and behave like the chat rows above them; the group is not there when there are none.
  import { onMount } from 'svelte';
  import { app, go } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { followPlans, plans } from '../../lib/mado/plans.svelte';
  import { planMark, planWhere } from '../../lib/mado/plans';
  import type { PlanEntry } from '../../lib/mado/types';

  let { projects = [] }: { /** The linked folders: the list is fetched again when they change. */ projects?: any[] } = $props();

  onMount(() => { followPlans(); });
  let linked = -1;
  $effect(() => { const n = projects.length; if (linked >= 0 && n !== linked) followPlans(); linked = n; });

  const name = (p: PlanEntry) => p.name || projects.find((x) => x.id === p.projectId)?.name || t('shell.project');
  const where = (p: PlanEntry) => { const w = planWhere(p); return `${t(w.stepKey)} · ${t(w.stateKey)}`; };
  const markLabel = (p: PlanEntry) => { const m = planMark(p); return m === 'you' ? t('mado.plans.mark.you') : m === 'run' ? t('mado.plans.mark.run') : ''; };
  const isOpen = (p: PlanEntry) => app.view.kind === 'mado' && app.view.id === p.projectId;
</script>

{#if plans.list.length}
  <div class="group" id="side-plans">{t('mado.plans.title')}</div>
  <ul class="plans" aria-labelledby="side-plans">
    {#each plans.list as p (p.projectId)}
      {@const mark = planMark(p)}
      <li>
        <button type="button" class="chat" class:active={isOpen(p)} aria-current={isOpen(p) ? 'page' : undefined} title={`${name(p)} — ${where(p)}`} onclick={() => go({ kind: 'mado', id: p.projectId })}>
          <span class="dot" class:run={mark === 'run'} class:ask={mark === 'you'} title={markLabel(p) || undefined}></span>
          <span class="txt">
            <span class="ell" dir="auto">{name(p)}</span>
            <span class="ell where">{where(p)}</span>
          </span>
          {#if markLabel(p)}<span class="sr-only">({markLabel(p)})</span>{/if}
        </button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  /* The same measures as the chat rows and group names in App.svelte. */
  .group { font-size: var(--fs-xs); color: var(--muted); padding: 8px 8px 2px; }
  .plans { list-style: none; margin: 0; padding: 0; }
  .chat { font: inherit; background: none; border: 0; text-align: start; width: 100%; display: flex; align-items: center; gap: 9px; padding: 5px 8px; border-radius: 6px; cursor: pointer; color: var(--fg); font-size: 13px; }
  .chat:hover { background: var(--fill); }
  .chat.active { background: var(--tint); color: var(--on-tint); font-weight: 600; }
  .chat:focus-visible { outline-offset: -2px; }
  .txt { display: grid; min-width: 0; flex: 1; line-height: 1.3; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .where { font-size: var(--fs-xs); color: var(--muted); font-weight: 400; }
  .chat.active .where { color: inherit; opacity: .85; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--border); flex: none; }
  /* State by shape as well as colour, as for chats: at work = a ring, waiting for you = a filled dot with a halo. */
  .dot.run { width: 9px; height: 9px; background: transparent; border: 2px solid var(--accent); animation: plans-pulse 1s infinite alternate; }
  .dot.ask { width: 9px; height: 9px; background: var(--accent); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 30%, transparent); }
  :global([data-theme="contrast"]) .chat.active .dot { background: currentColor; }
  :global([data-theme="contrast"]) .chat.active .dot.run { background: transparent; border-color: currentColor; }
  :global([data-theme="contrast"]) .chat.active .dot.ask { background: currentColor; box-shadow: 0 0 0 2px currentColor; }
  @keyframes plans-pulse { to { opacity: .3; } }
  @media (prefers-reduced-motion: reduce) { .dot.run { animation: none; } }
</style>
