<script lang="ts">
  // A PDF in the window engine's own PDF viewer, from the file's bytes typed as a PDF (FilePreview),
  // so nothing else can be made of them. Its links and forms stay inside that viewer.
  import { t } from '../../i18n';
  import type { RendererProps } from '../renderers';

  let { data, name, fill }: RendererProps = $props();

  const src = $derived(URL.createObjectURL(data));
  $effect(() => { const u = src; return () => URL.revokeObjectURL(u); });
</script>

<iframe class="pdf" class:fill {src} title={t('docs3.pdfTitle', { name })}></iframe>

<style>
  .pdf { display: block; width: 100%; height: calc(100vh - 230px); min-height: 360px; border: var(--border-w) solid var(--border); border-radius: var(--radius); background: #fff; }
  .pdf.fill { height: 100%; min-height: 0; border: 0; border-radius: 0; }
</style>
