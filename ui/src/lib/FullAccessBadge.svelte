<script lang="ts">
  // The persistent sign that full access is on (lib/fullAccess.ts): small, always in the sidebar
  // (or its rail), and one click from the switch in Settings.
  import Icon from './Icon.svelte';
  import { go } from './store.svelte';
  import { t } from './i18n';
  import { fullAccessOn } from './fullAccess';

  let { compact = false }: { compact?: boolean } = $props();
  function open() {
    go({ kind: 'settings', id: 'privacy' }).then(() => setTimeout(() => document.getElementById('st-full-access')?.scrollIntoView({ block: 'center' }), 60));
  }
</script>

{#if fullAccessOn()}
  <button type="button" class="fa-badge" class:compact title={t('fullaccess.badgeTip')} aria-label={compact ? t('fullaccess.badgeTip') : undefined} onclick={open}>
    <Icon name="warning" size={14} />{#if !compact}<span>{t('fullaccess.badge')}</span>{/if}
  </button>
{/if}

<style>
  .fa-badge { display: flex; align-items: center; gap: 6px; margin: 4px 10px; padding: 4px 10px; border-radius: 99px; border: 1.5px solid var(--danger);
    background: transparent; color: var(--danger); font: inherit; font-size: var(--fs-xs); font-weight: 600; cursor: pointer; text-align: start; }
  .fa-badge:hover { background: var(--fill); }
  .fa-badge.compact { margin: 4px auto; padding: 4px; justify-content: center; }
</style>
