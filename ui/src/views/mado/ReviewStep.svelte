<script lang="ts">
  import HelpTip from "../../lib/HelpTip.svelte";
  // The check before building: does every feature and every screen have work planned for it, is
  // anything wrong with the plan, what will be built side by side — and every command the app
  // would run on this computer, word for word, because approving this step is what allows them.
  import { TriangleAlert, CircleCheck, Terminal, Layers } from '@lucide/svelte';
  import { t, tn } from '../../lib/i18n';
  import { blockingProblems, BLOCKING } from '../../lib/mado/store.svelte';
  import type { PlanDoc, Review } from '../../lib/mado/types';

  let { review, plan }: { review: Review | null | undefined; plan: PlanDoc | null | undefined } = $props();
  const titles = $derived(new Map((plan?.tasks ?? []).map((x) => [x.id, x.title])));
  const name = (id: string) => titles.get(id) ?? id;
  const blocking = $derived(blockingProblems(review ?? null));
  const laneCount = $derived((review?.waves ?? []).reduce((n, w) => Math.max(n, w.lanes.length), 0));
  // Setup commands first (they run before any task), then the checks, each group as the service listed it.
  const setupSet = $derived(new Set(plan?.setup ?? []));
  const setupCmds = $derived((review?.commands ?? []).filter((c) => setupSet.has(c)));
  const checkCmds = $derived((review?.commands ?? []).filter((c) => !setupSet.has(c)));
</script>

{#if review === undefined}
  <div class="state muted" role="status">{t('mado.loading')}</div>
{:else if review === null}
  <div class="empty"><Layers size={26} strokeWidth={1.5} aria-hidden="true" /><b>{t('mado.review.emptyTitle')}</b><span class="muted">{t('mado.review.emptyBody')}</span></div>
{:else}
  <div class="grid">
    <section class="card wide" aria-labelledby="rv-problems">
      <h3 id="rv-problems">{t('mado.review.problems')}</h3>
      {#if !review.problems.length}
        <p class="good"><CircleCheck size={17} aria-hidden="true" /> {t('mado.review.noProblems')}</p>
      {:else}
        {#if blocking.length}<p class="blocked" role="alert"><TriangleAlert size={16} aria-hidden="true" /> {tn('mado.review.blocked', blocking.length)}</p>{/if}
        <ul class="problems">
          {#each review.problems as p}
            <li class:hard={BLOCKING.includes(p.kind)}>
              <span class="kind">{t(`mado.problem.${p.kind}`)}</span>
              <span class="what">
                <span dir="auto">{p.message}</span>
                {#if p.tasks?.length}<span class="muted tasks">{t('mado.review.inTasks')} {#each p.tasks as id, i}{i ? ' · ' : ''}<bdi>{name(id)}</bdi>{/each}</span>{/if}
              </span>
              <span class="pill" class:stop={BLOCKING.includes(p.kind)}>{t(BLOCKING.includes(p.kind) ? 'mado.review.mustFix' : 'mado.review.worthALook')}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section class="card" aria-labelledby="rv-features">
      <h3 id="rv-features">{t('mado.review.features')} <HelpTip text={t('mado.review.featuresSub')} /></h3>
      <div class="tablewrap"><table>
        <thead><tr><th scope="col">{t('mado.review.feature')}</th><th scope="col">{t('mado.review.plannedWork')}</th></tr></thead>
        <tbody>
          {#each review.features as f (f.id)}
            <tr class:gap={!f.tasks.length}><th scope="row"><span class="pill">{f.id}</span> <span dir="auto">{f.name}</span></th>
              <td>{#each f.tasks as id}<span class="chip" dir="auto">{name(id)}</span>{:else}<span class="none">{t('mado.review.noWork')}</span>{/each}</td></tr>
          {:else}<tr><td colspan="2" class="muted">{t('mado.review.noFeatures')}</td></tr>{/each}
        </tbody>
      </table></div>
    </section>

    <section class="card" aria-labelledby="rv-screens">
      <h3 id="rv-screens">{t('mado.review.screens')} <HelpTip text={t('mado.review.screensSub')} /></h3>
      <div class="tablewrap"><table>
        <thead><tr><th scope="col">{t('mado.review.screen')}</th><th scope="col">{t('mado.review.plannedWork')}</th></tr></thead>
        <tbody>
          {#each review.screens as s (s.id)}
            <tr class:gap={!s.tasks.length}><th scope="row" dir="auto">{s.label}</th>
              <td>{#each s.tasks as id}<span class="chip" dir="auto">{name(id)}</span>{:else}<span class="none">{t('mado.review.noWork')}</span>{/each}</td></tr>
          {:else}<tr><td colspan="2" class="muted">{t('mado.review.noScreens')}</td></tr>{/each}
        </tbody>
      </table></div>
    </section>

    <section class="card wide" aria-labelledby="rv-order">
      <h3 id="rv-order">{t('mado.review.order')}</h3>
      <p class="muted sub">{t('mado.review.orderSub')}{#if laneCount > 1}{' '}{tn('mado.review.sideBySide', laneCount)}{/if}</p>
      <ol class="waves">
        {#each review.waves as w, i}
          <li class="wave">
            <div class="wh"><b>{t('mado.review.round', { n: i + 1 })}</b>
              <span class="muted">{w.lanes.length > 1 ? tn('mado.review.together', w.lanes.length) : t('mado.review.alone')}</span></div>
            <div class="lanes">
              {#each w.lanes as lane (lane.id)}
                <div class="lane" title={lane.tasks.length > 1 ? t('mado.review.laneTip') : undefined}>
                  {#each lane.tasks as id, k}{#if k}<span class="then" aria-hidden="true">↓</span><span class="sr-only">{t('mado.review.then')}</span>{/if}<span class="lt" dir="auto">{name(id)}</span>{/each}
                </div>
              {/each}
            </div>
          </li>
        {:else}<li class="muted">{t('mado.review.noOrder')}</li>{/each}
      </ol>
      <p class="muted sub foot">{t('mado.review.laneNote')}</p>
    </section>

    <section class="card wide cmds" aria-labelledby="rv-commands">
      <h3 id="rv-commands"><Terminal size={17} aria-hidden="true" /> {t('mado.review.commands')}</h3>
      <p class="warn"><TriangleAlert size={16} aria-hidden="true" /> <span>{t('mado.review.commandsWarn')}</span></p>
      {#if review.commands.length}
        {#if setupCmds.length}
          <h4 class="cmdgroup">{t('mado.review.setupCmds')}</h4>
          <p class="muted sub">{t('mado.review.setupCmdsSub')}</p>
          <ol class="cmdlist" aria-label={t('mado.review.setupCmds')}>
            {#each setupCmds as c}<li><code dir="ltr">{c}</code></li>{/each}
          </ol>
          <h4 class="cmdgroup">{t('mado.review.checkCmds')}</h4>
        {/if}
        <ol class="cmdlist" aria-label={setupCmds.length ? t('mado.review.checkCmds') : t('mado.review.commands')} style:counter-reset="cmd {setupCmds.length}">
          {#each checkCmds as c}<li><code dir="ltr">{c}</code></li>{/each}
        </ol>
        <p class="muted sub foot">{tn('mado.review.commandCount', review.commands.length)}</p>
      {:else}
        <p class="muted">{t('mado.review.noCommands')}</p>
      {/if}
    </section>
  </div>
{/if}

<style>
  .grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; }
  .wide { grid-column: 1 / -1; }
  .card { padding: 18px 20px; }
  h3 { margin: 0 0 4px; font-size: 16px; font-weight: 600; display: flex; gap: 8px; align-items: center; }
  .sub { font-size: var(--fs-sm); margin: 0 0 12px; }
  .foot { margin: 12px 0 0; }
  .good { display: flex; gap: 8px; align-items: center; color: var(--ok); margin: 8px 0 0; font-weight: 500; }
  .blocked { display: flex; gap: 8px; align-items: center; color: var(--danger); font-weight: 600; margin: 8px 0 10px; }
  .problems { list-style: none; margin: 8px 0 0; padding: 0; display: grid; gap: 8px; }
  .problems li { display: grid; grid-template-columns: minmax(120px, max-content) 1fr auto; gap: 12px; align-items: start; padding: 10px 12px; border-radius: var(--radius); background: var(--fill); border-inline-start: 3px solid var(--control-border); }
  .problems li.hard { border-inline-start-color: var(--danger); }
  .kind { font-weight: 600; font-size: 13px; }
  .what { display: grid; gap: 2px; font-size: 13.5px; min-width: 0; overflow-wrap: anywhere; }
  .tasks { font-size: var(--fs-xs); }
  .pill.stop { background: color-mix(in srgb, var(--danger) 16%, transparent); color: var(--danger); font-weight: 600; }
  :global([data-theme="contrast"]) .pill.stop { background: var(--danger); color: var(--bg); }
  .tablewrap { overflow-x: auto; overscroll-behavior-y: auto; }
  table { width: 100%; font-size: 13px; }
  th, td { border: 0; border-top: 1px solid var(--hairline); padding: 8px 6px; text-align: start; vertical-align: top; font-weight: 400; }
  thead th { border-top: 0; color: var(--muted); font-size: var(--fs-xs); font-weight: 500; }
  tbody th { font-weight: 500; }
  .chip { display: inline-block; background: var(--fill); padding: 1px 8px; border-radius: 99px; font-size: var(--fs-xs); margin: 0 0 4px; margin-inline-end: 5px; }
  .none { color: var(--danger); font-weight: 600; font-size: var(--fs-xs); }
  .waves { list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; }
  .wave { display: grid; grid-template-columns: 150px 1fr; gap: 14px; padding: 12px; border-radius: var(--radius); background: var(--fill); }
  .wh { display: grid; gap: 2px; align-content: start; font-size: 13px; }
  .wh .muted { font-size: var(--fs-xs); }
  .lanes { display: flex; flex-wrap: wrap; gap: 8px; align-items: flex-start; }
  .lane { display: grid; gap: 3px; justify-items: start; padding: 8px 10px; border-radius: var(--radius); background: var(--panel); border: var(--border-w) solid var(--hairline); font-size: 13px; min-width: 120px; }
  .then { color: var(--muted); font-size: 11px; line-height: 1; padding-inline-start: 4px; }
  .cmds h3 :global(svg) { color: var(--accent-2); }
  .warn { display: flex; gap: 8px; align-items: flex-start; margin: 8px 0 12px; padding: 10px 12px; border-radius: var(--radius); background: color-mix(in srgb, var(--accent-2) 12%, transparent); font-size: 13.5px; font-weight: 500; }
  .warn :global(svg) { flex: none; margin-top: 2px; color: var(--accent-2); }
  :global([data-theme="contrast"]) .warn { background: var(--panel); border: var(--border-w) solid var(--border); }
  .cmdgroup { margin: 10px 0 4px; font-size: 13.5px; font-weight: 600; }
  .cmdlist { margin: 0; padding: 0; list-style: none; counter-reset: cmd; display: grid; gap: 4px; }
  .cmdlist li { counter-increment: cmd; display: grid; grid-template-columns: 28px 1fr; align-items: baseline; }
  .cmdlist li::before { content: counter(cmd); color: var(--muted); font-size: var(--fs-xs); text-align: end; padding-inline-end: 8px; }
  .cmdlist code { display: block; padding: 6px 10px; border-radius: 6px; white-space: pre-wrap; overflow-wrap: anywhere; text-align: left; user-select: text; }
  .state { padding: 40px 0; text-align: center; }
  .empty { display: grid; justify-items: center; gap: 6px; text-align: center; padding: 56px 24px; border: 1px dashed var(--border); border-radius: var(--radius-lg); color: var(--muted); }
  .empty b { color: var(--fg); font-size: 15px; }
  @container main (max-width: 820px) { .grid { grid-template-columns: 1fr; } .wave { grid-template-columns: 1fr; } .problems li { grid-template-columns: 1fr; } }
</style>
