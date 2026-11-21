<script lang="ts">
  // The diagram itself. It draws `doc` with the user's pending edits laid over it and never changes `doc`:
  // every edit becomes an entry of the pending list, reported through `onops`.
  import { onDestroy, tick, untrack } from 'svelte';
  import { Background, BackgroundVariant, MiniMap, NodeToolbar, Position, SvelteFlow, useSvelteFlow, type Connection, type Node } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/base.css';
  import type { Comment, ErdField, GraphDoc, GraphOp, Pos } from '../types';
  import { t, tn, fmtNum, lang } from '../../i18n';
  import { a11y } from '../../store.svelte';
  import { provideGraph, type Target } from './ctx';
  import { BAND_PREFIX, LARGE, MINIMAP_FROM, toFlow, type FlowEdge, type FlowNode, type Selection } from './flow';
  import { ICON, lookIcon } from './icons';
  import { centres, layout } from './layout';
  import { buildView, cardinality, commentMarks, paletteFor, readingOrder, sanitize, stepFocus, zoomAround, type Dir, type VEdge, type VNode } from './model';
  import { emptyHistory, isNewNode, pushOp, record, redo, refuseEdge, refuseNode, restore, sameOps, undo, type Refusal } from './ops';
  import BandNode from './parts/BandNode.svelte';
  import CardNode from './parts/CardNode.svelte';
  import EntityNode from './parts/EntityNode.svelte';
  import FieldsEditor from './parts/FieldsEditor.svelte';
  import MadoEdge from './parts/MadoEdge.svelte';
  import Outline from './parts/Outline.svelte';
  import SelBar from './parts/SelBar.svelte';

  let { doc, comments = [], editable = true, pendingOps = [], onops, onpositions, oncomment, onselect }: {
    doc: GraphDoc; comments?: Comment[]; editable?: boolean; pendingOps?: GraphOp[];
    onops?: (ops: GraphOp[]) => void; onpositions?: (positions: Record<string, Pos>) => void;
    oncomment?: (target: { kind: 'node'; nodeId: string } | { kind: 'edge'; edgeId: string }) => void;
    onselect?: (sel: { kind: 'node' | 'edge'; id: string } | null) => void;
  } = $props();

  const flow = useSvelteFlow();
  const nodeTypes = { card: CardNode, entity: EntityNode, band: BandNode };
  const edgeTypes = { mado: MadoEdge };
  const still = () => { try { return a11y().reduceMotion || matchMedia('(prefers-reduced-motion: reduce)').matches; } catch { return false; } };
  const ms = () => (still() ? 0 : 220);
  const dir = $derived(lang() === 'ar' ? 'rtl' : 'ltr');

  // ----- the file, made safe, and the pending edits -----
  const clean = $derived(sanitize(doc));
  const canEdit = $derived(editable && clean.doc.kind !== 'tasks');
  // The pending list is the parent's (`pendingOps`); it is mirrored here so an edit shows at once, and
  // whatever the parent then hands back wins.
  let ops = $state.raw<GraphOp[]>([]);
  let history = $state.raw(emptyHistory());
  let seen = '';
  $effect.pre(() => {
    const json = JSON.stringify(pendingOps ?? []);
    if (json === seen) return;
    seen = json;
    untrack(() => {
      if (json === JSON.stringify(ops)) return;
      ops = Array.isArray(pendingOps) ? pendingOps : [];
      history = emptyHistory();
      forgetNewPlaces();
    });
  });
  const view = $derived(buildView(clean.doc, ops));
  const byKey = $derived(new Map(view.nodes.map((n) => [n.key, n])));
  const edgeByKey = $derived(new Map(view.edges.map((e) => [e.key, e])));
  const marks = $derived(commentMarks(comments));

  // ----- places -----
  // Once the diagram is arranged, every place is held here, so one edit never shuffles the rest.
  let local = $state.raw<Record<string, Pos>>({});
  let wantFit = true;
  $effect.pre(() => {
    const ids = clean.doc.nodes.map((n) => n.id);
    untrack(() => {
      const held = Object.keys(local);
      if (held.length && !ids.some((id) => id in local)) { local = {}; wantFit = true; sel = null; editing = null; fieldsFor = null; }
    });
  });
  const lay = $derived(layout(view, { local }));
  $effect(() => {
    const pos = lay.pos;
    untrack(() => {
      if (Object.keys(pos).some((k) => !(k in local))) local = { ...pos, ...local };
      if (wantFit && Object.keys(pos).length) { wantFit = false; requestAnimationFrame(() => fit(0)); }
    });
  });
  function forgetNewPlaces() {
    if (Object.keys(local).some(isNewNode)) local = Object.fromEntries(Object.entries(local).filter(([k]) => !isNewNode(k)));
  }

  let outPos: Record<string, Pos> = {};
  let posTimer: ReturnType<typeof setTimeout> | undefined;
  function flushPositions() {
    clearTimeout(posTimer);
    if (!Object.keys(outPos).length) return;
    const send = outPos;
    outPos = {};
    onpositions?.(send);
  }
  /** Tell the parent which nodes moved (a little later, so a run of small moves is one save). */
  function reportMoved(keys: string[], places: Record<string, Pos>) {
    for (const k of keys) { const v = byKey.get(k); if (v?.id && places[k]) outPos[v.id] = places[k]; }
    // A file with only some places saved would be arranged oddly next time: the first move saves them all.
    for (const n of view.nodes) if (n.id && !n.pos && !(n.id in outPos) && places[n.key]) outPos[n.id] = places[n.key];
    clearTimeout(posTimer);
    posTimer = setTimeout(flushPositions, 500);
  }
  onDestroy(flushPositions);

  // ----- selection, editing -----
  let sel = $state.raw<Selection>(null);
  let editing = $state.raw<Target | null>(null);
  let fieldsFor = $state<string | null>(null);
  let spot = $state<string | null>(null);
  let asList = $state(false);
  let adding = $state(false);
  let announce = $state('');
  let notice = $state('');
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  let root: HTMLDivElement;

  function say(text: string, show = false) {
    announce = announce === text ? `${text}\u00a0` : text;
    if (show) { notice = text; clearTimeout(noticeTimer); noticeTimer = setTimeout(() => (notice = ''), 3200); }
  }
  onDestroy(() => clearTimeout(noticeTimer));

  // What is selected or being edited may vanish (undo, a new file).
  $effect.pre(() => {
    const has = (x: Target | null) => !x || (x.kind === 'node' ? byKey.has(x.key) : edgeByKey.has(x.key));
    untrack(() => {
      if (!has(sel)) sel = null;
      if (!has(editing)) editing = null;
      if (fieldsFor && !byKey.has(fieldsFor)) fieldsFor = null;
    });
  });
  let lastSel = '';
  $effect(() => {
    const s = sel ? `${sel.kind}:${sel.key}` : '';
    if (s === lastSel) return;
    lastSel = s;
    const cur = sel;
    untrack(() => onselect?.(cur ? { kind: cur.kind, id: cur.key } : null));
  });
  function select(next: Selection) {
    if (next?.kind === sel?.kind && next?.key === sel?.key) return;
    sel = next;
    if (fieldsFor && !(next?.kind === 'node' && next.key === fieldsFor)) fieldsFor = null;
    adding = false;
  }

  const nameOf = (x: Target) => (x.kind === 'node' ? byKey.get(x.key)?.label : edgeTitle(edgeByKey.get(x.key))) ?? '';
  const edgeTitle = (e: VEdge | undefined) => (e ? t('mado-graph.edge.title', { from: byKey.get(e.from)?.label ?? '', to: byKey.get(e.to)?.label ?? '' }) : '');
  const kindName = (n: VNode) => (n.look === 'other' ? (n.kind === 'other' ? t('mado-graph.kind.other') : n.kind) : t(`mado-graph.kind.${n.look}`));
  function describe(n: VNode): string {
    const parts = [n.label, kindName(n)];
    if (n.state !== 'same') parts.push(t(`mado-graph.pending.${n.state}`));
    const m = n.id ? marks.nodes.get(n.id) : null;
    if (m) parts.push(tn('mado-graph.comments', m.total));
    if (view.kind === 'erd' && n.fields.length) parts.push(tn('mado-graph.fields.count', n.fields.length));
    return parts.join(t('mado-graph.sep'));
  }
  function describeEdge(e: VEdge): string {
    const parts = [edgeTitle(e)];
    if (e.label) parts.push(e.label);
    const c = view.kind === 'erd' ? cardinality(e.kind) : null;
    if (c) parts.push(t(`mado-graph.rel.${c.from}-${c.to}`));
    if (e.state !== 'same') parts.push(t(`mado-graph.pending.${e.state}`));
    const m = e.id ? marks.edges.get(e.id) : null;
    if (m) parts.push(tn('mado-graph.comments', m.total));
    return parts.join(t('mado-graph.sep'));
  }

  // ----- the elements on the canvas -----
  let nodes = $state.raw<FlowNode[]>([]);
  let edges = $state.raw<FlowEdge[]>([]);
  // Elements that did not change keep their object, so the canvas leaves them alone.
  const kept = new Map<string, { sig: string; el: FlowNode | FlowEdge }>();
  $effect.pre(() => {
    const f = toFlow(view, lay, marks, sel, { editable: canEdit, describe, describeEdge });
    untrack(() => {
      const live = new Set<string>();
      const keep = <T extends FlowNode | FlowEdge>(prefix: string, el: T): T => {
        const k = prefix + el.id;
        live.add(k);
        const sig = JSON.stringify(el);
        const had = kept.get(k);
        if (had && had.sig === sig) return had.el as T;
        kept.set(k, { sig, el });
        return el;
      };
      nodes = f.nodes.map((n) => keep('n', n.type === 'band' ? { ...n, class: 'mado-band' } : n));
      edges = f.edges.map((e) => keep('e', e));
      for (const k of kept.keys()) if (!live.has(k)) kept.delete(k);
    });
  });
  const order = $derived(readingOrder(view, centres(lay), lay.columns));
  const counts = $derived.by(() => { const m = new Map<string, number>(); for (const n of view.nodes) m.set(n.look, (m.get(n.look) ?? 0) + 1); return m; });
  const selNode = $derived(sel?.kind === 'node' ? byKey.get(sel.key) ?? null : null);
  const skippedCount = $derived(clean.skipped.nodes + clean.skipped.edges + clean.skipped.dangling);

  // ----- edits -----
  function apply(next: GraphOp[]): boolean {
    if (sameOps(next, ops)) return false;
    history = record(history, ops);
    ops = next;
    seen = JSON.stringify(next);
    onops?.(next);
    return true;
  }
  const WHY: Record<Refusal, string> = {
    self: 'mado-graph.no.self', duplicate: 'mado-graph.no.duplicate', gone: 'mado-graph.no.gone', full: 'mado-graph.no.full',
    empty: 'mado-graph.no.empty', long: 'mado-graph.no.long', readonly: 'mado-graph.no.readonly',
  };
  function travel(step: typeof undo, done: string) {
    if (!canEdit) return;
    const r = step(history, ops);
    if (!r) return;
    history = r.history;
    ops = r.ops;
    seen = JSON.stringify(ops);
    editing = null;
    fieldsFor = null;
    forgetNewPlaces();
    onops?.(ops);
    say(t(done));
  }
  function discard() { if (canEdit && ops.length && apply([])) { forgetNewPlaces(); editing = null; fieldsFor = null; say(t('mado-graph.discarded'), true); } }

  function focusEl(x: Target | null) {
    tick().then(() => {
      const el = x && root?.querySelector<HTMLElement>(asList
        ? `.outline [data-key="${CSS.escape(x.key)}"] .name`
        : `.svelte-flow__${x.kind}[data-id="${CSS.escape(x.key)}"]`);
      if (el) el.focus({ preventScroll: asList ? false : true }); else if (!asList) root?.querySelector<HTMLElement>('.svelte-flow')?.focus?.();
    });
  }
  function rename(x: Target) {
    const v = x.kind === 'node' ? byKey.get(x.key) : edgeByKey.get(x.key);
    if (!canEdit || !v || v.state === 'removed') return;
    select(x);
    fieldsFor = null;
    editing = x;
  }
  function commit(x: Target, text: string) {
    editing = null;
    if (x.kind === 'node') {
      if (text.trim()) apply(pushOp(clean.doc, ops, { op: 'rename', id: x.key, label: text }));
    } else apply(pushOp(clean.doc, ops, { op: 'relabel-edge', id: x.key, label: text }));
    focusEl(x);
  }
  function cancel() { const x = editing; editing = null; focusEl(x); }
  function remove(x: Target) {
    if (!canEdit) return;
    const name = nameOf(x);
    const wasNew = x.kind === 'node' && isNewNode(x.key);
    if (!apply(pushOp(clean.doc, ops, x.kind === 'node' ? { op: 'remove-node', id: x.key } : { op: 'remove-edge', id: x.key }))) return;
    editing = null;
    fieldsFor = null;
    if (wasNew) {
      // The pending nodes after it each move up one key: their places move with them.
      const n = Number(x.key.slice(4));
      const next: Record<string, Pos> = {};
      for (const [k, p] of Object.entries(local)) {
        if (!isNewNode(k)) next[k] = p;
        else { const i = Number(k.slice(4)); if (i !== n) next[`new:${i > n ? i - 1 : i}`] = p; }
      }
      local = next;
      select(null);
      say(t('mado-graph.said.dropped', { name }));
      focusEl(null);
    } else if (x.kind === 'edge' && !edgeByKey.get(x.key)?.id) { select(null); say(t('mado-graph.said.dropped', { name })); focusEl(null); }
    else say(t('mado-graph.said.removed', { name }));
  }
  function putBack(x: Target) {
    if (!canEdit) return;
    const v = x.kind === 'node' ? byKey.get(x.key) : edgeByKey.get(x.key);
    if (v?.id && apply(restore(ops, x.kind, v.id))) say(t('mado-graph.said.restored', { name: nameOf(x) }));
  }
  function comment(x: Target) {
    const v = x.kind === 'node' ? byKey.get(x.key) : edgeByKey.get(x.key);
    if (!v?.id) { say(t('mado-graph.bar.newTip'), true); return; }
    select(x);
    oncomment?.(x.kind === 'node' ? { kind: 'node', nodeId: v.id } : { kind: 'edge', edgeId: v.id });
  }
  function setFields(key: string, fields: ErdField[]) {
    fieldsFor = null;
    apply(pushOp(clean.doc, ops, { op: 'set-prop', id: key, key: 'fields', value: fields }));
    focusEl({ kind: 'node', key });
  }
  function addNode(kind: string) {
    adding = false;
    const no = refuseNode(clean.doc, ops);
    if (!canEdit || no) { if (no) say(t(WHY[no]), true); return; }
    const near = selNode ? (selNode.state === 'removed' ? undefined : selNode.key) : undefined;
    const op: GraphOp = { op: 'add-node', label: t(`mado-graph.new.${kind}`), kind };
    if (near) op.near = near;
    const next = pushOp(clean.doc, ops, op);
    if (!apply(next)) return;
    const key = `new:${next.filter((o) => o.op === 'add-node').length}`;
    sel = { kind: 'node', key };
    fieldsFor = null;
    say(t('mado-graph.said.added', { kind: t(`mado-graph.kind.${kind}`) }));
    // Once it has a place, bring it into view and let the user type its name.
    tick().then(() => {
      const p = lay.pos[key], s = lay.size[key];
      if (!asList && p && s) flow.setCenter(p.x + s.w / 2, p.y + s.h / 2, { zoom: Math.max(flow.getZoom(), 0.7), duration: ms() });
      editing = { kind: 'node', key };
    });
  }
  function connect(c: Connection) {
    if (!canEdit || !c.source || !c.target) return;
    const no = refuseEdge(clean.doc, ops, c.source, c.target);
    if (no) { say(t(WHY[no]), true); return; }
    const next = pushOp(clean.doc, ops, { op: 'add-edge', from: c.source, to: c.target });
    const added = next.filter((o) => o.op === 'add-edge').length > ops.filter((o) => o.op === 'add-edge').length;
    if (!apply(next)) return;
    if (added) sel = { kind: 'edge', key: `new-edge:${next.filter((o) => o.op === 'add-edge').length}` };
    say(t(added ? 'mado-graph.said.linked' : 'mado-graph.said.relinked', { from: byKey.get(c.source)?.label ?? '', to: byKey.get(c.target)?.label ?? '' }));
  }

  provideGraph({
    get editable() { return canEdit; },
    get editing() { return editing; },
    get fieldsFor() { return fieldsFor; },
    get spot() { return spot; },
    get dir() { return dir; },
    rename, commit, cancel, remove, restore: putBack, comment, setFields,
    fields: (key) => { fieldsFor = key; if (key) editing = null; },
  });

  // ----- the view -----
  // Room for the bars at the top and the bottom of the canvas (pixels).
  const FIT = { padding: { top: 62, bottom: 58, left: 36, right: 36 }, maxZoom: 1.15 };
  let alive = true;
  onDestroy(() => { alive = false; });
  function fit(duration = ms()) { if (alive) flow.fitView({ ...FIT, minZoom: 0.1, duration }); }
  // (The library's own zoomIn/zoomOut are bound before the canvas exists when used under a provider.)
  function zoomBy(k: number) { if (alive) flow.setZoom(Math.min(2.5, Math.max(0.1, flow.getZoom() * k)), { duration: ms() }); }
  function tidy() {
    const all = layout(view, { all: true });
    local = { ...all.pos };
    reportMoved(Object.keys(all.pos), all.pos);
    say(t('mado-graph.said.tidied'));
    requestAnimationFrame(() => fit());
  }
  function reveal(key: string) {
    const c = centres(lay)[key];
    if (c && !root?.querySelector(`.svelte-flow__node[data-id="${CSS.escape(key)}"]`)) flow.setCenter(c.x, c.y, { zoom: flow.getZoom(), duration: 0 });
  }
  function go(to: Target) {
    select(to);
    if (to.kind === 'node' && !asList) reveal(to.key);
    focusEl(to);
  }
  function nudge(key: string, dx: number, dy: number) {
    const p = lay.pos[key];
    if (!p) return;
    local = { ...local, [key]: { x: p.x + dx, y: p.y + dy } };
    reportMoved([key], local);
  }

  const ARROWS: Record<string, Dir> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };
  const typing = (el: EventTarget | null) => el instanceof HTMLElement && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  function onkeydown(e: KeyboardEvent) {
    if (typing(e.target) || e.isComposing) return;
    const mod = e.metaKey || e.ctrlKey;
    const k = e.key;
    if (mod && !e.altKey && k.toLowerCase() === 'z') { e.preventDefault(); if (e.shiftKey) travel(redo, 'mado-graph.said.redone'); else travel(undo, 'mado-graph.said.undone'); return; }
    if (e.ctrlKey && !e.metaKey && !e.altKey && k.toLowerCase() === 'y') { e.preventDefault(); travel(redo, 'mado-graph.said.redone'); return; }
    if (mod) return;
    if (k === 'Escape') {
      if (adding) { adding = false; e.preventDefault(); }
      else if (fieldsFor) { fieldsFor = null; e.preventDefault(); focusEl(sel); }
      else if (spot) { spot = null; e.preventDefault(); }
      else if (sel) { select(null); e.preventDefault(); }
      return;
    }
    const onButton = e.target instanceof HTMLElement && !!e.target.closest('button, a, [role="menu"], [role="dialog"], [role="toolbar"]');
    if (onButton) return;
    if (!asList && !e.altKey) {
      if (k === '+' || k === '=') { e.preventDefault(); zoomBy(1.3); return; }
      if (k === '-' || k === '_') { e.preventDefault(); zoomBy(1 / 1.3); return; }
      if (k === '0') { e.preventDefault(); fit(); return; }
    }
    if (!sel) return;
    if (k === 'Enter' || k === 'F2') { if (canEdit) { e.preventDefault(); rename(sel); } return; }
    if (k === 'Delete' || k === 'Backspace') {
      if (!canEdit) return;
      e.preventDefault();
      const v = sel.kind === 'node' ? byKey.get(sel.key) : edgeByKey.get(sel.key);
      if (v && v.state !== 'removed') remove(sel);
      return;
    }
    if (k.toLowerCase() === 'c' && !e.altKey) { e.preventDefault(); comment(sel); return; }
    const d = ARROWS[k];
    if (!d || asList) return;
    e.preventDefault();
    if (sel.kind === 'edge') {
      const edge = edgeByKey.get(sel.key);
      if (edge) go({ kind: 'node', key: d === 'left' || d === 'up' ? edge.from : edge.to });
    } else if (e.altKey) {
      const step = e.shiftKey ? 64 : 16;
      nudge(sel.key, d === 'left' ? -step : d === 'right' ? step : 0, d === 'up' ? -step : d === 'down' ? step : 0);
    } else {
      const to = stepFocus(view, centres(lay), sel.key, d);
      if (to) go({ kind: 'node', key: to });
    }
  }
  // Tab moves the focus from node to node: what has the focus is what is selected.
  function onfocusin(e: FocusEvent) {
    if (asList || !(e.target instanceof Element)) return;
    const el = e.target;
    if (el.classList.contains('svelte-flow__node')) { const id = el.getAttribute('data-id'); if (id && !id.startsWith(BAND_PREFIX) && byKey.has(id)) select({ kind: 'node', key: id }); }
    else if (el.classList.contains('svelte-flow__edge')) { const id = el.getAttribute('data-id'); if (id && edgeByKey.has(id)) select({ kind: 'edge', key: id }); }
  }

  // Pinching on a trackpad in WebKit (the app's window on macOS) arrives as gesture events, which the
  // canvas library does not handle: zoom around the pointer ourselves.
  let pinch = 1;
  type Gesture = Event & { scale: number; clientX: number; clientY: number };
  function ongesturestart(e: Event) { e.preventDefault(); pinch = flow.getZoom(); }
  function ongesturechange(e: Event) {
    e.preventDefault();
    const g = e as Gesture;
    const box = root.querySelector('.svelte-flow')?.getBoundingClientRect();
    if (!box || !Number.isFinite(g.scale)) return;
    flow.setViewport(zoomAround(flow.getViewport(), { x: g.clientX - box.left, y: g.clientY - box.top }, pinch * g.scale), { duration: 0 });
  }
  $effect(() => {
    const el = root;
    if (!el) return;
    el.addEventListener('gesturestart', ongesturestart);
    el.addEventListener('gesturechange', ongesturechange);
    el.addEventListener('gestureend', ongesturestart);
    return () => { el.removeEventListener('gesturestart', ongesturestart); el.removeEventListener('gesturechange', ongesturechange); el.removeEventListener('gestureend', ongesturestart); };
  });

  function dragStop(moved: Node[]) {
    const next = { ...local };
    const changed: string[] = [];
    for (const n of moved) {
      if (!byKey.has(n.id)) continue;
      const p = { x: Math.round(n.position.x), y: Math.round(n.position.y) };
      const was = lay.pos[n.id];
      if (was && was.x === p.x && was.y === p.y) continue;
      next[n.id] = p;
      changed.push(n.id);
    }
    if (!changed.length) return;
    local = next;
    reportMoved(changed, next);
  }

  const palette = $derived(canEdit ? paletteFor(view.kind) : []);
  const Add = ICON.add, Tidy = ICON.tidy, Fit = ICON.fit, In = ICON.zoomIn, Out = ICON.zoomOut, ListIco = ICON.list, MapIco = ICON.map, Undo = ICON.undo, Redo = ICON.redo, Info = ICON.info;
  const legend = $derived(view.kind === 'flow' || view.kind === 'architecture' ? [...counts.entries()].sort((a, b) => b[1] - a[1]) : []);
  const helpId = `mado-graph-keys-${Math.random().toString(36).slice(2, 8)}`;
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="mado-graph" bind:this={root} data-mado-graph data-kind={view.kind} role="group" aria-label={view.title || t(`mado-graph.title.${view.kind}`)} aria-describedby={helpId}
  {onkeydown} {onfocusin}>
  <div class="bar start" {dir}>
    {#if palette.length === 1}
      <button type="button" class="tb primary" onclick={() => addNode(palette[0])}><Add size={15} aria-hidden="true" /> {t(`mado-graph.add.${palette[0]}`)}</button>
    {:else if palette.length}
      <div class="menu-wrap">
        <button type="button" class="tb primary" aria-haspopup="menu" aria-expanded={adding} onclick={() => (adding = !adding)}><Add size={15} aria-hidden="true" /> {t('mado-graph.add')}</button>
        {#if adding}
          <div class="menu" role="menu" aria-label={t('mado-graph.add')}>
            {#each palette as kind (kind)}
              {@const Ico = lookIcon(kind)}
              <button type="button" role="menuitem" onclick={() => addNode(kind)}><Ico size={15} aria-hidden="true" /> {t(`mado-graph.kind.${kind}`)}</button>
            {/each}
            <p class="hint">{selNode && selNode.state !== 'removed' ? t('mado-graph.add.near', { name: selNode.label }) : t('mado-graph.add.hint')}</p>
          </div>
        {/if}
      </div>
    {/if}
    {#if !asList && view.nodes.length > 1}
      <button type="button" class="tb" onclick={tidy} title={t('mado-graph.tidy.tip')}><Tidy size={15} aria-hidden="true" /> {t('mado-graph.tidy')}</button>
    {/if}
  </div>

  <div class="bar end" {dir}>
    {#if canEdit && (ops.length || history.past.length || history.future.length)}
      <div class="pending" role="group" aria-label={t('mado-graph.pending.label')}>
        <span class="n" class:some={ops.length > 0} title={t('mado-graph.pending.tip')}>{ops.length ? tn('mado-graph.pending.count', ops.length) : t('mado-graph.pending.zero')}</span>
        <button type="button" class="tb icon" disabled={!history.past.length} onclick={() => travel(undo, 'mado-graph.said.undone')} aria-label={t('mado-graph.undo')} title={t('mado-graph.undo.tip')}><Undo size={15} class="flip-rtl" aria-hidden="true" /></button>
        <button type="button" class="tb icon" disabled={!history.future.length} onclick={() => travel(redo, 'mado-graph.said.redone')} aria-label={t('mado-graph.redo')} title={t('mado-graph.redo.tip')}><Redo size={15} class="flip-rtl" aria-hidden="true" /></button>
        <button type="button" class="tb" disabled={!ops.length} onclick={discard} title={t('mado-graph.discard.tip')}>{t('mado-graph.discard')}</button>
      </div>
    {/if}
    <button type="button" class="tb" aria-pressed={asList} onclick={() => { asList = !asList; adding = false; if (!asList) wantFit = false; }} title={t('mado-graph.list.tip')}>
      {#if asList}<MapIco size={15} aria-hidden="true" /> {t('mado-graph.showMap')}{:else}<ListIco size={15} aria-hidden="true" /> {t('mado-graph.showList')}{/if}
    </button>
  </div>

  {#if asList}
    <Outline {view} {order} {marks} {sel} onpick={(x) => { select(x); focusEl(x); }} />
  {:else}
    <div class="stage" dir="ltr">
      <SvelteFlow bind:nodes bind:edges {nodeTypes} {edgeTypes} minZoom={0.1} maxZoom={2.5} fitView fitViewOptions={FIT}
        nodesConnectable={canEdit} elementsSelectable nodesFocusable edgesFocusable disableKeyboardA11y deleteKey={null} selectionKey={null} multiSelectionKey={null}
        panOnScroll zoomOnPinch zoomOnDoubleClick={false} panOnDrag connectionRadius={36} nodeDragThreshold={3} autoPanOnNodeFocus
        onlyRenderVisibleElements={view.nodes.length > LARGE} proOptions={{ hideAttribution: true }}
        isValidConnection={(c) => canEdit && !!c.source && !!c.target && refuseEdge(clean.doc, ops, c.source, c.target) === null}
        onbeforeconnect={(c) => { connect(c); }}
        onbeforedelete={async () => false}
        onnodeclick={({ node }) => { if (node.type !== 'band') select({ kind: 'node', key: node.id }); }}
        onnodedragstart={({ targetNode }) => { if (targetNode && targetNode.type !== 'band') select({ kind: 'node', key: targetNode.id }); }}
        onnodedragstop={({ nodes: moved }) => dragStop(moved)}
        onedgeclick={({ edge }) => select({ kind: 'edge', key: edge.id })}
        onpaneclick={() => { select(null); editing = null; }}
        ariaLabelConfig={{
          'node.a11yDescription.default': t('mado-graph.keys'), 'node.a11yDescription.keyboardDisabled': t('mado-graph.keys'),
          'edge.a11yDescription.default': t('mado-graph.keys.edge'), 'minimap.ariaLabel': t('mado-graph.minimap'), 'handle.ariaLabel': t('mado-graph.handle'),
        }}>
        <Background variant={BackgroundVariant.Dots} gap={22} size={1.4} />
        {#if selNode && !editing && !fieldsFor}
          <NodeToolbar nodeId={selNode.key} isVisible position={Position.Top} offset={14}>
            <SelBar target={{ kind: 'node', key: selNode.key }} state={selNode.state} hasId={!!selNode.id} entity={view.kind === 'erd'} />
          </NodeToolbar>
        {/if}
        {#if view.nodes.length >= MINIMAP_FROM}
          <MiniMap position="bottom-right" pannable zoomable width={168} height={112} nodeBorderRadius={6} ariaLabel={t('mado-graph.minimap')} />
        {/if}
      </SvelteFlow>

      {#if !view.nodes.length}
        <div class="empty" {dir}><p>{t(canEdit ? 'mado-graph.empty.edit' : 'mado-graph.empty')}</p></div>
      {/if}

      <div class="zoom" {dir} role="group" aria-label={t('mado-graph.view')}>
        <button type="button" class="tb icon" onclick={() => zoomBy(1.3)} aria-label={t('mado-graph.zoomIn')} title={t('mado-graph.zoomIn')}><In size={15} aria-hidden="true" /></button>
        <button type="button" class="tb icon" onclick={() => zoomBy(1 / 1.3)} aria-label={t('mado-graph.zoomOut')} title={t('mado-graph.zoomOut')}><Out size={15} aria-hidden="true" /></button>
        <button type="button" class="tb icon" onclick={() => fit()} aria-label={t('mado-graph.fit')} title={t('mado-graph.fit')}><Fit size={15} aria-hidden="true" /></button>
      </div>

      {#if legend.length > 1 || skippedCount}
        <div class="legend" {dir}>
          {#if legend.length > 1}
            <div class="chips" role="group" aria-label={t('mado-graph.legend')}>
              {#each legend as [look, n] (look)}
                {@const Ico = lookIcon(look)}
                <button type="button" class="chip" aria-pressed={spot === look} onclick={() => (spot = spot === look ? null : look)} title={t('mado-graph.legend.tip')}>
                  <Ico size={13} aria-hidden="true" /> {t(`mado-graph.kind.${look}`)} <span class="num">{fmtNum(n)}</span>
                </button>
              {/each}
            </div>
          {/if}
          {#if skippedCount}
            <span class="chip warn" role="note" title={t('mado-graph.skipped.tip', { nodes: fmtNum(clean.skipped.nodes), edges: fmtNum(clean.skipped.edges + clean.skipped.dangling) })}>
              <Info size={13} aria-hidden="true" /> {tn('mado-graph.skipped', skippedCount)}
            </span>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if fieldsFor && byKey.get(fieldsFor)}
    {@const n = byKey.get(fieldsFor)!}
    <div class="fieldpanel" {dir}>
      {#key n.key}<FieldsEditor name={n.label} fields={n.fields} {dir} onapply={(f) => setFields(n.key, f)} oncancel={() => { fieldsFor = null; focusEl({ kind: 'node', key: n.key }); }} />{/key}
    </div>
  {/if}

  {#if notice}<div class="notice" {dir} aria-hidden="true">{notice}</div>{/if}
  <p class="sr-only" id={helpId}>{t(canEdit ? 'mado-graph.keys' : 'mado-graph.keys.readonly')}</p>
  <div class="sr-only" role="status" aria-live="polite">{announce}</div>
</div>

<style>
  .mado-graph {
    position: relative; width: 100%; height: 100%; min-height: 420px; overflow: hidden; border-radius: var(--radius-lg); background: var(--bg); color: var(--fg);
    border: var(--border-w) solid var(--hairline); isolation: isolate;
    --xy-background-color: transparent;
    --xy-background-pattern-color: color-mix(in srgb, var(--fg) 16%, transparent);
    --xy-edge-stroke: color-mix(in srgb, var(--fg) 42%, var(--bg));
    --xy-edge-stroke-selected: var(--accent);
    --xy-connectionline-stroke: var(--accent);
    --xy-connectionline-stroke-width: 2;
    --xy-handle-background-color: var(--panel);
    --xy-minimap-background-color: var(--panel);
    --xy-minimap-mask-background-color: color-mix(in srgb, var(--fg) 12%, transparent);
    --xy-minimap-mask-stroke-color: var(--accent);
    --xy-minimap-mask-stroke-width: 1;
    --xy-minimap-node-background-color: color-mix(in srgb, var(--fg) 26%, var(--panel));
    --xy-minimap-node-stroke-color: transparent;
    --xy-selection-background-color: var(--tint);
    --xy-selection-border: 1px dashed var(--accent);
    --xy-node-color: var(--fg);
  }
  .stage { position: absolute; inset: 0; }
  .stage :global(.svelte-flow) { background: transparent; font: inherit; color: inherit; }
  .stage :global(.svelte-flow__node) { outline: none; border-radius: 12px; }
  .stage :global(.svelte-flow__node:focus-visible) { outline: 2px solid var(--accent); outline-offset: 4px; }
  .stage :global(.svelte-flow__edge:focus-visible) { outline: none; }
  .stage :global(.svelte-flow__edge:focus-visible .line) { stroke: var(--accent); stroke-width: 3; }
  .stage :global(.svelte-flow__node.mado-band) { pointer-events: none; cursor: default; }
  .stage :global(.svelte-flow__edge-label) { background: none; padding: 0; }
  .stage :global(.svelte-flow__minimap) { border: var(--border-w) solid var(--hairline); border-radius: 10px; overflow: hidden; box-shadow: var(--shadow); margin: 12px; }
  .stage :global(.svelte-flow__minimap-node) { fill: color-mix(in srgb, var(--fg) 26%, var(--panel)); }
  .stage :global(.svelte-flow__connection-path) { stroke-dasharray: 6 5; }
  .stage :global(.svelte-flow__pane) { cursor: grab; }
  .stage :global(.svelte-flow__pane.dragging) { cursor: grabbing; }

  .bar { position: absolute; top: 10px; z-index: 6; display: flex; gap: 6px; align-items: center; flex-wrap: wrap; max-width: calc(100% - 20px); pointer-events: none; }
  .bar > :global(*) { pointer-events: auto; }
  .bar.start { inset-inline-start: 10px; }
  .bar.end { inset-inline-end: 10px; justify-content: flex-end; }
  .tb {
    font: inherit; font-size: 13px; font-weight: 500; display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 10px; border-radius: 9px; cursor: pointer;
    color: var(--fg); background: var(--glass); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow); white-space: nowrap;
    -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px);
  }
  .tb:hover:not(:disabled) { background: var(--panel-2); }
  .tb:disabled { opacity: .45; cursor: default; }
  .tb.icon { width: 30px; padding: 0; justify-content: center; }
  .tb.primary { background: var(--accent); color: var(--accent-fg); border-color: transparent; }
  .tb.primary:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 88%, var(--fg)); }
  .tb[aria-pressed="true"] { background: var(--tint); color: var(--on-tint); border-color: color-mix(in srgb, var(--accent) 40%, transparent); }
  .pending { display: inline-flex; align-items: center; gap: 4px; padding: 3px 3px 3px 10px; border-radius: 12px; background: var(--glass); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow);
    -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px); }
  [dir="rtl"] .pending { padding: 3px 10px 3px 3px; }
  .pending .tb { box-shadow: none; background: transparent; border-color: transparent; height: 26px; }
  .pending .tb.icon { width: 26px; }
  .pending .tb:hover:not(:disabled) { background: var(--fill-2); }
  .pending .n { font-size: 12.5px; color: var(--muted); margin-inline-end: 4px; white-space: nowrap; }
  .pending .n.some { color: var(--on-tint); font-weight: 600; }

  .fieldpanel { position: absolute; top: 50px; inset-inline-end: 10px; z-index: 7; max-width: calc(100% - 20px); }
  .menu-wrap { position: relative; }
  .menu { position: absolute; top: 36px; inset-inline-start: 0; min-width: 210px; padding: 5px; display: grid; gap: 1px; background: var(--panel); border: var(--border-w) solid var(--hairline);
    border-radius: 12px; box-shadow: var(--shadow-lg); z-index: 7; }
  .menu button { font: inherit; font-size: 13px; display: flex; align-items: center; gap: 8px; padding: 7px 9px; border: 0; border-radius: 8px; background: transparent; color: var(--fg); cursor: pointer; text-align: start; }
  .menu button:hover, .menu button:focus-visible { background: var(--fill-2); }
  .menu .hint { margin: 4px 0 2px; padding: 6px 9px 2px; border-top: 1px solid var(--hairline); font-size: 12px; color: var(--muted); max-width: 240px; overflow-wrap: anywhere; }

  .zoom { position: absolute; bottom: 12px; left: 12px; z-index: 5; display: flex; gap: 4px; }
  .legend { position: absolute; bottom: 12px; left: 124px; right: 12px; z-index: 5; display: flex; gap: 6px; flex-wrap: wrap; align-items: center; pointer-events: none; }
  .legend > :global(*) { pointer-events: auto; }
  .chips { display: flex; gap: 4px; flex-wrap: wrap; }
  .chip { font: inherit; font-size: 12px; display: inline-flex; align-items: center; gap: 5px; height: 26px; padding: 0 9px; border-radius: 999px; color: var(--muted); background: var(--glass);
    border: var(--border-w) solid var(--hairline); cursor: pointer; white-space: nowrap; -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px); }
  .chip:hover { color: var(--fg); }
  .chip[aria-pressed="true"] { background: var(--tint); color: var(--on-tint); border-color: color-mix(in srgb, var(--accent) 40%, transparent); }
  .chip .num { font-variant-numeric: tabular-nums; opacity: .8; }
  .chip.warn { cursor: help; color: var(--fg); }
  .empty { position: absolute; inset: 0; display: grid; place-items: center; pointer-events: none; color: var(--muted); padding: 60px 20px; text-align: center; }
  .notice { position: absolute; bottom: 52px; left: 50%; transform: translateX(-50%); z-index: 8; max-width: min(520px, 90%); padding: 7px 14px; border-radius: 999px; font-size: 13px;
    background: var(--fg); color: var(--bg); box-shadow: var(--shadow-lg); pointer-events: none; text-align: center; }
  :global([data-theme="contrast"]) .mado-graph, :global([data-theme="contrast"]) .tb, :global([data-theme="contrast"]) .chip, :global([data-theme="contrast"]) .pending, :global([data-theme="contrast"]) .menu { border-color: var(--border); }
</style>
