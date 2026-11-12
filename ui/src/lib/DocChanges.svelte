<script lang="ts">
  // What a Wrap or a Reorganize changes, one document at a time: what happened to it (new, changed,
  // moved, removed) and the lines that changed, with a few lines around them. Never raw diff text.
  // The service sends this shape (`changes::between`).
  import Icon from './Icon.svelte';
  import { go } from './store.svelte';
  import { fmtNum, t, tn } from './i18n';

  type Hunk = { line: number; lines: [string, string][] };
  export type Doc = { path: string; from?: string | null; change: string; detail: boolean; binary: boolean; tooLarge?: boolean; added?: number | null; removed?: number | null; more: number; hunks: Hunk[] };

  let { files, canOpen = false, labels, single = false }: {
    files: Doc[];
    canOpen?: boolean;
    /** When comparing two versions (not showing a change): what a `+` and a `-` line mean here
     *  ("Only in this version"…). Shown as a key above the documents and read out for each line. */
    labels?: { added: string; removed: string };
    /** One document whose name is already shown around this view: its lines only. */
    single?: boolean;
  } = $props();

  // A few documents are all shown open; with many, the first ones are, and the rest are a tap away.
  let open = $state<Record<string, boolean>>({});
  const isOpen = (d: Doc, i: number) => open[d.path] ?? (files.length <= 6 || i < 3);
  const kind = (c: string) => (['added', 'deleted', 'moved'].includes(c) ? c : 'edited');
  const hasBody = (d: Doc) => !!d.from || d.binary || !d.detail || d.hunks.length > 0;
  function counts(d: Doc): string {
    const a = d.added ?? 0, r = d.removed ?? 0;
    if (!a && !r) return '';
    if (!r) return tn('wrapb.doc.linesAdded', a);
    if (!a) return tn('wrapb.doc.linesRemoved', r);
    return t('wrapb.doc.linesBoth', { added: tn('wrapb.doc.linesAdded', a), removed: tn('wrapb.doc.linesRemoved', r) });
  }
</script>

{#if labels && files.some((d) => d.hunks?.length)}
  <p class="key muted" aria-hidden="true">
    <span><span class="sw add">+</span> {labels.added}</span>
    <span><span class="sw rem">−</span> {labels.removed}</span>
  </p>
{/if}
{#snippet lines(d: Doc)}
  {#each d.hunks as h, hi}
    {#if hi > 0 || h.line > 1}<p class="gap muted">{t('wrapb.doc.at', { n: fmtNum(h.line) })}</p>{/if}
    <div class="lines">
      {#each h.lines as [k, s]}
        <div class="ln" class:add={k === '+'} class:rem={k === '-'}>
          <span class="sign" aria-hidden="true">{k === '+' ? '+' : k === '-' ? '−' : ''}</span>
          {#if k !== ' '}<span class="sr">{labels ? (k === '+' ? labels.added : labels.removed) + ': ' : t(k === '+' ? 'wrapb.doc.added.line' : 'wrapb.doc.removed.line')}</span>{/if}
          <span class="txt" dir="auto">{s || ' '}</span>
        </div>
      {/each}
    </div>
  {/each}
  {#if d.more}<p class="note muted">{tn('wrapb.doc.more', d.more)}</p>{/if}
{/snippet}
{#if single && files.length === 1}
  {@const d = files[0]}
  <div class="one">
    {#if d.binary}<p class="note muted">{t('wrapb.doc.binary')}</p>
    {:else if d.tooLarge}<p class="note muted">{t('revcore.doc.tooLarge')}</p>
    {:else if !d.detail}<p class="note muted">{t('wrapb.doc.noDetail')}</p>
    {:else if !d.hunks.length}<p class="note muted">{t('histc.cv.same')}</p>
    {:else}{@render lines(d)}{/if}
  </div>
{:else}
<ul class="docs" aria-label={t('wrapb.doc.list')}>
  {#each files as d, i (d.path)}
    {@const shown = isOpen(d, i) && hasBody(d)}
    <li class="doc">
      <div class="head row">
        <button class="toggle grow" aria-expanded={shown} disabled={!hasBody(d)} onclick={() => (open[d.path] = !isOpen(d, i))}>
          {#if hasBody(d)}<Icon name={shown ? 'down' : 'right'} size={14} class={shown ? '' : 'flip-rtl'} />{/if}
          <span class="pill k-{kind(d.change)}">{t('wrapb.doc.' + kind(d.change))}</span>
          <bdi class="path ell" dir="ltr">{d.path}</bdi>
          {#if counts(d)}<span class="n muted">{counts(d)}</span>{/if}
        </button>
        {#if canOpen && d.change !== 'deleted'}
          <button class="btn ghost sm" onclick={() => go({ kind: 'docs', path: d.path })} aria-label={t('wrapa.openDoc', { path: d.path })}>{t('wrapb.doc.open')}</button>
        {/if}
      </div>
      {#if shown}
        <div class="body">
          {#if d.from}<p class="note muted">{t('wrapb.doc.movedFrom')} <bdi dir="ltr">{d.from}</bdi></p>{/if}
          {#if d.binary}<p class="note muted">{t('wrapb.doc.binary')}</p>
          {:else if d.tooLarge}<p class="note muted">{t('revcore.doc.tooLarge')}</p>
          {:else if !d.detail}<p class="note muted">{t('wrapb.doc.noDetail')}</p>
          {:else}
            {@render lines(d)}
          {/if}
        </div>
      {/if}
    </li>
  {/each}
</ul>
{/if}

<style>
  .docs { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .doc { border: var(--border-w, 1px) solid var(--border); border-radius: var(--radius); background: var(--panel); min-width: 0; }
  .head { padding: 4px 6px; }
  .toggle { display: flex; align-items: center; gap: 8px; min-width: 0; font: inherit; font-size: var(--fs-sm); color: inherit; background: none; border: 0; padding: 4px; cursor: pointer; text-align: start; border-radius: 6px; }
  .toggle:disabled { cursor: default; opacity: 1; }
  .toggle:hover:not(:disabled) { background: var(--fill); }
  .toggle :global(.icon) { flex: none; }
  .path { min-width: 0; }
  .n { flex: none; font-size: var(--fs-xs); margin-inline-start: auto; padding-inline-start: 8px; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pill { flex: none; }
  .k-added { background: color-mix(in srgb, var(--ok) 18%, transparent); color: var(--fg); }
  .k-deleted { background: color-mix(in srgb, var(--danger) 16%, transparent); color: var(--fg); }
  .k-moved { background: var(--tint); color: var(--on-tint); }
  .sm { font-size: var(--fs-xs); padding: 3px 8px; }
  .body { padding: 0 10px 10px; max-height: 50vh; overflow: auto; }
  .note, .gap { margin: 4px 0; font-size: var(--fs-xs); }
  .lines { border-radius: 6px; overflow: hidden; border: var(--border-w, 1px) solid var(--border); }
  .ln { display: flex; gap: 6px; font-size: var(--fs-sm); line-height: 1.5; padding: 0 8px; }
  .ln.add { background: color-mix(in srgb, var(--ok) 14%, transparent); }
  .ln.rem { background: color-mix(in srgb, var(--danger) 12%, transparent); }
  .sign { flex: none; width: 1ch; font-family: var(--mono); color: var(--muted); }
  .ln.add .sign { color: var(--ok); } .ln.rem .sign { color: var(--danger); }
  .txt { white-space: pre-wrap; overflow-wrap: anywhere; min-width: 0; }
  .key { display: flex; flex-wrap: wrap; gap: 4px 14px; margin: 0 0 6px; font-size: var(--fs-xs); }
  .key > span { display: inline-flex; align-items: center; gap: 6px; }
  .sw { display: inline-grid; place-items: center; width: 18px; height: 18px; border-radius: 4px; font-weight: 600; }
  .sw.add { background: color-mix(in srgb, var(--ok) 14%, transparent); color: var(--ok); }
  .sw.rem { background: color-mix(in srgb, var(--danger) 12%, transparent); color: var(--danger); }
  .one { min-width: 0; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; padding: 0; border: 0; }
</style>
