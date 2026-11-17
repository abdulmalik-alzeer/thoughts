<script lang="ts">
  // A Word document from the library, shown as a readable page (see lib/content/docxview.ts for
  // what is and is not shown).
  import { onDestroy, untrack } from 'svelte';
  import Icon from '../../lib/Icon.svelte';
  import { apiBlob } from '../../lib/api';
  import { t } from '../../lib/i18n';
  import { errDetail } from '../../lib/errors';
  import { docxToDom } from '../../lib/content/docxview';
  import { docDir } from '../../lib/content/docx';
  import { zoomImages } from '../../lib/zoomImages';

  let { path, name }: { path: string; name: string } = $props();

  let host = $state<HTMLElement>();
  let phase = $state<'loading' | 'ready' | 'empty' | 'error'>('loading');
  let skipped = $state(false);
  let detail = $state('');
  let urls: string[] = [];
  const drop = () => { for (const u of urls.splice(0)) URL.revokeObjectURL(u); };

  async function load(p: string) {
    phase = 'loading'; skipped = false; detail = '';
    drop();
    try {
      const b = await apiBlob(`/library/raw?path=${encodeURIComponent(p)}`);
      const view = await docxToDom(new Uint8Array(await b.arrayBuffer()));
      if (p !== path) { for (const u of view.urls) URL.revokeObjectURL(u); return; }
      urls = view.urls;
      skipped = view.skipped;
      const empty = !(view.node.textContent ?? '').trim() && !view.node.querySelector('img');
      phase = empty ? 'empty' : 'ready';
      await Promise.resolve();
      // The page runs the way most of the document reads (tables and lists of an Arabic document start on the right).
      if (host) { host.dir = docDir(view.node.textContent ?? ''); host.replaceChildren(view.node); }
    } catch (e) {
      if (p !== path) return;
      phase = 'error';
      detail = errDetail(e) || '';
    }
  }
  $effect(() => { const p = path; untrack(() => load(p)); });
  onDestroy(drop);
</script>

{#if phase === 'loading'}
  <div class="pad" role="status" aria-busy="true"><span class="sr-only">{t('docsb.docxLoading')}</span><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div></div>
{:else if phase === 'error'}
  <div class="msg" role="alert"><Icon name="warning" size={18} /><div><p>{t('docsb.docxFailed')}</p><p class="muted sm">{t('docsb.docxFailedHint')}</p>
    {#if detail && !/not a zip|broken zip|unreadable|not a word/i.test(detail)}<p class="muted sm" dir="auto">{detail}</p>{/if}</div></div>
{:else if phase === 'empty'}
  <p class="muted msg">{t('docsb.docxEmpty')}</p>
{/if}
{#if skipped && phase === 'ready'}
  <p class="muted sm note"><Icon name="info" size={14} /> {t('docsb.docxSkipped')}</p>
{/if}
<article class="md page" class:hide={phase !== 'ready'} bind:this={host} aria-label={t('docsb.docxLabel', { name })} use:zoomImages></article>

<style>
  .page { margin-top: 16px; padding: 28px 32px; background: var(--panel); border: var(--border-w) solid var(--border); border-radius: var(--radius); max-width: 820px; overflow-wrap: anywhere; }
  .page :global(img) { max-width: 100%; height: auto; }
  .page :global(p:empty) { min-height: 1em; }
  .page :global(table) { border-collapse: collapse; }
  .page :global(td), .page :global(th) { border: 1px solid var(--border); padding: 4px 8px; vertical-align: top; text-align: start; }
  .page :global(td p), .page :global(th p) { margin: 0; }
  .page :global(p), .page :global(li) { white-space: pre-wrap; }
  .hide { display: none; }
  .msg { display: flex; gap: 10px; align-items: flex-start; margin-top: 16px; }
  .msg p { margin: 0 0 4px; }
  .note { display: flex; align-items: center; gap: 6px; margin: 14px 0 0; }
  .sm { font-size: var(--fs-xs); }
  .pad { padding-top: 16px; display: grid; gap: 12px; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .skel.h { height: 26px; width: 45%; }
  .w70 { width: 70%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel { animation: none; } }
</style>
