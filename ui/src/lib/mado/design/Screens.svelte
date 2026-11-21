<script lang="ts">
  // The screens of the flow: a gallery of small sealed pictures, and one screen opened large with
  // device widths, zoom and comment pins. Mock-ups are fetched cleaned from the daemon and only
  // ever handed to MockFrame.
  import { untrack } from 'svelte';
  import { t, tn } from '../../i18n';
  import Icon from '../../Icon.svelte';
  import type { Comment, ScreenInfo } from '../types';
  import { client } from './client';
  import MockFrame, { type Pin } from './MockFrame.svelte';
  import { DEVICES, LENGTHS, ZOOM, type Device } from './srcdoc';

  let {
    projectId, screens, tokens, comments = [], locked = false, rev = 0, unchecked = false, onpin,
  }: {
    projectId: string;
    screens: ScreenInfo[];
    tokens: unknown;
    comments?: Comment[];
    locked?: boolean;
    /** Changes when the drawings may have changed: fetch them again. */
    rev?: number;
    unchecked?: boolean;
    onpin?: (screenId: string, x: number, y: number) => void;
  } = $props();

  type Got = { html?: string; error?: string; loading?: boolean };
  let got = $state<Record<string, Got>>({});
  let open = $state<string | null>(null);
  let device = $state<Device>('desktop');
  let length = $state<number>(1);
  let zoom = $state<number | null>(null);
  let scale = $state(1);

  // Fetch every drawn screen, two at a time, and again whenever `rev` or the list changes.
  let run = 0;
  $effect(() => {
    void rev;
    const ids = screens.filter((s) => s.hasMock).map((s) => s.id);
    const pid = projectId;
    untrack(() => {
      const mine = ++run;
      const keep: Record<string, Got> = {};
      for (const id of ids) keep[id] = got[id]?.html !== undefined ? { html: got[id].html, loading: true } : { loading: true };
      got = keep;
      const queue = [...ids];
      const worker = async () => {
        while (queue.length && mine === run) {
          const id = queue.shift()!;
          try {
            const r = await client().screen(pid, id);
            if (mine === run) got[id] = { html: String(r?.html ?? '') };
          } catch (e: any) {
            if (mine === run) got[id] = { error: String(e?.message ?? e) };
          }
        }
      };
      void worker(); void worker();
    });
  });
  // A screen that left the flow closes.
  $effect(() => { if (open && !screens.some((s) => s.id === open)) open = null; });

  const pinsOf = (id: string): Pin[] => comments
    .filter((c) => c.target.kind === 'screen' && c.target.screenId === id && c.status !== 'orphaned')
    .map((c, i) => ({ n: i + 1, x: (c.target as any).x, y: (c.target as any).y, body: c.body, done: c.status === 'resolved' }))
    .filter((p) => Number.isFinite(p.x) && Number.isFinite(p.y) && p.x >= 0 && p.x <= 1 && p.y >= 0 && p.y <= 1);

  const at = $derived(screens.findIndex((s) => s.id === open));
  const cur = $derived(at >= 0 ? screens[at] : null);
  const step = (d: number) => { if (screens.length) open = screens[(at + d + screens.length) % screens.length].id; };
  const zoomBy = (d: number) => { zoom = Math.round(Math.min(ZOOM.max, Math.max(ZOOM.min, (zoom ?? scale) + d)) * 100) / 100; };
  const canPin = $derived(!locked && !!onpin);
</script>

<section class="scr" aria-labelledby="mds-title">
  {#if !cur}
    <header class="head">
      <h2 id="mds-title">{t('madoDesign.screens.title')}</h2>
      <p class="muted">{t('madoDesign.screens.sub')}</p>
    </header>
    {#if screens.length === 0}
      <p class="empty muted">{t('madoDesign.screens.none')}</p>
    {:else}
      <ul class="grid">
        {#each screens as s (s.id)}
          {@const g = got[s.id]}
          {@const n = pinsOf(s.id).length}
          <li>
            <button type="button" class="tile" aria-label={t('madoDesign.screens.open', { name: s.label })} onclick={() => (open = s.id)}>
              <span class="shot">
                {#if !s.hasMock}
                  <span class="blank"><Icon name="edit" size={18} /> {t('madoDesign.screens.notDrawnShort')}</span>
                {:else if g?.html !== undefined}
                  <MockFrame html={g.html} {tokens} name={s.label} device="desktop" thumb {unchecked} />
                {:else if g?.error}
                  <span class="blank bad"><Icon name="warning" size={18} /> {t('madoDesign.screens.failed')}</span>
                {:else}
                  <span class="skel fill"></span>
                {/if}
              </span>
              <span class="cap">
                <strong class="grow">{s.label}</strong>
                {#if n}<span class="pill"><Icon name="chat" size={12} /> {n}</span>{/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
    {@const g = got[cur.id]}
    <div class="tools" role="toolbar" aria-label={cur.label}>
      <button type="button" class="btn ghost" onclick={() => (open = null)}><Icon name="left" size={15} class="flip" /> {t('madoDesign.screens.all')}</button>
      <h2 id="mds-title" class="title grow">{cur.label}</h2>
      <span class="muted sm">{t('madoDesign.screens.count', { i: at + 1, n: screens.length })}</span>
      <button type="button" class="iconb" aria-label={t('madoDesign.screens.prev')} title={t('madoDesign.screens.prev')} onclick={() => step(-1)}><Icon name="left" size={16} class="flip" /></button>
      <button type="button" class="iconb" aria-label={t('madoDesign.screens.next')} title={t('madoDesign.screens.next')} onclick={() => step(1)}><Icon name="right" size={16} class="flip" /></button>
    </div>
    <div class="tools second">
      <div class="seg" role="group" aria-label={t('madoDesign.screens.device')}>
        {#each Object.keys(DEVICES) as d (d)}
          <button type="button" class="segb" aria-pressed={device === d} onclick={() => (device = d as Device)}>{t(`madoDesign.device.${d}`)}</button>
        {/each}
      </div>
      <label class="inline">
        <span class="sr-only">{t('madoDesign.screens.length')}</span>
        <select class="len" value={String(length)} onchange={(e) => (length = +e.currentTarget.value)} aria-label={t('madoDesign.screens.length')}>
          {#each LENGTHS as l (l)}<option value={String(l)}>{t(`madoDesign.length.${l}`)}</option>{/each}
        </select>
      </label>
      <span class="grow"></span>
      <button type="button" class="iconb" aria-label={t('madoDesign.zoom.out')} title={t('madoDesign.zoom.out')} disabled={scale <= ZOOM.min} onclick={() => zoomBy(-ZOOM.step)}><Icon name="zoomOut" size={16} /></button>
      <span class="zoom" aria-live="polite" dir="ltr">{t('madoDesign.zoom.now', { n: Math.round(scale * 100) })}</span>
      <button type="button" class="iconb" aria-label={t('madoDesign.zoom.in')} title={t('madoDesign.zoom.in')} disabled={scale >= ZOOM.max} onclick={() => zoomBy(ZOOM.step)}><Icon name="zoomIn" size={16} /></button>
      <button type="button" class="btn ghost" aria-pressed={zoom === null} onclick={() => (zoom = null)}><Icon name="fit" size={15} /> {t('madoDesign.zoom.fit')}</button>
      {#if canPin && cur.hasMock}
        <button type="button" class="btn" onclick={() => onpin?.(cur.id, 0.5, 0.5)}><Icon name="chat" size={15} /> {t('madoDesign.pin.add')}</button>
      {/if}
    </div>

    <div class="canvas scroll">
      {#if !cur.hasMock}
        <p class="state muted"><Icon name="edit" size={22} /> {t('madoDesign.screens.notDrawn')}</p>
      {:else if g?.html !== undefined}
        <MockFrame html={g.html} {tokens} name={cur.label} {device} {length} {zoom} pins={pinsOf(cur.id)} {unchecked} bind:scale
          onpin={canPin ? (x, y) => onpin?.(cur.id, x, y) : undefined} />
      {:else if g?.error}
        <p class="state bad" role="alert"><Icon name="warning" size={22} /> {t('madoDesign.screens.failed')} <span class="muted">{g.error}</span></p>
      {:else}
        <p class="state muted" role="status">{t('madoDesign.screens.loading')}</p>
      {/if}
    </div>
    <p class="foot muted">
      <Icon name="lock" size={13} /> {t('madoDesign.screens.sealed')}
      {#if canPin && cur.hasMock}<span>· {t('madoDesign.pin.hint')}</span>{/if}
    </p>
  {/if}
</section>

<style>
  .scr { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .head h2 { margin: 0 0 2px; }
  .head p { margin: 0; }
  .grid { list-style: none; margin: 6px 0 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 16px; }
  .tile { font: inherit; color: inherit; text-align: start; width: 100%; padding: 0; cursor: pointer; display: flex; flex-direction: column; border-radius: var(--radius-lg); overflow: hidden; background: var(--panel); border: var(--border-w) solid var(--border); box-shadow: var(--shadow); transition: transform .18s ease, box-shadow .18s ease; }
  .tile:hover { transform: translateY(-2px); box-shadow: var(--shadow-lg); }
  .tile:focus-visible { outline: 3px solid var(--accent); outline-offset: 2px; }
  .shot { display: block; height: 168px; overflow: hidden; background: var(--fill); border-bottom: var(--border-w) solid var(--border); position: relative; }
  .blank { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; color: var(--muted); font-size: var(--fs-sm); background-image: radial-gradient(var(--hairline) 1px, transparent 1px); background-size: 12px 12px; }
  .blank.bad { color: var(--danger); }
  .fill { position: absolute; inset: 0; border-radius: 0; }
  .cap { display: flex; align-items: center; gap: 8px; padding: 10px 12px; font-size: 13px; }
  .cap .pill { display: inline-flex; align-items: center; gap: 4px; background: var(--tint); color: var(--on-tint); }
  .empty { margin: 0; padding: 22px; border: 1px dashed var(--control-border); border-radius: var(--radius-lg); }
  .tools { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .title { margin: 0; font-size: 16px; font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .grow { flex: 1; min-width: 0; }
  .sm { font-size: var(--fs-sm); }
  .iconb { font: inherit; color: var(--fg); background: var(--fill); border: 0; border-radius: var(--radius); width: 30px; height: 30px; display: grid; place-items: center; cursor: pointer; }
  .iconb:hover { background: var(--fill-2); }
  .iconb:disabled { opacity: .45; cursor: default; }
  .btn[aria-pressed='true'] { background: var(--tint); color: var(--on-tint); }
  .inline { margin: 0; }
  .len { width: auto; padding: 5px 8px; font-size: 13px; }
  .zoom { min-width: 44px; text-align: center; font-size: var(--fs-sm); font-variant-numeric: tabular-nums; color: var(--muted); }
  /* The table the drawing lies on: the theme's own quiet ground with a dot grid, so the drawing's edges read in Day and Night. */
  .canvas { border-radius: var(--radius-lg); border: var(--border-w) solid var(--border); background-color: var(--fill); background-image: radial-gradient(var(--hairline) 1px, transparent 1px); background-size: 14px 14px; padding: 22px; min-height: 320px; max-height: 76vh; overflow: auto; }
  .state { margin: 0; min-height: 260px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; text-align: center; }
  .state.bad { color: var(--danger); }
  .foot { margin: 0; font-size: var(--fs-xs); display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  :global([dir='rtl']) .scr :global(.flip) { transform: scaleX(-1); }
</style>
