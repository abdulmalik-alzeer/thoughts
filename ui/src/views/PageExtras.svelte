<script lang="ts">
  // What a customisation adds to one of the app's own pages (`pages.json`): at the top, the line
  // under the heading and the add-on boxes placed there; at the bottom, the boxes placed there.
  // The top also says, in the app's own words, when an add-on page that normally replaces this page
  // is not shown and why (it failed, it is missing, the person chose the app's page), with the way
  // back; and when the page's customisation names something this version does not have.
  import Icon from '../lib/Icon.svelte';
  import PageBox from './PageBox.svelte';
  import { t } from '../lib/i18n';
  import { post } from '../lib/api';
  import { failed } from '../lib/errors';
  import { reloadLayer } from '../lib/layer';
  import { pageBoxes, pageIntro, pageWarnings, swapState, useSwap } from '../lib/pages.svelte';

  let { page, place = 'top', intro = true }: { page: string; place?: 'top' | 'bottom'; intro?: boolean } = $props();

  const boxes = $derived(pageBoxes(page, place));
  const line = $derived(place === 'top' && intro ? pageIntro(page) : '');
  // A replacement still loading for a moment is not a problem: only say so when it stays away.
  let settled = $state(false);
  $effect(() => { const timer = setTimeout(() => (settled = true), 1500); return () => clearTimeout(timer); });
  const swap = $derived(place === 'top' ? swapState(page) : null);
  const shownSwap = $derived(swap && (swap.state !== 'missing' || settled) ? swap : null);
  const warnings = $derived(place === 'top' ? pageWarnings(page) : []);
  let busy = $state(false);

  /** Keep the app's own page (true) or use the replacement again (false): one undoable history entry. */
  async function keep(on: boolean) {
    busy = true;
    try { await post('/layer/pages/builtin', { page, on }); if (!on) useSwap(page); await reloadLayer(); }
    catch (e) { failed(t('pgs.switchFailed'), e); }
    finally { busy = false; }
  }
  function again() { useSwap(page); reloadLayer().catch((e) => failed(t('shell.layerFailed'), e)); }
</script>

{#if shownSwap}
  <div class="swap no-print" class:bad={shownSwap.state === 'failed' || shownSwap.state === 'missing'} role={shownSwap.state === 'failed' ? 'alert' : 'status'}>
    <Icon name={shownSwap.state === 'failed' || shownSwap.state === 'missing' ? 'warning' : 'plugin'} size={14} />
    <div class="grow">
      <span>{t('pgs.swap.' + shownSwap.state, { name: shownSwap.name })}</span>
      {#if shownSwap.reason}<span class="why" dir="auto">{shownSwap.reason}</span>{/if}
    </div>
    {#if shownSwap.state === 'builtin'}
      <button class="btn ghost sm" disabled={busy} onclick={() => keep(false)}>{t('pgs.useSwapAgain')}</button>
    {:else}
      <button class="btn ghost sm" disabled={busy} onclick={again}>{t(shownSwap.state === 'original' ? 'r10-cust-pages.useSwap' : 'pgs.tryAgain')}</button>
      <button class="btn ghost sm" disabled={busy} onclick={() => keep(true)}>{t('pgs.keepOwn')}</button>
    {/if}
  </div>
{/if}
{#if warnings.length}
  <p class="warn no-print muted" role="status"><Icon name="info" size={13} /> <span>{t('pgs.unknown', { list: warnings.map((w) => `\u2068${w}\u2069`).join(t('ldb.listSep')) })}</span></p>
{/if}
{#if line}<p class="intro muted" dir="auto">{line}</p>{/if}
{#if boxes.length}
  <div class="boxes {place}">
    {#each boxes as w (w.plugin + ':' + w.id)}<PageBox {w} />{/each}
  </div>
{/if}

<style>
  .intro { margin: -4px 0 16px; max-width: 70ch; }
  .boxes { display: grid; gap: 14px; }
  .boxes.top { margin-bottom: 18px; }
  .boxes.bottom { margin-top: 24px; }
  .swap { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin: 0 0 14px; padding: 6px 10px; border: var(--border-w, 1px) solid var(--border); border-radius: var(--radius); color: var(--muted); font-size: var(--fs-sm, 13px); }
  .swap.bad { color: var(--fg); border-color: var(--danger); }
  .swap :global(.icon) { flex: none; }
  .swap.bad :global(.icon) { color: var(--danger); }
  .why { display: block; font-size: var(--fs-xs); color: var(--muted); overflow-wrap: anywhere; }
  .warn { display: flex; gap: 6px; align-items: center; font-size: var(--fs-xs); margin: 0 0 10px; }
</style>
