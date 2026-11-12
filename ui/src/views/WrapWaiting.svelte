<script lang="ts">
  // Shown on every page while a Wrap that was cut off waits for a decision: the library holds
  // part of it, and nothing else may change the library until the person has chosen.
  import Icon from '../lib/Icon.svelte';
  import { app, go } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { finishWrap, keepCutOffWrap, undoCutOffWrap } from '../lib/wrap';

  const id = $derived(((app.wrap as any).interrupted ?? '') as string);
  const canUndo = $derived((app.wrap as any).interruptedCanUndo !== false);
  // On the Wrap's own page the same choices are already on screen.
  const here = $derived(app.view.kind === 'chat' && app.view.id === id);
  let busy = $state(false);
  async function act(fn: (id: string) => Promise<unknown>) {
    if (busy || !id) return;
    busy = true;
    try { await fn(id); } finally { busy = false; }
  }
</script>

{#if id && !here}
  <div class="waiting no-print" role="alert">
    <Icon name="warning" size={16} />
    <span class="grow">{t('wrapa.banner')}</span>
    <button class="btn primary" disabled={busy} onclick={() => act(finishWrap)} title={t('wrapa.finishTip')}><Icon name="check" size={14} /> {t('wrapa.finish')}</button>
    {#if canUndo}<button class="btn" disabled={busy} onclick={() => act(undoCutOffWrap)} title={t('wrapa.undoTip')}><Icon name="undo" size={14} /> {t('wrapa.undo')}</button>{/if}
    <button class="btn" disabled={busy} onclick={() => act(keepCutOffWrap)} title={t('wrapa.keepTip')}>{t('wrapa.keep')}</button>
    <button class="link" onclick={() => go({ kind: 'chat', id })}>{t('wrapa.open')}</button>
  </div>
{/if}

<style>
  .waiting { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin: 4px 28px 0; padding: 10px 14px; border-radius: var(--radius-lg); background: var(--panel); border: var(--border-w) solid var(--accent-2); font-size: 13px; }
  .waiting > :global(.icon) { flex: none; color: var(--accent-2); }
  .waiting .grow { min-width: 220px; }
  .link { font: inherit; font-weight: 600; color: var(--accent); background: none; border: 0; padding: 2px 0; cursor: pointer; text-decoration: underline; text-underline-offset: 3px; }
</style>
