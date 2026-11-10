<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, post } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';

  let history = $state<any[]>([]);
  let busy = $state(false);
  onMount(async () => { history = await api('/layer/history'); });

  async function restore(hash: string) {
    if (!confirm('Restore the app to this version? Your current version is kept in history, so this is undoable too.')) return;
    busy = true;
    try { await post('/layer/rollback', { hash }); location.replace(location.pathname); }
    catch (e: any) { toast(e.message); busy = false; }
  }
</script>

<div class="modal-bg" role="presentation" onclick={(e) => e.target === e.currentTarget && (app.showRollback = false)}>
  <div class="modal">
    <div class="row"><h1 class="grow row"><Icon name="rollback" size={22} /> {t('rollback')}</h1><button class="btn ghost" onclick={() => (app.showRollback = false)}>{t('close')}</button></div>
    <p class="muted">Every customisation of the app is a commit. Pick a version to go back to. Your library (ideas, notes, todos) is not affected — it has its own history.</p>
    <p><button class="btn" onclick={() => location.replace(location.pathname + '?safe=1')}>Reload in safe mode (customisations off)</button></p>
    {#each history as h, i}
      <div class="row item">
        <code>{h.short}</code><span class="grow">{h.subject}</span><span class="muted sm">{new Date(h.date).toLocaleString()}</span>
        {#if i === 0}<span class="pill accent">current</span>{:else}<button class="btn" disabled={busy} onclick={() => restore(h.hash)}>Restore</button>{/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .item { padding: 7px 0; border-bottom: var(--border-w) solid var(--border); }
  .sm { font-size: 12px; }
</style>
