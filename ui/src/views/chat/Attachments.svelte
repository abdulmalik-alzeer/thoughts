<script lang="ts" module>
  // Pictures and PDFs already fetched this session, by chat and file name (they never change).
  const urls = new Map<string, Promise<string | null>>();
</script>

<script lang="ts">
  // The files sent with a message: pictures as thumbnails, PDFs and pictures open in a viewer
  // inside the app, anything else is named. The service serves only this chat's own files.
  import Icon from '../../lib/Icon.svelte';
  import { apiBlob, inTauri } from '../../lib/api';
  import { toast } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { isImageName } from '../../lib/content/util';
  import { trapFocus } from '../../lib/focus';

  let { chatId, paths }: { chatId: string; paths: string[] } = $props();

  type F = { name: string; image: boolean; pdf: boolean };
  const files = $derived(paths.map((p): F => {
    const name = p.split(/[\\/]/).pop() || p;
    return { name, image: isImageName(name), pdf: /\.pdf$/i.test(name) };
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
  // something when opened never do (the desktop app checks the same list).
  const OWN_APP = /\.(docx?|xlsx?|pptx?|csv|tsv|txt|md|rtf|od[tsp]|epub|heic|tiff|bmp|mp3|m4a|wav|mp4|mov)$/i;
  const openable = (name: string) => inTauri() && OWN_APP.test(name);
  async function openOutside(name: string) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('open_chat_attachment', { chatId, name });
    } catch { toast(t('nativeb.att.openFailed', { name }), { kind: 'error' }); }
  }

  let viewing = $state<F | null>(null);
  let viewUrl = $state<string | null>(null);
  let viewFailed = $state(false);
  let opened = 0;
  async function view(f: F) {
    const n = ++opened;
    viewing = f; viewUrl = null; viewFailed = false;
    const u = await url(f.name);
    if (n !== opened) return; // another file was opened meanwhile
    if (u) viewUrl = u; else viewFailed = true;
  }
</script>

<div class="atts">
  {#each files as f, i (f.name + i)}
    {#if f.image}
      {#await url(f.name)}
        <span class="thumb wait" aria-hidden="true"></span>
      {:then u}
        {#if u}
          <button type="button" class="thumb" onclick={() => view(f)} title={t('chatc.att.open', { name: f.name })} aria-label={t('chatc.att.open', { name: f.name })}><img src={u} alt="" /></button>
        {:else}
          <span class="fchip" title={t('chatc.att.gone')}><Icon name="attach" size={12} /><bdi class="ell">{f.name}</bdi></span>
        {/if}
      {/await}
    {:else if f.pdf}
      <button type="button" class="fchip open" onclick={() => view(f)} title={t('chatc.att.open', { name: f.name })}><Icon name="docs" size={12} /><bdi class="ell">{f.name}</bdi></button>
    {:else if openable(f.name)}
      <button type="button" class="fchip open" onclick={() => openOutside(f.name)} title={t('nativeb.att.open', { name: f.name })}><Icon name="attach" size={12} /><bdi class="ell">{f.name}</bdi></button>
    {:else}
      <span class="fchip"><Icon name="attach" size={12} /><bdi class="ell">{f.name}</bdi></span>
    {/if}
  {/each}
  <!-- Inside the list, so no stray line appears in the message bubble around it. -->
  {#if viewing}
    <div class="viewer-back" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) viewing = null; }}>
      <div class="viewer" role="dialog" aria-modal="true" aria-label={viewing.name} use:trapFocus={{ onEscape: () => (viewing = null) }}>
        <div class="vhead">
          <bdi class="ell vname">{viewing.name}</bdi>
          <button type="button" class="btn ghost" onclick={() => (viewing = null)} aria-label={t('close')} title={t('close')}><Icon name="close" /></button>
        </div>
        {#if viewFailed}
          <p class="muted vmsg">{t('chatc.att.gone')}</p>
        {:else if !viewUrl}
          <p class="muted vmsg" role="status">{t('chatc.att.loading')}</p>
        {:else if viewing.image}
          <img class="vimg" src={viewUrl} alt={viewing.name} />
        {:else}
          <iframe class="vpdf" src={viewUrl} title={viewing.name}></iframe>
        {/if}
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
  .vmsg { padding: 24px; margin: 0; text-align: center; }
  .vimg { max-width: 100%; max-height: calc(100vh - 120px); object-fit: contain; margin: auto; display: block; padding: 12px; }
  .vpdf { width: 100%; height: calc(100vh - 120px); border: 0; background: #fff; }
  @media print { .viewer-back { display: none; } .thumb { cursor: default; } }
</style>
