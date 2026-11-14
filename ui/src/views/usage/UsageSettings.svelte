<script lang="ts">
  // Settings › Usage: which runs the API-price figures cover, and the usage at a glance.
  import Icon from '../../lib/Icon.svelte';
  import { go } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import UsageGlance from './UsageGlance.svelte';

  let { value = $bindable(), disabled = false, vis = (..._k: string[]) => true }: { value: any; disabled?: boolean; vis?: (...k: string[]) => boolean } = $props();
  const whole = $derived(!!value?.wholeComputer);
  const set = (on: boolean) => { value = { ...(value ?? { monthlyLimitUsd: null, warnPercent: 80 }), wholeComputer: on }; };
</script>

{#if vis('r10-usage.set.glance')}
  <div class="card">
    <div class="row hd">
      <h3 class="grow">{t('r10-usage.set.glance')}</h3>
      <button type="button" class="btn" onclick={() => go({ kind: 'usage' })}><Icon name="usage" size={14} /> {t('r10-usage.set.open')}</button>
    </div>
    <UsageGlance />
  </div>
{/if}
{#if vis('r10-usage.set.scopeTitle')}
  <div class="card">
    <fieldset class="scope" {disabled}>
      <legend><h3>{t('r10-usage.set.scopeTitle')}</h3></legend>
      <p class="muted sm">{t('r10-usage.set.scopeHint')}</p>
      <label class="opt"><input type="radio" name="r10-usage-scope" checked={!whole} onchange={() => set(false)} />
        <span><b>{t('r10-usage.scope.thoughts')}</b><span class="muted sm">{t('r10-usage.scope.thoughtsHint')}</span></span></label>
      <label class="opt"><input type="radio" name="r10-usage-scope" checked={whole} onchange={() => set(true)} />
        <span><b>{t('r10-usage.scope.computer')}</b><span class="muted sm">{t('r10-usage.scope.computerHint')}</span></span></label>
    </fieldset>
    <p class="muted sm note">{t('r10-usage.set.priceNote')}</p>
  </div>
{/if}

<style>
  .card { margin-bottom: 12px; }
  .hd { margin-bottom: 12px; align-items: center; }
  .hd h3 { margin: 0; }
  .scope { border: 0; padding: 0; margin: 0; min-width: 0; display: flex; flex-direction: column; gap: 10px; }
  legend { padding: 0; }
  legend h3 { margin: 0; }
  .scope > p { margin: 0; }
  .opt { display: flex; gap: 10px; align-items: flex-start; cursor: pointer; }
  .opt input { margin-top: 3px; flex: none; }
  .opt > span { display: flex; flex-direction: column; gap: 2px; }
  .sm { font-size: var(--fs-xs); }
  .note { margin: 12px 0 0; }
</style>
