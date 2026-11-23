<script lang="ts">
  // The style gallery: every candidate as a live sample card, the app's examples and the
  // assistant's suggestions apart and labelled, one of them picked.
  import { t, tn } from '../../i18n';
  import Icon from '../../Icon.svelte';
  import type { StyleCandidate } from '../types';
  import StylePreview from './StylePreview.svelte';

  let {
    styles, picked = null, current = null, dropped = 0, changed = false, locked = false, busy = null, error = '', onpick, ontune,
  }: {
    styles: StyleCandidate[];
    picked?: string | null;
    /** Round 0.5.0: the candidate that is a built product's look today (shown first; suggested while nothing is picked). */
    current?: string | null;
    /** Suggestions of the assistant that were not valid and are not shown. */
    dropped?: number;
    /** The user changed the picked style: picking another replaces those changes, so ask first. */
    changed?: boolean;
    locked?: boolean;
    /** The id being applied right now. */
    busy?: string | null;
    error?: string;
    onpick: (id: string) => void;
    ontune: () => void;
  } = $props();

  const groups = $derived([
    { key: 'assistant', title: t('madoDesign.gallery.assistant'), list: styles.filter((s) => s.source === 'assistant') },
    { key: 'app', title: t('madoDesign.gallery.app'), list: styles.filter((s) => s.source === 'app') },
  ]);
  /** The card waiting for "replace my changes?". */
  let asking = $state<string | null>(null);

  function use(id: string) {
    if (changed && picked && asking !== id) { asking = id; return; }
    asking = null;
    onpick(id);
  }
</script>

<section class="gal" aria-labelledby="mdg-title">
  <header class="head">
    <h2 id="mdg-title">{t('madoDesign.gallery.title')}</h2>
    <p class="muted">{t('madoDesign.gallery.sub')}</p>
  </header>
  {#if error}<p class="err" role="alert">{error}</p>{/if}

  {#each groups as g (g.key)}
    <h3 class="eyebrow group">{g.title}</h3>
    {#if g.key === 'assistant' && dropped > 0}
      <p class="note" role="status"><Icon name="info" size={14} /> {tn('madoDesign.gallery.dropped', dropped)}</p>
    {/if}
    {#if g.list.length === 0}
      <p class="empty muted">{t('madoDesign.gallery.noAssistant')}</p>
    {:else}
      <ul class="grid">
        {#each g.list as s (s.id)}
          {@const on = s.id === picked}
          <li class="cardx" class:on class:suggested={!picked && s.id === current}>
            <div class="sample"><StylePreview tokens={s.tokens} /></div>
            <div class="meta">
              <div class="row">
                <strong class="grow name">{s.name}</strong>
                {#if s.id === current}<span class="pill now">{t('mado-v050.design.current')}</span>{/if}
                {#if on}<span class="pill accent"><Icon name="check" size={12} /> {t('madoDesign.gallery.picked')}</span>{/if}
              </div>
              {#if s.description}<p class="desc muted">{s.description}</p>{/if}
              <div class="swatches" aria-hidden="true">
                {#each ['bg', 'surface', 'primary', 'accent', 'text'] as r (r)}
                  <span class="sw" style="background:{/^#[0-9a-f]{6}$/i.test((s.tokens.color as any)[r]) ? (s.tokens.color as any)[r] : 'transparent'}"></span>
                {/each}
              </div>
              {#if asking === s.id}
                <p class="ask" role="alert">{t('madoDesign.gallery.replaceHint')}</p>
                <div class="row">
                  <button type="button" class="btn primary" onclick={() => use(s.id)}>{t('madoDesign.gallery.replace')}</button>
                  <button type="button" class="btn" onclick={() => (asking = null)}>{t('madoDesign.gallery.keep')}</button>
                </div>
              {:else if on}
                <button type="button" class="btn" onclick={ontune}>{t('madoDesign.gallery.tuneNext')} <Icon name="arrow" size={14} class="flip" /></button>
              {:else}
                <button type="button" class="btn primary" disabled={locked || busy !== null} aria-describedby="mdg-title" onclick={() => use(s.id)}>
                  {busy === s.id ? t('madoDesign.gallery.picking') : t('madoDesign.gallery.use')}
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  {/each}
</section>

<style>
  .gal { display: flex; flex-direction: column; gap: 10px; }
  .head h2 { margin: 0 0 2px; }
  .head p { margin: 0; }
  .group { margin: 14px 0 0; text-transform: none; }
  .grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr)); gap: 16px; }
  .cardx {
    display: flex; flex-direction: column; border-radius: var(--radius-lg); overflow: hidden; background: var(--panel);
    border: var(--border-w) solid var(--border); box-shadow: var(--shadow); transition: transform .18s ease, box-shadow .18s ease, border-color .18s ease;
  }
  .cardx:hover { transform: translateY(-2px); box-shadow: var(--shadow-lg); }
  .cardx.on { border-color: var(--accent); box-shadow: 0 0 0 2px var(--accent), var(--shadow); }
  .cardx.suggested { border-color: color-mix(in srgb, var(--accent) 55%, var(--hairline)); border-style: dashed; }
  .pill.now { background: var(--tint); color: var(--on-tint); }
  .sample { height: 262px; overflow: hidden; border-bottom: var(--border-w) solid var(--border); }
  .meta { padding: 12px 14px 14px; display: flex; flex-direction: column; gap: 8px; flex: 1; }
  .meta .btn { align-self: flex-start; margin-top: auto; }
  .name { font-size: 14px; }
  .desc { margin: 0; font-size: var(--fs-sm); line-height: 1.45; }
  .pill.accent { background: var(--tint); color: var(--on-tint); display: inline-flex; align-items: center; gap: 4px; }
  .swatches { display: flex; }
  .sw { width: 18px; height: 18px; border-radius: 50%; border: 1px solid var(--control-border); margin-inline-end: -5px; }
  .ask { margin: 0; font-size: var(--fs-sm); }
  .note { margin: 0; font-size: var(--fs-sm); color: var(--muted); display: flex; align-items: center; gap: 6px; }
  .empty { margin: 0; padding: 18px; border: 1px dashed var(--control-border); border-radius: var(--radius-lg); }
  .err { margin: 0; color: var(--danger); }
  :global([dir='rtl']) .gal :global(.flip) { transform: scaleX(-1); }
</style>
