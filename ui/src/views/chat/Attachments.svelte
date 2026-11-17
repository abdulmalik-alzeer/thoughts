<script lang="ts" module>
  // Pictures and PDFs already fetched this session, by chat and file name (they never change).
  const urls = new Map<string, Promise<string | null>>();
</script>

<script lang="ts">
  // The files sent with a message: pictures as thumbnails that open in the app's picture viewer
  // (lib/ImageViewer.svelte), PDFs in a viewer of their own here, anything else is named. The service serves only this chat's own files.
  import Icon from '../../lib/Icon.svelte';
  import { apiBlob, inTauri } from '../../lib/api';
  import { toast } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { isImageName } from '../../lib/content/util';
  import { trapFocus } from '../../lib/focus';
  import { canOpenInApp, revealChatAttachment, revealLabelKey } from '../../lib/openInApp';
  import { openImage } from '../../lib/imageOpen.svelte';
  import FilePreview from '../../lib/preview/FilePreview.svelte';
  import { previewKind } from '../../lib/preview/kinds';
  import { renderers } from '../../lib/preview/renderers';

  let { chatId, paths }: { chatId: string; paths: string[] } = $props();

  // `pdf`: shown in the viewer here (lib/preview: PDFs, text, code, Word… whatever has a preview).
  type F = { name: string; image: boolean; pdf: boolean };
  const files = $derived(paths.map((p): F => {
    const name = p.split(/[\\/]/).pop() || p;
    return { name, image: isImageName(name), pdf: !!renderers[previewKind(name)] };
  }));

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

  // Other documents (Word, Excel…) open in their own app on the desktop; kinds that could run
  // something when opened (old Office files, programs…) are only shown in their folder (the
  // desktop app checks the same list, `lib/openInApp.ts`).
  const openable = (name: string) => canOpenInApp(name);
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
  const view = (f: F) => { viewing = f; };
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
    {:else if f.pdf}
      <button type="button" class="fchip open" onclick={() => view(f)} title={t('chatc.att.open', { name: f.name })}><Icon name="docs" size={12} /><bdi class="ell">{f.name}</bdi></button>
    {:else if openable(f.name)}
      <button type="button" class="fchip open" onclick={() => openOutside(f.name)} title={t('nativeb.att.open', { name: f.name })}><Icon name="attach" size={12} /><bdi class="ell">{f.name}</bdi></button>
    {:else if inTauri()}
      <button type="button" class="fchip open" onclick={() => revealOutside(f.name)} title={`${t('sec-shell-open.revealTip', { name: f.name })} (${t(revealLabelKey())})`}><Icon name="folder" size={12} /><bdi class="ell">{f.name}</bdi></button>
    {:else}
      <span class="fchip"><Icon name="attach" size={12} /><bdi class="ell">{f.name}</bdi></span>
    {/if}
  {/each}
  <!-- Inside the list, so no stray line appears in the message bubble around it. -->
  {#if viewing}
    <!-- Like the picture viewer, it closes only with Esc or the close button: a text selection or a
         scroll that ends beside the page would otherwise close it. -->
    <div class="viewer-back" role="presentation">
      <div class="viewer" role="dialog" aria-modal="true" aria-label={viewing.name} use:trapFocus={{ onEscape: () => (viewing = null) }}>
        <div class="vhead">
          <bdi class="ell vname">{viewing.name}</bdi>
          <button type="button" class="btn ghost" onclick={() => (viewing = null)} aria-label={t('close')} title={t('close')}><Icon name="close" /></button>
        </div>
        <div class="vbody"><FilePreview name={viewing.name} load={fetchFile(viewing.name)} fill onReveal={inTauri() ? () => revealOutside(viewing!.name) : undefined} /></div>
      </div>
    </div>
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
  .viewer-back { white-space: normal; text-align: start; position: fixed; inset: 0; z-index: 50; background: color-mix(in srgb, #000 55%, transparent); display: grid; place-items: center; padding: 24px; }
  .viewer { background: var(--panel); color: var(--fg); border-radius: var(--radius-lg, 14px); box-shadow: var(--shadow-lg); width: min(1000px, 100%); max-height: 100%;
    display: flex; flex-direction: column; overflow: hidden; }
  .vhead { display: flex; align-items: center; gap: 8px; padding: 8px 8px 8px 14px; border-bottom: var(--border-w) solid var(--border); }
  .vname { flex: 1; font-weight: 600; min-width: 0; }
  .vbody { height: calc(100vh - 120px); min-height: 0; }
  @media print { .viewer-back { display: none; } .thumb { cursor: default; } }
</style>
