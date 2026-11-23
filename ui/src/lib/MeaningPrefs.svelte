<script lang="ts">
  // Settings › Features › Search by meaning (lib/meaning.svelte.ts). Applied at once, never part of
  // the page's unsaved draft. Nothing is downloaded until "Switch on and download" is pressed; the
  // bar has the update bar's look (Settings › Maintenance).
  import { onMount } from 'svelte';
  import { t, tn, fmtDate, fmtNum } from './i18n';
  import { toast } from './store.svelte';
  import { failed } from './errors';
  import { fmtSize } from './content/util';
  import { confirmDialog } from './dialog.svelte';
  import { meaning, watchMeaning, install, pause, rebuild, remove, mb } from './meaning.svelte';

  let busy = $state(false);
  const info = $derived(meaning.info);
  const st = $derived(info?.state ?? 'off');
  const dl = $derived(info?.download);
  const ix = $derived(info?.index);
  const recommended = $derived(st === 'off' && !info?.quiet && info?.suggest?.why === 'size');

  onMount(() => { watchMeaning(); });

  async function run(work: () => Promise<unknown>) {
    if (busy) return;
    busy = true;
    try { await work(); } catch (e) { failed(t('meaning.failed'), e); } finally { busy = false; }
  }

  async function cancelDownload() {
    const ok = await confirmDialog({ title: t('meaning.cancelTitle'), body: t('meaning.cancelBody', { size: fmtSize(info?.diskBytes ?? 0) }),
      confirmLabel: t('meaning.cancelYes'), cancelLabel: t('meaning.keep'), danger: true });
    if (ok) await run(remove);
  }

  async function removeAll() {
    const ok = await confirmDialog({ title: t('meaning.removeTitle'), body: t('meaning.removeBody', { size: fmtSize(info?.diskBytes ?? 0) }),
      confirmLabel: t('meaning.removeYes'), danger: true });
    if (!ok) return;
    await run(async () => { const freed = await remove(); toast(t('meaning.removed', { size: fmtSize(freed) }), { kind: 'success' }); });
  }
</script>

<div class="card mp" id="st-meaning">
  <h3>{t('meaning.title')} {#if recommended}<span class="pill accent">{t('meaning.recommended')}</span>{/if}</h3>
  <p class="sm">{t('meaning.intro')}</p>
  {#if info}
    <p class="muted sm"><bdi>{t('meaning.model', { name: info.model.name, size: mb(info.model.bytes), license: info.model.license })}</bdi></p>
  {/if}
  <p class="muted sm">{t('meaning.private')}</p>
  {#if recommended && info?.suggest?.files}<p class="muted sm">{tn('meaning.recommendedWhy', info.suggest.files)}</p>{/if}

  <div role="status" aria-live="polite">
    {#if st === 'downloading' || (st === 'paused' && dl) || (st === 'error' && dl)}
      {@const known = !!dl && dl.total > 0 && dl.got > 0}
      {@const label = !known ? t('meaning.starting') : st === 'paused' ? t('meaning.paused', { done: mb(dl!.got), total: mb(dl!.total) }) : t('meaning.downloading', { done: mb(dl!.got), total: mb(dl!.total) })}
      <progress class="upd-bar" max={known ? dl!.total : undefined} value={known ? Math.min(dl!.got, dl!.total) : undefined} aria-label={label}></progress>
      <p class="muted sm">{label}</p>
    {:else if st === 'indexing' || st === 'paused'}
      {@const known = !!ix?.total && ix.total > 0}
      {@const label = st === 'paused' ? t('meaning.pausedIndex') : known ? t('meaning.indexing', { done: fmtNum(ix!.done ?? 0), total: fmtNum(ix!.total!) }) : t('meaning.indexingStart')}
      <progress class="upd-bar" max={known ? ix!.total : undefined} value={known ? Math.min(ix!.done ?? 0, ix!.total!) : undefined} aria-label={label}></progress>
      <p class="muted sm">{label} {#if st === 'indexing'}{t('meaning.indexingHint')}{/if}</p>
    {:else if st === 'ready'}
      <p class="sm">{tn('meaning.ready', ix?.files ?? 0)}</p>
      {#if ix?.updated}<p class="muted sm">{t('meaning.updated', { when: fmtDate(ix.updated, { dateStyle: 'medium', timeStyle: 'short' }) })}</p>{/if}
    {/if}
    {#if st === 'error' && info?.error}<p class="err sm" dir="auto">{t('meaning.error', { error: info.error })}</p>{/if}
  </div>

  <div class="row wrap">
    {#if st === 'off'}
      <button type="button" class="btn primary" disabled={busy || !info} aria-busy={busy} onclick={() => run(install)}>{t('meaning.install')}</button>
    {:else if st === 'downloading'}
      <button type="button" class="btn" disabled={busy} onclick={() => run(pause)}>{t('meaning.pause')}</button>
      <button type="button" class="btn ghost" disabled={busy} onclick={cancelDownload}>{t('meaning.cancel')}</button>
    {:else if st === 'paused' || st === 'error'}
      <button type="button" class="btn primary" disabled={busy} onclick={() => run(install)}>{st === 'error' ? t('meaning.retry') : t('meaning.continue')}</button>
      {#if dl}<button type="button" class="btn ghost" disabled={busy} onclick={cancelDownload}>{t('meaning.cancel')}</button>
      {:else}<button type="button" class="btn ghost" disabled={busy} onclick={removeAll}>{t('meaning.remove')}</button>{/if}
    {:else if st === 'indexing'}
      <button type="button" class="btn" disabled={busy} onclick={() => run(pause)}>{t('meaning.pause')}</button>
    {:else if st === 'ready'}
      <button type="button" class="btn ghost" disabled={busy} onclick={() => run(rebuild)}>{t('meaning.rebuild')}</button>
      <button type="button" class="btn ghost" disabled={busy} onclick={removeAll}>{t('meaning.remove')}</button>
    {/if}
  </div>
  {#if st === 'off'}<p class="muted sm">{t('meaning.installNote')}</p>
  {:else if info?.diskBytes}<p class="muted sm">{t('meaning.space', { size: fmtSize(info.diskBytes) })}</p>{/if}
</div>

<style>
  /* The same bar as the app update's (Settings › Maintenance). */
  .upd-bar { display: block; width: 100%; max-width: 420px; height: 8px; margin-block: 10px 6px; accent-color: var(--accent); }
  .mp h3 { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
  .err { color: var(--danger); }
  .row { display: flex; gap: 8px; align-items: center; margin-top: 8px; }
  .wrap { flex-wrap: wrap; }
</style>
