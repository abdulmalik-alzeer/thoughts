<script lang="ts">
  import { dialogs } from './dialog.svelte';
  import { t } from './i18n';
  import { trapFocus } from './focus';

  let value = $state('');
  $effect(() => { if (dialogs.current) value = dialogs.current.value ?? ''; });

  function close(result: any) {
    const d = dialogs.current;
    dialogs.current = null;
    d?.resolve(result);
  }
  const cancel = () => close(dialogs.current?.kind === 'prompt' ? null : false);
  const ok = () => close(dialogs.current?.kind === 'prompt' ? value : true);
</script>

{#if dialogs.current}
  {@const d = dialogs.current}
  <div class="modal-bg" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) cancel(); }}>
    <div class="modal dlg" role="alertdialog" aria-modal="true" aria-labelledby="dlg-title" tabindex="-1"
      use:trapFocus={{ onEscape: cancel }}>
      <h2 id="dlg-title">{d.title}</h2>
      {#if d.body}<p class="muted" dir="auto">{d.body}</p>{/if}
      {#if d.list?.length}<ul dir="auto">{#each d.list as li}<li><bdi>{li}</bdi></li>{/each}</ul>{/if}
      {#if d.kind === 'prompt'}
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value placeholder={d.placeholder ?? ''} dir="auto" autofocus
          onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); ok(); } }} />
      {/if}
      <div class="row end">
        <button class="btn" onclick={cancel}>{d.cancelLabel ?? t('cancel')}</button>
        <button class="btn primary" class:danger={d.danger} onclick={ok}>{d.confirmLabel ?? t('confirm')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .dlg { max-width: 440px; display: grid; gap: 12px; }
  h2 { margin: 0; font-size: 17px; }
  p { margin: 0; white-space: pre-wrap; }
  ul { margin: 0; padding-inline-start: 18px; display: grid; gap: 4px; font-size: 13px; }
  .end { justify-content: flex-end; gap: 8px; margin-top: 4px; }
  .btn.danger { background: #c0392b; border-color: #c0392b; color: #fff; }
</style>
