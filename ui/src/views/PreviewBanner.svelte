<script lang="ts">
  // The "you are previewing a change" banner. Like the Review panel it is drawn inside a shadow root
  // (lib/layer/shield.ts), so the staged CSS being previewed cannot hide or restyle it.
  import Icon from '../lib/Icon.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { shield } from '../lib/layer/shield';
  import { discardPending, keepPending, openReview, stopPreview } from '../lib/layer';
  import { app } from '../lib/store.svelte';

  let busy = $state(false);
  async function act(fn: () => Promise<unknown>, failKey: string) {
    if (busy) return;
    busy = true;
    try { await fn(); } catch (e) { failed(t(failKey), e); }
    finally { busy = false; }
  }
  const blocked = $derived(!!app.pending?.problems?.length);
</script>

<div class="no-print" use:shield={{ box: 'position:sticky;top:30px;z-index:6' }}>
  <div class="banner" role="status">
    <Icon name="eye" size={16} />
    <span class="grow">{t('layer.previewBanner')} {t('layer3.previewReadOnly')}</span>
    <button class="btn primary" disabled={busy || blocked} onclick={() => act(() => keepPending(), 'layer3.keepFailed')}>{t('layer.keep')}</button>
    <button class="btn" disabled={busy} onclick={() => act(discardPending, 'layer.discardFailed')}>{t('layer.discard')}</button>
    <button class="btn" disabled={busy} onclick={() => act(stopPreview, 'shell.layerFailed')}>{t('layer.stopPreview')}</button>
    <button class="btn ghost" disabled={busy} onclick={openReview}>{t('layer.details')}</button>
  </div>
</div>
