<script lang="ts">
  // One preview for any file, wherever the app shows one (Docs, chat attachments…). It works out the
  // kind from the name (kinds.ts), checks the size, loads the bytes through `load`, and hands them to
  // the renderer for that kind (renderers.ts, each loaded on first use). Loading, failure, "too big"
  // and "can't be shown here" are handled here, the same for every kind; the last two offer
  // "Show in Finder" when the caller passes `onReveal`.
  //
  // A preview never runs anything from the file and never loads anything from the internet: see
  // the renderers for how each kind keeps to that.
  import { untrack, type Component } from 'svelte';
  import Icon from '../Icon.svelte';
  import { t } from '../i18n';
  import { ApiError } from '../api';
  import { errDetail } from '../errors';
  import { fmtSize, notify } from '../content/util';
  import { revealLabelKey } from '../openInApp';
  import { codeLang, LIMITS, mimeFor, previewKind, tooLarge } from './kinds';
  import { renderers, type RendererProps } from './renderers';

  let {
    name,
    load,
    size,
    onReveal,
    zoom,
    fill = false,
    label,
  }: {
    /** The file's name or library path (its kind comes from it). */
    name: string;
    /** Fetch the whole file. Called again when `name` or `load` change, and on Retry. */
    load: () => Promise<Uint8Array | Blob>;
    /** The size in bytes when already known: a file over its kind's limit is then never fetched. */
    size?: number;
    /** "Show in Finder" / "Show in folder", offered when the file is too big or can't be shown. */
    onReveal?: () => unknown;
    /** Open a picture in the app's zoom-and-pan viewer (pictures become clickable when set). */
    zoom?: (src: string, alt: string) => void;
    /** Fill the parent's height (a viewer dialog) instead of a page-sized frame. */
    fill?: boolean;
    /** The region's accessible name (default: "Preview of {name}"). */
    label?: string;
  } = $props();

  type Phase = 'loading' | 'ready' | 'error' | 'large' | 'unsupported';
  let phase = $state<Phase>('loading');
  let data = $state<Blob | null>(null);
  let Comp = $state<Component<RendererProps> | null>(null);
  let detail = $state('');
  let bigSize = $state<number | undefined>(undefined);
  let seq = 0;

  const base = $derived(name.split(/[\\/]/).pop() || name);
  const kind = $derived(previewKind(name, size));

  async function run() {
    const n = ++seq;
    const k = previewKind(name, size);
    data = null; Comp = null; detail = ''; bigSize = size;
    const loader = renderers[k];
    if (size !== undefined && k === 'none' && previewKind(name) !== 'none') { phase = 'large'; return; }
    if (!loader) { phase = 'unsupported'; return; }
    if (tooLarge(k, size)) { phase = 'large'; return; }
    phase = 'loading';
    try {
      const [raw, mod] = await Promise.all([load(), loader()]);
      if (n !== seq) return;
      const blob = raw instanceof Blob ? raw : new Blob([raw as BlobPart]);
      if (blob.size > LIMITS[k]) { bigSize = blob.size; phase = 'large'; return; }
      // Typed by us from the name, never by what the file claims to be.
      data = blob.type === mimeFor(name, k) ? blob : new Blob([blob], { type: mimeFor(name, k) });
      Comp = mod.default;
      phase = 'ready';
    } catch (e) {
      if (n !== seq) return;
      if (e instanceof ApiError && e.status === 413) { phase = 'large'; return; }
      detail = errDetail(e);
      phase = 'error';
    }
  }
  $effect(() => { void name; void load; void size; untrack(run); });

  function unsupported(why?: string) {
    if (phase !== 'ready') return;
    detail = why ?? '';
    phase = 'unsupported';
    data = null; Comp = null;
  }

  async function reveal() {
    try { await onReveal?.(); }
    catch (e) { notify(t('docs.revealFailed'), { kind: 'error', detail: errDetail(e) }); }
  }
</script>

<section class="fp" class:fill aria-label={label ?? t('libf-preview-core.region', { name: base })} aria-busy={phase === 'loading'} data-kind={kind}>
  {#if phase === 'loading'}
    <div class="pad" role="status"><span class="sr-only">{t('libf-preview-core.loading', { name: base })}</span><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div></div>
  {:else if phase === 'error'}
    <div class="state" role="alert">
      <Icon name="warning" size={22} />
      <p>{t('libf-preview-core.failed')}</p>
      {#if detail}<p class="muted sm" dir="auto">{detail}</p>{/if}
      <button type="button" class="btn" onclick={run}><Icon name="retry" size={14} /> {t('libf-preview-core.retry')}</button>
    </div>
  {:else if phase === 'large' || phase === 'unsupported'}
    <div class="state" role="note">
      <Icon name={phase === 'large' ? 'info' : 'eye'} size={22} />
      <p>{phase === 'large'
        ? (bigSize !== undefined ? t('libf-preview-core.tooLarge', { size: fmtSize(bigSize) }) : t('libf-preview-core.tooLargePlain'))
        : t('libf-preview-core.cannot')}</p>
      {#if phase === 'unsupported' && detail}<p class="muted sm" dir="auto">{detail}</p>{/if}
      {#if onReveal}<button type="button" class="btn" onclick={reveal}><Icon name="folder" size={14} /> {t(revealLabelKey())}</button>{/if}
    </div>
  {:else if Comp && data}
    <Comp {data} name={base} {kind} lang={codeLang(name)} {fill} {unsupported} {zoom} />
  {/if}
</section>

<style>
  .fp { margin-top: 14px; min-width: 0; }
  .fp.fill { margin-top: 0; height: 100%; display: flex; flex-direction: column; min-height: 0; }
  .fp.fill > :global(*) { flex: 1; min-height: 0; }
  .state { display: grid; justify-items: center; gap: 8px; text-align: center; padding: 28px 16px; border: var(--border-w) dashed var(--border); border-radius: var(--radius); color: var(--muted); }
  .fp.fill .state { border: 0; align-content: center; }
  .state p { margin: 0; color: var(--fg); max-width: 52ch; }
  .state p.muted { color: var(--muted); }
  .state .btn { margin-top: 4px; }
  .sm { font-size: var(--fs-xs); }
  .pad { padding-top: 4px; display: grid; gap: 12px; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .skel.h { height: 26px; width: 45%; }
  .w70 { width: 70%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel { animation: none; } }
  :global([data-motion="reduce"]) .skel { animation: none; }
</style>
