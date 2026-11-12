<script lang="ts">
  import { dialogs, dismissDialog } from './dialog.svelte';
  import { t } from './i18n';
  import { trapFocus } from './focus';
  import TrustBody from './TrustBody.svelte';

  let value = $state('');
  $effect(() => { if (dialogs.current) value = dialogs.current.value ?? ''; });

  function close(result: any) {
    const d = dialogs.current;
    dialogs.current = null;
    d?.resolve(result);
  }
  const cancel = () => { dismissDialog(); };
  const ok = () => close(dialogs.current?.kind === 'prompt' ? value : true);
</script>

{#if dialogs.current}
  {@const d = dialogs.current}
  <div class="modal-bg top" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) cancel(); }}>
    <div class="modal dlg" class:wide={!!d.trust} role="alertdialog" aria-modal="true" aria-labelledby="dlg-title" aria-describedby={d.body || d.trust ? 'dlg-body' : undefined} tabindex="-1"
      use:trapFocus={{ onEscape: cancel }}>
      <h2 id="dlg-title" dir="auto">{d.title}</h2>
      {#if d.trust}<TrustBody info={d.trust} />{:else if d.body}<p id="dlg-body" class="muted" dir="auto">{d.body}</p>{/if}
      {#if d.list?.length}<ul class="scroll">{#each d.list as li}<li><bdi>{li}</bdi></li>{/each}</ul>{/if}
      {#if d.note}<p class="note" dir="auto">{d.note}</p>{/if}
      {#if d.kind === 'prompt'}
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value placeholder={d.placeholder ?? ''} aria-labelledby="dlg-title" dir="auto" autofocus
          onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); ok(); } }} />
      {/if}
      <div class="row end">
        <button class="btn" onclick={cancel}>{d.cancelLabel ?? t('cancel')}</button>
        {#if d.kind === 'choice'}
          {#each d.choices ?? [] as c (c.id)}
            <button class="btn" class:primary={c.primary} class:danger={c.danger} onclick={() => close(c.id)}>{c.label}</button>
          {/each}
        {:else}
          <button class="btn primary" class:danger={d.danger} onclick={ok}>{d.confirmLabel ?? t('confirm')}</button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .dlg { max-width: 480px; display: grid; gap: 12px; }
  .dlg.wide { max-width: 600px; }
  h2 { margin: 0; font-size: 17px; }
  p { margin: 0; white-space: pre-wrap; }
  ul { margin: 0; padding-inline-start: 18px; display: grid; gap: 4px; font-size: 13px; max-height: 30vh; }
  /* Long items (a web address, a file path) wrap instead of widening the dialog. */
  li { overflow-wrap: anywhere; }
  .note { font-size: 13px; color: var(--fg); background: var(--fill); border-radius: var(--radius); padding: 8px 10px; }
  .end { justify-content: flex-end; gap: 8px; margin-top: 4px; flex-wrap: wrap; }
  .btn.primary.danger { background: var(--danger); color: var(--bg); }
</style>
