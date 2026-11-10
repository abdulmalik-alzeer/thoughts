<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, post } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { t, fmtDate } from '../lib/i18n';
  import { trapFocus } from '../lib/focus';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { clearLayerBootFlag, retryLayer } from '../lib/layer';

  let history = $state<any[]>([]);
  let loading = $state(true);
  let failed = $state('');
  let busy = $state(false);
  const close = () => (app.showRollback = false);

  async function load() {
    loading = true; failed = '';
    try { history = await api('/layer/history'); }
    catch (e: any) { failed = e.message; }
    finally { loading = false; }
  }
  onMount(load);

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
      location.replace(location.pathname);
    } catch (e: any) { toast(t('shell.rb.failed'), { kind: 'error', detail: e.message }); busy = false; }
  }
</script>

<div class="modal-bg no-print" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="rb-title" aria-describedby="rb-desc" tabindex="-1" use:trapFocus={{ onEscape: close }}>
    <div class="row"><h1 class="grow row" id="rb-title"><Icon name="rollback" size={22} /> {t('shell.rb.title')}</h1><button class="btn ghost" onclick={close}>{t('close')}</button></div>
    <p class="muted" id="rb-desc">{t('shell.rb.intro')}</p>
    <p class="row wrap">
      {#if app.safeMode}
        <button class="btn" onclick={retryLayer}><Icon name="retry" size={15} /> {t('shell.safe.leave')}</button>
      {:else}
        <button class="btn" onclick={() => location.replace(location.pathname + '?safe=1')}><Icon name="shield" size={15} /> {t('shell.rb.safeMode')}</button>
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
          <li class="row item">
            <span class="grow"><span dir="auto">{h.subject}</span><br /><span class="muted sm">{fmtDate(h.date, { dateStyle: 'medium', timeStyle: 'short' })} · <code dir="ltr">{h.short}</code></span></span>
            {#if i === 0}<span class="pill accent">{t('shell.rb.current')}</span>{:else}<button class="btn" disabled={busy} onclick={() => restore(h)}>{t('shell.rb.restore')}</button>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .hist { list-style: none; margin: 0; padding: 0; }
  .item { padding: 8px 0; border-bottom: var(--border-w) solid var(--border); }
  .sm { font-size: 12px; }
  .wrap { flex-wrap: wrap; }
</style>
