<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, post } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { t, tn, fmtDate } from '../lib/i18n';
  import { trapFocus } from '../lib/focus';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { clearLayerBootFlag, retryLayer, revertChange } from '../lib/layer';
  import { reloadApp } from '../lib/api';
  import { failed as failToast } from '../lib/errors';

  /** `embedded`: drawn inline (a tab of the History view) instead of as a dialog. */
  let { embedded = false }: { embedded?: boolean } = $props();

  let history = $state<any[]>([]);
  let loading = $state(true);
  let failed = $state('');
  let busy = $state(false);
  // The list is read a page at a time, back to the very first entry.
  const PAGE = 50;
  let more = $state(false);
  let loadingOlder = $state(false);
  let olderFailed = $state(false);
  const close = () => (app.showRollback = false);

  /** One page of entries after the `offset` newest. One extra is asked for, to know whether more exist. */
  async function page(offset: number): Promise<any[]> {
    const list = await api(`/layer/history?offset=${offset}&limit=${PAGE + 1}`);
    const got = Array.isArray(list) ? list : [];
    more = got.length > PAGE;
    return got.slice(0, PAGE);
  }
  async function load() {
    loading = true; failed = '';
    try { history = await page(0); }
    catch (e: any) { failed = e.message; }
    finally { loading = false; }
  }
  async function older() {
    if (loadingOlder) return;
    loadingOlder = true; olderFailed = false;
    try {
      const next = await page(history.length);
      const have = new Set(history.map((h) => h.hash));
      history = [...history, ...next.filter((h) => !have.has(h.hash))];
    } catch { olderFailed = true; }
    finally { loadingOlder = false; }
  }
  onMount(load);
  /** The first entry and the app's own housekeeping cannot be undone on their own (older services say nothing: allow). */
  const canUndo = (h: any) => h?.undoable !== false;

  async function restore(h: any) {
    const ok = await confirmDialog({
      title: t('shell.rb.confirmTitle'),
      body: t('shell.rb.confirmBody', { date: fmtDate(h.date, { dateStyle: 'medium', timeStyle: 'short' }) }),
      confirmLabel: t('shell.rb.restore'),
    });
    if (!ok) return;
    busy = true;
    try {
      await post('/layer/rollback', { hash: h.hash });
      clearLayerBootFlag();
      reloadApp();
    } catch (e) { failToast(t('shell.rb.failed'), e); busy = false; }
  }

  /** Undo one change and keep everything made after it. */
  async function undo(h: any) {
    const ok = await confirmDialog({ title: t('layer.undoTitle'), body: t('layer.undoBody'), list: [String(h.summary ?? h.subject ?? '')], confirmLabel: t('layer.undoThis') });
    if (!ok) return;
    busy = true;
    try {
      await revertChange(h.hash);
      toast(t('layer.undone'), { kind: 'success', key: 'layer' });
      await load();
    } catch (e) { failToast(t('layer.undoFailed'), e); }
    finally { busy = false; }
  }
</script>

{#snippet content()}
    <p class="muted" id="rb-desc">{t('shell.rb.intro')}</p>
    <p class="row wrap">
      {#if app.safeMode}
        <button class="btn" onclick={retryLayer}><Icon name="retry" size={15} /> {t('shell.safe.leave')}</button>
      {:else}
        <button class="btn" onclick={() => reloadApp('safe=1')}><Icon name="shield" size={15} /> {t('shell.rb.safeMode')}</button>
      {/if}
    </p>
    {#if loading}
      <p class="muted" role="status">{t('shell.loading')}</p>
    {:else if failed}
      <p class="muted" role="alert">{t('shell.rb.loadFailed')} <bdi>{failed}</bdi></p>
      <button class="btn" onclick={load}><Icon name="retry" size={15} /> {t('shell.retry')}</button>
    {:else if !history.length}
      <p class="muted">{t('shell.rb.empty')}</p>
    {:else}
      <ul class="hist">
        {#each history as h, i (h.hash)}
          <li class="item">
            <div class="row top">
              <span class="grow"><b dir="auto">{h.summary ?? h.subject}</b><br /><span class="muted sm">{fmtDate(h.date, { dateStyle: 'medium', timeStyle: 'short' })}</span></span>
              {#if i === 0}<span class="pill accent">{t('shell.rb.current')}</span>{/if}
            </div>
            {#if h.files?.length}
              <details class="sm">
                <summary class="muted">{tn('layer.filesChanged', h.files.length)}</summary>
                <ul class="fl">{#each h.files as f}<li><bdi>{f}</bdi></li>{/each}</ul>
              </details>
            {/if}
            <div class="row wrap acts">
              {#if canUndo(h)}
                <button class="btn" disabled={busy} onclick={() => undo(h)}><Icon name="undo" size={14} /> {t('layer.undoThis')}</button>
              {:else}
                <span class="muted sm">{t(h.kind === 'start' ? 'lda.hist.start' : 'lda.hist.system')}</span>
              {/if}
              {#if i > 0}<button class="btn ghost" disabled={busy} onclick={() => restore(h)}>{t('layer.goBackHere')}</button>{/if}
            </div>
          </li>
        {/each}
      </ul>
      {#if more}
        <p class="row wrap">
          <button class="btn" disabled={loadingOlder || busy} aria-busy={loadingOlder} onclick={older}>{t(loadingOlder ? 'lda.hist.olderLoading' : 'lda.hist.older')}</button>
          {#if olderFailed}<span class="muted" role="alert">{t('lda.hist.olderFailed')}</span>{/if}
        </p>
      {:else if history.length > PAGE}
        <p class="muted sm">{t('lda.hist.end')}</p>
      {/if}
    {/if}
{/snippet}

{#if embedded}
  <section class="emb" aria-describedby="rb-desc">{@render content()}</section>
{:else}
<div class="modal-bg no-print" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="rb-title" aria-describedby="rb-desc" tabindex="-1" use:trapFocus={{ onEscape: close }}>
    <div class="row"><h1 class="grow row" id="rb-title"><Icon name="rollback" size={22} /> {t('shell.rb.title')}</h1><button class="btn ghost" onclick={close}>{t('close')}</button></div>
    {@render content()}
  </div>
</div>
{/if}

<style>
  .hist { list-style: none; margin: 0; padding: 0; }
  .item { padding: 10px 0; border-bottom: var(--border-w) solid var(--border); display: grid; gap: 6px; }
  .top { align-items: flex-start; }
  .fl { margin: 4px 0 0; padding-inline-start: 18px; font-family: var(--mono); overflow-wrap: anywhere; }
  summary { cursor: pointer; }
  .acts { gap: 6px; }
  .sm { font-size: var(--fs-xs); }
  .wrap { flex-wrap: wrap; }
</style>
