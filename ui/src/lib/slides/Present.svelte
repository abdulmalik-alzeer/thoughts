<script lang="ts">
  // A deck presented with reveal.js inside the app window (lib/slides/present.ts mounts this).
  // Each slide is md()'s sanitised output — the same path Docs uses, never raw markdown HTML — with
  // the library's pictures fetched the way Docs fetches them and every remote picture taken out.
  // reveal.js is loaded only now, from the app's own bundle, with no plugins, no postMessage and no
  // link previews. Esc leaves; O shows every slide; S shows the presenter panel (notes, the next
  // slide, a timer). While a deck is shown the app's own shortcuts are paused.
  import { onMount, onDestroy, tick } from 'svelte';
  import { md, resolveLink } from '../md';
  import { t, fmtNum } from '../i18n';
  import { a11y } from '../store.svelte';
  import { rawBlobUrl } from '../content/util';
  import { inTauri } from '../api';
  import { recording } from '../keys/registry';
  import { slideTitle, type Deck } from './deck';
  import './theme.css';

  let { deck, path = '', onclose }: { deck: Deck; path?: string; onclose: () => void } = $props();

  let root = $state<HTMLDivElement | null>(null);
  let revealEl = $state<HTMLDivElement | null>(null);
  let slidesEl = $state<HTMLDivElement | null>(null);
  let index = $state(0);
  let presenter = $state(false);
  let elapsed = $state(0);
  let ready = $state(false);
  let failed = $state('');
  let api: any = null;
  let timer: ReturnType<typeof setInterval> | undefined;
  let blobs: string[] = [];
  let wasRecording = false;
  let closed = false;

  const reduce = () => !!a11y().reduceMotion || (typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches);
  const total = $derived(deck.slides.length);
  const notes = $derived(md(deck.slides[index]?.notes ?? ''));
  const next = $derived(index + 1 < total ? slideTitle(deck.slides[index + 1].body) : '');
  const clock = $derived(`${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, '0')}`);
  // `theme: light|dark` shows the deck in that look of the user's theme; `app` follows the window.
  const mode = $derived(deck.theme === 'app' ? undefined : deck.theme);
  const themeAttr = typeof document !== 'undefined' ? document.documentElement.getAttribute('data-theme') ?? undefined : undefined;

  /** A picture inside a slide: library files are fetched, a `data:` picture stays, anything else goes. */
  function picture(img: HTMLImageElement) {
    const src = img.getAttribute('src') || '';
    if (/^data:image\//i.test(src)) return;
    img.removeAttribute('src');
    img.removeAttribute('srcset');
    if (!path || !src || /^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(src)) return;
    rawBlobUrl(resolveLink(path, src)).then((u) => {
      if (!u) return;
      if (closed || !img.isConnected) { URL.revokeObjectURL(u); return; }
      blobs.push(u);
      img.src = u;
    });
  }

  function build() {
    if (!slidesEl) return;
    for (const s of deck.slides) {
      const sec = document.createElement('section');
      // md() is the app's sanitiser (DOMPurify allowlist); its output is what Docs shows.
      sec.innerHTML = md(s.body);
      for (const img of Array.from(sec.querySelectorAll('img'))) picture(img);
      slidesEl.append(sec);
    }
  }

  async function fullScreen(on: boolean) {
    try {
      if (inTauri()) {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('present_fullscreen', { on });
      } else if (on && root?.requestFullscreen && !document.fullscreenElement) {
        await root.requestFullscreen();
      } else if (!on && document.fullscreenElement) {
        await document.exitFullscreen();
      }
    } catch { /* the deck still shows, in the window */ }
  }

  function togglePresenter() {
    presenter = !presenter;
    tick().then(() => api?.layout?.());
  }

  function close() {
    if (closed) return;
    closed = true;
    onclose();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (api?.isOverview?.()) api.toggleOverview(false); else close();
    }
  }

  onMount(() => {
    wasRecording = recording.on;
    recording.on = true; // the app's shortcuts wait while a deck is shown (lib/keys/registry.ts)
    window.addEventListener('keydown', onKey, true);
    build();
    void fullScreen(true);
    (async () => {
      try {
        const [{ default: Reveal }] = await Promise.all([import('reveal.js'), import('reveal.js/reveal.css')]);
        if (closed || !revealEl) return;
        api = new Reveal(revealEl, {
          embedded: true, hash: false, history: false, controls: true, progress: true, center: true,
          width: 1280, height: 720, rtl: deck.dir === 'rtl', transition: reduce() ? 'none' : deck.transition, backgroundTransition: 'none',
          plugins: [], help: false, overview: true, jumpToSlide: false, navigationMode: 'linear', fragments: false, slideNumber: false,
          keyboard: { 27: () => {}, 83: () => togglePresenter() }, keyboardCondition: null,
          postMessage: false, postMessageEvents: false, fragmentInURL: false, previewLinks: false, autoPlayMedia: false,
          scrollActivationWidth: 0, view: null, touch: true, mouseWheel: false,
        });
        await api.initialize();
        if (closed) { api.destroy?.(); return; }
        api.on('slidechanged', (ev: any) => { index = ev.indexh ?? 0; });
        ready = true;
        revealEl.focus({ preventScroll: true });
      } catch (e) {
        failed = String((e as Error)?.message ?? e);
      }
    })();
    timer = setInterval(() => { elapsed += 1; }, 1000);
  });

  onDestroy(() => {
    closed = true;
    window.removeEventListener('keydown', onKey, true);
    recording.on = wasRecording;
    clearInterval(timer);
    try { api?.destroy?.(); } catch { /* already gone */ }
    for (const u of blobs.splice(0)) URL.revokeObjectURL(u);
    void fullScreen(false);
  });
</script>

<div class="present" bind:this={root} role="dialog" aria-modal="true" aria-label={t('slides.presenting', { title: deck.title || t('slides.untitled') })}
  data-theme={themeAttr} data-mode={mode} lang={deck.dir === 'rtl' ? 'ar' : undefined}>
  <div class="stage" class:with-panel={presenter}>
    <div class="reveal" bind:this={revealEl} dir={deck.dir} tabindex="-1">
      <div class="slides" bind:this={slidesEl}></div>
    </div>
    {#if !ready && !failed}<p class="loading" role="status">{t('slides.loading')}</p>{/if}
    {#if failed}<p class="loading" role="alert">{t('slides.failed')}</p>{/if}
    <button type="button" class="leave" onclick={close} aria-label={t('slides.leave')} title={t('slides.leave')}>×</button>
  </div>
  {#if presenter}
    <aside class="panel" aria-label={t('slides.panel')}>
      <div class="row"><strong class="count">{t('slides.count', { n: fmtNum(index + 1), total: fmtNum(total) })}</strong><span class="clock" dir="ltr" aria-label={t('slides.timer')}>{clock}</span></div>
      <h2>{t('slides.notes')}</h2>
      {#if deck.slides[index]?.notes}<div class="notes md" dir="auto">{@html notes}</div>{:else}<p class="muted">{t('slides.noNotes')}</p>{/if}
      <h2>{t('slides.next')}</h2>
      <p dir="auto">{next || t('slides.last')}</p>
      <p class="muted sm">{t('slides.keys')}</p>
    </aside>
  {/if}
</div>

<style>
  .present { position: fixed; inset: 0; z-index: 9000; display: flex; background: var(--bg); color: var(--fg); }
  .stage { position: relative; flex: 1; min-width: 0; height: 100%; }
  .stage .reveal { width: 100%; height: 100%; outline: none; }
  .panel { width: min(380px, 36%); border-inline-start: 1px solid var(--hairline); background: var(--panel); padding: 18px; overflow: auto; font-size: 15px; }
  .panel h2 { font-size: 13px; text-transform: none; margin: 18px 0 6px; color: var(--muted); }
  .panel .row { display: flex; justify-content: space-between; align-items: baseline; gap: 8px; }
  .panel .clock { font-family: var(--mono); font-size: 20px; }
  .panel .sm { font-size: 12px; margin-top: 18px; }
  .loading { position: absolute; inset-block-start: 45%; inset-inline: 0; text-align: center; }
  .leave { position: absolute; inset-block-start: 10px; inset-inline-end: 12px; z-index: 2; width: 32px; height: 32px; border-radius: 50%; border: 1px solid var(--hairline);
    background: var(--panel); color: var(--fg); font-size: 20px; line-height: 1; cursor: pointer; opacity: .6; }
  .leave:hover, .leave:focus-visible { opacity: 1; }
</style>
