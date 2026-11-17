<script lang="ts">
  // A picture, including SVG. An SVG is only ever shown through <img>, never put into the page: as a
  // picture it can run no script and load nothing (not even the files it links to). A format the
  // window's engine can't decode (HEIC on some systems) fails to load and is reported as unshowable.
  import { t, fmtNum } from '../../i18n';
  import type { RendererProps } from '../renderers';

  let { data, name, fill, unsupported, zoom }: RendererProps = $props();

  const src = $derived(URL.createObjectURL(data));
  $effect(() => { const u = src; return () => URL.revokeObjectURL(u); });
  let dims = $state('');
  function loaded(e: Event) {
    const i = e.currentTarget as HTMLImageElement;
    dims = i.naturalWidth && i.naturalHeight ? t('libf-preview-core.dims', { w: fmtNum(i.naturalWidth), h: fmtNum(i.naturalHeight) }) : '';
  }
</script>

<figure class="img" class:fill>
  {#if zoom}
    <button type="button" class="zoom" onclick={() => zoom?.(src, name)} data-no-zoom aria-label={t('libf-preview-core.zoom', { name })} title={t('libf-preview-core.zoom', { name })}>
      <img {src} alt={name} onload={loaded} onerror={() => unsupported()} />
    </button>
  {:else}
    <img {src} alt={name} onload={loaded} onerror={() => unsupported()} />
  {/if}
  {#if dims}<figcaption class="muted">{dims}</figcaption>{/if}
</figure>

<style>
  .img { margin: 0; display: grid; justify-items: center; gap: 6px; min-height: 0; }
  .img.fill { height: 100%; grid-template-rows: minmax(0, 1fr) auto; padding: 12px; }
  img { display: block; max-width: 100%; max-height: 75vh; height: auto; border-radius: var(--radius); object-fit: contain;
    /* A see-through picture shows on a checkerboard, so it reads in both themes. */
    background: repeating-conic-gradient(var(--fill) 0% 25%, transparent 0% 50%) 50% / 16px 16px; }
  .fill img { max-height: 100%; height: 100%; }
  .zoom { padding: 0; border: 0; background: none; cursor: zoom-in; border-radius: var(--radius); display: block; max-width: 100%; min-height: 0; }
  .fill .zoom { height: 100%; }
  .zoom:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
  figcaption { font-size: var(--fs-xs); }
</style>
