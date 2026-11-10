<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { app } from '../lib/store.svelte';
  import { t, tn } from '../lib/i18n';
  import { trapFocus } from '../lib/focus';
  let { startWrap }: { startWrap: () => void } = $props();
  const close = () => (app.showWrapNag = false);
</script>

<div class="modal-bg no-print" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="modal" style="max-width:520px" role="dialog" aria-modal="true" aria-labelledby="wn-title" aria-describedby="wn-body" tabindex="-1" use:trapFocus={{ onEscape: close }}>
    <h1 class="row" id="wn-title"><Icon name="wrap" size={22} /> {t('shell.wn.title')}</h1>
    <p id="wn-body">{tn('wrapNagBody', app.wrap.count)}</p>
    <p class="muted">{t('shell.wn.preview')}</p>
    <div class="row"><span class="grow"></span>
      <button class="btn" onclick={close}>{t('shell.wn.later')}</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button class="btn primary" autofocus onclick={startWrap}>{t('shell.wn.now')}</button>
    </div>
  </div>
</div>
