<script lang="ts">
  // A presentation (PowerPoint .pptx, OpenDocument .odp) one slide at a time: each slide's title,
  // text with its bullets, tables, own pictures and speaker notes (office/slides.ts). The layout of
  // the slide itself is not copied; it reads like an outline. Arrow keys, Page Up/Down, Home and End
  // move between slides once the viewer has focus. Picture object URLs are revoked when it goes.
  import { onDestroy, untrack } from 'svelte';
  import Icon from '../../Icon.svelte';
  import { t, fmtNum } from '../../i18n';
  import { docDir } from '../../content/docx';
  import { zoomImages } from '../../zoomImages';
  import { readOdp, readPptx, type Deck, type Para, type Run } from '../office/slides';
  import { problemOf } from '../office/safezip';
  import { problemText } from '../office/problems';
  import type { RendererProps } from '../renderers';

  let { data, name, kind, fill, unsupported }: RendererProps = $props();
  const uid = $props.id();

  let deck = $state<Deck | null>(null);
  let at = $state(0);
  let seq = 0;
  let urls: string[] = [];
  const drop = () => { for (const u of urls.splice(0)) URL.revokeObjectURL(u); };

  async function read(b: Blob) {
    const n = ++seq;
    deck = null; at = 0;
    drop();
    try {
      const bytes = new Uint8Array(await b.arrayBuffer());
      const d = kind === 'odp' ? await readOdp(bytes) : await readPptx(bytes);
      if (n !== seq) { for (const u of d.urls) URL.revokeObjectURL(u); return; }
      urls = d.urls;
      deck = d;
    } catch (e) {
      if (n === seq) unsupported(problemText(problemOf(e)));
    }
  }
  $effect(() => { const b = data; untrack(() => read(b)); });
  onDestroy(() => { seq++; drop(); });

  const count = $derived(deck?.slides.length ?? 0);
  const slide = $derived(deck?.slides[at] ?? null);
  const go = (i: number) => { if (count) at = Math.max(0, Math.min(count - 1, i)); };

  function key(e: KeyboardEvent) {
    if (!count || (e.target as HTMLElement).closest('select, summary, a, input, textarea')) return;
    const rtl = getComputedStyle(e.currentTarget as Element).direction === 'rtl';
    let to = -1;
    if (e.key === 'ArrowRight') to = at + (rtl ? -1 : 1);
    else if (e.key === 'ArrowLeft') to = at + (rtl ? 1 : -1);
    else if (e.key === 'PageDown' || e.key === 'ArrowDown') to = at + 1;
    else if (e.key === 'PageUp' || e.key === 'ArrowUp') to = at - 1;
    else if (e.key === 'Home') to = 0;
    else if (e.key === 'End') to = count - 1;
    else return;
    e.preventDefault();
    go(to);
  }

  /** Paragraphs grouped into nested lists by their bullets and levels. */
  type Item = { para: Para; sub: ONode[] };
  type ONode = { kind: 'p'; para: Para } | { kind: 'list'; ordered: boolean; items: Item[] };
  function outline(paras: Para[]): ONode[] {
    const root: ONode[] = [];
    // stack of open lists: the list node and its level
    const stack: { list: Extract<ONode, { kind: 'list' }>; level: number; into: ONode[] }[] = [];
    for (const p of paras) {
      if (p.list === 'none') { stack.length = 0; root.push({ kind: 'p', para: p }); continue; }
      const ordered = p.list === 'number';
      while (stack.length && stack[stack.length - 1].level > p.level) stack.pop();
      let top = stack[stack.length - 1];
      if (top && top.level === p.level && top.list.ordered !== ordered) { stack.pop(); top = stack[stack.length - 1]; }
      if (!top || top.level < p.level) {
        const into = top ? (top.list.items[top.list.items.length - 1]?.sub ?? top.into) : root;
        const list: Extract<ONode, { kind: 'list' }> = { kind: 'list', ordered, items: [] };
        into.push(list);
        stack.push({ list, level: p.level, into });
        top = stack[stack.length - 1];
      }
      top.list.items.push({ para: p, sub: [] });
    }
    return root;
  }
  const textOf = (p: Para) => p.runs.map((r) => r.t).join('');
  const slideDir = $derived(slide ? docDir(slide.blocks.map((b) => (b.kind === 'text' ? b.paras.map(textOf).join(' ') : b.kind === 'table' ? b.rows.flat().join(' ') : '')).join(' ')) : 'ltr');
  const label = (i: number) => {
    const s = deck?.slides[i];
    return s?.title ? t('libf-preview-office.slides.option', { n: fmtNum(i + 1), title: s.title }) : t('libf-preview-office.slides.number', { n: fmtNum(i + 1) });
  };
</script>

{#snippet runs(list: Run[])}
  {#each list as r, i (i)}
    {#if r.href}<a href={r.href}>{@render styled(r)}</a>{:else}{@render styled(r)}{/if}
  {/each}
{/snippet}
{#snippet styled(r: Run)}
  {#if r.b}<strong>{@render it(r)}</strong>{:else}{@render it(r)}{/if}
{/snippet}
{#snippet it(r: Run)}
  {#if r.i}<em>{@render ul(r)}</em>{:else}{@render ul(r)}{/if}
{/snippet}
{#snippet ul(r: Run)}
  {#if r.u}<u>{@render st(r)}</u>{:else}{@render st(r)}{/if}
{/snippet}
{#snippet st(r: Run)}
  {#if r.s}<s>{r.t}</s>{:else}{r.t}{/if}
{/snippet}
{#snippet nodes(list: ONode[])}
  {#each list as n, i (i)}
    {#if n.kind === 'p'}
      <p dir={n.para.rtl ? 'rtl' : 'auto'} style:text-align={n.para.align}>{@render runs(n.para.runs)}</p>
    {:else if n.ordered}
      <ol>{#each n.items as it, j (j)}<li dir={it.para.rtl ? 'rtl' : 'auto'}>{@render runs(it.para.runs)}{#if it.sub.length}{@render nodes(it.sub)}{/if}</li>{/each}</ol>
    {:else}
      <ul>{#each n.items as it, j (j)}<li dir={it.para.rtl ? 'rtl' : 'auto'}>{@render runs(it.para.runs)}{#if it.sub.length}{@render nodes(it.sub)}{/if}</li>{/each}</ul>
    {/if}
  {/each}
{/snippet}

{#if deck === null}
  <div class="pad" role="status"><span class="sr-only">{t('libf-preview-core.loading', { name })}</span><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div></div>
{:else if !count}
  <p class="muted empty">{t('libf-preview-office.slides.empty')}</p>
{:else}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <section class="deck" class:fill tabindex="0" aria-label={t('libf-preview-office.slides.label', { name })} aria-keyshortcuts="ArrowLeft ArrowRight PageUp PageDown Home End" onkeydown={key}>
    <div class="bar">
      <button type="button" class="btn icon" onclick={() => go(at - 1)} disabled={at === 0} aria-label={t('libf-preview-office.slides.prev')} title={t('libf-preview-office.slides.prev')}><Icon name="left" size={16} class="flip-rtl" /></button>
      <span class="pos" aria-live="polite">{t('libf-preview-office.slides.of', { n: fmtNum(at + 1), total: fmtNum(count) })}</span>
      <button type="button" class="btn icon" onclick={() => go(at + 1)} disabled={at === count - 1} aria-label={t('libf-preview-office.slides.next')} title={t('libf-preview-office.slides.next')}><Icon name="right" size={16} class="flip-rtl" /></button>
      {#if count > 1}
        <label class="jump"><span class="sr-only">{t('libf-preview-office.slides.jump')}</span>
          <select value={at} onchange={(e) => go(+(e.currentTarget as HTMLSelectElement).value)}>
            {#each deck.slides as _, i (i)}<option value={i}>{label(i)}</option>{/each}
          </select>
        </label>
      {/if}
    </div>
    {#if slide}
      <article class="slide" dir={slideDir} aria-labelledby="{uid}-pos" use:zoomImages>
        <h2 class="sr-only" id="{uid}-pos">{label(at)}</h2>
        {#if slide.hidden}<p class="muted sm tag"><Icon name="eye" size={13} /> {t('libf-preview-office.slides.hidden')}</p>{/if}
        {#each slide.blocks as b, i (i)}
          {#if b.kind === 'text' && b.role === 'title'}
            <div class="title">{@render nodes(outline(b.paras))}</div>
          {:else if b.kind === 'text' && b.role === 'subtitle'}
            <div class="subtitle muted">{@render nodes(outline(b.paras))}</div>
          {:else if b.kind === 'text'}
            <div class="body">{@render nodes(outline(b.paras))}</div>
          {:else if b.kind === 'image'}
            <img src={b.src} alt={b.alt} />
          {:else if b.kind === 'table'}
            <div class="tw">
              <table>
                {#if b.rows.length > 1}<thead><tr>{#each b.rows[0] as c, j (j)}<th scope="col" dir="auto">{c}</th>{/each}</tr></thead>{/if}
                <tbody>{#each b.rows.slice(b.rows.length > 1 ? 1 : 0) as row, j (j)}<tr>{#each row as c, k (k)}<td dir="auto">{c}</td>{/each}</tr>{/each}</tbody>
              </table>
            </div>
          {/if}
        {/each}
        {#if !slide.blocks.length}<p class="muted">{t('libf-preview-office.slides.blank')}</p>{/if}
        {#if slide.skipped}<p class="muted sm tag"><Icon name="info" size={13} /> {t('libf-preview-office.slides.skipped')}</p>{/if}
      </article>
      {#if slide.notes}
        <details class="notes">
          <summary>{t('libf-preview-office.slides.notes')}</summary>
          <p dir="auto">{slide.notes}</p>
        </details>
      {/if}
    {/if}
  </section>
{/if}

<style>
  .deck { display: flex; flex-direction: column; gap: 10px; border-radius: var(--radius); outline: none; }
  .deck:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .deck.fill { height: 100%; min-height: 0; padding: 10px; overflow: auto; }
  .bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .pos { font-size: var(--fs-sm, 13px); color: var(--muted); min-width: 9ch; text-align: center; font-variant-numeric: tabular-nums; }
  .btn.icon { padding: 4px 8px; display: inline-flex; align-items: center; }
  .jump { margin-inline-start: auto; min-width: 0; }
  .jump select { max-width: 32ch; }
  .slide { background: var(--panel); border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 28px 32px; min-height: 260px; display: flex; flex-direction: column; gap: 12px; overflow-wrap: anywhere; max-width: 960px; }
  .title :global(p), .title :global(li) { font-size: 1.5em; font-weight: 650; margin: 0; line-height: 1.25; }
  .subtitle :global(p) { margin: 0; font-size: 1.1em; }
  .body :global(p) { margin: 0 0 6px; white-space: pre-wrap; }
  .body :global(ul), .body :global(ol) { margin: 0 0 6px; padding-inline-start: 1.4em; }
  .body :global(li) { margin: 2px 0; white-space: pre-wrap; }
  .slide img { max-width: 100%; max-height: 60vh; height: auto; align-self: center; border-radius: 4px; }
  .tw { overflow-x: auto; overscroll-behavior-y: auto; }
  table { border-collapse: collapse; font-size: var(--fs-sm, 13px); }
  th, td { border: 1px solid var(--border); padding: 4px 8px; vertical-align: top; text-align: start; white-space: pre-wrap; }
  th { background: var(--fill); font-weight: 600; }
  .tag { display: flex; align-items: center; gap: 6px; margin: 0; }
  .sm { font-size: var(--fs-xs); }
  .notes { max-width: 960px; border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 8px 12px; background: var(--panel); }
  .notes summary { cursor: pointer; color: var(--muted); font-size: var(--fs-sm, 13px); }
  .notes p { margin: 8px 0 0; white-space: pre-wrap; }
  .empty { padding: 12px 0; }
  .pad { display: grid; gap: 12px; }
  .skel { height: 13px; margin: 5px 6px; border-radius: 8px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .skel.h { height: 26px; width: 45%; }
  .w70 { width: 70%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel { animation: none; } }
  :global([data-motion="reduce"]) .skel { animation: none; }
</style>
