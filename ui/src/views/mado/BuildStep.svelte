<script lang="ts">
  import HelpTip from '../../lib/HelpTip.svelte';
  // The build: the app does the planned work itself, on a copy of the project, a few pieces side
  // by side. This view shows where it stands, lets the user start, pause, resume or stop it, try a
  // failed piece again, and — only once everything is done — bring the result into their own branch.
  // Round 0.4.0: after the last round of work the build looks at the product's screens (through this
  // window, when it is the desktop app) and has the result reviewed; once it is in the user's branch
  // the view becomes a finished state that says how to run what was built.
  import { Play, Pause, Square, RotateCw, GitMerge, Hourglass, CircleCheck, TriangleAlert, Hammer, MessageCircle, Minus, Plus, Wrench, ListChecks, ShieldCheck, ScanEye, ClipboardCheck, Copy, Check, PartyPopper, PencilRuler } from '@lucide/svelte';
  import { findingCounts, localUrl } from '../../lib/mado/plans';
  import { lookAtScreens, looks } from '../../lib/mado/screenwatch.svelte';
  import ProjectDoc from './ProjectDoc.svelte';
  import { t, tn, fmtDate, fmtNum } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import { go, toast } from '../../lib/store.svelte';
  import { confirmDialog } from '../../lib/dialog.svelte';
  import { clampParallel } from '../../lib/mado/api';
  import type { MadoStore } from '../../lib/mado/store.svelte';
  import type { BuildTask, TaskStatus } from '../../lib/mado/types';

  let { store, onplan }: { store: MadoStore; /** Back to the steps, to plan a change. */ onplan?: () => void } = $props();
  const build = $derived(store.build);
  const status = $derived(build?.status ?? 'idle');
  const reviewed = $derived(store.status('review') === 'approved');
  /** Why a start would be refused right now (not a repository, uncommitted changes…), from the service. */
  const blocked = $derived(build?.blocked ?? '');
  const openChat = (id: string) => go({ kind: 'chat', id });
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

  // ---------- round 0.4.0 ----------
  const landed = $derived(!!build?.landedAt);
  const screens = $derived(build?.screens ?? null);
  const review = $derived(build?.review ?? null);
  const counts = $derived(findingCounts(review));
  const look = $derived(looks[store.projectId]);
  const lookBad = $derived(look === 'cannot' || look === 'failed' || look === 'badUrl' || look === 'tooMany');
  /** A build of a plan made before round 0.4.0: no whole-project checks, no screens, no review (the engine runs those only for a newer plan). */
  const oldPlan = $derived(!screens && !review && !(build?.waves ?? []).some((w) => w.verified !== undefined || w.gateFixChatId));
  // While the build waits for a window to look at the screens: ask, look, answer (once, and only one window).
  $effect(() => {
    if (status !== 'checking') return;
    const pid = store.projectId;
    const go = () => { lookAtScreens(pid).catch(() => {}); };
    go();
    const timer = setInterval(go, 4000);
    return () => clearInterval(timer);
  });
  // Once the build is in the user's branch: how to run it.
  $effect(() => { if (landed && store.guide === undefined) store.refreshGuide(); });
  const guide = $derived(store.guide);
  const address = $derived(localUrl(guide?.url));
  let copied = $state(false);
  async function copy(text: string) {
    try { await navigator.clipboard.writeText(text); copied = true; setTimeout(() => (copied = false), 2000); }
    catch { toast(t('md.copyFailed'), { kind: 'error' }); }
  }
  const TONE: Record<TaskStatus, string> = { pending: 'wait', running: 'run', checking: 'run', done: 'ok', failed: 'bad', conflict: 'bad', skipped: 'wait' };
</script>

{#if build === undefined}
  <div class="state muted" role="status">{t('mado.loading')}</div>
{:else}
  {#if landed}
  <section class="card hero finished" aria-labelledby="mado-finished">
    <div class="row top">
      <span class="badge ok" aria-hidden="true"><PartyPopper size={22} /></span>
      <div class="grow">
        <h3 id="mado-finished" role="status">{t('mado.finished.title')}</h3>
        <p class="muted sub">{t('mado.finished.what', { branch: build?.branch ?? '', base: build?.base ?? '' })}{#if build?.landedAt}{' '}{t('mado.finished.when', { when: fmtDate(build.landedAt, { dateStyle: 'medium', timeStyle: 'short' }) })}{/if}</p>
      </div>
      <div class="row acts">
        <button type="button" class="btn primary" onclick={() => onplan?.()} title={t('mado.finished.changeTip')}><PencilRuler size={15} aria-hidden="true" /> {t('mado.finished.change')}</button>
      </div>
    </div>
    <ul class="facts">
      <li><GitMerge size={15} aria-hidden="true" /> <span>{t('mado.finished.merged')} <code dir="ltr">{build?.branch}</code> → <code dir="ltr">{build?.base}</code> · {t('mado.build.progress', { done: fmtNum(done), all: fmtNum(all.length) })}</span></li>
      <li class:warnline={counts.all > 0}>
        {#if !review}<ClipboardCheck size={15} aria-hidden="true" /> <span>{t(oldPlan ? 'mado.finished.noReviewOld' : 'mado.finished.noReview')}</span>
        {:else if counts.all === 0}<CircleCheck size={15} aria-hidden="true" /> <span>{t('mado.finished.clean')}</span>
        {:else}<TriangleAlert size={15} aria-hidden="true" /> <span>{tn('mado.finished.findings', counts.all)}{#if counts.must} ({tn('mado.finished.must', counts.must)}){/if} <a href="#mado-review">{t('mado.finished.see')}</a></span>{/if}
      </li>
    </ul>

    <div class="howrun" aria-labelledby="mado-howrun">
      <h4 id="mado-howrun">{t('mado.run.title')}</h4>
      {#if guide === undefined}
        <p class="muted sm" role="status">{t('mado.loading')}</p>
      {:else}
        {#if guide?.start}
          <p class="muted sm lead">{t('mado.run.lead')}</p>
          <div class="cmdrow">
            <code class="cmd" dir="ltr">{guide.start}</code>
            <button type="button" class="btn" onclick={() => copy(guide?.start ?? '')} aria-label={t('mado.run.copy')}>{#if copied}<Check size={14} aria-hidden="true" /> {t('shell.copied')}{:else}<Copy size={14} aria-hidden="true" /> {t('shell.copy')}{/if}</button>
          </div>
          {#if guide.url}<p class="sm addr">{t('mado.run.then')} {#if address}<a href={address} target="_blank" rel="noreferrer noopener" dir="ltr">{address}</a>{:else}<code dir="ltr">{guide.url}</code>{/if}</p>{/if}
        {:else}
          <p class="muted sm">{t('mado.run.none')}</p>
        {/if}
        <p class="muted sm">{t('mado.run.notStarted')}</p>
        {#if guide?.readme}
          <details class="readme" open>
            <summary>{t('mado.run.readme')}</summary>
            <ProjectDoc text={guide.readme} label={t('mado.run.readme')} max={8000} />
          </details>
        {/if}
      {/if}
    </div>

    {#if reviewed && !blocked}
      <div class="again2"><span class="muted sm grow">{t('mado.finished.againWhat')}</span>
        <button type="button" class="btn" onclick={start} disabled={!!busy} aria-busy={busy === 'start'}><Play size={14} aria-hidden="true" /> {t('mado.finished.again')}</button></div>
    {/if}
    {#if problem}<p class="note bad" role="alert"><TriangleAlert size={15} aria-hidden="true" /> <span dir="auto">{problem}</span></p>{/if}
  </section>
  {:else}
  <section class="card hero s-{status}">
    <div class="row top">
      <span class="badge" aria-hidden="true">
        {#if status === 'done'}<CircleCheck size={22} />{:else if status === 'failed' || status === 'waiting'}<TriangleAlert size={22} />{:else if status === 'paused'}<Pause size={22} />{:else if status === 'checking'}<ScanEye size={22} />{:else if status === 'reviewing'}<ClipboardCheck size={22} />{:else}<Hammer size={22} />{/if}
      </span>
      <div class="grow">
        <h3 role="status">{t(`mado.build.${status}`)} <HelpTip text={t(`mado.build.${status}.what`)} /></h3>
      </div>
      <div class="row acts">
        {#if status === 'running'}
          <button type="button" class="btn" onclick={pause} disabled={!!busy}><Pause size={14} aria-hidden="true" /> {t('mado.build.pause')}</button>
          <button type="button" class="btn danger" onclick={stop} disabled={!!busy}><Square size={13} aria-hidden="true" /> {t('mado.build.stop')}</button>
        {:else if status === 'checking' || status === 'reviewing'}
          <button type="button" class="btn danger" onclick={stop} disabled={!!busy}><Square size={13} aria-hidden="true" /> {t('mado.build.stop')}</button>
        {:else if status === 'paused'}
          <button type="button" class="btn primary" onclick={resume} disabled={!!busy}><Play size={14} aria-hidden="true" /> {t('mado.build.resume')}</button>
          <button type="button" class="btn danger" onclick={stop} disabled={!!busy}><Square size={13} aria-hidden="true" /> {t('mado.build.stop')}</button>
        {:else if status === 'waiting'}
          <button type="button" class="btn primary" onclick={resume} disabled={!!busy} aria-busy={busy === 'resume'} title={t('mado.build.resumeWaitingTip')}><Play size={14} aria-hidden="true" /> {t('mado.build.resume')}</button>
          <button type="button" class="btn danger" onclick={stop} disabled={!!busy}><Square size={13} aria-hidden="true" /> {t('mado.build.stop')}</button>
        {:else if status === 'done'}
          <button type="button" class="btn primary" onclick={land} disabled={!!busy} aria-busy={busy === 'land'}><GitMerge size={15} aria-hidden="true" /> {t('mado.build.land')}</button>
        {/if}
      </div>
    </div>

    {#if build?.message && status !== 'idle'}
      <p class="note" class:bad={status === 'failed' || status === 'waiting'} role="status"><TriangleAlert size={15} aria-hidden="true" /> <span class="msg" dir="auto">{build.message}</span></p>
    {/if}

    {#if all.length}
      <div class="progress" role="progressbar" aria-valuemin="0" aria-valuemax={all.length} aria-valuenow={done} aria-label={t('mado.build.progress', { done: fmtNum(done), all: fmtNum(all.length) })}>
        <i style:width="{share}%"></i>
      </div>
      <p class="muted meta">{t('mado.build.progress', { done: fmtNum(done), all: fmtNum(all.length) })}
        {#if build?.branch} · {t('mado.build.branch')} <code dir="ltr">{build.branch}</code>{/if}
        {#if build?.base} · {t('mado.build.from')} <code dir="ltr">{build.base}</code>{/if}
        {#if build?.startedAt} · {t('mado.build.started', { when: fmtDate(build.startedAt, { dateStyle: 'medium', timeStyle: 'short' }) })}{/if}</p>
    {/if}

    {#if status === 'done' && review && counts.all}
      <p class="note" role="status"><TriangleAlert size={15} aria-hidden="true" /> <span>{tn('mado.finished.findings', counts.all)}{#if counts.must} ({tn('mado.finished.must', counts.must)}){/if} {t('mado.review.stillLand')} <a href="#mado-review">{t('mado.finished.see')}</a></span></p>
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
          <button type="button" class="btn primary big" onclick={start} disabled={!!busy || !!blocked} aria-busy={busy === 'start'} aria-describedby={blocked ? 'mado-blocked' : undefined}><Play size={15} aria-hidden="true" /> {t(status === 'idle' ? 'mado.build.start' : 'mado.build.startAgain')}</button>
        </div>
        {#if blocked}
          <p class="note bad" id="mado-blocked" role="status"><TriangleAlert size={15} aria-hidden="true" /> <span><b>{t('mado.build.blocked')}</b> <span dir="auto">{blocked}</span></span></p>
        {/if}
        <p class="muted sm needs">{t('mado.build.needsGit')}</p>
        {#if build?.confined === false}<p class="note" role="note"><TriangleAlert size={15} aria-hidden="true" /> <span>{t('mado.build.unconfined')}</span></p>{/if}
      {/if}
    {/if}
    {#if problem}<p class="note bad" role="alert"><TriangleAlert size={15} aria-hidden="true" /> <span dir="auto">{problem}</span></p>{/if}
  </section>
  {/if}

  {#if screens || status === 'checking'}
    <section class="card phase" aria-labelledby="mado-screens">
      <h4 id="mado-screens"><ScanEye size={17} aria-hidden="true" /> {t('mado.screens.title')}
        <span class="st t-{screens?.status === 'done' ? (screens.failed ? 'bad' : 'ok') : screens?.status === 'skipped' ? 'wait' : 'run'}" role="status">{t(`mado.screens.${screens?.status ?? 'waiting'}`)}</span> <HelpTip text={t('mado.screens.what')} /></h4>
      {#if screens?.status === 'done'}
        <p class="sm">{tn('mado.screens.count', screens.count ?? 0)}{' · '}{#if screens.failed}<b class="badtxt">{tn('mado.screens.failed', screens.failed)}</b>{:else}{t('mado.screens.allFine')}{/if}</p>
      {:else if screens?.status === 'skipped'}
        <p class="sm"><span>{t('mado.screens.skippedWhat')}</span>{#if screens.reason} <span class="muted" dir="auto">{String(screens.reason).slice(0, 600)}</span>{/if}</p>
        {#if screens.count}<p class="sm">{tn('mado.screens.asked', screens.count)}{' · '}{#if screens.failed}<b class="badtxt">{tn('mado.screens.askedFailed', screens.failed)}</b>{:else}{t('mado.screens.askedFine')}{/if}</p>{/if}
      {/if}
      {#if status === 'checking' && look}
        <p class="note" class:bad={look === 'failed' || look === 'badUrl' || look === 'tooMany'} role="status">{#if lookBad}<TriangleAlert size={15} aria-hidden="true" />{:else}<ScanEye size={15} aria-hidden="true" />{/if} <span>{t(`mado.screens.look.${look}`)}</span></p>
      {/if}
    </section>
  {/if}

  {#if review}
    <section class="card phase" id="mado-review" aria-labelledby="mado-review-h" tabindex="-1">
      <h4 id="mado-review-h"><ClipboardCheck size={17} aria-hidden="true" /> {t('mado.breview.title')}
        <span class="st t-{review.status === 'passed' ? 'ok' : review.status === 'failed' ? 'bad' : review.status === 'running' ? 'run' : 'wait'}" role="status">{t(`mado.breview.${review.status}`)}</span>
        <span class="muted sm">{t('mado.breview.round', { n: fmtNum(Math.max(1, Number(review.round) || 1)) })}</span> <HelpTip text={t('mado.breview.what')} /></h4>
      {#if review.note}<p class="note bad" role="status"><TriangleAlert size={15} aria-hidden="true" /> <span dir="auto">{String(review.note).slice(0, 2000)}</span></p>{/if}
      <div class="row chats">
        {#if review.chatId}<button type="button" class="btn ghost sm" onclick={() => openChat(review.chatId!)} title={t('mado.breview.chatTip')}><MessageCircle size={13} aria-hidden="true" /> {t('mado.breview.chat')}</button>{/if}
        {#if review.fixChatId}<button type="button" class="btn ghost sm" onclick={() => openChat(review.fixChatId!)} title={t('mado.breview.fixTip')}><Wrench size={13} aria-hidden="true" /> {t('mado.breview.fix')}</button>{/if}
      </div>
      {#if counts.all}
        <ul class="findings" aria-label={t('mado.breview.findings')}>
          {#each review.findings.slice(0, 100) as f}
            <li class:must={f.severity === 'must'}>
              <span class="pill" class:stop={f.severity === 'must'}>{t(f.severity === 'must' ? 'mado.breview.must' : 'mado.breview.should')}</span>
              <span class="ftext">
                {#if f.where}<code dir="ltr">{String(f.where).slice(0, 300)}</code>{/if}
                <span dir="auto">{String(f.text ?? '').slice(0, 2000)}</span>
              </span>
            </li>
          {/each}
        </ul>
        {#if review.findings.length > 100}<p class="muted sm">{tn('mado.breview.more', review.findings.length - 100)}</p>{/if}
      {:else if review.status !== 'running'}
        <p class="good"><CircleCheck size={16} aria-hidden="true" /> {t('mado.breview.none')}</p>
      {/if}
    </section>
  {/if}

  {#if build?.waves?.length}
    <ol class="waves">
      {#each build.waves as w, i}
        <li class="wave">
          <div class="wh"><b>{t('mado.review.round', { n: i + 1 })}</b> <span class="st t-{TONE[w.status]}">{t(`mado.task.${w.status}`)}</span>
            {#if w.lanes.length > 1}<span class="muted sm">{tn('mado.review.together', w.lanes.length)}</span>{/if}
            {#if w.checked}<span class="st t-ok" title={t('mado.build.checkedTip')}><ListChecks size={13} aria-hidden="true" /> {t('mado.build.checked')}</span>{/if}
            {#if w.fixChatId}<button type="button" class="btn ghost sm" onclick={() => openChat(w.fixChatId!)} title={t('mado.build.waveFixTip')}><Wrench size={13} aria-hidden="true" /> {t('mado.build.waveFix')}</button>{/if}
            {#if w.verified === true}<span class="st t-ok" title={t('mado.build.verifiedTip')}><ShieldCheck size={13} aria-hidden="true" /> {t('mado.build.verified')}</span>
            {:else if w.verified === false}<span class="st t-bad" title={t('mado.build.verifiedTip')}><TriangleAlert size={13} aria-hidden="true" /> {t('mado.build.notVerified')}</span>{/if}
            {#if w.gateFixChatId}<button type="button" class="btn ghost sm" onclick={() => openChat(w.gateFixChatId!)} title={t('mado.build.gateFixTip')}><Wrench size={13} aria-hidden="true" /> {t('mado.build.gateFix')}</button>{/if}</div>
          <div class="lanes">
            {#each w.lanes as lane (lane.id)}
              <ul class="lane" aria-label={t('mado.build.lane')}>
                {#if lane.fixChatId}<li class="lanefix"><button type="button" class="btn ghost sm" onclick={() => openChat(lane.fixChatId!)} title={t('mado.build.laneFixTip')}><Wrench size={13} aria-hidden="true" /> {t('mado.build.laneFix')}</button></li>{/if}
                {#each lane.tasks as task (task.id)}
                  <li class="task t-{TONE[task.status]}">
                    <span class="dot" aria-hidden="true"></span>
                    <span class="tt">
                      <span class="name" dir="auto">{titles.get(task.id) ?? task.id}</span>
                      <span class="st t-{TONE[task.status]}">{t(`mado.task.${task.status}`)}{#if (task.attempts ?? 0) > 1} · {t('mado.build.attempt', { n: fmtNum(task.attempts ?? 0) })}{/if}{#if task.commit} · <code dir="ltr">{task.commit.slice(0, 8)}</code>{/if}</span>
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
  .msg { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 240px; overflow: auto; }
  .lanefix { list-style: none; }
  .btn.sm { padding: 2px 8px; font-size: var(--fs-xs); }
  .tnote { font-size: var(--fs-xs); overflow-wrap: anywhere; }
  .ta { gap: 4px; }
  .again { justify-self: start; margin-top: 6px; }
  .ib { padding: 4px 6px; }
  .state { padding: 40px 0; text-align: center; }
  /* Round 0.4.0: the phases after the last round, and the finished state. */
  .badge.ok { background: color-mix(in srgb, var(--ok) 18%, transparent); color: var(--ok); }
  .phase { margin-top: 12px; padding: 16px 18px; display: grid; gap: 8px; }
  .phase h4, .howrun h4 { margin: 0; font-size: 15px; font-weight: 600; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .phase h4 :global(svg) { color: var(--accent); flex: none; }
  .phase p { margin: 0; }
  .phase:focus { outline: none; }
  .badtxt { color: var(--danger); }
  .good { display: flex; gap: 8px; align-items: center; color: var(--ok); font-weight: 500; }
  .chats { gap: 6px; flex-wrap: wrap; }
  .findings { list-style: none; margin: 4px 0 0; padding: 0; display: grid; gap: 6px; }
  .findings li { display: grid; grid-template-columns: max-content 1fr; gap: 10px; align-items: start; padding: 9px 12px; border-radius: var(--radius); background: var(--fill); border-inline-start: 3px solid var(--control-border); }
  .findings li.must { border-inline-start-color: var(--danger); }
  .ftext { display: grid; gap: 3px; min-width: 0; font-size: 13.5px; overflow-wrap: anywhere; }
  .ftext > span { white-space: pre-wrap; }
  .ftext code { justify-self: start; font-size: var(--fs-xs); white-space: pre-wrap; }
  .pill.stop { background: color-mix(in srgb, var(--danger) 16%, transparent); color: var(--danger); font-weight: 600; }
  :global([data-theme="contrast"]) .pill.stop { background: var(--danger); color: var(--bg); }
  .facts { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; font-size: 13.5px; }
  .facts li { display: flex; gap: 8px; align-items: flex-start; }
  .facts :global(svg) { flex: none; margin-top: 3px; color: var(--ok); }
  .facts .warnline :global(svg) { color: var(--accent-2); }
  .facts code, .cmd { overflow-wrap: anywhere; }
  .howrun { display: grid; gap: 8px; padding: 14px 16px; border-radius: var(--radius-lg); background: var(--fill); }
  .howrun p { margin: 0; }
  .cmdrow { display: flex; gap: 8px; align-items: stretch; flex-wrap: wrap; }
  .cmdrow .cmd { flex: 1 1 240px; min-width: 0; padding: 8px 12px; border-radius: 8px; background: var(--code-bg); border: var(--border-w) solid var(--hairline); white-space: pre-wrap; text-align: left; user-select: all; }
  .addr a { overflow-wrap: anywhere; }
  .readme { margin-top: 6px; padding: 10px 14px; border-radius: var(--radius); background: var(--panel); border: var(--border-w) solid var(--hairline); }
  .readme summary { cursor: pointer; font-weight: 600; font-size: 13.5px; }
  .readme[open] summary { margin-bottom: 12px; }
  .again2 { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; padding-top: 12px; border-top: 1px solid var(--hairline); }
  @container main (max-width: 560px) { .findings li { grid-template-columns: 1fr; } }
  .empty { margin-top: 16px; display: grid; justify-items: center; gap: 6px; text-align: center; padding: 48px 24px; border: 1px dashed var(--border); border-radius: var(--radius-lg); color: var(--muted); }
  .empty b { color: var(--fg); font-size: 15px; }
  .empty span { max-width: 50ch; }
</style>
