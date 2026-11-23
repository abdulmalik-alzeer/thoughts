<script lang="ts">
  // The card under an answer whose assistant could not find what the user meant: it suggests search
  // by meaning. "Set it up" only opens Settings › Features, where the size and licence are shown
  // and the user presses the button themselves; nothing is downloaded from here.
  import Icon from './Icon.svelte';
  import { t } from './i18n';
  import { go } from './store.svelte';
  import { failed } from './errors';
  import { meaning, decline } from './meaning.svelte';

  let { chatId }: { chatId: string } = $props();
  let busy = $state(false);

  function setUp() {
    delete meaning.offers[chatId];
    go({ kind: 'settings', id: 'modules' });
  }
  async function no(op: 'later' | 'never') {
    if (busy) return;
    busy = true;
    try { await decline(op, chatId); } catch (e) { failed(t('meaning.failed'), e); } finally { busy = false; }
  }
</script>

<div class="offer" role="note" aria-label={t('meaning.offer.label')}>
  <Icon name="search" size={15} />
  <div class="grow">
    <p>{t('meaning.offer.text')}</p>
    <div class="row">
      <button type="button" class="btn primary" onclick={setUp}>{t('meaning.setUp')}</button>
      <button type="button" class="btn ghost" disabled={busy} onclick={() => no('later')}>{t('meaning.notNow')}</button>
      <button type="button" class="btn ghost" disabled={busy} onclick={() => no('never')}>{t('meaning.offer.never')}</button>
    </div>
  </div>
</div>

<style>
  .offer { display: flex; gap: 10px; align-items: flex-start; margin: 6px 0 12px; padding: 10px 12px; border: var(--border-w, 1px) solid var(--hairline);
    border-radius: var(--radius-lg, 12px); background: var(--panel); font-size: var(--fs-sm); max-width: 640px; }
  .offer :global(.icon) { flex: none; margin-top: 2px; color: var(--accent); }
  .offer p { margin: 0 0 8px; }
  .grow { flex: 1; min-width: 0; }
  .row { display: flex; gap: 8px; flex-wrap: wrap; }
</style>
