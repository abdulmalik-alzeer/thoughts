<script lang="ts">
  // Home's usage tile: plan limits and the month's use at a glance, with the way to the details.
  import Icon from '../../lib/Icon.svelte';
  import { go } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import UsageGlance from './UsageGlance.svelte';
</script>

<section class="block usage-tile">
  <div class="surface panel">
    <div class="ph">
      <div class="phl"><div class="eyebrow">{t('r10-usage.tile.eyebrow')}</div><h2>{t('r10-usage.title')}</h2></div>
      <button class="more" onclick={() => go({ kind: 'usage' })}>{t('r10-usage.tile.details')} <Icon name="right" size={14} class="flip-rtl" /></button>
    </div>
    <div class="body"><UsageGlance /></div>
  </div>
</section>

<style>
  .usage-tile { margin-top: 22px; }
  .panel { padding: 18px 20px; }
  .ph { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
  .ph h2 { margin: 0; font-size: 17px; }
  .eyebrow { font-size: var(--fs-xxs, 11px); letter-spacing: .08em; text-transform: uppercase; color: var(--muted); margin-bottom: 2px; }
  .more { font: inherit; font-size: 13px; color: var(--accent); background: none; border: 0; cursor: pointer; display: inline-flex; align-items: center; gap: 4px; padding: 0; }
  .more:hover { text-decoration: underline; }
</style>
