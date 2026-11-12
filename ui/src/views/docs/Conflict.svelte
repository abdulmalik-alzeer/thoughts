<script lang="ts">
  // The open file changed somewhere else while it was being edited: both versions side by side,
  // differing lines highlighted, and a safe choice first (keep both: save mine as a copy).
  import Icon from '../../lib/Icon.svelte';
  import { trapFocus } from '../../lib/focus';
  import { t } from '../../lib/i18n';

  type ConflictChoice = 'copy' | 'theirs' | 'mine' | null;

  let { name, theirs, mine, gone = false, done }: {
    name: string;
    /** What is saved on disk now ('' when the file is gone). */
    theirs: string;
    mine: string;
    /** The file was moved or deleted. */
    gone?: boolean;
    done: (c: ConflictChoice) => void;
  } = $props();

  type Line = { text: string; diff: boolean };
  /** Lines of `a` marked when `b` has no such line, matched in order (a longest common subsequence). */
  function mark(a: string[], b: string[]): Line[] {
    // Large files: a plain "is this line anywhere in the other" test keeps it instant.
    if (a.length * b.length > 4_000_000) { const other = new Set(b); return a.map((text) => ({ text, diff: !other.has(text) })); }
    const n = a.length, m = b.length;
    const L: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) L[i][j] = a[i] === b[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
    const out: Line[] = [];
    let i = 0, j = 0;
    while (i < n) {
      if (j < m && a[i] === b[j]) { out.push({ text: a[i], diff: false }); i++; j++; }
      else if (j < m && L[i][j + 1] >= L[i + 1][j]) j++;
      else { out.push({ text: a[i], diff: true }); i++; }
    }
    return out;
  }
  const tl = $derived(theirs.split('\n'));
  const ml = $derived(mine.split('\n'));
  const left = $derived(gone ? [] : mark(tl, ml));
  const right = $derived(mark(ml, gone ? [] : tl));
</script>

<div class="modal-bg no-print" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) done(null); }}>
  <div class="modal cf" role="dialog" aria-modal="true" aria-labelledby="cf-title" aria-describedby="cf-body" tabindex="-1" use:trapFocus={{ onEscape: () => done(null) }}>
    <div class="row head">
      <h2 id="cf-title" class="grow"><span dir="auto">{t('docsa.conflictTitle', { name })}</span></h2>
      <button class="btn ghost x" onclick={() => done(null)} aria-label={t('close')} title={t('close')}><Icon name="close" size={16} /></button>
    </div>
    <p id="cf-body" class="muted">{t(gone ? 'docsa.goneBanner' : 'docsa.conflictBody')}</p>
    <div class="cols">
      <section aria-labelledby="cf-theirs">
        <h3 id="cf-theirs">{t('docsa.theirs')}</h3>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div class="pane scroll" tabindex="0" role="region" aria-labelledby="cf-theirs">
          {#if gone}<p class="muted sm">{t('docsa.goneTitle')}</p>
          {:else}{#each left as l, i (i)}<div class="ln" class:d={l.diff}>{l.text || ' '}</div>{/each}{/if}
        </div>
      </section>
      <section aria-labelledby="cf-mine">
        <h3 id="cf-mine">{t('docsa.mine')}</h3>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div class="pane scroll" tabindex="0" role="region" aria-labelledby="cf-mine">
          {#each right as l, i (i)}<div class="ln" class:d={l.diff}>{l.text || ' '}</div>{/each}
        </div>
      </section>
    </div>
    <div class="row foot">
      <button class="btn" onclick={() => done(null)}>{t('docsa.keepEditing')}</button>
      <span class="grow"></span>
      {#if !gone}<button class="btn" onclick={() => done('theirs')}>{t('docsa.useTheirs')}</button>{/if}
      <button class="btn danger" onclick={() => done('mine')}>{t('docsa.useMine')}</button>
      <button class="btn primary" data-autofocus onclick={() => done('copy')}><Icon name="copy" size={14} /> {t('docsa.saveCopy')}</button>
    </div>
  </div>
</div>

<style>
  .cf { width: min(1080px, calc(100% - 32px)); display: flex; flex-direction: column; gap: 10px; overflow: hidden; }
  .head h2 { margin: 0; font-size: 17px; overflow-wrap: anywhere; }
  .x { padding: 6px 8px; }
  .cf > p { margin: 0; }
  .cols { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 12px; min-height: 0; height: 56vh; }
  .cols section { display: flex; flex-direction: column; gap: 6px; min-height: 0; }
  h3 { margin: 0; font-size: var(--fs-sm); font-weight: 600; }
  .pane { flex: 1; min-height: 0; border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 8px 10px; background: var(--bg);
    font-family: var(--mono); font-size: 12.5px; line-height: 1.5; }
  /* Each line finds its own direction, so Arabic lines sit right-to-left among English front matter. */
  .ln { white-space: pre-wrap; overflow-wrap: anywhere; unicode-bidi: plaintext; text-align: start; border-radius: 3px; padding-inline: 4px; }
  .ln.d { background: color-mix(in srgb, var(--accent-2) 20%, transparent); border-inline-start: 3px solid var(--accent-2); }
  .foot { gap: 8px; flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
  @media (max-width: 760px) { .cols { grid-template-columns: minmax(0, 1fr); grid-template-rows: 1fr 1fr; } }
</style>
