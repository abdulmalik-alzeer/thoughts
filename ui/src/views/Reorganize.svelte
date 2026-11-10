<script lang="ts">
  import { onMount } from 'svelte';
  import { api, post } from '../lib/api';
  import { go, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { confirmDialog } from '../lib/dialog.svelte';
  import Icon from '../lib/Icon.svelte';
  let { id }: { id?: string } = $props();
  let d = $state<any>(null);
  let failed = $state('');
  let busy = $state(false);

  async function load() {
    failed = '';
    try { d = await api(`/reorganize/${id}`); } catch (e: any) { failed = e.message; }
  }
  onMount(load);

  async function finish(op: 'merge' | 'discard') {
    const ok = await confirmDialog(op === 'merge'
      ? { title: t('shell.reorg.keepTitle'), body: t('shell.reorg.keepBody'), confirmLabel: t('shell.reorg.keep') }
      : { title: t('shell.reorg.discardTitle'), body: t('shell.reorg.discardBody'), confirmLabel: t('shell.reorg.discard'), danger: true });
    if (!ok) return;
    busy = true;
    try {
      await post(`/reorganize/${id}/${op}`);
      toast(op === 'merge' ? t('shell.reorg.kept') : t('shell.reorg.discarded'), { kind: 'success' });
      go({ kind: 'docs' });
    } catch (e: any) { toast(t('shell.reorg.failed'), { kind: 'error', detail: e.message }); busy = false; }
  }
  const lines = $derived((d?.diff || '').split('\n'));
  const phase = $derived.by(() => { const k = 'shell.reorg.phase.' + d?.phase; const s = t(k); return s === k ? d?.phase : s; });
</script>

<div class="page">
  <h1>{t('shell.reorg.title')}</h1>
  {#if d}
    <p class="muted">{t('shell.reorg.intro')}</p>
    <div class="row"><span class="pill">{phase}</span><span class="grow"></span>
      <button class="btn" disabled={busy} onclick={() => finish('discard')}>{t('shell.reorg.discard')}</button>
      <button class="btn primary" disabled={busy} onclick={() => finish('merge')}>{t('shell.reorg.keep')}</button></div>
    <h2>{t('shell.reorg.summary')}</h2>
    <pre dir="ltr">{d.stat}</pre>
    <h2>{t('shell.reorg.changes')}</h2>
    <p class="muted sm">{t('shell.reorg.legend')}</p>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre class="diff" dir="ltr" tabindex="0" aria-label={t('shell.reorg.changes')}>{#each lines as l}<span class:add={l.startsWith('+')} class:rem={l.startsWith('-')} class:hd={l.startsWith('@@') || l.startsWith('diff')}>{l}
</span>{/each}</pre>
  {:else if failed}
    <p class="muted" role="alert">{t('shell.reorg.loadFailed')} <bdi>{failed}</bdi></p>
    <button class="btn" onclick={load}><Icon name="retry" size={15} /> {t('shell.retry')}</button>
  {:else}
    <p class="muted" role="status">{t('shell.loading')}</p>
  {/if}
</div>

<style>
  .diff { font-size: 12px; max-height: 70vh; }
  .sm { font-size: 12px; }
  .add { color: var(--ok); } .rem { color: var(--danger); } .hd { color: var(--accent-2); }
</style>
