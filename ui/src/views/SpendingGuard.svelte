<script lang="ts">
  import HelpTip from '../lib/HelpTip.svelte';
  // The monthly spending limit (Settings › Privacy and cost). Edits the draft `value`
  // ({ monthlyLimitUsd, warnPercent }); the page saves it with the rest.
  import { app, saveSettings } from '../lib/store.svelte';
  import { fmtMoney, t } from '../lib/i18n';
  import { approval } from '../lib/consent';
  import { failed } from '../lib/errors';

  let { value = $bindable(), disabled = false, error = '', buildError = '' }: { value: { monthlyLimitUsd: number | null; warnPercent: number; buildHoldPercent?: number; confineCommands?: boolean }; disabled?: boolean; error?: string; buildError?: string } = $props();
  // The MADO build's hold (`spending.buildHoldPercent`): shown at its default until the user changes it.
  const hold = { get v() { return value.buildHoldPercent ?? 80; }, set v(n: number) { value.buildHoldPercent = n; } };
  const on = $derived(value.monthlyLimitUsd != null);
  const usage = $derived(app.status?.usage);

  // The confinement of a build's commands is saved at once, not with the page: switching it off is
  // asked by the desktop app in its own warning, and only its approval lets the service do it.
  const confined = $derived(app.settings?.spending?.confineCommands !== false);
  let confining = $state(false);
  async function setConfine(on: boolean, box: HTMLInputElement) {
    if (confining) return;
    confining = true;
    try {
      let query = '';
      if (!on) {
        const a = await approval('confine-off');
        if (a === null || a === undefined) { box.checked = true; return; }
        query = `?confirm=confine-off&approval=${encodeURIComponent(a)}`;
      }
      await saveSettings((now) => ({ ...now, spending: { ...(now?.spending ?? {}), confineCommands: on } }), query);
      value.confineCommands = on;
    } catch (e) {
      box.checked = confined;
      failed(t('runc.build.confineFailed'), e);
    } finally { confining = false; }
  }

  function toggle(checked: boolean) {
    value.monthlyLimitUsd = checked ? (value.monthlyLimitUsd ?? 20) : null;
  }
</script>

<h3>{t('runc.spend.title')} <HelpTip text={t('runc.spend.hint')} /></h3>
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

<h3 class="buildh">{t('runc.build.title')} <HelpTip text={t('runc.build.hint')} /></h3>
<div class="fields">
  <div>
    <label for="st-build-hold">{t('runc.build.hold')}</label>
    <input id="st-build-hold" class="num" type="number" inputmode="numeric" min="10" max="100" step="5" dir="ltr" bind:value={hold.v} {disabled} aria-invalid={!!buildError} aria-describedby="st-build-hint" />
  </div>
</div>
{#if buildError}<p class="err" role="alert">{buildError}</p>{/if}
<label class="check confine"><input type="checkbox" checked={confined} disabled={disabled || confining} onchange={(e) => setConfine(e.currentTarget.checked, e.currentTarget)} /> {t('runc.build.confine')} <HelpTip text={t('runc.build.confineHint')} /></label>

<style>
  .fields { display: flex; gap: 16px; flex-wrap: wrap; margin-top: 8px; }
  .fields > div { display: grid; gap: 4px; }
  .num { width: 9em; }
  .buildh { margin-top: 20px; }
  .confine { margin-top: 12px; }
</style>
