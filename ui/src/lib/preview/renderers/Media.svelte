<script lang="ts">
  // Sound and video with the window's own player. Only what the window can play plays; anything
  // else (a codec it lacks) reports the file as unshowable. Never starts by itself.
  import { t } from '../../i18n';
  import type { RendererProps } from '../renderers';

  let { data, name, kind, fill, unsupported }: RendererProps = $props();

  const src = $derived(URL.createObjectURL(data));
  $effect(() => { const u = src; return () => URL.revokeObjectURL(u); });
  const failed = () => unsupported(t('libf-preview-core.media.cannot'));
</script>

<div class="media" class:fill class:audio={kind === 'audio'}>
  {#if kind === 'video'}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video {src} controls preload="metadata" playsinline aria-label={name} onerror={failed}></video>
  {:else}
    <audio {src} controls preload="metadata" aria-label={name} onerror={failed}></audio>
  {/if}
</div>

<style>
  .media { display: grid; place-items: center; min-height: 0; }
  .media.fill { height: 100%; padding: 12px; }
  .media.audio { padding: 18px; border: var(--border-w) solid var(--border); border-radius: var(--radius); background: var(--panel); }
  .media.audio.fill { border: 0; }
  video { display: block; max-width: 100%; max-height: 75vh; border-radius: var(--radius); background: #000; }
  .fill video { max-height: 100%; }
  audio { width: min(520px, 100%); }
</style>
