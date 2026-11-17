<script lang="ts">
  // The picture viewer: one for the whole app, mounted once in App.svelte and opened with
  // `openImage` (lib/imageOpen.svelte.ts). Zoom with the wheel, a trackpad pinch, the +/− buttons
  // or keys (0 shows the whole picture again); move it by dragging or with the arrow keys; a
  // double-click switches between the whole picture and its own size. It closes only with Esc or
  // the close button: a click beside the picture does nothing, so a drag that ends there loses nothing.
  // Moving and zooming change only the picture's transform, at most once a frame.
  import Icon from './Icon.svelte';
  import { t } from './i18n';
  import { a11y } from './store.svelte';
  import { imageViewer, closeImage } from './imageOpen.svelte';
  import {
    type Size, type View, fitView, clampPan, clampScale, scaleRange, zoomAt, zoomStep, panBy, arrowPan,
    toggleZoom, isFit, percent, wheelFactor, wheelPixels, nameFromSrc,
  } from './imageView';

  const cur = $derived(imageViewer.open);
  const name = $derived(cur ? (cur.name || nameFromSrc(cur.src) || cur.alt || '').trim() : '');
  const uid = `iv${Math.random().toString(36).slice(2, 8)}`;

  let phase = $state<'loading' | 'ready' | 'error'>('loading');
  let pct = $state(100);
  let canIn = $state(false);
  let canOut = $state(false);
  let whole = $state(true);
  let dragging = $state(false);
  /** The zoom level read out by screen readers, once it has settled. */
  let said = $state('');
  let sayTimer: ReturnType<typeof setTimeout> | undefined;

  function still(): boolean {
    try { return a11y().reduceMotion || matchMedia('(prefers-reduced-motion: reduce)').matches; } catch { return false; }
  }

  /** Everything the viewer does, set up when a picture opens and taken down when it closes. */
  function viewer(root: HTMLElement) {
    const dialog = root.querySelector<HTMLElement>('[role="dialog"]')!;
    const stage = root.querySelector<HTMLElement>('.iv-stage')!;
    const img = root.querySelector<HTMLImageElement>('.iv-img')!;
    const from = imageViewer.open?.from ?? null;
    const before = document.activeElement as HTMLElement | null;

    let v: View = { scale: 1, x: 0, y: 0 };
    let pic: Size = { w: 0, h: 0 };
    let room: Size = { w: stage.clientWidth, h: stage.clientHeight };
    let zoomed = false; // the user changed the zoom: a resize keeps it rather than fitting again
    let frame = 0;
    let anim = false;
    phase = 'loading';

    const measure = () => { room = { w: stage.clientWidth, h: stage.clientHeight }; };
    const paint = () => {
      frame = 0;
      img.classList.toggle('anim', anim);
      img.style.transform = `translate3d(${v.x}px, ${v.y}px, 0) scale(${v.scale})`;
    };
    function show(next: View, animate = false) {
      v = next;
      pct = percent(v);
      const r = scaleRange(pic, room);
      canIn = v.scale < r.max - 1e-3;
      canOut = v.scale > r.min + 1e-3;
      whole = isFit(v, pic, room);
      anim = animate && !still();
      if (!frame) frame = requestAnimationFrame(paint);
      clearTimeout(sayTimer);
      sayTimer = setTimeout(() => (said = t('chatfViewer.zoomNow', { pct: String(pct) })), 500);
    }
    const ready = () => phase === 'ready';
    const at = (e: { clientX: number; clientY: number }) => {
      const r = stage.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };

    function loaded() {
      measure();
      // A drawing without a size of its own (some SVG files) is given the room there is.
      pic = img.naturalWidth > 0 && img.naturalHeight > 0
        ? { w: img.naturalWidth, h: img.naturalHeight }
        : { w: Math.max(1, room.w - 48), h: Math.max(1, room.h - 48) };
      img.style.width = `${pic.w}px`;
      img.style.height = `${pic.h}px`;
      phase = 'ready';
      zoomed = false;
      show(fitView(pic, room));
    }
    function failed() { phase = 'error'; }
    img.addEventListener('load', loaded);
    img.addEventListener('error', failed);
    if (img.complete) { if (img.naturalWidth > 0) loaded(); else if (img.currentSrc) failed(); }

    // Buttons in the toolbar.
    const api = {
      zoomIn() { if (ready()) { zoomed = true; show(zoomStep(v, 1, pic, room), true); } },
      zoomOut() { if (ready()) { zoomed = true; show(zoomStep(v, -1, pic, room), true); } },
      fit() { if (ready()) { zoomed = false; show(fitView(pic, room), true); } },
    };
    controls = api;

    const ro = new ResizeObserver(() => {
      measure();
      if (!ready()) return;
      if (!zoomed) show(fitView(pic, room));
      else show(clampPan({ ...v, scale: clampScale(v.scale, pic, room) }, pic, room));
    });
    ro.observe(stage);

    // ---- zoom: wheel, Chrome-style pinch (ctrl + wheel), WebKit pinch (gesture events) ----
    let pinching = false;
    let pinchBase = 1;
    function onWheel(e: WheelEvent) {
      e.preventDefault();
      if (!ready() || pinching) return;
      const px = wheelPixels(e.deltaY, e.deltaMode, room.h) * (e.ctrlKey ? 4 : 1);
      if (!px) return;
      const p = at(e);
      zoomed = true;
      show(zoomAt(v, v.scale * wheelFactor(px), p.x, p.y, pic, room));
    }
    function onGestureStart(e: Event) { e.preventDefault(); pinching = true; pinchBase = v.scale; }
    function onGestureChange(e: Event) {
      e.preventDefault();
      if (!ready()) return;
      const g = e as Event & { scale: number; clientX: number; clientY: number };
      const p = Number.isFinite(g.clientX) ? at(g) : { x: room.w / 2, y: room.h / 2 };
      zoomed = true;
      show(zoomAt(v, pinchBase * (g.scale || 1), p.x, p.y, pic, room));
    }
    function onGestureEnd(e: Event) { e.preventDefault(); pinching = false; }

    // ---- move: drag with the pointer held ----
    let drag: { id: number; x: number; y: number; from: View } | null = null;
    function onDown(e: PointerEvent) {
      if (!ready() || e.button !== 0 || !e.isPrimary) return;
      e.preventDefault();
      drag = { id: e.pointerId, x: e.clientX, y: e.clientY, from: v };
      try { stage.setPointerCapture(e.pointerId); } catch { /* the pointer is already gone */ }
      dragging = true;
    }
    function onMove(e: PointerEvent) {
      if (!drag || e.pointerId !== drag.id) return;
      show(panBy(drag.from, e.clientX - drag.x, e.clientY - drag.y, pic, room));
    }
    function onUp(e: PointerEvent) {
      if (!drag || e.pointerId !== drag.id) return;
      drag = null; dragging = false;
      try { stage.releasePointerCapture(e.pointerId); } catch { /* released already */ }
    }
    function onDbl(e: MouseEvent) {
      if (!ready()) return;
      e.preventDefault();
      const p = at(e);
      const next = toggleZoom(v, p.x, p.y, pic, room);
      zoomed = !isFit(next, pic, room);
      show(next, true);
    }

    // ---- keys: caught before anything else in the app sees them ----
    const focusables = () => Array.from(dialog.querySelectorAll<HTMLElement>('button:not([disabled])')).filter((b) => b.offsetParent !== null);
    function onKey(e: KeyboardEvent) {
      if (e.isComposing) return;
      if (e.key === 'Escape') { e.preventDefault(); e.stopImmediatePropagation(); closeImage(); return; }
      if (e.key === 'Tab') {
        const list = focusables();
        e.stopImmediatePropagation();
        if (!list.length) { e.preventDefault(); return; }
        const i = list.indexOf(document.activeElement as HTMLElement);
        if (i < 0) { e.preventDefault(); list[e.shiftKey ? list.length - 1 : 0].focus(); }
        else if (e.shiftKey && i === 0) { e.preventDefault(); list[list.length - 1].focus(); }
        else if (!e.shiftKey && i === list.length - 1) { e.preventDefault(); list[0].focus(); }
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === '+' || e.key === '=') api.zoomIn();
      else if (e.key === '-' || e.key === '_') api.zoomOut();
      else if (e.key === '0') api.fit();
      else if (e.key.startsWith('Arrow')) {
        if (!ready()) return;
        const next = arrowPan(v, e.key, pic, room, e.shiftKey);
        if (!next) return;
        show(next, !e.repeat);
      } else return;
      e.preventDefault();
      e.stopImmediatePropagation();
    }
    // A press outside the buttons must not leave focus behind the viewer.
    function onRootDown(e: PointerEvent) {
      if (!dialog.contains(document.activeElement)) (dialog.querySelector<HTMLElement>('.iv-close') ?? dialog).focus({ preventScroll: true });
      if (!(e.target as HTMLElement).closest('button')) e.preventDefault();
    }

    const opts = { passive: false } as AddEventListenerOptions;
    stage.addEventListener('wheel', onWheel, opts);
    root.addEventListener('wheel', (e) => e.preventDefault(), opts); // the page behind never scrolls
    stage.addEventListener('gesturestart', onGestureStart, opts);
    stage.addEventListener('gesturechange', onGestureChange, opts);
    stage.addEventListener('gestureend', onGestureEnd, opts);
    stage.addEventListener('pointerdown', onDown);
    stage.addEventListener('pointermove', onMove);
    stage.addEventListener('pointerup', onUp);
    stage.addEventListener('pointercancel', onUp);
    stage.addEventListener('dblclick', onDbl);
    root.addEventListener('pointerdown', onRootDown);
    window.addEventListener('keydown', onKey, true);
    queueMicrotask(() => dialog.querySelector<HTMLElement>('.iv-close')?.focus({ preventScroll: true }));

    return {
      destroy() {
        cancelAnimationFrame(frame);
        clearTimeout(sayTimer);
        said = '';
        ro.disconnect();
        window.removeEventListener('keydown', onKey, true);
        if (controls === api) controls = null;
        dragging = false;
        const back = from?.isConnected ? from : before?.isConnected ? before : null;
        queueMicrotask(() => back?.focus?.({ preventScroll: true }));
      },
    };
  }

  let controls: { zoomIn(): void; zoomOut(): void; fit(): void } | null = null;
</script>

{#if cur}
  {#key cur.seq}
    <div class="iv-back modal-bg no-print" role="presentation" use:viewer>
      <div class="iv" role="dialog" aria-modal="true" aria-label={name ? t('chatfViewer.labelNamed', { name }) : t('chatfViewer.label')}
        aria-describedby="{uid}-hint" tabindex="-1">
        <div class="iv-bar">
          <bdi class="iv-name" title={name || undefined}>{name || t('chatfViewer.untitled')}</bdi>
          <div class="iv-zoom" role="group" aria-label={t('chatfViewer.zoomGroup')}>
            <button type="button" class="iconbtn" onclick={() => controls?.zoomOut()} disabled={phase !== 'ready' || !canOut}
              aria-label={t('chatfViewer.zoomOut')} title={`${t('chatfViewer.zoomOut')} (−)`}><Icon name="zoomOut" size={17} /></button>
            <button type="button" class="iv-pct" onclick={() => controls?.fit()} disabled={phase !== 'ready' || whole}
              aria-label={t('chatfViewer.fitNamed', { pct: String(pct) })} title={`${t('chatfViewer.fit')} (0)`}><bdi dir="ltr">{phase === 'ready' ? pct : 100}%</bdi></button>
            <button type="button" class="iconbtn" onclick={() => controls?.zoomIn()} disabled={phase !== 'ready' || !canIn}
              aria-label={t('chatfViewer.zoomIn')} title={`${t('chatfViewer.zoomIn')} (+)`}><Icon name="zoomIn" size={17} /></button>
          </div>
          <button type="button" class="iconbtn iv-close" onclick={closeImage} aria-label={t('chatfViewer.close')} title={`${t('chatfViewer.close')} (Esc)`}><Icon name="close" size={18} /></button>
        </div>
        <div class="iv-stage" class:ready={phase === 'ready'} class:grabbing={dragging}>
          <img class="iv-img" class:hide={phase !== 'ready'} src={cur.src} alt={cur.alt || name} draggable="false" decoding="async" />
          {#if phase === 'loading'}
            <p class="iv-msg muted" role="status">{t('chatfViewer.loading')}</p>
          {:else if phase === 'error'}
            <div class="iv-msg" role="alert"><Icon name="warning" size={22} /><p>{t('chatfViewer.failed')}</p></div>
          {/if}
        </div>
        <p class="iv-hint muted" id="{uid}-hint">{t('chatfViewer.hint')}</p>
        <span class="sr-only" role="status" aria-live="polite">{phase === 'ready' ? said : ''}</span>
      </div>
    </div>
  {/key}
{/if}

<style>
  /* Above every other dialog: a picture can be opened from one. */
  .iv-back { z-index: 95; place-items: stretch; background: color-mix(in srgb, var(--bg) 86%, transparent); padding: 0; }
  .iv { display: grid; grid-template-rows: auto 1fr auto; width: 100vw; height: 100vh; outline: none; color: var(--fg); }
  .iv-bar { display: flex; align-items: center; gap: 10px; padding: 10px 12px; padding-inline-start: 18px;
    background: var(--glass); -webkit-backdrop-filter: blur(30px) saturate(160%); backdrop-filter: blur(30px) saturate(160%);
    border-bottom: var(--border-w) solid var(--hairline); min-width: 0; }
  .iv-name { flex: 1; min-width: 0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .iv-zoom { display: inline-flex; align-items: center; gap: 2px; background: var(--fill); border-radius: 10px; padding: 2px; }
  .iv-pct { font: inherit; font-size: var(--fs-sm); font-variant-numeric: tabular-nums; min-width: 58px; padding: 5px 6px; border: 0; border-radius: 8px;
    background: transparent; color: var(--fg); cursor: pointer; }
  .iv-pct:hover:not(:disabled) { background: var(--fill-2); }
  .iv-pct:disabled { cursor: default; color: var(--muted); }
  .iv .iconbtn:disabled { opacity: .4; cursor: default; }
  .iv button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  /* Picture positions are physical (left/top), in either reading direction. */
  .iv-stage { position: relative; overflow: hidden; direction: ltr; touch-action: none; user-select: none; -webkit-user-select: none; min-height: 0; }
  .iv-stage.ready { cursor: grab; }
  .iv-stage.grabbing { cursor: grabbing; }
  .iv-img { position: absolute; left: 0; top: 0; max-width: none; max-height: none; transform-origin: 0 0; will-change: transform;
    -webkit-user-drag: none; box-shadow: var(--shadow-lg); }
  .iv-img:global(.anim) { transition: transform .18s var(--ease, ease-out); }
  .iv-img.hide { visibility: hidden; }
  .iv-msg { position: absolute; inset: 0; display: grid; place-content: center; justify-items: center; gap: 8px; margin: 0; padding: 24px; text-align: center; }
  .iv-msg p { margin: 0; }
  .iv-hint { margin: 0; padding: 6px 16px 10px; font-size: var(--fs-xs); text-align: center; }
  @media (max-height: 480px) { .iv-hint { display: none; } }
  @media (prefers-reduced-motion: reduce) { .iv-img:global(.anim) { transition: none; } }
  :global([data-motion="reduce"]) .iv-img:global(.anim) { transition: none; }
</style>
