<script lang="ts">
  // "Linked from": the notes whose links lead to the open one, under the note.
  import { api } from '../../lib/api';
  import { t, tn, fmtNum } from '../../lib/i18n';
  import { untrack } from 'svelte';
  import { prettyName } from '../../lib/content/util';

  let { path, version = 0, open }: { path: string; version?: number; open: (p: string) => void } = $props();

  let items = $state<any[] | null>(null);
  let failed = $state(false);
  let showAll = $state(false);
  const FIRST = 8;

  async function load(p: string) {
    failed = false;
    try {
      const r = await api(`/library/backlinks?path=${encodeURIComponent(p)}`);
      if (p === path) items = Array.isArray(r) ? r : [];
    } catch { if (p === path) { items = null; failed = true; } }
  }
  // A different note starts from scratch; a library change refreshes quietly.
  let shownFor = '';
  $effect(() => {
    const p = path;
    void version;
    untrack(() => {
      if (p !== shownFor) { shownFor = p; items = null; showAll = false; }
      load(p);
    });
  });

  const shown = $derived(items ? (showAll ? items : items.slice(0, FIRST)) : []);
  /** The line around the link as a reader sees it: `[words](target)` → words. */
  const plain = (s: string) => String(s).replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1').replace(/^[-*+>#\s]+/, '');
  const titleOf = (it: any) => String(it.title || '') || prettyName(String(it.path));
</script>

<section class="bl no-print" aria-labelledby="bl-h">
  <h2 id="bl-h">{t('docsb.linkedFrom')}{#if items?.length}<span class="n muted">{fmtNum(items.length)}</span>{/if}</h2>
  {#if failed}
    <p class="muted sm">{t('docsb.linkedFailed')} <button class="lnk" onclick={() => load(path)}>{t('docs3.retry')}</button></p>
  {:else if items === null}
    <p class="muted sm" role="status">{t('docsb.linkedLoading')}</p>
  {:else if !items.length}
    <p class="muted sm">{t('docsb.linkedNone')}</p>
  {:else}
    <ul>
      {#each shown as it (it.path)}
        <li>
          <button class="it" onclick={() => open(it.path)} title={it.path}>
            <span class="ti" dir="auto">{titleOf(it)}</span>
            {#if it.count > 1}<span class="muted sm">{tn('docsb.times', it.count)}</span>{/if}
            <bdi class="p">{it.path}</bdi>
            {#if it.text}<span class="tx muted" dir="auto">{plain(it.text)}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
    {#if items.length > FIRST && !showAll}
      <button class="lnk" onclick={() => (showAll = true)}>{tn('docsb.showAll', items.length)}</button>
    {/if}
  {/if}
</section>

<style>
  .bl { margin-top: 32px; padding-top: 14px; border-top: var(--border-w) solid var(--border); max-width: 760px; }
  h2 { display: flex; align-items: center; gap: 6px; font-size: var(--fs-sm); font-weight: 600; margin: 0 0 8px; color: var(--muted); letter-spacing: 0; }
  .n { font-weight: 400; margin-inline-start: 4px; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
  .it { font: inherit; color: inherit; background: none; border: 0; cursor: pointer; text-align: start; width: 100%; padding: 6px 8px; border-radius: 6px;
    display: grid; grid-template-columns: auto auto 1fr; align-items: baseline; gap: 2px 8px; }
  .it:hover, .it:focus-visible { background: var(--panel-2); }
  .ti { font-weight: 500; }
  .p { font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; justify-self: end; max-width: 100%; }
  .tx { grid-column: 1 / -1; font-size: var(--fs-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; unicode-bidi: plaintext; }
  .sm { font-size: var(--fs-xs); }
  p { margin: 0; }
  .lnk { font: inherit; font-size: var(--fs-xs); color: var(--accent); background: none; border: 0; padding: 4px 8px; cursor: pointer; text-decoration: underline; }
</style>
