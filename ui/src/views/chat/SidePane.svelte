<script lang="ts">
  // A file read beside the chat, without leaving it: a note an answer changed or linked to, or a
  // file a message mentioned. Read-only; "Open in Docs" is where it is changed. It follows the file
  // as the assistant or anyone else changes it, sits at the end of the line (the left in Arabic),
  // is as wide as the user drags it (remembered on this computer), and lies over the chat when the
  // window is too narrow for both. Esc or its Close button closes it; the parent gives focus back.
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import Icon from '../../lib/Icon.svelte';
  import FilePreview from '../../lib/preview/FilePreview.svelte';
  import { api, apiBlob, on, ApiError } from '../../lib/api';
  import { hasHiddenText, md, mdClick, resolveLink } from '../../lib/md';
  import { zoomImages } from '../../lib/zoomImages';
  import { imageViewer } from '../../lib/imageOpen.svelte';
  import { errDetail } from '../../lib/errors';
  import { prettyName, rawBlobUrl } from '../../lib/content/util';
  import { go } from '../../lib/store.svelte';
  import { titleOf } from '../../lib/printPdf';
  import { fmtNum, t } from '../../lib/i18n';
  import { clampWidth, dragWidth, isNote, isOverlay, keyWidth, loadWidth, looksLikeFolder, maxWidth, minWidth, saveWidth } from './sidePane';

  let { path, chatId, avail, onclose, onopen }: {
    /** The library file to show (a folder shows its front page). */
    path: string;
    /** The chat beside it: its steps tell the pane a file may have just changed. */
    chatId: string;
    /** The width of the space the chat and the pane share, in points. */
    avail: number;
    onclose: () => void;
    /** A link inside the shown note was clicked (the parent decides: here, or Docs). */
    onopen: (path: string, e: MouseEvent) => void;
  } = $props();

  type Phase = 'loading' | 'note' | 'file' | 'folder' | 'gone' | 'error';
  let phase = $state<Phase>('loading');
  /** The file actually shown (a folder's front page, for a folder). */
  let shown = $state('');
  let content = $state('');
  let hash = '';
  let size = $state<number | undefined>(undefined);
  let modified = '';
  /** Bumped when a non-note file changed, so the preview loads it again. */
  let rev = $state(0);
  let err = $state('');
  /** Read out when the file changed while open. */
  let said = $state('');

  // A note goes by its title (frontmatter, else first heading), as in Docs; the path is shown below it.
  const name = $derived(phase === 'note' ? titleOf(content, prettyName(shown || path)) : prettyName(shown || path));
  const hidden = $derived(phase === 'note' && hasHiddenText(content));
  const empty = $derived(phase === 'note' && !content.replace(/^﻿/, '').replace(/^---\n[\s\S]*?\n---\n?/, '').trim());
  const headId = `pane-h-${Math.random().toString(36).slice(2, 8)}`;

  let root = $state<HTMLElement>();
  let heading = $state<HTMLElement>();
  let article = $state<HTMLElement>();
  let bodyEl = $state<HTMLElement>();

  // ---- width ----
  let want = $state(loadWidth());
  const width = $derived(clampWidth(want, avail));
  const overlay = $derived(isOverlay(avail));
  const rtl = () => !!root && getComputedStyle(root).direction === 'rtl';
  let drag: { x: number; w: number; id: number } | null = null;
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    drag = { x: e.clientX, w: width, id: e.pointerId };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    want = clampWidth(dragWidth(drag.w, drag.x, e.clientX, rtl()), avail);
  }
  function up(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    drag = null;
    saveWidth(want);
  }
  function handleKey(e: KeyboardEvent) {
    const next = keyWidth(e.key, width, avail, rtl(), e.shiftKey);
    if (next === null) return;
    e.preventDefault();
    want = next;
    saveWidth(want);
  }

  // ---- loading the file ----
  const status = (e: unknown) => (e instanceof ApiError ? e.status : Number((e as any)?.status) || 0);
  const read = (p: string) => api(`/library/file?path=${encodeURIComponent(p)}`);
  let seq = 0;
  /** `quiet`: a refresh after a change; what is on screen stays until the new version is in. */
  async function load(p: string, quiet = false) {
    const n = ++seq;
    if (!quiet) { phase = 'loading'; err = ''; said = ''; }
    const was = phase;
    try {
      if (isNote(p) || looksLikeFolder(p)) {
        let target = p;
        let d: any;
        if (!isNote(p)) {
          target = `${p}/README.md`;
          try { d = await read(target); }
          catch (e) {
            if (status(e) !== 404) throw e;
            // Not a folder with a front page after all: maybe a file without an extension.
            try { d = await read(p); target = p; }
            catch (e2) { if (n === seq) { shown = p; phase = status(e2) === 404 ? 'folder' : 'error'; err = phase === 'error' ? errDetail(e2) : ''; } return; }
          }
        } else d = await read(p);
        if (n !== seq) return;
        if (quiet && was === 'note' && shown === target && d?.hash && d.hash === hash) return;
        shown = target; content = String(d?.content ?? ''); hash = String(d?.hash ?? ''); phase = 'note';
        if (quiet && was === 'note') said = t('chatfPane.updated', { name: prettyName(target) });
      } else {
        // Other files: the library's list says whether it is there, how big, and when it changed.
        const list: any[] = await api('/library/tree');
        if (n !== seq) return;
        const f = Array.isArray(list) ? list.find((x) => x?.path === p) : null;
        if (!f) { shown = p; phase = 'gone'; return; }
        const m = String(f.modified ?? '');
        if (quiet && was === 'file' && m === modified && Number(f.size) === size) return;
        shown = p; size = Number(f.size) || undefined; modified = m; phase = 'file'; rev++;
        if (quiet && was === 'file') said = t('chatfPane.updated', { name: prettyName(p) });
      }
    } catch (e) {
      if (n !== seq) return;
      if (status(e) === 404) { shown = p; phase = 'gone'; return; }
      // A refresh that failed (the service restarting) keeps what is shown.
      if (quiet && (was === 'note' || was === 'file')) return;
      shown = p; err = errDetail(e) || String((e as any)?.message ?? e); phase = 'error';
    }
  }
  // Another file asked for while open: start at its top.
  $effect(() => { const p = path; untrack(() => { load(p); if (bodyEl) bodyEl.scrollTop = 0; }); });

  const preview = $derived.by(() => {
    void rev;
    const p = shown;
    return () => apiBlob(`/library/raw?path=${encodeURIComponent(p)}`);
  });

  // Pictures inside a note are library files: fetch their bytes and swap them in (as Docs does).
  let inline: string[] = [];
  const dropInline = () => { for (const u of inline.splice(0)) URL.revokeObjectURL(u); };
  $effect(() => {
    const el = article, p = shown;
    void content;
    dropInline();
    if (!el || !p || phase !== 'note') return;
    tick().then(() => {
      for (const img of Array.from(el.querySelectorAll('img'))) {
        const src = img.getAttribute('src') || '';
        if (!src || /^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(src)) continue;
        img.removeAttribute('src');
        rawBlobUrl(resolveLink(p, src)).then((u) => {
          if (!u) return;
          if (shown !== p || !img.isConnected) { URL.revokeObjectURL(u); return; }
          inline.push(u);
          img.src = u;
        });
      }
    });
  });

  /** A link inside the note: another library file opens (here, or in Docs with ⌘/Ctrl); web links
   *  are left to the window, which asks before opening them. */
  function clickLink(e: MouseEvent) {
    if (e.type === 'click' && mdClick(e)) return;
    const a = (e.target as HTMLElement | null)?.closest?.('a');
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('#') || href.startsWith('//')) return;
    e.preventDefault();
    const target = resolveLink(shown, href);
    if (target) onopen(target, e);
  }

  function openDocs() { go({ kind: 'docs', path: phase === 'folder' ? path : shown || path }); }

  // ---- keys ----
  /** Esc closes the pane, but never under a picture or a dialog opened over it (they close first). */
  function onWindowKey(e: KeyboardEvent) {
    if (e.key !== 'Escape' || e.defaultPrevented || e.isComposing) return;
    if (imageViewer.open || document.querySelector('.modal-bg')) return;
    const inside = !!root && root.contains(document.activeElement);
    if (!inside && !overlay) return;
    e.preventDefault();
    onclose();
  }
  /** Over the chat, Tab stays inside the pane (the chat under it cannot be reached anyway). */
  function paneKey(e: KeyboardEvent) {
    if (!overlay || e.key !== 'Tab' || !root) return;
    const list = Array.from(root.querySelectorAll<HTMLElement>('a[href],button:not([disabled]),[tabindex]:not([tabindex="-1"])')).filter((x) => x.offsetParent !== null);
    if (!list.length) return;
    const i = list.indexOf(document.activeElement as HTMLElement);
    if (e.shiftKey && i <= 0) { e.preventDefault(); list[list.length - 1].focus(); }
    else if (!e.shiftKey && (i === list.length - 1 || i < 0)) { e.preventDefault(); list[0].focus(); }
  }

  // ---- following changes ----
  let timer: ReturnType<typeof setTimeout> | undefined;
  let off: (() => void) | undefined;
  onMount(() => {
    window.addEventListener('keydown', onWindowKey);
    off = on((e: any) => {
      const step = e?.type === 'chat.message' && e.chatId === chatId && e.message?.role === 'tool_result';
      if (!step && e?.type !== 'library.changed' && e?.type !== 'resync') return;
      // Bursts (a wrap writes many files, a run many steps) settle into one refresh.
      clearTimeout(timer);
      timer = setTimeout(() => load(path, true), 300);
    });
    tick().then(() => heading?.focus({ preventScroll: true }));
  });
  onDestroy(() => {
    window.removeEventListener('keydown', onWindowKey);
    off?.();
    clearTimeout(timer);
    dropInline();
  });
</script>

{#if overlay}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim no-print" onclick={onclose}></div>
{/if}
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside class="pane no-print" class:overlay bind:this={root} style:width="{width}px" onkeydown={paneKey}
  role={overlay ? 'dialog' : 'complementary'} aria-modal={overlay ? 'true' : undefined} aria-labelledby={headId}>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div class="handle" role="separator" aria-orientation="vertical" tabindex="0"
    aria-label={t('chatfPane.resize')} title={t('chatfPane.resizeTip')}
    aria-valuenow={width} aria-valuemin={minWidth(avail)} aria-valuemax={maxWidth(avail)} aria-valuetext={t('chatfPane.width', { n: fmtNum(width) })}
    onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onkeydown={handleKey}></div>
  <header class="head">
    <Icon name={phase === 'file' ? 'attach' : phase === 'folder' ? 'folder' : 'docs'} size={16} />
    <div class="names">
      <h2 id={headId} class="ell" tabindex="-1" bind:this={heading} dir="auto" aria-label={t('chatfPane.region', { name })}>{name}</h2>
      <bdi class="where muted ell" dir="ltr" title={shown || path}>{shown || path}</bdi>
    </div>
    {#if phase !== 'gone'}
      <button type="button" class="btn sm" onclick={openDocs} title={t('chatfPane.openDocsTip')}><Icon name="external" size={14} class="flip-rtl" /> <span>{t('chatfPane.openDocs')}</span></button>
    {/if}
    <button type="button" class="btn ghost x" onclick={onclose} aria-label={t('chatfPane.close')} title={t('chatfPane.closeTip')}><Icon name="close" size={16} /></button>
  </header>
  <div class="body scroll" bind:this={bodyEl}>
    {#if phase === 'loading'}
      <div class="pad" role="status" aria-busy="true"><span class="sr">{t('chatfPane.loading', { name })}</span><div class="skel h"></div><div class="skel"></div><div class="skel w70"></div><div class="skel w40"></div></div>
    {:else if phase === 'gone' || phase === 'error'}
      <div class="state" role="alert">
        <Icon name="warning" size={22} />
        <p>{t(phase === 'gone' ? 'chatfPane.goneTitle' : 'chatfPane.failedTitle')}</p>
        {#if phase === 'gone'}<p class="muted sm">{t('chatfPane.goneBody')}</p>{:else if err}<p class="muted sm" dir="auto">{err}</p>{/if}
        <div class="row">
          <button type="button" class="btn" onclick={() => load(path)}><Icon name="retry" size={14} /> {t('chatfPane.retry')}</button>
          {#if phase === 'error'}<button type="button" class="btn" onclick={openDocs}>{t('chatfPane.openDocs')}</button>{/if}
        </div>
      </div>
    {:else if phase === 'folder'}
      <div class="state" role="note">
        <Icon name="folder" size={22} />
        <p>{t('chatfPane.folder')}</p>
        <button type="button" class="btn" onclick={openDocs}>{t('chatfPane.openDocs')}</button>
      </div>
    {:else if phase === 'file'}
      <FilePreview name={shown} load={preview} {size} label={t('chatfPane.contents', { name })} />
    {:else}
      {#if hidden}<p class="note" role="note"><Icon name="eye" size={14} /> <span>{t('chatfPane.hidden')}</span></p>{/if}
      {#if empty}<p class="muted empty">{t('chatfPane.empty')}</p>
      {:else}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <article class="md" dir="auto" bind:this={article} aria-label={t('chatfPane.contents', { name })} onclick={clickLink} onauxclick={clickLink} use:zoomImages>{@html md(content)}</article>
      {/if}
    {/if}
  </div>
  <p class="sr" role="status" aria-live="polite">{said}</p>
</aside>

<style>
  .pane { position: relative; flex: none; display: flex; flex-direction: column; min-width: 0; height: calc(100vh - 26px); background: var(--panel, var(--bg)); border-inline-start: var(--border-w, 1px) solid var(--border); animation: pane-in .18s ease-out backwards; }
  .pane.overlay { position: absolute; inset-block: 0; inset-inline-end: 0; height: auto; z-index: 30; max-width: 100%; box-shadow: var(--shadow-lg, 0 10px 40px rgba(0, 0, 0, .22)); }
  .scrim { position: absolute; inset: 0; z-index: 29; background: color-mix(in srgb, var(--fg) 12%, transparent); animation: fade-in .18s ease-out both; }
  @keyframes pane-in { from { opacity: 0; transform: translateX(12px); } }
  :global([dir="rtl"]) .pane { animation-name: pane-in-rtl; }
  @keyframes pane-in-rtl { from { opacity: 0; transform: translateX(-12px); } }
  @keyframes fade-in { from { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .pane, .scrim { animation: none; } }

  /* The handle straddles the pane's start edge (its left in English, its right in Arabic). */
  .handle { position: absolute; inset-block: 0; inset-inline-start: -4px; width: 8px; cursor: col-resize; z-index: 2; touch-action: none; }
  .handle::after { content: ''; position: absolute; inset-block: 0; inset-inline-start: 3px; width: 2px; background: transparent; transition: background .15s; }
  .handle:hover::after, .handle:focus-visible::after { background: var(--accent); }
  .handle:focus-visible { outline: none; }

  .head { display: flex; align-items: center; gap: 8px; padding-block: 10px 8px; padding-inline: 16px 10px; border-bottom: var(--border-w, 1px) solid var(--border); min-width: 0; }
  .head > :global(.icon) { flex: none; color: var(--muted); }
  .names { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  h2 { margin: 0; font-size: var(--fs-md, 15px); font-weight: 600; line-height: 1.3; border-radius: 4px; }
  h2:focus { outline: none; }
  h2:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .where { font-size: var(--fs-xs); display: block; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .btn.sm { font-size: var(--fs-xs); padding: 4px 9px; flex: none; display: inline-flex; align-items: center; gap: 5px; }
  .x { flex: none; padding: 4px; }

  .body { flex: 1; min-height: 0; overflow: auto; padding: 14px 18px 24px; }
  .body :global(.fp) { margin-top: 0; }
  .md { overflow-wrap: anywhere; }
  .note { display: flex; gap: 6px; align-items: flex-start; margin: 0 0 12px; font-size: var(--fs-xs); padding: 6px 10px; border-radius: var(--radius); background: var(--panel-2); border: var(--border-w, 1px) dashed var(--border); }
  .note :global(.icon) { flex: none; margin-top: 1px; }
  .empty { margin: 8px 0; }

  .state { display: grid; justify-items: center; gap: 8px; text-align: center; padding: 28px 12px; color: var(--muted); }
  .state p { margin: 0; color: var(--fg); max-width: 40ch; }
  .state p.muted { color: var(--muted); }
  .state .row { display: flex; gap: 8px; flex-wrap: wrap; justify-content: center; margin-top: 4px; }
  .sm { font-size: var(--fs-xs); }
  .pad { display: grid; gap: 12px; padding-top: 4px; }
  .skel { height: 13px; border-radius: 8px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .skel.h { height: 22px; width: 50%; }
  .w70 { width: 70%; } .w40 { width: 40%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel { animation: none; } }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; padding: 0; border: 0; margin: 0; }
  @media print { .pane, .scrim { display: none !important; } }
</style>
