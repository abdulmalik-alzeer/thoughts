<script lang="ts">
  // What is inside a ZIP: the names and sizes from its table of contents. Nothing is unpacked or
  // opened, so nothing inside can do anything.
  import { untrack } from 'svelte';
  import Icon from '../../Icon.svelte';
  import { t, tn, fmtNum } from '../../i18n';
  import { fmtSize } from '../../content/util';
  import { ZipReader } from '../../content/zip';
  import type { RendererProps } from '../renderers';

  let { data, name, fill, unsupported }: RendererProps = $props();

  /** Rows listed before "… n more". */
  const MAX_ROWS = 2000;

  type Row = { name: string; size: number };
  let rows = $state<Row[] | null>(null);
  let folders = $state(0);
  let total = $state(0);
  let seq = 0;

  async function read(b: Blob) {
    const n = ++seq;
    rows = null;
    try {
      const z = new ZipReader(new Uint8Array(await b.arrayBuffer()));
      if (n !== seq) return;
      const all = z.list();
      // Folders are named with a trailing slash; the files inside carry the folder in their names.
      // macOS's own bookkeeping (__MACOSX/, .DS_Store) is not the user's.
      const files = all.filter((e) => !e.name.endsWith('/') && !/^__MACOSX\/|(^|\/)\.DS_Store$/.test(e.name));
      folders = new Set(files.flatMap((e) => e.name.split('/').slice(0, -1).map((_, i, a) => a.slice(0, i + 1).join('/')))).size;
      total = files.reduce((s, e) => s + e.size, 0);
      rows = files.map((e) => ({ name: e.name, size: e.size })).sort((a, b) => a.name.localeCompare(b.name));
    } catch {
      if (n === seq) unsupported(t('libf-preview-core.zip.cannot'));
    }
  }
  $effect(() => { const b = data; untrack(() => read(b)); });
</script>

{#if rows === null}
  <div class="pad" role="status"><span class="sr-only">{t('libf-preview-core.loading', { name })}</span><div class="skel"></div><div class="skel"></div></div>
{:else}
  <div class="zip" class:fill>
    <p class="head muted sm">
      <Icon name="archive" size={14} />
      {tn('libf-preview-core.zip.files', rows.length)}{#if folders}{' · '}{tn('libf-preview-core.zip.folders', folders)}{/if}{' · '}{t('libf-preview-core.zip.unpacked', { size: fmtSize(total) })}
    </p>
    {#if rows.length === 0}
      <p class="muted empty">{t('libf-preview-core.zip.empty')}</p>
    {:else}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="scroll" tabindex="0" role="region" aria-label={t('libf-preview-core.zip.caption', { name })}>
        <table>
          <caption class="sr-only">{t('libf-preview-core.zip.caption', { name })}</caption>
          <thead><tr><th scope="col">{t('libf-preview-core.zip.name')}</th><th scope="col" class="num">{t('libf-preview-core.zip.size')}</th></tr></thead>
          <tbody>
            {#each rows.slice(0, MAX_ROWS) as r, i (i)}
              <tr><td><bdi dir="auto">{r.name}</bdi></td><td class="num">{fmtSize(r.size)}</td></tr>
            {/each}
          </tbody>
        </table>
        {#if rows.length > MAX_ROWS}<p class="muted sm more">{t('libf-preview-core.zip.more', { n: fmtNum(rows.length - MAX_ROWS) })}</p>{/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .zip { border: var(--border-w) solid var(--border); border-radius: var(--radius); background: var(--panel); overflow: hidden; display: flex; flex-direction: column; }
  .zip.fill { height: 100%; border: 0; border-radius: 0; }
  .head { display: flex; align-items: center; gap: 6px; margin: 0; padding: 8px 12px; border-bottom: var(--border-w) solid var(--border); flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
  .scroll { overflow: auto; max-height: 70vh; }
  .fill .scroll { max-height: none; flex: 1; min-height: 0; }
  table { width: 100%; border-collapse: collapse; font-size: var(--fs-sm, 13px); }
  th { position: sticky; top: 0; background: var(--panel); text-align: start; font-weight: 600; color: var(--muted); font-size: var(--fs-xs); padding: 6px 12px; border-bottom: var(--border-w) solid var(--border); }
  td { padding: 5px 12px; border-bottom: 1px solid var(--fill-2); overflow-wrap: anywhere; vertical-align: top; }
  .num { text-align: end; white-space: nowrap; width: 1%; font-variant-numeric: tabular-nums; }
  .empty, .more { padding: 12px; margin: 0; }
  .pad { display: grid; gap: 10px; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: var(--fill); }
</style>
