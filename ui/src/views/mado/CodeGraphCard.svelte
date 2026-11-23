<script lang="ts">
  // The code graph card (round 0.5.0): whether the user's own code-review-graph tool is on this
  // computer, whether this project's graph is built and up to date, a Build/Update button with its
  // progress, and why it cannot be built when it cannot. Without the tool: what it adds, the install
  // command to copy and the tool's page. The app never installs it or runs it on its own; nothing
  // in MADO waits for it. Everything shown comes from `GET /graph` (the service's own words).
  import { onMount } from 'svelte';
  import { Network, Copy, Check, ExternalLink, RotateCw, X, LoaderCircle, TriangleAlert } from '@lucide/svelte';
  import { t, ago } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import { toast } from '../../lib/store.svelte';
  import type { MadoStore } from '../../lib/mado/store.svelte';
  import { canBuildGraph, graphCardState, graphProgress, installCommand, openToolPage, safeToolPage } from '../../lib/mado/existing';

  let { store, onhide }: { store: MadoStore; /** Put the card away (remembered per project by the view). */ onhide?: () => void } = $props();

  const g = $derived(store.codeGraph);
  const cardState = $derived(graphCardState(g));
  const share = $derived(graphProgress(g));
  const command = $derived(installCommand(g?.recommend?.command));
  const page = $derived(safeToolPage(g?.recommend?.url));
  let busy = $state(false);
  let copied = $state(false);
  let checking = $state(false);

  onMount(() => {
    if (store.codeGraph === undefined) store.refreshCodeGraph().catch((e) => failed(t('mado-v050.graph.loadFailed'), e));
  });

  // While a build runs, the service tells only its start and end: the progress is asked for now and then.
  $effect(() => {
    if (cardState !== 'building') return;
    const timer = setInterval(() => { store.refreshCodeGraph().catch(() => {}); }, 1500);
    return () => clearInterval(timer);
  });

  async function build() {
    if (busy) return;
    busy = true;
    try { await store.buildCodeGraph(); }
    catch (e) { failed(t('mado-v050.graph.buildFailed'), e); store.refreshCodeGraph().catch(() => {}); }
    finally { busy = false; }
  }
  async function check() {
    if (checking) return;
    checking = true;
    try { await store.refreshCodeGraph(); }
    catch (e) { failed(t('mado-v050.graph.loadFailed'), e); }
    finally { checking = false; }
  }
  async function copy() {
    if (!command) return;
    try { await navigator.clipboard.writeText(command); copied = true; toast(t('mado-v050.graph.copied'), { kind: 'success' }); setTimeout(() => (copied = false), 1800); }
    catch { toast(t('mado-v050.graph.copyFailed'), { kind: 'error' }); }
  }
  const open = () => openToolPage(page);
  const builtLine = $derived.by(() => {
    if (!g?.builtAt) return '';
    const when = ago(g.builtAt);
    return g.files != null ? t('mado-v050.graph.built', { when, n: g.files }) : t('mado-v050.graph.builtWhen', { when });
  });
</script>

{#if g === undefined}
  <section class="card surface" aria-busy="true" aria-label={t('mado-v050.graph.title')}><span class="muted">{t('mado.loading')}</span></section>
{:else if g && cardState}
  <section class="card surface st-{cardState}" aria-labelledby="mado-graph-title">
    <header class="row">
      <span class="emblem" aria-hidden="true"><Network size={16} /></span>
      <h3 id="mado-graph-title">{t('mado-v050.graph.title')}</h3>
      <span class="pill state" role="status">
        {#if cardState === 'building'}<LoaderCircle size={12} class="spin" aria-hidden="true" />{/if}
        {t(`mado-v050.graph.state.${cardState}`)}
      </span>
      <span class="grow"></span>
      {#if onhide}<button type="button" class="btn ghost icon" onclick={onhide} title={t('mado-v050.graph.hideTip')} aria-label={t('mado-v050.graph.hide')}><X size={15} aria-hidden="true" /></button>{/if}
    </header>

    <p class="muted what">{t('mado-v050.graph.what')}</p>

    {#if cardState === 'missing' || cardState === 'old'}
      <p class="adds">{t('mado-v050.graph.adds')}</p>
      {#if cardState === 'old'}<p dir="auto">{g.recommend.text}</p>{/if}
      {#if command}
        <p class="muted small">{t('mado-v050.graph.install')}</p>
        <div class="cmd">
          <code dir="ltr">{command}</code>
          <button type="button" class="btn" onclick={copy}>{#if copied}<Check size={14} aria-hidden="true" />{:else}<Copy size={14} aria-hidden="true" />{/if} {t('mado-v050.graph.copy')}</button>
        </div>
      {/if}
      <div class="row acts">
        {#if page}<button type="button" class="btn ghost" onclick={open}><ExternalLink size={14} class="flip-rtl" aria-hidden="true" /> {t('mado-v050.graph.page')}</button>{/if}
        <button type="button" class="btn ghost" onclick={check} disabled={checking} aria-busy={checking}><RotateCw size={13} aria-hidden="true" /> {t('mado-v050.graph.check')}</button>
        <span class="muted small">{t('mado-v050.graph.afterInstall')}</span>
      </div>
    {:else}
      <p class="small">{g.tool.version ? t('mado-v050.graph.found', { version: g.tool.version }) : t('mado-v050.graph.foundNoVersion')}{#if builtLine}{' '}{builtLine}{/if}</p>
      {#if cardState === 'building'}
        <div class="bar" role="progressbar" aria-label={t('mado-v050.graph.state.building')} aria-valuemin="0" aria-valuemax="100" aria-valuenow={share === null ? undefined : Math.round(share * 100)}>
          <i class:indef={share === null} style:width={share === null ? undefined : `${Math.round(share * 100)}%`}></i>
        </div>
        <p class="muted small" aria-live="polite">{g.progress ? t('mado-v050.graph.progress', { done: g.progress.done, total: g.progress.total }) : t('mado-v050.graph.progressStart')}</p>
      {:else if cardState === 'blocked' && g.reason}
        <p class="reason" role="note"><TriangleAlert size={14} aria-hidden="true" /> <span dir="auto">{g.reason.message}</span></p>
      {:else if cardState === 'stale'}
        <p class="reason soft"><RotateCw size={13} aria-hidden="true" /> <span>{t('mado-v050.graph.stale')}</span></p>
      {/if}
      {#if g.error && cardState !== 'building'}
        <div class="reason" role="alert"><TriangleAlert size={14} aria-hidden="true" />
          <span class="grow"><span dir="auto">{g.error.message}</span>
            {#if g.error.detail}<details><summary>{t('mado-v050.graph.details')}</summary><pre dir="ltr">{g.error.detail}</pre></details>{/if}
          </span>
        </div>
      {/if}
      {#if canBuildGraph(cardState)}
        <div class="row acts">
          <button type="button" class="btn" class:primary={cardState === 'unbuilt' || cardState === 'stale' || cardState === 'failed'} onclick={build} disabled={busy} aria-busy={busy}>
            <Network size={14} aria-hidden="true" /> {t(g.built ? 'mado-v050.graph.update' : 'mado-v050.graph.build')}</button>
        </div>
      {/if}
      <p class="muted small">{t('mado-v050.graph.where')}</p>
    {/if}
  </section>
{/if}

<style>
  .card { display: grid; gap: 8px; padding: 14px 16px; border-radius: var(--radius-lg); background: var(--panel); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow); }
  header { gap: 8px; align-items: center; }
  h3 { margin: 0; font-size: 15px; font-weight: 600; }
  .emblem { width: 26px; height: 26px; border-radius: 8px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); flex: none; }
  .pill.state { display: inline-flex; gap: 4px; align-items: center; }
  .st-ready .pill.state { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--ok); }
  .st-stale .pill.state, .st-failed .pill.state, .st-blocked .pill.state, .st-old .pill.state { background: color-mix(in srgb, var(--danger) 12%, transparent); color: var(--danger); }
  .st-building .pill.state, .st-unbuilt .pill.state { background: var(--tint); color: var(--on-tint); }
  :global([data-theme="contrast"]) .card .pill.state { background: var(--panel-2); color: var(--fg); border: var(--border-w) solid var(--border); }
  p { margin: 0; }
  .what { font-size: 13.5px; max-width: 80ch; }
  .adds { font-size: 13.5px; max-width: 80ch; }
  .small { font-size: var(--fs-sm); }
  .cmd { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .cmd code { font-family: var(--mono); font-size: 13px; padding: 7px 10px; border-radius: var(--radius); background: var(--fill); border: var(--border-w) solid var(--hairline); user-select: all; overflow-wrap: anywhere; }
  .acts { gap: 8px; flex-wrap: wrap; align-items: center; }
  .icon { padding: 4px; }
  .reason { display: flex; gap: 6px; align-items: flex-start; font-size: 13.5px; padding: 8px 10px; border-radius: var(--radius); background: color-mix(in srgb, var(--danger) 9%, transparent); }
  .reason :global(svg) { color: var(--danger); flex: none; margin-top: 2px; }
  .reason.soft { background: var(--fill); }
  .reason.soft :global(svg) { color: var(--muted); }
  .reason pre { white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--fs-xs); margin: 6px 0 0; max-height: 160px; overflow: auto; }
  :global([data-theme="contrast"]) .reason { background: var(--panel); border: var(--border-w) solid var(--border); }
  .bar { position: relative; height: 6px; border-radius: 6px; background: color-mix(in srgb, var(--accent) 16%, transparent); overflow: hidden; }
  .bar i { position: absolute; inset-block: 0; inset-inline-start: 0; border-radius: 6px; background: var(--accent); transition: width .4s var(--ease); }
  .bar i.indef { width: 30%; animation: graph-slide 1.5s ease-in-out infinite; }
  @keyframes graph-slide { 0% { inset-inline-start: -30%; } 100% { inset-inline-start: 100%; } }
  .card :global(.spin) { animation: graph-spin 1.1s linear infinite; }
  @keyframes graph-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .bar i.indef, .card :global(.spin) { animation: none; } }
</style>
