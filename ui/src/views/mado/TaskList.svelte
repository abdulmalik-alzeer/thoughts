<script lang="ts">
  // The plan as a list: each piece of work with the files it owns, what it waits for and how it
  // is checked. The same tasks as the diagram, for reading rather than for seeing the shape.
  import { MessageSquarePlus, ListChecks, ShieldCheck } from '@lucide/svelte';
  import { kindKey, localUrl } from '../../lib/mado/plans';
  import { t, tn } from '../../lib/i18n';
  import type { Comment, CommentTarget, PlanDoc } from '../../lib/mado/types';

  let { plan, comments, canComment, focusId = null, oncomment }: {
    plan: PlanDoc | null | undefined;
    comments: Comment[];
    canComment: boolean;
    /** The task to bring into view. */
    focusId?: string | null;
    oncomment: (target: CommentTarget, label: string) => void;
  } = $props();

  const titles = $derived(new Map((plan?.tasks ?? []).map((x) => [x.id, x.title])));
  const count = (id: string) => comments.filter((c) => c.target.kind === 'task' && c.target.taskId === id && (c.status === 'open' || c.status === 'sent')).length;
  let list = $state<HTMLElement>();
  const verify = $derived((plan?.verify ?? []).filter((c) => typeof c === 'string'));
  const run = $derived(plan?.run && typeof plan.run === 'object' ? plan.run : null);
  const runScreens = $derived(Array.isArray(run?.screens) ? run!.screens : []);
  $effect(() => {
    const id = focusId;
    if (id && list) list.querySelector<HTMLElement>(`[data-task="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  });
</script>

{#if plan === undefined}
  <div class="state muted" role="status">{t('mado.loading')}</div>
{:else if !plan?.tasks?.length}
  <div class="empty"><ListChecks size={26} strokeWidth={1.5} aria-hidden="true" /><b>{t('mado.emptyGraph.tasks.title')}</b><span class="muted">{t('mado.emptyGraph.tasks.body')}</span></div>
{:else}
  <p class="muted lead">{tn('mado.tasks.count', plan.tasks.length)} · {t('mado.tasks.lead')}</p>
  <section class="selfcheck" aria-labelledby="mado-selfcheck">
    <h3 id="mado-selfcheck"><ShieldCheck size={16} aria-hidden="true" /> {t('mado.selfcheck.title')}</h3>
    <dl>
      <dt>{t('mado.selfcheck.verify')}</dt>
      <dd class="col">{#each verify as c}<code class="cmd" dir="ltr">{c}</code>{:else}<span class="muted">{t('mado.selfcheck.noVerify')}</span>{/each}
        {#if verify.length}<span class="muted note">{t('mado.selfcheck.verifyWhat')}</span>{/if}</dd>
      <dt>{t('mado.selfcheck.run')}</dt>
      <dd class="col">
        {#if run}
          <code class="cmd" dir="ltr">{String(run.start ?? '')}</code>
          <span class="muted note">{t('mado.selfcheck.runAt')} <code dir="ltr">{localUrl(run.url) || String(run.url ?? '')}</code> · {tn('mado.selfcheck.screens', runScreens.length)}</span>
          {#if runScreens.length}<span class="paths">{#each runScreens.slice(0, 60) as s}<code dir="ltr" title={String(s?.id ?? '')}>{String(s?.path ?? '')}</code>{/each}</span>{/if}
        {:else}<span class="muted">{t('mado.selfcheck.noRun')}</span>{/if}
      </dd>
    </dl>
  </section>
  <ol class="tasks" bind:this={list}>
    {#each plan.tasks as task, i (task.id)}
      {@const n = count(task.id)}
      <li class="task" class:focus={focusId === task.id} data-task={task.id}>
        <div class="row head">
          <span class="num" aria-hidden="true">{i + 1}</span>
          <h3 class="grow" dir="auto">{task.title}</h3>
          {#if kindKey(task.kind)}<span class="pill kind" title={t(`${kindKey(task.kind)}.tip`)}>{t(kindKey(task.kind))}</span>{/if}
          {#if task.feature}<span class="pill" title={t('mado.tasks.feature')}>{task.feature}</span>{/if}
          {#if n}<span class="pill accent">{tn('mado.comments.short', n)}</span>{/if}
          {#if canComment}
            <button type="button" class="btn ghost cb" onclick={() => oncomment({ kind: 'task', taskId: task.id }, task.title)} aria-label={t('mado.tasks.comment', { name: task.title })} title={t('mado.comment.add')}><MessageSquarePlus size={15} /></button>
          {/if}
        </div>
        {#if task.description}<p class="desc" dir="auto">{task.description}</p>{/if}
        <dl>
          <dt>{t('mado.tasks.owns')}</dt>
          <dd>{#each task.owns ?? [] as p}<code dir="ltr">{p}</code>{:else}<span class="muted">{t('mado.tasks.none')}</span>{/each}</dd>
          <dt>{t('mado.tasks.depends')}</dt>
          <dd>{#each task.dependsOn ?? [] as d}<span class="dep" dir="auto">{titles.get(d) ?? d}</span>{:else}<span class="muted">{t('mado.tasks.noDeps')}</span>{/each}</dd>
          <dt>{t('mado.tasks.acceptance')}</dt>
          <dd class="col">{#each task.acceptance ?? [] as c}<code class="cmd" dir="ltr">{c}</code>{:else}<span class="muted">{t('mado.tasks.noChecks')}</span>{/each}</dd>
          {#if task.screens?.length}<dt>{t('mado.tasks.screens')}</dt><dd>{#each task.screens as s}<span class="dep">{s}</span>{/each}</dd>{/if}
        </dl>
      </li>
    {/each}
  </ol>
{/if}

<style>
  .lead { font-size: var(--fs-sm); margin: 0 0 14px; }
  .selfcheck { padding: 14px 16px; margin-bottom: 14px; border-radius: var(--radius-lg); border: var(--border-w) solid var(--hairline); background: var(--fill); }
  .selfcheck h3 { display: flex; gap: 8px; align-items: center; font-size: 14px; }
  .selfcheck h3 :global(svg) { color: var(--accent); flex: none; }
  .selfcheck dl { padding-inline-start: 0; }
  .note { font-size: var(--fs-xs); }
  .paths { display: flex; flex-wrap: wrap; gap: 5px; }
  .kind { background: var(--tint); color: var(--on-tint); font-weight: 600; }
  :global([data-theme="contrast"]) .kind { background: var(--panel-2); color: var(--fg); border: var(--border-w) solid var(--border); }
  .tasks { list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; }
  .task { padding: 14px 16px; border-radius: var(--radius-lg); border: var(--border-w) solid var(--hairline); background: var(--panel); transition: border-color .2s, box-shadow .2s; }
  .task.focus { border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 20%, transparent); }
  .num { width: 24px; height: 24px; border-radius: 99px; display: grid; place-items: center; background: var(--fill); font-size: 12px; font-weight: 600; color: var(--muted); flex: none; }
  h3 { margin: 0; font-size: 15px; font-weight: 600; }
  .cb { padding: 4px 6px; }
  .desc { margin: 8px 0 0; color: var(--muted); font-size: 13.5px; padding-inline-start: 32px; }
  dl { display: grid; grid-template-columns: max-content 1fr; gap: 6px 14px; margin: 12px 0 0; padding-inline-start: 32px; font-size: 13px; }
  dt { color: var(--muted); font-size: var(--fs-xs); font-weight: 500; padding-top: 2px; }
  dd { margin: 0; display: flex; flex-wrap: wrap; gap: 5px; min-width: 0; }
  dd.col { flex-direction: column; align-items: flex-start; }
  code { overflow-wrap: anywhere; }
  .cmd { display: block; padding: 3px 8px; border-radius: 6px; max-width: 100%; }
  .dep { background: var(--fill); padding: 1px 8px; border-radius: 99px; font-size: var(--fs-xs); }
  .state { padding: 40px 0; text-align: center; }
  .empty { display: grid; justify-items: center; gap: 6px; text-align: center; padding: 56px 24px; border: 1px dashed var(--border); border-radius: var(--radius-lg); color: var(--muted); }
  .empty b { color: var(--fg); font-size: 15px; }
  .empty span { max-width: 46ch; }
  .head { flex-wrap: wrap; }
  @container main (max-width: 640px) { dl { grid-template-columns: 1fr; padding-inline-start: 0; } .desc { padding-inline-start: 0; } }
</style>
