<script lang="ts">
  // Plain text and source code (also YAML, XML and the source of JSON and HTML), highlighted by the
  // small in-house highlighter (../highlight.ts). Everything goes into the page as text. A long file
  // shows its first lines with a note, so a big log never stalls the window. Code runs left to right
  // whatever the app's language; plain text follows its own direction.
  import { untrack, type Snippet } from 'svelte';
  import Icon from '../../Icon.svelte';
  import { t, tn, fmtNum } from '../../i18n';
  import { copyText, notify } from '../../content/util';
  import { highlight, knownLang, lines, type Tok } from '../highlight';
  import { cutText, decodeText } from '../text';
  import type { RendererProps } from '../renderers';

  let { data, name, kind, lang, fill, unsupported, text: given, tools }: RendererProps & {
    /** The text to show instead of decoding `data` (the pretty-printed JSON). */
    text?: string;
    /** More buttons for the bar (HTML's "Show as page", JSON's views). */
    tools?: Snippet;
  } = $props();

  /** Highlighted: up to this many lines and characters (above it, plain text). */
  const HL_LINES = 5000, HL_CHARS = 512 * 1024;
  /** Plain text: up to this many lines and characters. */
  const MAX_LINES = 20000, MAX_CHARS = 2 * 1024 * 1024;

  let phase = $state<'loading' | 'ready'>('loading');
  let full = $state('');
  let plain = $state('');
  let rows = $state<Tok[][] | null>(null);
  let total = $state(0);
  let shown = $state(0);
  let cut = $state(false);
  let wrap = $state(untrack(() => kind === 'text')); // plain text wraps; code keeps its lines
  let seq = 0;

  const isCode = $derived(kind !== 'text' || lang === 'md');

  async function prepare(b: Blob, override: string | undefined) {
    const n = ++seq;
    phase = 'loading'; rows = null;
    let s = override;
    if (s === undefined) {
      const decoded = decodeText(new Uint8Array(await b.arrayBuffer()));
      if (n !== seq) return;
      if (decoded === null) { unsupported(t('libf-preview-core.notText')); return; }
      s = decoded;
    }
    // Let the loading state paint before the work below.
    await new Promise((r) => setTimeout(r, 0));
    if (n !== seq) return;
    full = s;
    const hl = knownLang(lang) && s.length <= HL_CHARS * 4;
    const c = hl ? cutText(s, HL_LINES, HL_CHARS) : cutText(s, MAX_LINES, MAX_CHARS);
    total = c.lines; shown = c.shownLines; cut = c.cut;
    if (hl) {
      const r = lines(highlight(c.text, lang));
      // A file's final line end does not start another (numbered) line.
      if (r.length > 1 && !r[r.length - 1].length && c.text.endsWith('\n')) r.pop();
      rows = r; plain = '';
    }
    else { rows = null; plain = c.text; }
    phase = 'ready';
  }
  $effect(() => { const b = data, g = given; untrack(() => prepare(b, g)); });

  async function copy() {
    const ok = await copyText(full);
    notify(t(ok ? 'md.copied' : 'md.copyFailed'), { kind: ok ? 'success' : 'error' });
  }
</script>

{#if phase === 'loading'}
  <div class="pad" role="status"><span class="sr-only">{t('libf-preview-core.loading', { name })}</span><div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div></div>
{:else}
  <div class="code" class:fill>
    <div class="bar" role="toolbar" aria-label={t('libf-preview-core.textTools')}>
      <span class="muted sm grow">{tn('libf-preview-core.lines', total)}</span>
      {@render tools?.()}
      <button type="button" class="tb" aria-pressed={wrap} onclick={() => (wrap = !wrap)}>{t('libf-preview-core.wrap')}</button>
      <button type="button" class="tb" onclick={copy}><Icon name="copy" size={13} /> {t('libf-preview-core.copyAll')}</button>
    </div>
    {#if cut}<p class="note muted sm" role="note"><Icon name="info" size={13} /> {t('libf-preview-core.cut', { n: fmtNum(shown), total: fmtNum(total) })}</p>{/if}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre class="src" class:wrap class:num={!!rows} dir={isCode ? 'ltr' : 'auto'} tabindex="0" aria-label={t('libf-preview-core.textOf', { name })}>{#if rows}{#each rows as row, i}<span class="ln"><span class="no" aria-hidden="true">{i + 1}</span><span class="tx">{#each row as tk}{#if tk.c}<span class="t-{tk.c}">{tk.t}</span>{:else}{tk.t}{/if}{/each}{'\n'}</span></span>{/each}{:else}{plain}{/if}</pre>
  </div>
{/if}

<style>
  .code { display: flex; flex-direction: column; border: var(--border-w) solid var(--border); border-radius: var(--radius); background: var(--code-bg); overflow: hidden; }
  .code.fill { height: 100%; border: 0; border-radius: 0; }
  .bar { display: flex; align-items: center; gap: 6px; padding: 6px 8px 6px 12px; border-bottom: var(--border-w) solid var(--border); background: var(--panel); flex-wrap: wrap; }
  .grow { flex: 1; min-width: 0; }
  .sm { font-size: var(--fs-xs); }
  .tb { font: inherit; font-size: var(--fs-xs); color: var(--fg); background: var(--fill); border: 0; border-radius: 7px; padding: 4px 9px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px; }
  .tb:hover { background: var(--fill-2); }
  .tb[aria-pressed="true"] { background: var(--tint); color: var(--on-tint); }
  .tb:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .note { display: flex; align-items: center; gap: 6px; margin: 0; padding: 6px 12px; border-bottom: var(--border-w) solid var(--border); }
  .src { margin: 0; padding: 10px 14px; font-family: var(--mono); font-size: 12.5px; line-height: 1.55; overflow: auto; max-height: 75vh; tab-size: 4; white-space: pre; color: var(--fg); text-align: start; }
  .fill .src { max-height: none; flex: 1; min-height: 0; }
  .src.wrap { white-space: pre-wrap; overflow-wrap: anywhere; }
  .src:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .src.num { padding-inline-start: 0; }
  .ln { display: flex; }
  .no { flex: none; width: 4.5ch; padding-inline-end: 1.5ch; text-align: end; color: var(--muted); opacity: .7; user-select: none; }
  .tx { flex: 1; min-width: 0; }
  .t-com { color: var(--muted); font-style: italic; }
  .t-str { color: var(--ok); }
  .t-num, .t-lit { color: var(--accent-2); }
  .t-kw, .t-tag { color: var(--accent); font-weight: 600; }
  .t-attr, .t-type { color: color-mix(in srgb, var(--accent-2) 70%, var(--fg)); }
  .t-key { color: color-mix(in srgb, var(--accent) 75%, var(--fg)); }
  .t-fn { color: var(--fg); font-weight: 600; }
  .t-meta { color: var(--muted); }
  .t-ins { color: var(--ok); }
  .t-del { color: var(--danger); }
  .pad { display: grid; gap: 10px; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .w70 { width: 70%; } .w50 { width: 50%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel { animation: none; } }
</style>
