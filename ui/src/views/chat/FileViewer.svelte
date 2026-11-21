<script lang="ts">
  // One file of a chat opened to look at: a dialog around the app's one file preview
  // (lib/preview/FilePreview.svelte), for files already sent (Attachments.svelte) and files still
  // waiting in the message box (Chat.svelte). Like the picture viewer, it closes only with Esc or
  // its close button: a text selection or a scroll that ends beside the page would otherwise close it.
  import Icon from '../../lib/Icon.svelte';
  import FilePreview from '../../lib/preview/FilePreview.svelte';
  import { t } from '../../lib/i18n';
  import { trapFocus } from '../../lib/focus';

  let { name, load, size, note, onClose, onReveal, onOpen }: {
    name: string;
    /** Fetch the whole file (FilePreview calls it). */
    load: () => Promise<Uint8Array | Blob>;
    size?: number;
    /** A few words beside the name ("Not sent yet"). */
    note?: string;
    onClose: () => void;
    /** Show the file in its folder (the desktop app, for a file that is on disk). */
    onReveal?: () => unknown;
    /** Open the file in its own app, when the desktop app may. */
    onOpen?: () => unknown;
  } = $props();
</script>

<div class="viewer-back" role="presentation">
  <div class="viewer" role="dialog" aria-modal="true" aria-label={name} use:trapFocus={{ onEscape: onClose }}>
    <div class="vhead">
      <bdi class="ell vname">{name}</bdi>
      {#if note}<span class="muted vnote">{note}</span>{/if}
      {#if onOpen}<button type="button" class="btn" onclick={() => onOpen?.()}><Icon name="external" size={14} /> {t('v012-files.openInApp')}</button>{/if}
      <button type="button" class="btn ghost" onclick={onClose} aria-label={t('close')} title={t('close')}><Icon name="close" /></button>
    </div>
    <div class="vbody"><FilePreview {name} {load} {size} fill {onReveal} /></div>
  </div>
</div>

<style>
  .viewer-back { white-space: normal; text-align: start; position: fixed; inset: 0; z-index: 50; background: color-mix(in srgb, #000 55%, transparent); display: grid; place-items: center; padding: 24px; }
  .viewer { background: var(--panel); color: var(--fg); border-radius: var(--radius-lg, 14px); box-shadow: var(--shadow-lg); width: min(1000px, 100%); max-height: 100%;
    display: flex; flex-direction: column; overflow: hidden; font-size: var(--fs-base, inherit); font-weight: normal; }
  .vhead { display: flex; align-items: center; gap: 8px; padding-block: 8px; padding-inline: 14px 8px; border-bottom: var(--border-w) solid var(--border); }
  .vname { flex: 1; font-weight: 600; min-width: 0; }
  .vnote { font-size: var(--fs-xs); white-space: nowrap; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .vbody { height: calc(100vh - 120px); min-height: 0; }
  @media print { .viewer-back { display: none; } }
</style>
