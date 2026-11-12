<script lang="ts" module>
  /** One document of a comparison, as the background service sends it (`diffs.rs`; Wrap and
   *  Reorganize send the same shape). `kind` of a line: `+` added, `-` removed, ` ` unchanged. */
  export type ChangeFile = {
    path: string;
    from?: string | null;
    change: 'added' | 'edited' | 'moved' | 'deleted' | string;
    detail?: boolean;
    binary?: boolean;
    added?: number | null;
    removed?: number | null;
    more?: number;
    hunks?: { line: number; lines: [string, string][] }[];
  };
</script>

<script lang="ts">
  // What changed, document by document, in plain form: lines that were added and removed, with a
  // few unchanged lines around them. Shared by History, Versions, a sync clash and a history repair
  // (and meant for Wrap and Reorganize too). Each line keeps its own direction, so Arabic reads
  // right to left and English left to right in either language of the app.
  import Icon from './Icon.svelte';
  import { t, tn, fmtNum } from './i18n';

  let {
    files,
    labels = { added: t('histc.cv.added'), removed: t('histc.cv.removed') },
    single = false,
    openCount = 6,
    onopen,
  }: {
    files: ChangeFile[];
    /** What a `+` and a `-` line mean in this comparison ("Only in this version"…). */
    labels?: { added: string; removed: string };
    /** One document whose name is already shown around this view: no heading per document. */
    single?: boolean;
    /** How many documents start unfolded. */
    openCount?: number;
    /** Open a document (its name becomes a button). */
    onopen?: (path: string) => void;
  } = $props();

  const changeLabel = (c: string) => t('histc.cv.change.' + (['added', 'edited', 'moved', 'deleted'].includes(c) ? c : 'edited'));
  const cls = (k: string) => (k === '+' ? 'add' : k === '-' ? 'del' : 'ctx');
  const counts = (f: ChangeFile) => {
    const parts: string[] = [];
    if (f.added) parts.push(tn('histc.cv.nAdded', f.added));
    if (f.removed) parts.push(tn('histc.cv.nRemoved', f.removed));
    return parts.join(t('histc.cv.and'));
  };
  const anyLines = $derived(files.some((f) => f.hunks?.length));
</script>

{#snippet body(f: ChangeFile)}
  {#if f.binary}
    <p class="muted sm note"><Icon name="docs" size={14} /> {t('histc.cv.binary')}</p>
  {:else if f.detail === false}
    <p class="muted sm note">{t('histc.cv.noDetail')}</p>
  {:else if !f.hunks?.length}
    <p class="muted sm note">{t(f.change === 'moved' ? 'histc.cv.movedOnly' : 'histc.cv.same')}</p>
  {:else}
    <div class="hunks">
      {#each f.hunks as h, hi (hi)}
        {#if hi > 0 || h.line > 1}<div class="at" aria-hidden="true">{t('histc.cv.line', { n: fmtNum(h.line) })}</div>{/if}
        {#each h.lines as [k, text], li (li)}
          <div class="ln {cls(k)}">
            <span class="sign" aria-hidden="true">{k === '+' ? '+' : k === '-' ? '−' : ''}</span>
            {#if k !== ' '}<span class="sr-only">{k === '+' ? labels.added : labels.removed}: </span>{/if}
            <span class="txt" dir="auto">{text || ' '}</span>
          </div>
        {/each}
      {/each}
    </div>
    {#if f.more}<p class="muted sm note">{tn('histc.cv.more', f.more)}</p>{/if}
  {/if}
{/snippet}

<div class="cv">
  {#if anyLines}
    <p class="legend muted sm" aria-hidden="true">
      <span class="key"><span class="sw add">+</span> {labels.added}</span>
      <span class="key"><span class="sw del">−</span> {labels.removed}</span>
    </p>
  {/if}
  {#if !files.length}
    <p class="muted sm note">{t('histc.cv.none')}</p>
  {:else if single}
    {@render body(files[0])}
  {:else}
    {#each files as f, i (f.path + i)}
      <details class="doc" open={i < openCount}>
        <summary>
          <span class="pill {f.change}">{changeLabel(f.change)}</span>
          {#if onopen && f.change !== 'deleted'}
            <button type="button" class="name link" onclick={(e) => { e.preventDefault(); onopen?.(f.path); }} title={t('histc.cv.open')}><bdi>{f.path}</bdi></button>
          {:else}
            <span class="name"><bdi>{f.path}</bdi></span>
          {/if}
          {#if f.from}<span class="muted sm from">{t('histc.cv.from')} <bdi>{f.from}</bdi></span>{/if}
          {#if counts(f)}<span class="muted sm count">{counts(f)}</span>{/if}
        </summary>
        {@render body(f)}
      </details>
    {/each}
  {/if}
</div>

<style>
  .cv { display: grid; gap: 8px; min-width: 0; }
  .legend { display: flex; flex-wrap: wrap; gap: 4px 14px; margin: 0; }
  .key { display: inline-flex; align-items: center; gap: 6px; }
  .sw { display: inline-grid; place-items: center; width: 18px; height: 18px; border-radius: 4px; font-weight: 600; font-size: 12px; }
  .sw.add, .ln.add { background: color-mix(in srgb, var(--ok) 16%, transparent); }
  .sw.del, .ln.del { background: color-mix(in srgb, var(--danger) 13%, transparent); }
  .sw.add, .ln.add .sign { color: var(--ok); }
  .sw.del, .ln.del .sign { color: var(--danger); }
  .doc { border: var(--border-w) solid var(--border); border-radius: var(--radius); background: var(--bg); min-width: 0; }
  .doc > summary { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; padding: 8px 10px; cursor: pointer; list-style-position: inside; }
  .doc[open] > summary { border-bottom: 1px solid var(--hairline); }
  .doc > :not(summary) { margin: 8px 10px; }
  .name { font-weight: 500; overflow-wrap: anywhere; min-width: 0; }
  .name.link { font: inherit; font-weight: 500; color: inherit; background: none; border: 0; padding: 0; cursor: pointer; text-align: start; text-decoration: underline; text-decoration-color: var(--hairline); text-underline-offset: 3px; }
  .name.link:hover { text-decoration-color: currentColor; }
  .from { overflow-wrap: anywhere; }
  .count { margin-inline-start: auto; }
  .pill.added { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--fg); }
  .pill.deleted { background: color-mix(in srgb, var(--danger) 13%, transparent); color: var(--fg); }
  .hunks { font-size: var(--fs-sm); line-height: 1.5; border-radius: 6px; overflow: hidden; }
  .at { font-size: var(--fs-xs); color: var(--muted); padding: 6px 4px 2px; }
  .ln { display: flex; gap: 6px; padding: 1px 6px; min-width: 0; }
  .sign { flex: none; width: 1ch; font-weight: 600; text-align: center; user-select: none; }
  .txt { flex: 1; min-width: 0; white-space: pre-wrap; overflow-wrap: anywhere; unicode-bidi: plaintext; text-align: start; }
  .ln.ctx .txt { color: var(--muted); }
  .note { margin: 4px 0; display: flex; align-items: center; gap: 6px; }
  .sm { font-size: var(--fs-xs); }
  /* High contrast: a change is also outlined, not told by its tint alone. */
  :global([data-theme="contrast"]) .ln.add { outline: 1px solid var(--ok); }
  :global([data-theme="contrast"]) .ln.del { outline: 1px solid var(--danger); }
</style>
