<script lang="ts">
  // Shown on every page while a finished customisation waits for the user's choice and is neither
  // being reviewed nor tried (the review and the preview banner offer the choice themselves). So a
  // closed review never leaves a change nobody can find: one click brings the choice back.
  // Hidden on the Customise page and in the change's own chat, which show their own Review button.
  import Icon from '../lib/Icon.svelte';
  import { app, go } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { openReview } from '../lib/layer';

  const p = $derived(app.pending);
  const chatId = $derived(p?.chatId ?? '');
  const inChat = $derived(!!chatId && app.view.kind === 'chat' && app.view.id === chatId);
  // (Not for a version the user went back to its chat about: that waits for their message, not a choice.)
  const show = $derived(!!p?.exists && !p.running && !p.continued && !app.showReview && !app.preview && app.view.kind !== 'customise' && !inChat);
</script>

{#if show}
  <div class="waiting no-print" role="status">
    <Icon name="palette" size={16} />
    <span class="grow">{t('r10-cust-dismiss.waiting')}</span>
    <button type="button" class="btn primary" onclick={openReview}><Icon name="eye" size={14} /> {t('r10-cust-dismiss.choose')}</button>
    {#if chatId}<button type="button" class="link" onclick={() => go({ kind: 'chat', id: chatId })}>{t('r10-cust-dismiss.openChat')}</button>{/if}
  </div>
{/if}

<style>
  .waiting { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin: 4px 28px 0; padding: 8px 14px; border-radius: var(--radius-lg); background: var(--panel); border: var(--border-w) solid var(--accent); font-size: 13px; }
  .waiting > :global(.icon) { flex: none; color: var(--accent); }
  .waiting .grow { min-width: 200px; }
  .link { font: inherit; font-weight: 600; color: var(--accent); background: none; border: 0; padding: 2px 0; cursor: pointer; text-decoration: underline; text-underline-offset: 3px; }
</style>
