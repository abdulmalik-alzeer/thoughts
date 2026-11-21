<script lang="ts">
  // The build: the app does the planned work itself, on a copy of the project, a few pieces side
  // by side. This view shows where it stands, lets the user start, pause, resume or stop it, try a
  // failed piece again, and — only once everything is done — bring the result into their own branch.
  import { Play, Pause, Square, RotateCw, GitMerge, Hourglass, CircleCheck, TriangleAlert, Hammer, MessageCircle, Minus, Plus } from '@lucide/svelte';
  import { t, tn, fmtDate, fmtNum } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import { go, toast } from '../../lib/store.svelte';
  import { confirmDialog } from '../../lib/dialog.svelte';
  import { clampParallel } from '../../lib/mado/api';
  import type { MadoStore } from '../../lib/mado/store.svelte';
  import type { BuildTask, TaskStatus } from '../../lib/mado/types';

  let { store }: { store: MadoStore } = $props();
  const build = $derived(store.build);
  const status = $derived(build?.status ?? 'idle');
  const reviewed = $derived(store.status('review') === 'approved');
  const titles = $derived(new Map((store.plan?.tasks ?? []).map((x) => [x.id, x.title])));
  const all = $derived((build?.waves ?? []).flatMap((w) => w.lanes.flatMap((l) => l.tasks)));
  const done = $derived(all.filter((x) => x.status === 'done' || x.status === 'skipped').length);
  const share = $derived(all.length ? Math.round((done / all.length) * 100) : 0);
  let parallel = $state(3);
  let seeded = false;
  $effect(() => { const m = build?.governor?.maxParallel; if (m && !seeded) { seeded = true; parallel = clampParallel(m); } });
  let busy = $state('');
  let problem = $state('');

  async function act(what: string, run: () => Promise<void>, failKey: string) {
    if (busy) return;
    busy = what; problem = '';
    try { await run(); }
    catch (e) { problem = e instanceof Error ? e.message : String(e); failed(t(failKey), e); }
    finally { busy = ''; }
  }
  const start = () => act('start', () => store.buildStart(parallel), 'mado.build.startFailed');
  const pause = () => act('pause', () => store.buildPause(), 'mado.build.failed');
  const resume = () => act('resume', () => store.buildResume(), 'mado.build.failed');
  async function stop() {
    if (!(await confirmDialog({ title: t('mado.build.stopTitle'), body: t('mado.build.stopBody'), confirmLabel: t('mado.build.stop'), danger: true }))) return;
    act('stop', () => store.buildStop(), 'mado.build.failed');
  }
  const retry = (task: BuildTask) => act('retry:' + task.id, () => store.retryTask(task.id), 'mado.build.failed');
  async function land() {
    const ok = await confirmDialog({
      title: t('mado.build.landTitle'), body: t('mado.build.landBody', { branch: build?.branch ?? '', base: build?.base ?? '' }),
      note: t('mado.build.landNote'), confirmLabel: t('mado.build.land'),
    });
    if (!ok) return;
    act('land', async () => { await store.land(); toast(t('mado.build.landed'), { kind: 'success' }); }, 'mado.build.landFailed');
  }
  const TONE: Record<TaskStatus, string> = { pending: 'wait', running: 'run', checking: 'run', done: 'ok', failed: 'bad', conflict: 'bad', skipped: 'wait' };
</script>

{#if build === undefined}
  <div class="state muted" role="status">{t('mado.loading')}</div>
{:else}
  <section class="card hero s-{status}">
    <div class="row top">
      <span class="badge" aria-hidden="true">
        {#if status === 'done'}<CircleCheck size={22} />{:else if status === 'failed' || status === 'waiting'}<TriangleAlert size={22} />{:else if status === 'paused'}<Pause size={22} />{:else}<Hammer size={22} />{/if}
      </span>
      <div class="grow">
        <h3 role="status">{t(`mado.build.${status}`)}</h3>
        <p class="muted sub">{t(`mado.build.${status}.what`)}</p>
      </div>
      <div class="row acts">
        {#if status === 'running'}
          <button type="button" class="btn" onclick={pause} disabled={!!busy}><Pause size={14} aria-hidden="true" /> {t('mado.build.pause')}</button>
          <button type="button" class="btn danger" onclick={stop} disabled={!!busy}><Square size={13} aria-hidden="true" /> {t('mado.build.stop')}</button>
        {:else if status === 'paused'}
          <button type="button" class="btn primary" onclick={resume} disabled={!!busy}><Play size={14} aria-hidden="true" /> {t('mado.build.resume')}</button>
          <button type="button" class="btn danger" onclick={stop} disabled={!!busy}><Square size={13} aria-hidden="true" /> {t('mado.build.stop')}</button>
        {:else if status === 'waiting'}
          <button type="button" class="btn danger" onclick={stop} disabled={!!busy}><Square size={13} aria-hidden="true" /> {t('mado.build.stop')}</button>
        {:else if status === 'done'}
          <button type="button" class="btn primary" onclick={land} disabled={!!busy} aria-busy={busy === 'land'}><GitMerge size={15} aria-hidden="true" /> {t('mado.build.land')}</button>
        {/if}
      </div>
    </div>

    {#if all.length}
      <div class="progress" role="progressbar" aria-valuemin="0" aria-valuemax={all.length} aria-valuenow={done} aria-label={t('mado.build.progress', { done: fmtNum(done), all: fmtNum(all.length) })}>
        <i style:width="{share}%"></i>
      </div>
      <p class="muted meta">{t('mado.build.progress', { done: fmtNum(done), all: fmtNum(all.length) })}
        {#if build?.branch} · {t('mado.build.branch')} <code dir="ltr">{build.branch}</code>{/if}
        {#if build?.base} · {t('mado.build.from')} <code dir="ltr">{build.base}</code>{/if}
        {#if build?.startedAt} · {t('mado.build.started', { when: fmtDate(build.startedAt, { dateStyle: 'medium', timeStyle: 'short' }) })}{/if}</p>
    {/if}

    {#if build?.governor && (status === 'running' || status === 'paused' || status === 'waiting')}
      <p class="gov" class:holding={build.governor.holding} role="status">
        {#if build.governor.holding}
          <Hourglass size={15} aria-hidden="true" />
          <span><b>{t('mado.build.holding')}</b> {build.governor.reason || t('mado.build.holdingWhy')} {t('mado.build.holdingThen')}</span>
        {:else}
          <span>{t('mado.build.working', { n: fmtNum(build.governor.running), max: fmtNum(build.governor.maxParallel) })}</span>
        {/if}
      </p>
    {/if}

    {#if status === 'idle' || status === 'stopped' || status === 'failed'}
      {#if !reviewed}
        <p class="note"><TriangleAlert size={15} aria-hidden="true" /> <span>{t('mado.build.needsReview')}</span></p>
      {:else}
        <div class="startrow">
          <div class="how">
            <span id="mado-par" class="lbl">{t('mado.build.parallel')}</span>
            <div class="stepper" role="group" aria-labelledby="mado-par">
              <button type="button" class="btn" onclick={() => (parallel = clampParallel(parallel - 1))} disabled={parallel <= 1} aria-label={t('mado.build.fewer')}><Minus size={14} /></button>
              <output aria-live="polite">{fmtNum(parallel)}</output>
              <button type="button" class="btn" onclick={() => (parallel = clampParallel(parallel + 1))} disabled={parallel >= 6} aria-label={t('mado.build.more')}><Plus size={14} /></button>
            </div>
            <span class="muted sm">{t('mado.build.parallelWhat')}</span>
          </div>
          <button type="button" class="btn primary big" onclick={start} disabled={!!busy} aria-busy={busy === 'start'}><Play size={15} aria-hidden="true" /> {t(status === 'idle' ? 'mado.build.start' : 'mado.build.startAgain')}</button>
        </div>
        <p class="muted sm needs">{t('mado.build.needsGit')}</p>
      {/if}
    {/if}
    {#if problem}<p class="note bad" role="alert"><TriangleAlert size={15} aria-hidden="true" /> <span dir="auto">{problem}</span></p>{/if}
  </section>

  {#if build?.waves?.length}
    <ol class="waves">
      {#each build.waves as w, i}
        <li class="wave">
          <div class="wh"><b>{t('mado.review.round', { n: i + 1 })}</b> <span class="st t-{TONE[w.status]}">{t(`mado.task.${w.status}`)}</span>
            {#if w.lanes.length > 1}<span class="muted sm">{tn('mado.review.together', w.lanes.length)}</span>{/if}</div>
          <div class="lanes">
            {#each w.lanes as lane (lane.id)}
              <ul class="lane" aria-label={t('mado.build.lane')}>
                {#each lane.tasks as task (task.id)}
                  <li class="task t-{TONE[task.status]}">
                    <span class="dot" aria-hidden="true"></span>
                    <span class="tt">
                      <span class="name" dir="auto">{titles.get(task.id) ?? task.id}</span>
                      <span class="st t-{TONE[task.status]}">{t(`mado.task.${task.status}`)}{#if task.commit} · <code dir="ltr">{task.commit.slice(0, 8)}</code>{/if}</span>
                      {#if task.note}<span class="tnote muted" dir="auto">{task.note}</span>{/if}
                      {#if task.status === 'failed' || task.status === 'conflict'}<button type="button" class="btn again" onclick={() => retry(task)} disabled={!!busy}><RotateCw size={13} aria-hidden="true" /> {t('mado.build.retry')}</button>{/if}
                    </span>
                    <span class="row ta">
                      {#if task.chatId}<button type="button" class="btn ghost ib" onclick={() => go({ kind: 'chat', id: task.chatId! })} aria-label={t('mado.build.openChat', { name: titles.get(task.id) ?? task.id })} title={t('mado.build.openChatTip')}><MessageCircle size={15} /></button>{/if}
                    </span>
                  </li>
                {/each}
              </ul>
            {/each}
          </div>
        </li>
      {/each}
    </ol>
  {:else if status === 'idle'}
    <div class="empty"><Hammer size={26} strokeWidth={1.5} aria-hidden="true" /><b>{t('mado.build.emptyTitle')}</b><span class="muted">{t('mado.build.emptyBody')}</span></div>
  {/if}
{/if}

<style>
  .hero { padding: 20px 22px; display: grid; gap: 14px; }
  .top { align-items: flex-start; gap: 14px; flex-wrap: wrap; }
  .badge { width: 44px; height: 44px; border-radius: 14px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); flex: none; }
  .s-done .badge { background: color-mix(in srgb, var(--ok) 18%, transparent); color: var(--ok); }
  .s-failed .badge, .s-waiting .badge { background: color-mix(in srgb, var(--danger) 16%, transparent); color: var(--danger); }
  :global([data-theme="contrast"]) .badge { background: var(--panel-2) !important; color: var(--fg) !important; }
  h3 { margin: 0; font-size: 20px; font-weight: 600; letter-spacing: -.02em; }
  .sub { margin: 2px 0 0; font-size: 13.5px; max-width: 64ch; }
  .acts { flex-wrap: wrap; }
  .progress { height: 8px; border-radius: 99px; background: var(--fill-2); overflow: hidden; }
  .progress i { display: block; height: 100%; border-radius: 99px; background: var(--accent); transition: width .5s var(--ease); }
  .meta { margin: -6px 0 0; font-size: var(--fs-xs); }
  .gov { margin: 0; font-size: 13px; display: flex; gap: 8px; align-items: flex-start; color: var(--muted); }
  .gov.holding { color: var(--fg); padding: 10px 12px; border-radius: var(--radius); background: color-mix(in srgb, var(--accent-2) 14%, transparent); }
  .gov :global(svg) { flex: none; margin-top: 2px; color: var(--accent-2); }
  .note { margin: 0; display: flex; gap: 8px; align-items: flex-start; font-size: 13.5px; }
  .note :global(svg) { flex: none; margin-top: 2px; color: var(--accent-2); }
  .note.bad { color: var(--danger); } .note.bad :global(svg) { color: var(--danger); }
  .startrow { display: flex; gap: 16px; align-items: flex-end; flex-wrap: wrap; justify-content: space-between; }
  .how { display: grid; gap: 6px; }
  .lbl { font-size: 13px; font-weight: 600; }
  .stepper { display: inline-flex; align-items: center; gap: 4px; }
  .stepper .btn { width: 32px; height: 32px; padding: 0; }
  .stepper output { min-width: 36px; text-align: center; font-size: 17px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .sm { font-size: var(--fs-xs); }
  .needs { margin: 0; }
  .big { padding: 9px 18px; font-size: 14px; }
  .waves { list-style: none; margin: 16px 0 0; padding: 0; display: grid; gap: 12px; }
  .wave { padding: 14px 16px; border-radius: var(--radius-lg); border: var(--border-w) solid var(--hairline); background: var(--panel); display: grid; gap: 10px; }
  .wh { display: flex; gap: 10px; align-items: baseline; flex-wrap: wrap; font-size: 14px; }
  .lanes { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; align-items: start; }
  .lane { list-style: none; margin: 0; padding: 6px; border-radius: var(--radius); background: var(--fill); display: grid; gap: 4px; }
  .task { display: grid; grid-template-columns: auto 1fr auto; gap: 10px; align-items: start; padding: 8px 10px; border-radius: 6px; background: var(--panel); }
  .dot { width: 10px; height: 10px; border-radius: 99px; margin-top: 5px; background: var(--control-border); }
  .t-run .dot { background: var(--accent); animation: mado-pulse 1.4s ease-in-out infinite; }
  .t-ok .dot { background: var(--ok); }
  .t-bad .dot { background: var(--danger); border-radius: 2px; }
  @keyframes mado-pulse { 50% { box-shadow: 0 0 0 5px color-mix(in srgb, var(--accent) 22%, transparent); } }
  .tt { display: grid; gap: 1px; min-width: 0; }
  .name { font-size: 13.5px; font-weight: 500; overflow-wrap: anywhere; }
  .st { font-size: var(--fs-xs); color: var(--muted); }
  .st.t-run { color: var(--on-tint); font-weight: 600; } .st.t-ok { color: var(--ok); } .st.t-bad { color: var(--danger); font-weight: 600; }
  :global([data-theme="contrast"]) .st.t-run { color: var(--fg); }
  .tnote { font-size: var(--fs-xs); overflow-wrap: anywhere; }
  .ta { gap: 4px; }
  .again { justify-self: start; margin-top: 6px; }
  .ib { padding: 4px 6px; }
  .state { padding: 40px 0; text-align: center; }
  .empty { margin-top: 16px; display: grid; justify-items: center; gap: 6px; text-align: center; padding: 48px 24px; border: 1px dashed var(--border); border-radius: var(--radius-lg); color: var(--muted); }
  .empty b { color: var(--fg); font-size: 15px; }
  .empty span { max-width: 50ch; }
</style>
