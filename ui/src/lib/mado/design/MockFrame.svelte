<script lang="ts">
  // One mock-up, shown the only way a mock-up is ever shown: as the `srcdoc` of a sealed frame
  // (see srcdoc.ts for the three walls). The frame gets no events at all; a transparent layer
  // over it takes the click that drops a comment pin. Pins are drawn on that layer by this window,
  // never inside the mock-up.
  import { untrack } from 'svelte';
  import { t } from '../../i18n';
  import { DEVICES, looksClean, pinAt, scaleFor, sealedPage, type Device } from './srcdoc';
  import { sane } from './tokens';

  export interface Pin { n: number; x: number; y: number; body: string; done: boolean }

  let {
    html, tokens, name, device = 'desktop', length = 1, zoom = null, pins = [], onpin, thumb = false, unchecked = false, scale = $bindable(1),
  }: {
    /** The mock-up as the daemon cleaned it. */
    html: string;
    tokens: unknown;
    /** The screen's name, for the frame's title. */
    name: string;
    device?: Device;
    /** How many screens tall the frame is. */
    length?: number;
    /** A fixed zoom, or `null` to fit the width available. */
    zoom?: number | null;
    pins?: Pin[];
    /** Present: a click on the layer asks for a comment at that place (0..1 of the frame). */
    onpin?: (x: number, y: number) => void;
    /** A small picture of the mock-up: fits its box, no layer, no pins. */
    thumb?: boolean;
    /** Dev demo only: show the text even if it does not look cleaned, to prove the frame holds it. */
    unchecked?: boolean;
    /** The scale it is drawn at, for the toolbar to show. */
    scale?: number;
  } = $props();

  let room = $state(0);
  const size = $derived({ w: DEVICES[device].w, h: DEVICES[device].h * Math.max(1, length) });
  const s = $derived(scaleFor(device, room, thumb ? null : zoom));
  $effect(() => { scale = s; });
  const ok = $derived(unchecked || looksClean(html));
  // The ground behind the frame while a new page loads: the design's own background (a checked
  // `#rrggbb`), so re-colouring does not flash.
  const ground = $derived(sane(tokens).color.bg);

  // Rebuilding the page is how colours change live. A colour picker fires many times a second:
  // the frame follows at a calm pace, and always ends on the last value.
  let page = $state('');
  let timer: ReturnType<typeof setTimeout> | null = null;
  let last = 0;
  $effect(() => {
    const next = ok ? sealedPage(html, tokens) : '';
    const first = untrack(() => page) === '';
    const wait = first ? 0 : Math.max(0, 80 - (performance.now() - last));
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => { page = next; last = performance.now(); timer = null; }, wait);
    return () => { if (timer) clearTimeout(timer); };
  });

  function click(e: MouseEvent) {
    if (!onpin) return;
    // From the keyboard there is no place: the middle.
    if (e.detail === 0) { onpin(0.5, 0.5); return; }
    const at = pinAt(e.clientX, e.clientY, (e.currentTarget as HTMLElement).getBoundingClientRect());
    if (at) onpin(at.x, at.y);
  }
</script>

<div class="stage" class:thumb bind:clientWidth={room}>
  {#if !ok}
    <p class="unsafe" role="alert">{t('madoDesign.screens.unsafe')}</p>
  {:else}
    <!-- Left to right always: the frame is a picture with physical places, whatever the app's direction. -->
    <div class="fit" dir="ltr" style="width:{size.w * s}px;height:{size.h * s}px;background:{ground}">
      {#if page}
        <iframe sandbox="" referrerpolicy="no-referrer" srcdoc={page} title={t('madoDesign.screens.frame', { name })} tabindex="-1"
          style="width:{size.w}px;height:{size.h}px;transform:scale({s})"></iframe>
      {/if}
      {#if !thumb}
        {#if onpin}
          <button type="button" class="layer can" aria-label={t('madoDesign.pin.layer')} onclick={click}></button>
        {:else}
          <div class="layer"></div>
        {/if}
        {#each pins as p (p.n)}
          <span class="pin" class:done={p.done} role="img" aria-label={t('madoDesign.pin.label', { n: p.n, body: p.body })} title={p.body}
            style="left:{p.x * 100}%;top:{p.y * 100}%">{p.n}</span>
        {/each}
      {:else}
        <div class="layer"></div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .stage { width: 100%; min-width: 0; display: flex; justify-content: center; }
  .stage.thumb { height: 100%; overflow: hidden; justify-content: flex-start; }
  .fit { position: relative; flex: none; overflow: hidden; border-radius: 6px; box-shadow: var(--shadow-lg); outline: 1px solid var(--hairline); }
  .thumb .fit { border-radius: 0; box-shadow: none; outline: 0; }
  /* The frame takes no pointer and no keyboard: everything goes to the layer above it. */
  iframe { display: block; border: 0; transform-origin: 0 0; pointer-events: none; background: transparent; }
  .layer { position: absolute; inset: 0; background: transparent; border: 0; padding: 0; margin: 0; border-radius: inherit; display: block; width: 100%; height: 100%; }
  .layer.can { cursor: crosshair; }
  .layer.can:focus-visible { outline: 3px solid var(--accent); outline-offset: -3px; }
  .pin {
    position: absolute; transform: translate(-50%, -100%); min-width: 22px; height: 22px; padding: 0 5px; display: grid; place-items: center;
    border-radius: 11px 11px 11px 2px; background: var(--accent); color: var(--accent-fg); font-size: var(--fs-xs); font-weight: 700; line-height: 1;
    box-shadow: 0 0 0 2px var(--panel), var(--shadow); pointer-events: auto; cursor: default; animation: pin-in .25s ease backwards;
  }
  .pin.done { background: var(--panel); color: var(--muted); box-shadow: 0 0 0 2px var(--control-border), var(--shadow); }
  @keyframes pin-in { from { opacity: 0; transform: translate(-50%, -130%); } }
  .unsafe { margin: 0; padding: 24px; color: var(--danger); text-align: center; }
</style>
