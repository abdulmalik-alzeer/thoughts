<script lang="ts">
  // The monthly spending limit (Settings › Privacy and cost). Edits the draft `value`
  // ({ monthlyLimitUsd, warnPercent }); the page saves it with the rest.
  import { app } from '../lib/store.svelte';
  import { fmtMoney, t } from '../lib/i18n';

  let { value = $bindable(), disabled = false, error = '' }: { value: { monthlyLimitUsd: number | null; warnPercent: number }; disabled?: boolean; error?: string } = $props();
  const on = $derived(value.monthlyLimitUsd != null);
  const usage = $derived(app.status?.usage);

  function toggle(checked: boolean) {
    value.monthlyLimitUsd = checked ? (value.monthlyLimitUsd ?? 20) : null;
  }
</script>

<h3>{t('runc.spend.title')}</h3>
<p class="muted sm" id="st-spend-hint">{t('runc.spend.hint')}</p>
<label class="check"><input type="checkbox" checked={on} {disabled} onchange={(e) => toggle(e.currentTarget.checked)} aria-describedby="st-spend-hint" /> {t('runc.spend.on')}</label>
{#if on}
  <div class="row wrap fields">
    <div>
      <label for="st-spend-amount">{t('runc.spend.amount')}</label>
      <input id="st-spend-amount" class="num" type="number" inputmode="decimal" min="1" max="100000" step="1" dir="ltr" bind:value={value.monthlyLimitUsd} {disabled} aria-invalid={!!error} />
    </div>
    <div>
      <label for="st-spend-warn">{t('runc.spend.warn')}</label>
      <input id="st-spend-warn" class="num" type="number" inputmode="numeric" min="50" max="95" step="5" dir="ltr" bind:value={value.warnPercent} {disabled} aria-invalid={!!error} />
    </div>
  </div>
  {#if error}<p class="err" role="alert">{error}</p>{/if}
  {#if typeof usage?.monthCostUsd === 'number' && usage.limitUsd}
    <p class="sm" class:warn={usage.level === 'warn'} class:err={usage.level === 'reached'} role="status">
      <bdi>{t('runc.spend.used', { spent: fmtMoney(usage.monthCostUsd), limit: fmtMoney(usage.limitUsd), percent: usage.percent ?? 0 })}</bdi>
      {#if usage.level === 'warn' || usage.level === 'reached'} {t('runc.spend.state.' + usage.level)}{/if}
    </p>
  {/if}
{/if}

<style>
  .fields { display: flex; gap: 16px; flex-wrap: wrap; margin-top: 8px; }
  .fields > div { display: grid; gap: 4px; }
  .num { width: 9em; }
</style>
