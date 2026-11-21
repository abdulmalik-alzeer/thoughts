<script lang="ts" module>
  // Pictures and PDFs already fetched this session, by chat and file name (they never change).
  const urls = new Map<string, Promise<string | null>>();
</script>

<script lang="ts">
  // The files sent with a message. Every one opens with a click: pictures as thumbnails that open
  // in the app's picture viewer (lib/ImageViewer.svelte), anything else in the app's file preview
  // (FileViewer.svelte around lib/preview: sheets, documents, slides, PDFs, text, code, sound…).
  // A file the preview has no way to show says so there, with "Open in its app" (where the desktop
  // app may open that kind, `lib/openInApp.ts`) and "Show in Finder". The service serves only this chat's own files.
  import Icon from '../../lib/Icon.svelte';
  import { apiBlob, inTauri } from '../../lib/api';
  import { toast } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { canOpenInApp, revealChatAttachment } from '../../lib/openInApp';
  import { openImage } from '../../lib/imageOpen.svelte';
  import FileViewer from './FileViewer.svelte';
  import { chipAction, fileName } from './attachView';

  let { chatId, paths }: { chatId: string; paths: string[] } = $props();

  type F = { name: string; image: boolean };
  const files = $derived(paths.map((p): F => { const name = fileName(p); return { name, image: chipAction(name) === 'picture' }; }));

  function url(name: string): Promise<string | null> {
    const k = `${chatId}/${name}`;
    let u = urls.get(k);
    if (!u) {
      u = apiBlob(`/chats/${encodeURIComponent(chatId)}/attachments/${encodeURIComponent(name)}`)
        .then((b) => URL.createObjectURL(b))
        .catch(() => { urls.delete(k); return null; });
      urls.set(k, u);
    }
    return u;
  }

  // Kinds that could run something when opened (old Office files, programs…) are never opened
  // from here, only shown in their folder (the desktop app checks the same list).
  async function openOutside(name: string) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('open_chat_attachment', { chatId, name });
    } catch { toast(t('nativeb.att.openFailed', { name }), { kind: 'error' }); }
  }
  async function revealOutside(name: string) {
    try { await revealChatAttachment(chatId, name); }
    catch { toast(t('sec-shell-open.revealFailed'), { kind: 'error' }); }
  }

  let viewing = $state<F | null>(null);
  const fetchFile = (name: string) => () => apiBlob(`/chats/${encodeURIComponent(chatId)}/attachments/${encodeURIComponent(name)}`);
</script>

<div class="atts">
  {#each files as f, i (f.name + i)}
    {#if f.image}
      {#await url(f.name)}
        <span class="thumb wait" aria-hidden="true"></span>
      {:then u}
        {#if u}
          <button type="button" class="thumb" onclick={(e) => openImage({ src: u, name: f.name, alt: f.name, from: e.currentTarget })} title={t('chatc.att.open', { name: f.name })} aria-label={t('chatc.att.open', { name: f.name })}><img src={u} alt="" /></button>
        {:else}
          <span class="fchip" title={t('chatc.att.gone')}><Icon name="attach" size={12} /><bdi class="ell">{f.name}</bdi></span>
        {/if}
      {/await}
    {:else}
      <button type="button" class="fchip open" onclick={() => (viewing = f)} title={t('v012-files.preview', { name: f.name })}><Icon name="docs" size={12} /><bdi class="ell">{f.name}</bdi></button>
    {/if}
  {/each}
  <!-- Inside the list, so no stray line appears in the message bubble around it. -->
  {#if viewing}
    {@const name = viewing.name}
    <FileViewer {name} load={fetchFile(name)} onClose={() => (viewing = null)}
      onOpen={canOpenInApp(name) ? () => openOutside(name) : undefined}
      onReveal={inTauri() ? () => revealOutside(name) : undefined} />
  {/if}
</div>



<style>
  .atts { display: flex; flex-wrap: wrap; align-items: flex-start; gap: 6px; margin-top: 6px; white-space: normal; }
  .thumb { padding: 0; border: 0; border-radius: 10px; overflow: hidden; cursor: zoom-in; background: var(--fill); display: block; line-height: 0; }
  .thumb img { max-width: 180px; max-height: 140px; object-fit: cover; display: block; }
  .thumb.wait { width: 72px; height: 54px; }
  .thumb:focus-visible { outline: 2px solid var(--accent-fg); outline-offset: 2px; }
  .fchip { font: inherit; font-size: var(--fs-xs); color: inherit; background: color-mix(in srgb, currentColor 12%, transparent); border: 0; border-radius: 8px;
    padding: 3px 8px; display: inline-flex; align-items: center; gap: 5px; max-width: 240px; }
  .fchip.open { cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  @media print { .thumb { cursor: default; } }
</style>
