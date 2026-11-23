<script lang="ts">
  import HelpTip from "../../lib/HelpTip.svelte";
  // Projects › a project › Plan: MADO, the app's way of planning and building a software project
  // in seven steps (docs/mado-spec.md). This view is the frame — the project, the stepper, and the
  // workspace of the step that is open; everything it knows lives in the store.
  import { onMount, untrack } from 'svelte';
  import { ChevronLeft, FolderKanban, Sparkles, TriangleAlert, FileSearch, FilePlus2, Network } from '@lucide/svelte';
  import { on } from '../../lib/api';
  import { app, go } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import { confirmDialog } from '../../lib/dialog.svelte';
  import type { MadoApi } from '../../lib/mado/api';
  import { MadoStore, firstOpenStep } from '../../lib/mado/store.svelte';
  import { STEPS, type StepId } from '../../lib/mado/types';
  import Stepper from './Stepper.svelte';
  import StepWorkspace from './StepWorkspace.svelte';
  import PlanOptions from './PlanOptions.svelte';
  import CodeGraphCard from './CodeGraphCard.svelte';
  import { cardHidden, foundSummary, graphCardState, isExisting, modeSwitchable, setCardHidden, startChoice } from '../../lib/mado/existing';
  import type { MadoMode } from '../../lib/mado/types';

  let { projectId, projects = [], api }: { projectId: string; projects?: any[]; /** For tests and the dev preview. */ api?: MadoApi } = $props();

  const store = untrack(() => new MadoStore(projectId, api));
  const name = $derived(store.overview?.project?.name ?? projects.find((p) => p.id === projectId)?.name ?? '');
  let current = $state<StepId>('brief');
  let tabs = $state<Partial<Record<StepId, string>>>({});
  let starting = $state<MadoMode | '' | 'plain'>('');
  // Round 0.5.0: the kind of plan, and the code graph card.
  const existing = $derived(isExisting(store.overview));
  const choice = $derived(startChoice(store.overview));
  const found = $derived(foundSummary(store.overview?.detected));
  const graphState = $derived(graphCardState(store.codeGraph));
  const toolMissing = $derived(graphState === 'missing' || graphState === 'old');
  let hidden = $state(untrack(() => cardHidden(projectId)));
  let cardOpen = $state<boolean | null>(null);
  /** Open by default for a built project when the tool is there (unless put away); without the tool
   *  a built project gets one recommending line instead; otherwise the card opens only when asked for. */
  const showCard = $derived(cardOpen ?? (existing && !hidden && !toolMissing));
  /** One line at the start of existing mode recommending the tool, while the card is closed. */
  const recommend = $derived(existing && toolMissing && !showCard && !hidden && store.status('brief') !== 'approved');
  function hideCard() { cardOpen = false; hidden = true; setCardHidden(projectId, true); }
  function toggleCard() { cardOpen = !showCard; if (cardOpen && hidden) { hidden = false; setCardHidden(projectId, false); } }
  const switchable = $derived(modeSwitchable(store.overview) && (existing || !!store.overview?.detected?.hasCode));
  const running = $derived(STEPS.some((s) => store.status(s) === 'drafting') || ['running', 'checking', 'reviewing'].includes(store.build?.status ?? ''));

  onMount(() => {
    store.load(() => { current = firstOpenStep(store.overview); }).catch((e) => failed(t('mado.loadFailed'), e));
    // The card's state (cheap: the service does not run the tool for it). An older service has none.
    store.refreshCodeGraph().catch(() => {});
    const off = on((e) => {
      if (e.type !== 'mado' && e.type !== 'resync') return;
      store.onEvent(e).catch(() => { /* the next event or a reconnect fetches again */ });
    });
    return off;
  });

  // What the open step shows is fetched when it is opened.
  $effect(() => {
    const step = current, there = store.exists;
    if (!there) return;
    untrack(() => { store.ensureStep(step).catch((e) => failed(t('mado.loadFailed'), e)); });
  });

  // Unsaved edits of a document: ask before the view is left.
  $effect(() => {
    const dirty = Object.keys(store.edits).length > 0;
    if (!dirty) return;
    const guard = () => confirmDialog({ title: t('mado.doc.discardTitle'), body: t('mado.doc.discardBody'), confirmLabel: t('mado.doc.discard'), danger: true });
    app.leaveGuard = guard;
    return () => { if (app.leaveGuard === guard) app.leaveGuard = null; };
  });

  async function start(mode?: MadoMode) {
    if (starting) return;
    starting = mode ?? 'plain';
    try { await store.start(mode); current = firstOpenStep(store.overview); }
    catch (e) { failed(t('mado.startFailed'), e); }
    finally { starting = ''; }
  }
  /** Switch the plan's kind while nothing is approved (the service refuses otherwise, with its reason). */
  async function switchMode() {
    const to: MadoMode = existing ? 'new' : 'existing';
    const ok = await confirmDialog({ title: t('mado-v050.mode.switchTitle'), body: t(to === 'new' ? 'mado-v050.mode.switchToNewBody' : 'mado-v050.mode.switchToExistingBody'), confirmLabel: t('mado-v050.mode.switch') });
    if (!ok || starting) return;
    starting = to;
    try { await store.start(to); current = firstOpenStep(store.overview); }
    catch (e) { failed(t('mado-v050.mode.switchFailed'), e); }
    finally { starting = ''; }
  }
</script>

<div class="mado">
  <header class="top">
    <button type="button" class="back" onclick={() => go({ kind: 'projects' })}><ChevronLeft size={15} class="flip-rtl" aria-hidden="true" /> {t('projects')}</button>
    <div class="row title">
      <span class="emblem" aria-hidden="true"><FolderKanban size={18} /></span>
      <h1 tabindex="-1"><span class="muted">{t('mado.title')}</span>{#if name} <span class="sep" aria-hidden="true">·</span> <bdi>{name}</bdi>{/if}</h1>
      {#if store.exists}
        <span class="pill mode" class:existing title={t('mado-v050.mode.label')}>{t(existing ? 'mado-v050.mode.existing' : 'mado-v050.mode.new')}</span>
        {#if switchable}<button type="button" class="btn ghost small" onclick={switchMode} disabled={!!starting} aria-busy={!!starting}>{t(existing ? 'mado-v050.mode.toNew' : 'mado-v050.mode.toExisting')}</button>{/if}
      {/if}
      <span class="grow"></span>
      {#if store.codeGraph && store.phase === 'ready'}
        <button type="button" class="btn ghost graphb" class:on={showCard} aria-expanded={showCard} aria-controls="mado-graph-card" onclick={toggleCard}>
          <Network size={14} aria-hidden="true" /> {t('mado-v050.graph.button')}{#if graphState}<span class="pill gs-{graphState}">{t(`mado-v050.graph.state.${graphState}`)}</span>{/if}</button>
      {/if}
      {#if store.exists}<PlanOptions madoApi={store.api} {running} />{/if}
    </div>
  </header>

  {#if store.phase === 'ready' && store.codeGraph && showCard}
    <div id="mado-graph-card"><CodeGraphCard {store} onhide={hideCard} /></div>
  {:else if store.phase === 'ready' && recommend}
    <p class="recommend" role="note"><Network size={14} aria-hidden="true" /> <span>{t('mado-v050.graph.recommend')}</span>
      <button type="button" class="btn ghost small" onclick={toggleCard}>{t('mado-v050.graph.recommendShow')}</button>
      <button type="button" class="btn ghost small" onclick={hideCard}>{t('mado-v050.graph.recommendHide')}</button></p>
  {/if}

  {#if store.phase === 'loading'}
    <div class="state muted" role="status">{t('mado.loading')}</div>
  {:else if store.phase === 'error'}
    <div class="state" role="alert">
      <TriangleAlert size={24} aria-hidden="true" />
      <b>{t('mado.loadFailed')}</b>
      <span class="muted" dir="auto">{store.error}</span>
      <button type="button" class="btn" onclick={() => { store.phase = 'loading'; store.load(() => { current = firstOpenStep(store.overview); }).catch(() => {}); }}>{t('mado.tryAgain')}</button>
    </div>
  {:else if !store.exists && choice === 'choose'}
    <section class="intro surface lit" aria-labelledby="mado-choose-title">
      <p class="eyebrow">{t('mado-v050.choose.eyebrow')}</p>
      <h2 id="mado-choose-title">{t('mado-v050.choose.title')} <HelpTip text={t('mado-v050.choose.body')} /></h2>
      {#if found}
        <p class="found"><span class="muted">{t('mado-v050.choose.found')}</span>
          {#each found.langs as l, i}{i ? ', ' : ' '}<bdi>{t('mado-v050.choose.lang', { lang: l.lang, n: l.files })}</bdi>{/each}{#if found.more}{' '}{t('mado-v050.choose.more')}{/if}
          {#if found.manifests.length}<span class="muted man"> · {t('mado-v050.choose.manifests', { list: '' })}{#each found.manifests as m, i}{i ? ', ' : ''}<code dir="ltr">{m}</code>{/each}</span>{/if}
        </p>
      {:else}
        <p class="found muted">{t('mado-v050.choose.notLooked')}</p>
      {/if}
      <div class="choices">
        <button type="button" class="choice primary-choice" onclick={() => start('existing')} disabled={!!starting} aria-busy={starting === 'existing'}>
          <span class="ci" aria-hidden="true"><FileSearch size={20} /></span>
          <b>{t(starting === 'existing' ? 'mado-v050.choose.starting' : 'mado-v050.choose.existing')}</b>
          <span class="muted">{t('mado-v050.choose.existingBody')}</span>
          <span class="muted fine">{t('mado-v050.choose.reads')}</span>
        </button>
        <button type="button" class="choice" onclick={() => start('new')} disabled={!!starting} aria-busy={starting === 'new'}>
          <span class="ci" aria-hidden="true"><FilePlus2 size={20} /></span>
          <b>{t(starting === 'new' ? 'mado-v050.choose.starting' : 'mado-v050.choose.new')}</b>
          <span class="muted">{t('mado-v050.choose.newBody')}</span>
        </button>
      </div>
      {#if toolMissing && !hidden && !showCard}
        <p class="recommend inline" role="note"><Network size={14} aria-hidden="true" /> <span>{t('mado-v050.graph.recommend')}</span>
          <button type="button" class="btn ghost small" onclick={toggleCard}>{t('mado-v050.graph.recommendShow')}</button></p>
      {/if}
      <span class="muted where">{t('mado.intro.where')}{#if store.overview?.project?.path} <code dir="ltr"><bdi>{store.overview.project.path}/mado</bdi></code>{/if}</span>
    </section>
  {:else if !store.exists}
    <section class="intro surface lit">
      <p class="eyebrow">{t('mado.intro.eyebrow')}</p>
      <h2>{t('mado.intro.title')} <HelpTip text={t('mado.intro.body')} /></h2>
      <ol class="seven">
        {#each STEPS as s, i}
          <li><span class="n" aria-hidden="true">{i + 1}</span><span><b>{t(`mado.step.${s}`)}</b><span class="muted">{t(`mado.intro.${s}`)}</span></span></li>
        {/each}
      </ol>
      <div class="go">
        <button type="button" class="btn primary big" onclick={() => start()} disabled={!!starting} aria-busy={!!starting}><Sparkles size={16} aria-hidden="true" /> {t(starting ? 'mado.intro.starting' : 'mado.intro.start')}</button>
        <span class="muted where">{t('mado.intro.where')}{#if store.overview?.project?.path} <code dir="ltr"><bdi>{store.overview.project.path}/mado</bdi></code>{/if}</span>
      </div>
    </section>
  {:else}
    <div class="stepper"><Stepper overview={store.overview} {current} build={store.build} onpick={(s) => (current = s)} /></div>
    {#key current}
      <StepWorkspace {store} step={current} onstep={(s) => (current = s)} bind:tab={() => tabs[current] ?? '', (v) => { tabs[current] = v; }} />
    {/key}
  {/if}
</div>

<style>
  .mado { padding: 16px 28px 40px; display: grid; gap: 18px; max-width: 1480px; margin: 0 auto; }
  .top { display: grid; gap: 6px; }
  .back { font: inherit; font-size: 13px; color: var(--muted); background: none; border: 0; padding: 2px 4px; margin-inline-start: -4px; border-radius: 6px; cursor: pointer; display: inline-flex; align-items: center; gap: 2px; justify-self: start; }
  .back:hover { color: var(--fg); background: var(--fill); }
  .title { gap: 12px; flex-wrap: wrap; }
  h1 { min-width: 0; overflow-wrap: anywhere; }
  .emblem { width: 36px; height: 36px; border-radius: 11px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); flex: none; }
  h1 { margin: 0; font-size: 24px; }
  h1 .muted { font-weight: 400; }
  .sep { color: var(--muted); margin-inline: 4px; }
  .stepper { padding: 8px 10px; border-radius: 16px; background: var(--panel); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow); }
  .state { display: grid; justify-items: center; gap: 8px; text-align: center; padding: 80px 20px; }
  .state :global(svg) { color: var(--danger); }
  .intro { padding: 36px 40px; display: grid; gap: 14px; max-width: 860px; justify-self: center; width: 100%; border-radius: 20px; }
  .intro h2 { margin: 0; font-size: 30px; font-weight: 500; letter-spacing: -.03em; line-height: 1.15; }
  .lead { margin: 0; font-size: 15px; max-width: 62ch; }
  .seven { list-style: none; margin: 10px 0 6px; padding: 0; display: grid; gap: 2px; }
  .seven li { display: grid; grid-template-columns: 30px 1fr; gap: 12px; align-items: start; padding: 9px 0; border-top: 1px solid var(--hairline); animation: mado-rise .45s var(--ease) backwards; }
  .seven li:nth-child(2) { animation-delay: 40ms; } .seven li:nth-child(3) { animation-delay: 80ms; } .seven li:nth-child(4) { animation-delay: 120ms; }
  .seven li:nth-child(5) { animation-delay: 160ms; } .seven li:nth-child(6) { animation-delay: 200ms; } .seven li:nth-child(7) { animation-delay: 240ms; }
  @keyframes mado-rise { from { opacity: 0; transform: translateY(8px); } }
  .seven .n { width: 26px; height: 26px; border-radius: 99px; display: grid; place-items: center; font-size: 12.5px; font-weight: 600; background: var(--tint); color: var(--on-tint); margin-top: 1px; }
  .seven li > span:last-child { display: grid; gap: 1px; }
  .seven b { font-weight: 600; }
  .seven .muted { font-size: 13.5px; }
  .go { display: flex; gap: 16px; align-items: center; flex-wrap: wrap; margin-top: 6px; }
  .big { padding: 10px 20px; font-size: 15px; border-radius: 10px; }
  .where { font-size: var(--fs-sm); max-width: 48ch; }
  .where code { overflow-wrap: anywhere; }
  .pill.mode { background: var(--fill); color: var(--muted); }
  .pill.mode.existing { background: var(--tint); color: var(--on-tint); }
  :global([data-theme="contrast"]) .pill.mode { background: var(--panel-2); color: var(--fg); border: var(--border-w) solid var(--border); }
  .small { font-size: var(--fs-sm); padding: 4px 8px; }
  .graphb { display: inline-flex; align-items: center; gap: 6px; }
  .graphb.on { background: var(--fill-2); }
  .graphb .pill { font-size: var(--fs-xs); }
  .gs-ready { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--ok); }
  .gs-stale, .gs-failed, .gs-blocked, .gs-old { background: color-mix(in srgb, var(--danger) 12%, transparent); color: var(--danger); }
  .recommend { margin: 0; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; font-size: 13.5px; padding: 8px 12px; border-radius: var(--radius-lg); background: var(--tint); color: var(--on-tint); }
  .recommend :global(svg) { flex: none; }
  .recommend.inline { background: var(--fill); color: var(--fg); }
  :global([data-theme="contrast"]) .recommend { background: var(--panel); color: var(--fg); border: var(--border-w) solid var(--border); }
  .found { margin: 0; font-size: 14px; overflow-wrap: anywhere; }
  .found code { font-size: var(--fs-sm); }
  .choices { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; margin: 6px 0; }
  .choice { font: inherit; color: inherit; text-align: start; display: grid; gap: 6px; align-content: start; padding: 16px 18px; border-radius: var(--radius-lg); background: var(--panel); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow); cursor: pointer; transition: border-color .15s, background .15s, transform .15s; }
  .choice:hover:not(:disabled) { border-color: var(--accent); background: var(--fill); }
  .choice:active:not(:disabled) { transform: scale(.99); }
  .choice:disabled { opacity: .7; cursor: default; }
  .choice:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .choice b { font-size: 16px; font-weight: 600; }
  .choice .muted { font-size: 13.5px; line-height: 1.5; }
  .choice .fine { font-size: var(--fs-xs); }
  .primary-choice { border-color: color-mix(in srgb, var(--accent) 45%, var(--hairline)); }
  .ci { width: 34px; height: 34px; border-radius: 10px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); }
  :global([data-theme="contrast"]) .choice { border: var(--border-w) solid var(--border); }
  @container main (max-width: 700px) { .mado { padding: 12px 14px 32px; } .intro { padding: 24px 20px; } .choices { grid-template-columns: 1fr; } }
</style>
