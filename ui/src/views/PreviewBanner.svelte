<script lang="ts">
  // The "you are previewing a change" banner. Like the Review panel it is drawn inside a shadow root
  // (lib/layer/shield.ts), so the staged CSS being previewed cannot hide or restyle it. It can still
  // be painted over by that CSS, which is why no button here keeps anything.
  import Icon from '../lib/Icon.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { shield } from '../lib/layer/shield';
  import { continueChatting, discardPending, openReview, stopPreview } from '../lib/layer';
  import { app } from '../lib/store.svelte';

  let busy = $state(false);
  async function act(fn: () => Promise<unknown>, failKey: string) {
    if (busy) return;
    busy = true;
    try { await fn(); } catch (e) { failed(t(failKey), e); }
    finally { busy = false; }
  }
</script>

<div class="no-print" use:shield={{ box: 'position:sticky;top:4px;z-index:6' }}>
  <div class="banner" role="status">
    <Icon name="eye" size={16} />
    <span class="grow">{t('layer.previewBanner')} {t('layer3.previewReadOnly')}</span>
    <!-- Nothing is kept from here. The change is decided on in the review, and the review is only ever
         shown with the preview switched off (lib/layer.ts `showReview`): whatever the previewed CSS
         does to this page, it is not loaded when the person presses Keep. -->
    <button class="btn primary" disabled={busy} onclick={openReview}>{t('lua.reviewKeep')}</button>
    <button class="btn" disabled={busy} onclick={() => act(discardPending, 'layer.discardFailed')}>{t('layer.discard')}</button>
    {#if app.pending?.canContinue}<button class="btn" disabled={busy} title={t('r10-cust-continue.hint')} onclick={() => act(continueChatting, 'r10-cust-continue.failed')}>{t('r10-cust-continue.button')}</button>{/if}
    <button class="btn" disabled={busy} onclick={() => act(stopPreview, 'shell.layerFailed')}>{t('layer.stopPreview')}</button>
  </div>
</div>
