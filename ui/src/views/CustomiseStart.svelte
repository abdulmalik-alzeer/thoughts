<script lang="ts">
  // The opening of a new customisation chat (nothing written yet): what to do here, and a few
  // examples that fill in the message box so the person can adjust them before sending. How a
  // customisation works is behind the "?" beside the question.
  import Icon from '../lib/Icon.svelte';
  import HelpTip from '../lib/HelpTip.svelte';
  import { t } from '../lib/i18n';

  let { onpick }: { onpick: (text: string) => void } = $props();
  const examples = ['ex1', 'ex2', 'ex3', 'ex4'];
</script>

<div class="cstart" role="note">
  <span class="mark" aria-hidden="true"><Icon name="palette" size={22} /></span>
  <h2>{t('r10-cust-start.emptyTitle')} <HelpTip label={t('czh.start')} text={t('r10-cust-start.emptyBody')} /></h2>
  <p class="muted sm label">{t('r10-cust-start.examples')}</p>
  <ul class="ex">
    {#each examples as k (k)}
      <li><button type="button" class="chip" onclick={() => onpick(t('r10-cust-start.' + k))}
        aria-label={t('r10-cust-start.useExample', { text: t('r10-cust-start.' + k) })}><span dir="auto">{t('r10-cust-start.' + k)}</span></button></li>
    {/each}
  </ul>
</div>

<style>
  .cstart { margin: auto; max-width: 520px; display: flex; flex-direction: column; align-items: center; text-align: center; gap: 6px; padding: 16px 8px; }
  .mark { display: grid; place-items: center; width: 44px; height: 44px; border-radius: 50%; background: var(--fill); color: var(--accent-2); margin-bottom: 4px; }
  h2 { font-size: 17px; font-weight: 600; margin: 0; }
  p { margin: 0; font-size: 13.5px; line-height: 1.5; }
  .label { margin-top: 10px; font-size: var(--fs-xs); }
  .ex { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; justify-content: center; gap: 6px; }
  .chip { font: inherit; background: none; cursor: pointer; font-size: var(--fs-sm); font-weight: 500; color: var(--muted); padding: 5px 11px; border-radius: 99px; border: 1px solid var(--hairline); text-align: start; }
  .chip:hover { color: var(--fg); border-color: color-mix(in srgb, var(--fg) 25%, transparent); }
  .chip:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  @media print { .cstart { display: none !important; } }
</style>
