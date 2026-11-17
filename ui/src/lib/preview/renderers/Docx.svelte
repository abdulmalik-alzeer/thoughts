<script lang="ts">
  // A text document shown as a readable page: Word (.docx, lib/content/docxview.ts), OpenDocument
  // text (.odt, office/odf.ts) and Rich Text (.rtf, office/rtf.ts; legacy kind). Old binary Word,
  // Excel and PowerPoint files (.doc, .xls, .ppt) can't be read safely here and say so, unless the
  // file is really one of the readable kinds under an old name. Pictures inside become object URLs,
  // revoked when the page goes.
  import { onDestroy, untrack } from 'svelte';
  import Icon from '../../Icon.svelte';
  import { t } from '../../i18n';
  import { docxToDom } from '../../content/docxview';
  import { docDir } from '../../content/docx';
  import { zoomImages } from '../../zoomImages';
  import { odtToDom } from '../office/odf';
  import { rtfToDom } from '../office/rtf';
  import { problemOf } from '../office/safezip';
  import { problemText } from '../office/problems';
  import type { RendererProps } from '../renderers';

  let { data, name, kind, unsupported }: RendererProps = $props();

  let host = $state<HTMLElement>();
  let phase = $state<'loading' | 'ready' | 'empty'>('loading');
  let skipped = $state(false);
  let word = $state(true);
  let urls: string[] = [];
  let seq = 0;
  const drop = () => { for (const u of urls.splice(0)) URL.revokeObjectURL(u); };

  /** Which reader a file needs: by its kind, and for old names by what the bytes really are. */
  function readerFor(b: Uint8Array): 'docx' | 'odt' | 'rtf' | null {
    if (kind === 'docx') return 'docx';
    if (kind === 'odt') return 'odt';
    const head = String.fromCharCode(...b.subarray(0, 5));
    if (head === '{\\rtf') return 'rtf';
    // A newer Word file saved under the old name.
    if (head.startsWith('PK') && /\.(doc|dot)$/i.test(name)) return 'docx';
    return null;
  }

  async function render(blob: Blob) {
    const n = ++seq;
    phase = 'loading'; skipped = false;
    drop();
    try {
      const bytes = new Uint8Array(await blob.arrayBuffer());
      if (n !== seq) return;
      const reader = readerFor(bytes);
      if (!reader) { unsupported(t('libf-preview-office.legacy')); return; }
      word = reader === 'docx';
      const view = reader === 'docx' ? await docxToDom(bytes) : reader === 'odt' ? await odtToDom(bytes) : rtfToDom(bytes);
      if (n !== seq) { for (const u of view.urls) URL.revokeObjectURL(u); return; }
      urls = view.urls;
      skipped = view.skipped;
      const empty = !(view.node.textContent ?? '').trim() && !view.node.querySelector('img');
      phase = empty ? 'empty' : 'ready';
      await Promise.resolve();
      // The page runs the way most of the document reads (tables and lists of an Arabic document start on the right).
      if (host) { host.dir = docDir(view.node.textContent ?? ''); host.replaceChildren(view.node); }
    } catch (e) {
      if (n === seq) unsupported(problemText(problemOf(e)));
    }
  }
  $effect(() => { const b = data; untrack(() => render(b)); });
  onDestroy(() => { seq++; drop(); });
</script>

{#if phase === 'loading'}
  <div class="pad" role="status" aria-busy="true"><span class="sr-only">{t(kind === 'docx' ? 'docsb.docxLoading' : 'libf-preview-office.doc.loading')}</span><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div></div>
{:else if phase === 'empty'}
  <p class="muted msg">{t(word ? 'docsb.docxEmpty' : 'libf-preview-office.doc.empty')}</p>
{/if}
{#if skipped && phase === 'ready'}
  <p class="muted sm note"><Icon name="info" size={14} /> {t(word ? 'docsb.docxSkipped' : 'libf-preview-office.doc.skipped')}</p>
{/if}
<article class="md page" class:hide={phase !== 'ready'} bind:this={host} aria-label={t('docsb.docxLabel', { name })} use:zoomImages></article>

<style>
  .page { padding: 28px 32px; background: var(--panel); border: var(--border-w) solid var(--border); border-radius: var(--radius); max-width: 820px; overflow-wrap: anywhere; overflow: auto; }
  .page :global(img) { max-width: 100%; height: auto; }
  .page :global(p:empty) { min-height: 1em; }
  .page :global(table) { border-collapse: collapse; }
  .page :global(td), .page :global(th) { border: 1px solid var(--border); padding: 4px 8px; vertical-align: top; text-align: start; }
  .page :global(th) { background: var(--fill); }
  .page :global(td p), .page :global(th p) { margin: 0; }
  .page :global(p), .page :global(li) { white-space: pre-wrap; }
  .hide { display: none; }
  .msg { display: flex; gap: 10px; align-items: flex-start; }
  .note { display: flex; align-items: center; gap: 6px; margin: 0 0 10px; }
  .sm { font-size: var(--fs-xs); }
  .pad { display: grid; gap: 12px; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .skel.h { height: 26px; width: 45%; }
  .w70 { width: 70%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel { animation: none; } }
  :global([data-motion="reduce"]) .skel { animation: none; }
</style>
