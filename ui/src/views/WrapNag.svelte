<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { app } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  let { startWrap }: { startWrap: () => void } = $props();
</script>

<div class="modal-bg">
  <div class="modal" style="max-width:520px">
    <h1 class="row"><Icon name="wrap" size={22} /> {t('wrapNagTitle')}</h1>
    <p>{t('wrapNagBody', { n: app.wrap.count })}</p>
    <p class="muted">Each chat already has a short mini-wrap, so this is quick: you'll see a preview of every change before anything is written.</p>
    <div class="row"><span class="grow"></span>
      <button class="btn" onclick={() => (app.showWrapNag = false)}>Later</button>
      <button class="btn primary" onclick={startWrap}>{t('wrap')} now</button>
    </div>
  </div>
</div>
