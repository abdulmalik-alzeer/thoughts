<script lang="ts">
  import { onMount } from 'svelte';
  import { api, post } from '../lib/api';
  import { go, toast } from '../lib/store.svelte';
  import { t, tn } from '../lib/i18n';
  import { choiceDialog, confirmDialog } from '../lib/dialog.svelte';
  import Icon from '../lib/Icon.svelte';
  import DocChanges from '../lib/DocChanges.svelte';
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
    } catch (e: any) {
      busy = false;
      const m = /REORG_CONFLICT::(.*)/s.exec(e.message || '');
      if (!m) { toast(t('shell.reorg.failed'), { kind: 'error', detail: e.message }); return; }
      // Notes were edited after the reorganize started, so it can no longer be applied cleanly.
      const choice = await choiceDialog({
        title: t('layer.reorg.conflictTitle'), body: t('layer.reorg.conflictBody'), list: m[1].split(';').map((f) => f.trim()).filter(Boolean),
        cancelLabel: t('layer.reorg.keepLater'), choices: [{ id: 'discard', label: t('shell.reorg.discard'), danger: true }],
      });
      if (choice !== 'discard') return;
      busy = true;
      try { await post(`/reorganize/${id}/discard`); toast(t('shell.reorg.discarded'), { kind: 'success' }); go({ kind: 'docs' }); }
      catch (err: any) { toast(t('shell.reorg.failed'), { kind: 'error', detail: err.message }); busy = false; }
    }
  }
  const phase = $derived.by(() => { const k = 'shell.reorg.phase.' + d?.phase; const s = t(k); return s === k ? d?.phase : s; });
  // Stopped or failed before it finished: what it left is incomplete, so it can only be discarded.
  const unfinished = $derived(d?.ok === false);
  const open = $derived(d?.phase === 'review');
  // "3 moved, 2 new, 1 changed": only the kinds there are.
  const summary = $derived.by(() => {
    const c = d?.counts ?? {};
    return (['moved', 'added', 'edited', 'deleted'] as const).filter((k) => c[k] > 0).map((k) => tn('wrapb.reorg.count.' + k, c[k])).join(t('wrapb.listSep'));
  });
</script>

<div class="page">
  <h1>{t('shell.reorg.title')}</h1>
  {#if d}
    <p class="muted">{t('shell.reorg.intro')}</p>
    {#if unfinished && open}
      <p class="warn card row" role="alert"><Icon name="warning" size={15} /> <span class="grow">{t('wrapb.reorg.unfinished')}</span></p>
    {/if}
    <div class="row"><span class="pill">{phase}</span><span class="grow"></span>
      {#if open}
        <button class="btn" class:primary={unfinished} disabled={busy} onclick={() => finish('discard')}>{t('shell.reorg.discard')}</button>
        {#if !unfinished}<button class="btn primary" disabled={busy} onclick={() => finish('merge')}>{t('shell.reorg.keep')}</button>{/if}
      {/if}
    </div>
    <h2>{t('shell.reorg.changes')}</h2>
    {#if d.total}
      <p class="muted sm">{tn('wrapb.reorg.total', d.total)}{summary ? ' — ' + summary : ''}</p>
      <DocChanges files={d.files ?? []} />
    {:else}
      <p class="muted">{t('wrapb.reorg.none')}</p>
    {/if}
  {:else if failed}
    <p class="muted" role="alert">{t('shell.reorg.loadFailed')} <bdi>{failed}</bdi></p>
    <button class="btn" onclick={load}><Icon name="retry" size={15} /> {t('shell.retry')}</button>
  {:else}
    <p class="muted" role="status">{t('shell.loading')}</p>
  {/if}
</div>

<style>
  .sm { font-size: var(--fs-sm); }
  .warn { gap: 8px; align-items: flex-start; margin: 0 0 12px; }
  .warn :global(.icon) { flex: none; margin-top: 2px; color: var(--danger); }
</style>
