<script lang="ts">
  // Projects › a project › Plan: MADO, the app's way of planning and building a software project
  // in seven steps (docs/mado-spec.md). This view is the frame — the project, the stepper, and the
  // workspace of the step that is open; everything it knows lives in the store.
  import { onMount, untrack } from 'svelte';
  import { ChevronLeft, FolderKanban, Sparkles, TriangleAlert } from '@lucide/svelte';
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

  let { projectId, projects = [], api }: { projectId: string; projects?: any[]; /** For tests and the dev preview. */ api?: MadoApi } = $props();

  const store = untrack(() => new MadoStore(projectId, api));
  const name = $derived(store.overview?.project?.name ?? projects.find((p) => p.id === projectId)?.name ?? '');
  let current = $state<StepId>('brief');
  let tabs = $state<Partial<Record<StepId, string>>>({});
  let starting = $state(false);

  onMount(() => {
    store.load(() => { current = firstOpenStep(store.overview); }).catch((e) => failed(t('mado.loadFailed'), e));
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

  async function start() {
    if (starting) return;
    starting = true;
    try { await store.start(); current = firstOpenStep(store.overview); }
    catch (e) { failed(t('mado.startFailed'), e); }
    finally { starting = false; }
  }
</script>

<div class="mado">
  <header class="top">
    <button type="button" class="back" onclick={() => go({ kind: 'projects' })}><ChevronLeft size={15} class="flip-rtl" aria-hidden="true" /> {t('projects')}</button>
    <div class="row title">
      <span class="emblem" aria-hidden="true"><FolderKanban size={18} /></span>
      <h1 tabindex="-1"><span class="muted">{t('mado.title')}</span>{#if name} <span class="sep" aria-hidden="true">·</span> <bdi>{name}</bdi>{/if}</h1>
    </div>
  </header>

  {#if store.phase === 'loading'}
    <div class="state muted" role="status">{t('mado.loading')}</div>
  {:else if store.phase === 'error'}
    <div class="state" role="alert">
      <TriangleAlert size={24} aria-hidden="true" />
      <b>{t('mado.loadFailed')}</b>
      <span class="muted" dir="auto">{store.error}</span>
      <button type="button" class="btn" onclick={() => { store.phase = 'loading'; store.load(() => { current = firstOpenStep(store.overview); }).catch(() => {}); }}>{t('mado.tryAgain')}</button>
    </div>
  {:else if !store.exists}
    <section class="intro surface lit">
      <p class="eyebrow">{t('mado.intro.eyebrow')}</p>
      <h2>{t('mado.intro.title')}</h2>
      <p class="lead muted">{t('mado.intro.body')}</p>
      <ol class="seven">
        {#each STEPS as s, i}
          <li><span class="n" aria-hidden="true">{i + 1}</span><span><b>{t(`mado.step.${s}`)}</b><span class="muted">{t(`mado.intro.${s}`)}</span></span></li>
        {/each}
      </ol>
      <div class="go">
        <button type="button" class="btn primary big" onclick={start} disabled={starting} aria-busy={starting}><Sparkles size={16} aria-hidden="true" /> {t(starting ? 'mado.intro.starting' : 'mado.intro.start')}</button>
        <span class="muted where">{t('mado.intro.where')}{#if store.overview?.project?.path} <code dir="ltr"><bdi>{store.overview.project.path}/mado</bdi></code>{/if}</span>
      </div>
    </section>
  {:else}
    <div class="stepper"><Stepper overview={store.overview} {current} onpick={(s) => (current = s)} /></div>
    {#key current}
      <StepWorkspace {store} step={current} bind:tab={() => tabs[current] ?? '', (v) => { tabs[current] = v; }} />
    {/key}
  {/if}
</div>

<style>
  .mado { padding: 16px 28px 40px; display: grid; gap: 18px; max-width: 1480px; margin: 0 auto; }
  .top { display: grid; gap: 6px; }
  .back { font: inherit; font-size: 13px; color: var(--muted); background: none; border: 0; padding: 2px 4px; margin-inline-start: -4px; border-radius: 6px; cursor: pointer; display: inline-flex; align-items: center; gap: 2px; justify-self: start; }
  .back:hover { color: var(--fg); background: var(--fill); }
  .title { gap: 12px; }
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
  @container main (max-width: 700px) { .mado { padding: 12px 14px 32px; } .intro { padding: 24px 20px; } }
</style>
