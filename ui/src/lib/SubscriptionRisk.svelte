<script lang="ts">
  // The terms-risk disclaimer shown before a Claude or ChatGPT subscription sign-in. `ack` is the
  // acknowledgement time (ISO string) or null; it is stored in settings as `subscriptionRiskAck`.
  import Icon from './Icon.svelte';
  import { t } from './i18n';

  let { ack = $bindable(null), disabled = false }: { ack?: string | null; disabled?: boolean } = $props();
  const id = `subrisk-${Math.random().toString(36).slice(2, 9)}`;
</script>

<div class="card risk" role="group" aria-labelledby={id}>
  <p class="head"><Icon name="warning" size={16} /> <b {id}>{t('subRisk.title')}</b></p>
  <p>{t('subRisk.body')}</p>
  <label class="check"><input type="checkbox" checked={!!ack} {disabled} onchange={(e) => (ack = e.currentTarget.checked ? new Date().toISOString() : null)} /> {t('subRisk.ack')}</label>
</div>

<style>
  .risk { border: var(--border-w) solid var(--danger); border-radius: var(--radius); padding: 12px 14px; margin: 10px 0; display: grid; gap: 6px; background: var(--panel); }
  .risk p { margin: 0; }
  .head { display: flex; align-items: center; gap: 6px; }
  .head :global(svg) { color: var(--danger); flex: none; }
  .check { display: flex; align-items: flex-start; gap: 8px; }
</style>
