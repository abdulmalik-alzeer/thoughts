<script lang="ts" module>
  // Map or list, as last chosen while the app is open.
  let lastList = false;
</script>

<script lang="ts">
  // The Graph page: every item and note of the library as a dot, every link between them as a line,
  // the areas in colours of the theme. Opened from an item's page it shows only that item and what is
  // near it ("links around"). The layout is worked out once, away from the window, and then stands
  // still; the same connections can be read as a list instead, for the keyboard and screen readers.
  import { onMount, tick, untrack } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { api, on } from '../lib/api';
  import { a11y, app, go } from '../lib/store.svelte';
  import { t, tn, fmtNum, lang } from '../lib/i18n';
  import { errDetail } from '../lib/errors';
  import { areaLabel } from '../lib/terms';
  import { prettyName } from '../lib/content/util';
  import { openPath } from '../lib/item';
  import { fold } from '../lib/CommandPalette.svelte';
  import { buildModel, clampZoom, fit, hit, search, step, type Camera, type GraphData, type Model } from '../lib/graph/model';
  import { runLayout } from '../lib/graph/layoutClient';
  import { draw, colourOf } from '../lib/graph/draw';
  import { readTheme, type Ink } from '../lib/graph/palette';

  let { path }: { path?: string } = $props();
  const focusPath = $derived(String(path ?? '').trim());

  // ----- data -----
  let data = $state<GraphData | null>(null);
  let model = $state.raw<Model | null>(null);
  let xs = $state.raw<Float32Array>(new Float32Array(0));
  let ys = $state.raw<Float32Array>(new Float32Array(0));
  let phase = $state<'loading' | 'layout' | 'ready' | 'error'>('loading');
  let loadError = $state('');
  let depth = $state(1);
  let lastJson = '';
  let seq = 0;

  const label = (n: { title?: string; path: string }) => {
    const ti = String(n.title ?? '').trim();
    if (ti) return ti;
    const p = n.path.split('/');
    // An item without a title is known by its folder.
    return p.length === 3 && p[2] === 'README.md' ? prettyName(p[1]) : prettyName(n.path);
  };

  async function load(quiet = false) {
    const mine = ++seq;
    if (!quiet) { phase = 'loading'; loadError = ''; }
    try {
      const q = focusPath ? `?focus=${encodeURIComponent(focusPath)}&depth=${depth}` : '';
      const v = (await api(`/library/graph${q}`)) as GraphData;
      if (mine !== seq) return;
      const json = JSON.stringify(v);
      if (quiet && json === lastJson && model) return;
      lastJson = json;
      const m = buildModel(v, label);
      if (!quiet || !model) phase = 'layout';
      const groups = Int32Array.from(m.nodes.map((n) => n.group));
      const edges = new Int32Array(m.edges.length * 2);
      m.edges.forEach((e, k) => { edges[2 * k] = e.s; edges[2 * k + 1] = e.t; });
      const r = await runLayout({ n: m.nodes.length, edges, groups });
      if (mine !== seq) return;
      const keep = quiet && model ? model.nodes[active]?.path : undefined;
      data = v; model = m; xs = r.x; ys = r.y;
      active = keep !== undefined ? (m.byPath.get(keep) ?? -1) : -1;
      hover = -1;
      phase = 'ready';
      await tick();
      if (!quiet) fitAll(false);
      paint();
    } catch (e) {
      if (mine !== seq) return;
      if (quiet && model) return; // keep showing what is there; the next change tries again
      loadError = errDetail(e) || String(e);
      phase = 'error';
    }
  }

  $effect(() => { void focusPath; void depth; untrack(() => { lastJson = ''; active = -1; query = ''; load(); }); });

  let timer: ReturnType<typeof setTimeout> | undefined;
  onMount(() => {
    const off = on((e) => {
      if (e.type !== 'library.changed' && e.type !== 'resync') return;
      clearTimeout(timer);
      timer = setTimeout(() => load(true), 1200);
    });
    return () => { off(); clearTimeout(timer); };
  });

  // ----- filters, search -----
  let areaOff = $state<string[]>([]);
  let status = $state('');
  let query = $state('');
  const shownFn = $derived.by(() => {
    const m = model, off = new Set(areaOff), st = status;
    return (i: number) => { const n = m?.nodes[i]; return !!n && !off.has(n.area) && (!st || n.status === st); };
  });
  const visible = $derived(model ? model.nodes.filter((n) => shownFn(n.i)).map((n) => n.i) : []);
  const statuses = $derived([...new Set((model?.nodes ?? []).map((n) => n.status).filter(Boolean))].sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b) || a.localeCompare(b)));
  const ORDER = ['active', 'seed', 'parked', 'done', 'dropped'];
  const statusLabel = (s: string) => { const k = `idea.status.${s}`; const x = t(k); return x === k ? s : x; };
  const areaName = (a: string) => (a ? areaLabel(a) : t('libf-graph.topLevel'));

  let qShown = $state('');
  let qTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => { const q = query; clearTimeout(qTimer); qTimer = setTimeout(() => (qShown = q), 180); return () => clearTimeout(qTimer); });
  const hits = $derived(model && qShown.trim() ? search(model, qShown, fold, shownFn) : []);
  const hitSet = $derived(qShown.trim() ? new Set(hits) : null);
  let hitAt = $state(0);
  // A new search: the view moves to show what was found.
  $effect(() => {
    const h = hits;
    untrack(() => {
      hitAt = 0;
      if (!h.length || phase !== 'ready') { paint(); return; }
      if (h.length === 1) centreOn(h[0]); else moveTo(fit(xs, ys, h.slice(0, 200), W, H));
      say(tn('libf-graph.found', h.length));
    });
  });
  function nextHit(d = 1) {
    if (!hits.length) return;
    hitAt = (hitAt + d + hits.length) % hits.length;
    active = hits[hitAt];
    centreOn(active);
    say(describe(active));
  }
  $effect(() => { void shownFn; untrack(() => { if (active >= 0 && !shownFn(active)) active = -1; paint(); }); });

  // ----- the canvas -----
  let wrap = $state<HTMLDivElement | null>(null);
  let canvas = $state<HTMLCanvasElement | null>(null);
  let W = $state(0), H = $state(0);
  let cam: Camera = { k: 1, x: 0, y: 0 };
  /** Bumped whenever the picture is drawn, so what sits on the map (the tip) follows the view. */
  let drawn = $state(0);
  let hover = $state(-1);
  let active = $state(-1);
  let ink: Ink = { bg: '#fff', fg: '#000', muted: '#555', accent: '#14a', line: '#ccc', panel: '#fff' };
  let palette = $state<string[]>([]);
  let font = '12px system-ui';
  let strong = false;
  const still = () => { try { return a11y().reduceMotion || matchMedia('(prefers-reduced-motion: reduce)').matches; } catch { return false; } };

  function readLook() {
    try {
      const th = readTheme(wrap ?? document.documentElement);
      ink = th.ink; palette = th.palette;
      const cs = getComputedStyle(wrap ?? document.body);
      font = `${lang() === 'ar' ? 13 : 12}px ${cs.fontFamily}`;
      strong = document.documentElement.dataset.theme === 'contrast';
    } catch { /* no document */ }
    paint();
  }
  onMount(() => {
    readLook();
    // Another theme, Day / Night or language: new colours.
    const mo = new MutationObserver(() => readLook());
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-mode', 'dir', 'lang', 'class', 'style'] });
    return () => mo.disconnect();
  });
  $effect(() => {
    const el = wrap;
    if (!el) return;
    const measure = () => {
      const r = el.getBoundingClientRect();
      const first = W === 0;
      W = Math.max(0, Math.floor(r.width)); H = Math.max(0, Math.floor(r.height));
      if (first && W > 0 && model) fitAll(false);
      paint();
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    // Measured at once too: the first resize report can come late (a window in the background).
    untrack(measure);
    return () => ro.disconnect();
  });

  let frame = 0;
  function paint() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      const c = canvas, m = model;
      if (!c || !m || phase !== 'ready' || W === 0) return;
      const dpr = Math.min(3, window.devicePixelRatio || 1);
      if (c.width !== Math.round(W * dpr) || c.height !== Math.round(H * dpr)) { c.width = Math.round(W * dpr); c.height = Math.round(H * dpr); }
      const ctx = c.getContext('2d');
      if (!ctx) return;
      const centre = focusPath && data?.focus ? (m.byPath.get(data.focus) ?? -1) : -1;
      draw(ctx, { m, xs, ys, cam, w: W, h: H, dpr, palette, ink, shown: shownFn, active: hover >= 0 ? hover : active, hits: hitSet, centre, font, strong });
      drawn++;
    });
  }
  onMount(() => () => { if (frame) cancelAnimationFrame(frame); cancelAnimationFrame(anim); });
  $effect(() => { void hover; void active; void hitSet; void palette; void W; void H; untrack(paint); });

  // ----- moving the view -----
  let anim = 0;
  function moveTo(to: Camera) {
    cancelAnimationFrame(anim);
    if (still()) { cam = to; paint(); return; }
    const from = { ...cam }, t0 = performance.now(), ms = 280;
    const ease = (p: number) => 1 - Math.pow(1 - p, 3);
    const tickAnim = (now: number) => {
      const p = Math.min(1, (now - t0) / ms), e = ease(p);
      cam = { k: from.k + (to.k - from.k) * e, x: from.x + (to.x - from.x) * e, y: from.y + (to.y - from.y) * e };
      paint();
      if (p < 1) anim = requestAnimationFrame(tickAnim);
    };
    anim = requestAnimationFrame(tickAnim);
  }
  function fitAll(animate = true) {
    if (!model) return;
    const which = visible.length ? visible : model.nodes.map((n) => n.i);
    const to = fit(xs, ys, which, W, H);
    if (animate) moveTo(to); else { cam = to; paint(); }
  }
  function centreOn(i: number, zoomIn = true) {
    if (i < 0) return;
    const k = zoomIn ? Math.max(cam.k, 1.2) : cam.k;
    moveTo({ k, x: W / 2 - xs[i] * k, y: H / 2 - ys[i] * k });
  }
  function zoomBy(f: number, px = W / 2, py = H / 2) {
    const k = clampZoom(cam.k * f);
    const wx = (px - cam.x) / cam.k, wy = (py - cam.y) / cam.k;
    cancelAnimationFrame(anim);
    cam = { k, x: px - wx * k, y: py - wy * k };
    paint();
  }
  function panBy(dx: number, dy: number) { cancelAnimationFrame(anim); cam = { ...cam, x: cam.x + dx, y: cam.y + dy }; paint(); }
  /** Keep a note in view after the keyboard moved to it. */
  function keepInView(i: number) {
    const x = xs[i] * cam.k + cam.x, y = ys[i] * cam.k + cam.y, pad = 60;
    if (x < pad || x > W - pad || y < pad || y > H - pad) centreOn(i, false); else paint();
  }

  // ----- pointer -----
  const pts = new Map<number, { x: number; y: number }>();
  let downAt: { x: number; y: number } | null = null;
  let moved = false;
  let pinch = 0;
  const local = (e: PointerEvent | WheelEvent) => { const r = canvas!.getBoundingClientRect(); return { x: e.clientX - r.left, y: e.clientY - r.top }; };
  function onDown(e: PointerEvent) {
    if (e.button !== 0 && e.pointerType === 'mouse') return;
    canvas?.setPointerCapture(e.pointerId);
    const p = local(e);
    pts.set(e.pointerId, p);
    if (pts.size === 1) { downAt = p; moved = false; }
    if (pts.size === 2) { const [a, b] = [...pts.values()]; pinch = Math.hypot(a.x - b.x, a.y - b.y); moved = true; }
  }
  function onMove(e: PointerEvent) {
    const p = local(e);
    const prev = pts.get(e.pointerId);
    if (prev) {
      pts.set(e.pointerId, p);
      if (pts.size === 2) {
        const [a, b] = [...pts.values()];
        const d = Math.hypot(a.x - b.x, a.y - b.y);
        if (pinch > 0 && d > 0) zoomBy(d / pinch, (a.x + b.x) / 2, (a.y + b.y) / 2);
        pinch = d;
        return;
      }
      if (downAt && Math.hypot(p.x - downAt.x, p.y - downAt.y) > 4) moved = true;
      if (moved) { panBy(p.x - prev.x, p.y - prev.y); hover = -1; return; }
    }
    if (!model) return;
    const h = hit(model, xs, ys, cam, p.x, p.y, shownFn);
    if (h !== hover) { hover = h; tipAt = p; }
    else if (h >= 0) tipAt = p;
  }
  function onUp(e: PointerEvent) {
    const p = local(e);
    pts.delete(e.pointerId);
    if (pts.size < 2) pinch = 0;
    if (pts.size === 0) {
      if (!moved && model) {
        const h = hit(model, xs, ys, cam, p.x, p.y, shownFn);
        if (h >= 0) openNode(h);
      }
      downAt = null;
    }
  }
  function onLeave() { if (!pts.size) hover = -1; }
  $effect(() => {
    const c = canvas;
    if (!c) return;
    // Not passive: the wheel zooms the map instead of scrolling the page.
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      const p = local(e);
      if (e.ctrlKey || e.metaKey || Math.abs(e.deltaY) >= Math.abs(e.deltaX)) zoomBy(Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0018)), p.x, p.y);
      else panBy(-e.deltaX, 0);
    };
    c.addEventListener('wheel', wheel, { passive: false });
    return () => c.removeEventListener('wheel', wheel);
  });

  // ----- keyboard -----
  let announce = $state('');
  function say(s: string) { announce = ''; tick().then(() => (announce = s)); }
  function describe(i: number): string {
    const n = model?.nodes[i];
    if (!n || !model) return '';
    const kind = n.kind === 'item' ? t('libf-graph.kind.item') : t('libf-graph.kind.note');
    const bits = [n.label, kind, areaName(n.area)];
    if (n.status) bits.push(statusLabel(n.status));
    bits.push(tn('libf-graph.linksTo', new Set(model.out[i]).size), tn('libf-graph.linkedFrom', new Set(model.inn[i]).size));
    return bits.join(t('libf-graph.sep'));
  }
  function start(): number {
    if (!model) return -1;
    const c = focusPath && data?.focus ? model.byPath.get(data.focus) : undefined;
    if (c !== undefined && shownFn(c)) return c;
    let best = -1;
    for (const i of visible) if (best < 0 || model.nodes[i].deg > model.nodes[best].deg) best = i;
    return best;
  }
  function onKey(e: KeyboardEvent) {
    if (!model || e.altKey || e.metaKey || e.ctrlKey) return;
    const dirs: Record<string, 'left' | 'right' | 'up' | 'down'> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };
    const d = dirs[e.key];
    if (d && e.shiftKey) { e.preventDefault(); const s = 80; panBy(d === 'left' ? s : d === 'right' ? -s : 0, d === 'up' ? s : d === 'down' ? -s : 0); return; }
    if (d) {
      e.preventDefault();
      hover = -1;
      if (active < 0 || !shownFn(active)) { active = start(); }
      else {
        const n = step(model, xs, ys, active, d, shownFn);
        if (n < 0) { say(t('libf-graph.noneThatWay')); return; }
        active = n;
      }
      if (active >= 0) { keepInView(active); say(describe(active)); }
      return;
    }
    if (e.key === 'Enter' || e.key === ' ') { if (active >= 0) { e.preventDefault(); openNode(active); } return; }
    if (e.key === '+' || e.key === '=') { e.preventDefault(); zoomBy(1.25); return; }
    if (e.key === '-' || e.key === '_') { e.preventDefault(); zoomBy(0.8); return; }
    if (e.key === '0') { e.preventDefault(); fitAll(); return; }
    if (e.key === 'Home') { e.preventDefault(); active = start(); if (active >= 0) { keepInView(active); say(describe(active)); } return; }
    if (e.key === 'Escape' && active >= 0) { e.stopPropagation(); active = -1; paint(); }
  }
  function onFocus() { if (active < 0 && model) say(t('libf-graph.canvasHint')); }

  // ----- opening -----
  function openNode(i: number) {
    const n = model?.nodes[i];
    if (n) openPath(n.path);
  }

  // ----- the tip by the pointer or the keyboard's note -----
  let tipAt = $state({ x: 0, y: 0 });
  const tipFor = $derived(hover >= 0 ? hover : active);
  const tipPos = $derived.by(() => {
    void drawn;
    if (tipFor < 0) return null;
    if (hover >= 0) return { x: tipAt.x, y: tipAt.y };
    return { x: xs[tipFor] * cam.k + cam.x, y: ys[tipFor] * cam.k + cam.y };
  });
  const tipStyle = $derived.by(() => {
    if (!tipPos) return '';
    const tw = 280, left = Math.min(Math.max(8, tipPos.x + 14), Math.max(8, W - tw - 8));
    const top = tipPos.y + 16 + 120 > H ? Math.max(8, tipPos.y - 130) : tipPos.y + 16;
    return `left:${left}px;top:${top}px;max-width:${tw}px`;
  });

  // ----- list view -----
  let asList = $state(lastList);
  $effect(() => { lastList = asList; });
  function toggleList() {
    asList = !asList;
    // The map is measured again when it comes back, and then shows everything.
    if (asList) { W = 0; H = 0; hover = -1; }
    say(asList ? t('libf-graph.nowList') : t('libf-graph.nowMap'));
  }
  const LIST_STEP = 150;
  let listMax = $state(LIST_STEP);
  $effect(() => { void visible; void qShown; untrack(() => (listMax = LIST_STEP)); });
  const listNodes = $derived.by(() => {
    if (!model) return [];
    const base = hitSet ? hits : [...visible].sort((a, b) => model!.nodes[a].label.localeCompare(model!.nodes[b].label, lang()));
    return base;
  });
  const uniq = (a: number[]) => [...new Set(a)].filter((j) => shownFn(j));

  const areaToggle = (a: string) => { areaOff = areaOff.includes(a) ? areaOff.filter((x) => x !== a) : [...areaOff, a]; };
  const allAreasOn = () => { areaOff = []; };
  const onlyArea = (a: string) => { areaOff = (model?.groups ?? []).map((g) => g.area).filter((x) => x !== a); };

  const centreTitle = $derived(model && data?.focus ? (model.nodes[model.byPath.get(data.focus) ?? -1]?.label ?? '') : '');
  const empty = $derived(phase === 'ready' && !!model && !focusPath && (model.nodes.length === 0 || model.edges.length === 0));
  const title = $derived(focusPath ? (centreTitle ? t('libf-graph.aroundTitle', { name: centreTitle }) : t('libf-graph.around')) : t('libf-graph.title'));
</script>

<div class="page graph">
  <div class="row head">
    <div class="grow">
      {#if focusPath}
        <nav class="crumbs" aria-label={t('libf-graph.backLabel')}>
          <button class="crumb" onclick={() => go({ kind: 'graph' })}><Icon name="left" size={14} class="flip-rtl" /> {t('libf-graph.whole')}</button>
        </nav>
      {/if}
      <h1 dir="auto">{title}</h1>
      <p class="muted sub">{focusPath ? t('libf-graph.aroundSub') : t('libf-graph.sub')}</p>
    </div>
    {#if focusPath}
      <div class="seg" role="group" aria-label={t('libf-graph.depth')}>
        {#each [1, 2, 3] as d (d)}<button type="button" class="segb" aria-pressed={depth === d} onclick={() => (depth = d)}>{tn('libf-graph.steps', d)}</button>{/each}
      </div>
      {#if centreTitle && data?.focus}
        <button class="btn" onclick={() => openPath(data!.focus!)}><Icon name="external" size={14} /> {t('libf-graph.openCentre')}</button>
      {/if}
    {/if}
  </div>

  {#if phase === 'loading' || phase === 'layout'}
    <div class="state" role="status" aria-busy="true">
      <span class="spin" aria-hidden="true"></span>
      <p>{phase === 'loading' ? t('libf-graph.loading') : t('libf-graph.arranging')}</p>
    </div>
  {:else if phase === 'error'}
    <div class="state" role="alert">
      <Icon name="warning" size={26} />
      <p>{t('libf-graph.failed')}</p>
      {#if loadError}<p class="muted sm" dir="auto">{loadError}</p>{/if}
      <button class="btn primary" onclick={() => load()}><Icon name="retry" size={14} /> {t('libf-graph.retry')}</button>
    </div>
  {:else if empty && model}
    <div class="state">
      <Icon name="graph" size={28} />
      {#if model.nodes.length === 0}
        <h2>{t('libf-graph.empty.title')}</h2>
        <p class="muted">{t('libf-graph.empty.body')}</p>
      {:else}
        <h2>{t('libf-graph.noLinks.title')}</h2>
        <p class="muted">{t('libf-graph.noLinks.body')}</p>
      {/if}
      <button class="btn" onclick={() => go({ kind: 'docs' })}><Icon name="docs" size={14} /> {t('libf-graph.toDocs')}</button>
    </div>
  {:else if model && phase === 'ready' && model.nodes.length === 0}
    <div class="state" role="note">
      <Icon name="search" size={26} />
      <p>{t('libf-graph.aroundGone')}</p>
      <button class="btn" onclick={() => go({ kind: 'graph' })}>{t('libf-graph.whole')}</button>
    </div>
  {:else if model}
    {#if data?.truncated}
      <p class="note muted sm" role="note"><Icon name="info" size={12} /> {t('libf-graph.truncated', { shown: fmtNum(model.nodes.length), total: fmtNum(data.total?.nodes ?? model.nodes.length) })}</p>
    {/if}
    {#if focusPath && !model.edges.length}
      <p class="note muted sm" role="note"><Icon name="info" size={12} /> {t('libf-graph.aroundNone')}</p>
    {/if}

    <div class="tools" role="toolbar" aria-label={t('libf-graph.tools')}>
      <div class="find">
        <Icon name="search" size={14} />
        <input type="search" dir="auto" bind:value={query} placeholder={t('libf-graph.search')} aria-label={t('libf-graph.search')}
          onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); nextHit(e.shiftKey ? -1 : 1); } }} />
      </div>
      {#if qShown.trim()}
        <span class="muted sm" aria-hidden="true">{hits.length ? t('libf-graph.hitOf', { n: fmtNum(hitAt + 1), total: fmtNum(hits.length) }) : t('libf-graph.noHits')}</span>
        {#if hits.length > 1}
          <button class="btn ghost icon" onclick={() => nextHit(-1)} aria-label={t('libf-graph.prevHit')} title={t('libf-graph.prevHit')}><Icon name="left" size={14} class="flip-rtl" /></button>
          <button class="btn ghost icon" onclick={() => nextHit(1)} aria-label={t('libf-graph.nextHit')} title={t('libf-graph.nextHit')}><Icon name="right" size={14} class="flip-rtl" /></button>
        {/if}
      {/if}
      {#if statuses.length}
        <select class="status" bind:value={status} aria-label={t('libf-graph.status')}>
          <option value="">{t('libf-graph.allStatuses')}</option>
          {#each statuses as s (s)}<option value={s}>{statusLabel(s)}</option>{/each}
        </select>
      {/if}
      <span class="grow"></span>
      {#if !asList}
        <button class="btn ghost icon" onclick={() => zoomBy(1.25)} aria-label={t('libf-graph.zoomIn')} title={t('libf-graph.zoomIn')}><Icon name="zoomIn" size={15} /></button>
        <button class="btn ghost icon" onclick={() => zoomBy(0.8)} aria-label={t('libf-graph.zoomOut')} title={t('libf-graph.zoomOut')}><Icon name="zoomOut" size={15} /></button>
        <button class="btn ghost" onclick={() => fitAll()} title={t('libf-graph.fitTip')}><Icon name="fit" size={14} /> {t('libf-graph.fit')}</button>
      {/if}
      <button class="btn" aria-pressed={asList} onclick={toggleList}><Icon name={asList ? 'graph' : 'list'} size={14} /> {asList ? t('libf-graph.showMap') : t('libf-graph.showList')}</button>
    </div>

    <div class="legend" role="group" aria-label={t('libf-graph.areas')}>
      {#each model.groups as g, gi (g.area)}
        {@const on = !areaOff.includes(g.area)}
        <button type="button" class="chip" class:off={!on} aria-pressed={on} onclick={() => areaToggle(g.area)} ondblclick={() => onlyArea(g.area)}
          title={t('libf-graph.areaTip')}>
          <span class="dot" style="background:{colourOf(palette, gi)}" aria-hidden="true"></span>
          <span dir="auto">{areaName(g.area)}</span><span class="n muted">{fmtNum(g.count)}</span>
        </button>
      {/each}
      {#if areaOff.length}<button type="button" class="lnk" onclick={allAreasOn}>{t('libf-graph.allAreas')}</button>{/if}
      <span class="key muted sm" aria-hidden="true"><span class="dot big"></span> {t('libf-graph.kind.item')} <span class="dot"></span> {t('libf-graph.kind.note')}</span>
    </div>

    {#if asList}
      <section class="list" aria-label={t('libf-graph.listLabel')}>
        <p class="muted sm">{tn('libf-graph.listCount', listNodes.length)}</p>
        {#if !listNodes.length}<p class="muted">{qShown.trim() ? t('libf-graph.noHits') : t('libf-graph.nothingShown')}</p>{/if}
        <ul>
          {#each listNodes.slice(0, listMax) as i (i)}
            {@const n = model.nodes[i]}
            {@const to = uniq(model.out[i])}
            {@const from = uniq(model.inn[i])}
            <li>
              <div class="lh">
                <span class="dot" style="background:{colourOf(palette, n.group)}" aria-hidden="true"></span>
                <button class="lnk ti" onclick={() => openNode(i)} dir="auto">{n.label}</button>
                <span class="muted sm">{n.kind === 'item' ? t('libf-graph.kind.item') : t('libf-graph.kind.note')}{t('libf-graph.sep')}<span dir="auto">{areaName(n.area)}</span>{#if n.status}{t('libf-graph.sep')}{statusLabel(n.status)}{/if}</span>
              </div>
              {#if n.summary}<p class="sum muted sm" dir="auto">{n.summary}</p>{/if}
              <dl>
                <div><dt>{t('libf-graph.linksToLabel')}</dt><dd>{#if to.length}{#each to as j, k (j)}{#if k}{t('libf-graph.listSep')}{/if}<button class="lnk" onclick={() => openNode(j)} dir="auto">{model.nodes[j].label}</button>{/each}{:else}<span class="muted">{t('libf-graph.none')}</span>{/if}</dd></div>
                <div><dt>{t('libf-graph.linkedFromLabel')}</dt><dd>{#if from.length}{#each from as j, k (j)}{#if k}{t('libf-graph.listSep')}{/if}<button class="lnk" onclick={() => openNode(j)} dir="auto">{model.nodes[j].label}</button>{/each}{:else}<span class="muted">{t('libf-graph.none')}</span>{/if}</dd></div>
              </dl>
            </li>
          {/each}
        </ul>
        {#if listNodes.length > listMax}<button class="btn" onclick={() => (listMax += LIST_STEP * 2)}>{tn('libf-graph.showMore', Math.min(LIST_STEP * 2, listNodes.length - listMax))}</button>{/if}
      </section>
    {:else}
      <!-- A map you move around in with the arrow keys: an application region, its keys described below it. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div class="stage" bind:this={wrap} tabindex="0" role="application" aria-roledescription={t('libf-graph.roledesc')}
        aria-label={tn('libf-graph.canvasLabel', visible.length)} aria-describedby="graph-keys"
        onkeydown={onKey} onfocus={onFocus}>
        <canvas bind:this={canvas} aria-hidden="true" class:pointer={hover >= 0}
          onpointerdown={onDown} onpointermove={onMove} onpointerup={onUp} onpointercancel={onUp} onpointerleave={onLeave}></canvas>
        {#if tipFor >= 0 && tipPos && model.nodes[tipFor]}
          {@const n = model.nodes[tipFor]}
          <div class="tip" style={tipStyle} aria-hidden="true">
            <b dir="auto">{n.label}</b>
            <span class="muted sm">{n.kind === 'item' ? t('libf-graph.kind.item') : t('libf-graph.kind.note')}{t('libf-graph.sep')}<span dir="auto">{areaName(n.area)}</span>{#if n.status}{t('libf-graph.sep')}{statusLabel(n.status)}{/if}</span>
            {#if n.summary}<span class="sum" dir="auto">{n.summary}</span>{/if}
            <span class="muted sm">{tn('libf-graph.linksTo', new Set(model.out[tipFor]).size)}{t('libf-graph.sep')}{tn('libf-graph.linkedFrom', new Set(model.inn[tipFor]).size)}</span>
            <span class="muted sm">{hover >= 0 ? t('libf-graph.clickOpens') : t('libf-graph.enterOpens')}</span>
          </div>
        {/if}
      </div>
      <p id="graph-keys" class="keys muted sm">{t('libf-graph.keys')}</p>
    {/if}
  {/if}
  <div class="sr-only" role="status" aria-live="polite">{announce}</div>
</div>

<style>
  .graph { max-width: none; }
  .head { align-items: flex-start; flex-wrap: wrap; }
  .sub { margin: -8px 0 12px; }
  .crumbs { margin-bottom: 6px; }
  .crumb { font: inherit; font-size: var(--fs-sm); color: var(--muted); background: none; border: 0; padding: 2px 4px; cursor: pointer; display: inline-flex; align-items: center; gap: 4px; border-radius: 6px; }
  .crumb:hover { color: var(--fg); background: var(--fill); }
  .state { display: grid; justify-items: center; gap: 8px; text-align: center; padding: 56px 16px; color: var(--muted); }
  .state h2 { margin: 6px 0 0; color: var(--fg); }
  .state p { margin: 0; max-width: 520px; }
  .spin { width: 22px; height: 22px; border-radius: 50%; border: 2px solid var(--hairline); border-top-color: var(--accent); animation: spin 0.9s linear infinite; }
  :global([data-motion="reduce"]) .spin { animation: none; border-top-color: var(--accent); }
  @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
  @keyframes spin { to { transform: rotate(360deg); } }
  .note { display: flex; align-items: center; gap: 6px; margin: 0 0 8px; }
  .tools { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-bottom: 8px; }
  .find { position: relative; display: flex; align-items: center; min-width: 200px; flex: 0 1 300px; }
  .find :global(.icon) { position: absolute; inset-inline-start: 10px; color: var(--muted); pointer-events: none; }
  .find input { padding-inline-start: 30px; }
  .status { width: auto; }
  .btn.icon { padding: 6px 8px; }
  .legend { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 6px; margin-bottom: 10px; }
  .chip { font: inherit; font-size: var(--fs-sm); color: var(--fg); background: var(--fill); border: var(--border-w) solid transparent; border-radius: 99px; padding: 2px 10px 2px 8px; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; }
  :global([data-theme="contrast"]) .chip { border-color: var(--border); }
  .chip:hover { background: var(--fill-2); }
  .chip.off { color: var(--muted); background: transparent; border-color: var(--hairline); text-decoration: line-through; }
  .chip.off .dot { opacity: .35; }
  .n { font-size: var(--fs-xs); }
  .dot { display: inline-block; width: 9px; height: 9px; border-radius: 50%; flex: none; background: var(--muted); }
  .dot.big { width: 12px; height: 12px; }
  .key { display: inline-flex; align-items: center; gap: 6px; margin-inline-start: 8px; }
  .lnk { font: inherit; color: var(--accent); background: none; border: 0; padding: 0 2px; cursor: pointer; text-decoration: underline; text-underline-offset: 2px; border-radius: 3px; text-align: start; }
  .stage { position: relative; height: max(380px, calc(100vh - 290px)); border-radius: var(--radius-lg); border: var(--border-w) solid var(--hairline); background: var(--bg); overflow: hidden; }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; display: block; cursor: grab; touch-action: none; border-radius: inherit; }
  canvas:active { cursor: grabbing; }
  canvas.pointer { cursor: pointer; }
  .stage:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .tip { position: absolute; z-index: 2; display: grid; gap: 3px; padding: 8px 10px; border-radius: var(--radius); background: var(--glass); -webkit-backdrop-filter: blur(20px); backdrop-filter: blur(20px);
    border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow); pointer-events: none; font-size: var(--fs-sm); text-align: start; }
  :global([data-theme="contrast"]) .tip { background: var(--panel); border-color: var(--border); }
  .tip .sum { overflow: hidden; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; unicode-bidi: plaintext; }
  .keys { margin: 6px 0 0; }
  .list ul { list-style: none; margin: 8px 0 12px; padding: 0; display: grid; gap: 4px; max-width: 900px; }
  .list li { padding: 8px 10px; border-radius: var(--radius); border: var(--border-w) solid var(--hairline); }
  .lh { display: flex; align-items: baseline; flex-wrap: wrap; gap: 4px 8px; }
  .lh .dot { align-self: center; }
  .ti { font-weight: 600; }
  .sum { margin: 2px 0 0; unicode-bidi: plaintext; }
  dl { margin: 4px 0 0; display: grid; gap: 2px; font-size: var(--fs-sm); }
  dl div { display: flex; flex-wrap: wrap; gap: 2px 6px; }
  dt { color: var(--muted); font-weight: 500; }
  dd { margin: 0; }
  .sm { font-size: var(--fs-xs); }
</style>
