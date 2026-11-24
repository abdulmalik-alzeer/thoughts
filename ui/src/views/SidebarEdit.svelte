<script lang="ts">
  // "Edit sidebar", in place (round 0.7.2): the sidebar's own list becomes the editor. Each page
  // keeps its row and gains a handle to drag it by, an icon to click (pick another), a name to click
  // (rename) and an eye (hide); headings of groups are rows too; hidden pages wait in a "Hidden"
  // list underneath, so nothing is ever lost. There is no form and no Save: a change shows at once,
  // is written a moment later (`PUT /api/layer/nav`, checked by the service exactly as before and
  // kept in History), and "Undo" takes the last one back.
  // Every change is a pure function of lib/navEdit.ts; this file only draws, drags and saves.
  // Keyboard: Tab to a row, Alt + ↑/↓ moves it (announced), ↑/↓ walks the rows, F2 or Enter on the
  // name renames, ⌘Z undoes. The four keys are in the registry (scope `sidebar-edit`), so they can be
  // changed in Settings like any other.
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import { flip } from 'svelte/animate';
  import { cubicOut } from 'svelte/easing';
  import Icon from '../lib/Icon.svelte';
  import HelpTip from '../lib/HelpTip.svelte';
  import NavMenu, { type MenuItem } from '../lib/NavMenu.svelte';
  import { app, a11y, type NavConfig } from '../lib/store.svelte';
  import { t, lang } from '../lib/i18n';
  import { put } from '../lib/api';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { fullAccessOn } from '../lib/fullAccess';
  import * as E from '../lib/navEdit';
  import { matchKey, keys } from '../lib/keys/registry';

  type Item = E.EditItem;
  type Row = E.EditRow<Item>;
  let { items, close, first = null }: { items: Item[]; close: () => void; first?: { id: string; act: 'rename' | 'icon' } | null } = $props();

  const full = fullAccessOn();
  const L = lang() as E.Lang;
  const G = E.guards(full);
  /** Bursts of changes (a held arrow key, several quick drags) are written once, after this pause. */
  const SAVE_IDLE_MS = 700;
  const MOVE_MS = 170;

  const start = E.draftOf($state.snapshot(app.nav) as NavConfig | null);
  let draft = $state.raw<NavConfig>(start);
  /** What the service has (the last arrangement it accepted). */
  let saved: NavConfig = start;
  const undo = new E.Undo<NavConfig>();
  let undoN = $state(0);
  let saving = $state(false);
  let problem = $state('');
  let announce = $state('');
  let root: HTMLDivElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  const view = $derived(E.editRows(items, draft, full));
  const nameOf = (it: Item) => E.labelOf(draft, it, L, full);
  const groupOf = (id: string | null) => (id ? E.groupName((draft.groups ?? []).find((g) => g.id === id)?.label, L) : '');
  const still = () => { try { return a11y().reduceMotion || matchMedia('(prefers-reduced-motion: reduce)').matches; } catch { return true; } };

  // ---------- change, undo, save ----------
  function change(next: NavConfig, say = ''): boolean {
    if (next === draft || JSON.stringify(next) === JSON.stringify(draft)) return false;
    undo.push(draft); undoN = undo.size;
    draft = next;
    if (say) announce = say;
    queueSave();
    return true;
  }
  function undoLast() {
    const was = undo.pop();
    if (!was) return;
    undoN = undo.size; draft = was; announce = t('sb72.undone');
    editing = null; picking = null;
    queueSave();
  }
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running: Promise<boolean> | null = null;
  function queueSave() { clearTimeout(timer); timer = setTimeout(() => { timer = undefined; void save(); }, SAVE_IDLE_MS); }
  /** Write the arrangement if it differs from what the service has. False when it was refused. */
  async function save(): Promise<boolean> {
    clearTimeout(timer); timer = undefined;
    if (running) await running;
    if (E.sameNav(draft, saved, full)) { problem = ''; return true; }
    const sent = draft;
    saving = true;
    running = put('/layer/nav', E.cleanNav(sent, full)).then(() => {
      saved = sent; problem = '';
      // The rest of the window (the rail, the palette, the list when editing ends) follows at once;
      // the service's own "the layer changed" then reads the same thing again.
      if (!app.preview) app.nav = { ...E.emptyNav(), ...E.cleanNav(sent, full) } as NavConfig;
      return true;
    }, (e: any) => { problem = e?.message ?? String(e); return false; });
    const ok = await running;
    running = null; saving = false;
    return ok && draft !== sent ? save() : ok;
  }
  // The sidebar changed from somewhere else (the assistant's change was kept, an Undo in History)
  // while nothing of ours waits: show that, and start the steps back afresh.
  $effect(() => {
    const cur = app.nav;
    untrack(() => {
      if (running || timer !== undefined || !E.sameNav(draft, saved, full) || E.sameNav(cur, saved, full)) return;
      draft = E.draftOf($state.snapshot(cur) as NavConfig | null); saved = draft;
      undo.clear(); undoN = 0;
    });
  });
  onDestroy(() => {
    clearTimeout(timer); endDrag();
    // Closed with a change still waiting (the sidebar was folded away): it is written all the same.
    if (!E.sameNav(draft, saved, full)) put('/layer/nav', E.cleanNav(draft, full)).catch(() => {});
  });
  async function done() {
    commitRename();
    if (await save()) close();
  }
  async function reset() {
    const ok = await confirmDialog({ title: t('sb72.resetTitle'), body: t('sb72.resetBody'), confirmLabel: t('sb72.resetConfirm'), danger: true });
    if (ok) change(E.emptyNav(), t('sb72.a.reset'));
  }

  // ---------- focus ----------
  // A row that moves is the same row: the control that had the focus keeps it (the browser drops
  // the focus of an element that is moved in the page, so it is given back after the update).
  const rowEl = (key: string) => [...(root?.querySelectorAll<HTMLElement>('[data-key]') ?? [])].find((e) => e.dataset.key === key);
  function mark(): { key: string; part: string } | null {
    const a = document.activeElement as HTMLElement | null;
    const row = a?.closest<HTMLElement>('[data-key]');
    return row && root?.contains(row) ? { key: row.dataset.key!, part: a!.dataset.part ?? 'grip' } : null;
  }
  async function focusPart(key: string, part: string) {
    await tick();
    const row = rowEl(key);
    (row?.querySelector<HTMLElement>(`[data-part="${part}"]`) ?? row?.querySelector<HTMLElement>('[data-part]'))?.focus();
  }
  const keep = (m: { key: string; part: string } | null) => { if (m) void focusPart(m.key, m.part); };

  // ---------- what a row can do ----------
  function sayPlace(it: Item, nav: NavConfig) {
    const rows = E.editRows(items, nav, full).rows;
    const me = rows.find((r) => r.kind === 'item' && r.item.id === it.id);
    if (!me) return '';
    const mine = rows.filter((r) => r.kind === 'item' && r.group === me.group);
    const at = { name: nameOf(it), n: mine.indexOf(me) + 1, of: mine.length };
    return me.group ? t('sb72.a.movedIn', { ...at, group: E.groupName((nav.groups ?? []).find((g) => g.id === me.group)?.label, L) }) : t('sb72.a.moved', at);
  }
  function moveBy(r: Row, d: -1 | 1) {
    const m = mark();
    if (r.kind === 'item') {
      const s = E.step(items, draft, r.item.id, d, full);
      if (!s) { announce = t('sb72.a.edge', { name: nameOf(r.item) }); return; }
      change(s.nav, sayPlace(r.item, s.nav));
    } else {
      const groups = draft.groups ?? [], at = groups.findIndex((g) => g.id === r.group);
      const next = E.moveGroup(items, draft, r.group, at + d, full);
      if (next === draft) { announce = t('sb72.a.edge', { name: groupOf(r.group) }); return; }
      change(next, t('sb72.a.group', { name: groupOf(r.group), n: at + d + 1, of: groups.length }));
    }
    keep(m);
  }
  function hide(it: Item, on: boolean) {
    const m = mark();
    change(E.setHidden(draft, it, on, full), t(on ? 'sb72.a.hidden' : 'sb72.a.shown', { name: nameOf(it) }));
    void focusPart('i:' + it.id, m?.part === 'eye' || !m ? 'eye' : m.part);
  }
  function removeGroup(id: string) {
    change(E.removeGroup(items, draft, id, full));
    void tick().then(() => root?.querySelector<HTMLElement>('.newgroup')?.focus());
  }
  function newGroup(holding?: Item) {
    const made = E.addGroup(draft, L, t('sb72.newGroup'), full);
    if (!made) return;
    change(holding ? E.place(items, made.nav, holding.id, { group: made.id, index: 0 }, full) : made.nav);
    void tick().then(() => { const r = view.rows.find((x) => x.key === 'g:' + made.id); if (r) void startRename(r); });
  }

  // ---------- rename ----------
  let editing = $state<string | null>(null);
  let text = $state('');
  async function startRename(r: Row) {
    if (r.kind === 'item' && !E.canRestyle(r.item, full)) return;
    commitRename();
    picking = null;
    text = r.kind === 'item' ? nameOf(r.item) : groupOf(r.group);
    editing = r.key;
    await tick();
    const el = rowEl(r.key)?.querySelector<HTMLInputElement>('input');
    el?.scrollIntoView({ block: 'nearest' });
    el?.focus(); el?.select();
  }
  function commitRename(refocus = false) {
    const key = editing;
    if (!key) return;
    editing = null;
    const r = view.rows.find((x) => x.key === key);
    if (r?.kind === 'item') {
      // Typing the app's own name back means "no name of my own".
      const name = text.trim() === r.item.base ? '' : text;
      const next = E.rename(draft, r.item, L, name, full);
      change(next, t('sb72.a.renamed', { name: E.labelOf(next, r.item, L, full) }));
    } else if (r?.kind === 'head') {
      change(E.renameGroup(draft, r.group, L, text), t('sb72.a.renamed', { name: text.trim() }));
    }
    if (refocus) void focusPart(key, 'name');
  }
  function nameKey(e: KeyboardEvent) {
    if (e.key === 'Enter') { e.preventDefault(); commitRename(true); }
    else if (e.key === 'Escape') { e.preventDefault(); const key = editing; editing = null; if (key) void focusPart(key, 'name'); }
    e.stopPropagation(); // typing never moves a row or leaves the editor
  }

  // ---------- icon ----------
  let picking = $state<string | null>(null);
  const iconName = (ic: string) => (t('sbe.ic.' + ic) !== 'sbe.ic.' + ic ? t('sbe.ic.' + ic) : ic);
  function pickIcon(it: Item, icon: string) {
    picking = null;
    change(E.setIcon(draft, it, icon, full));
    void focusPart('i:' + it.id, 'icon');
  }
  async function openPicker(it: Item) {
    commitRename();
    picking = picking === it.id ? null : it.id;
    if (!picking) return;
    await tick();
    rowEl('i:' + it.id)?.querySelector<HTMLElement>('.picker')?.scrollIntoView({ block: 'nearest' });
  }

  // ---------- the menu (right-click, or the "more" button) ----------
  let menu = $state.raw<{ x: number; y: number; label: string; items: MenuItem[]; run: (id: string) => void; back: HTMLElement | null } | null>(null);
  function openMenu(r: Row, x: number, y: number, back: HTMLElement | null) {
    commitRename();
    const at = view.rows.indexOf(r), last = view.rows.length - 1;
    if (r.kind === 'item') {
      const it = r.item, can = E.canRestyle(it, full);
      const list: MenuItem[] = [];
      if (can) list.push({ id: 'rename', label: t('sb72.m.rename'), icon: 'edit' }, { id: 'icon', label: t('sb72.m.icon'), icon: 'palette' });
      if (can && E.restyled(draft, it)) list.push({ id: 'usual', label: t('sb72.m.usual'), icon: 'undo' });
      list.push({ id: 'up', label: t('sb72.m.up'), icon: 'send', disabled: at <= 0, sep: list.length > 0 }, { id: 'down', label: t('sb72.m.down'), icon: 'arrowDown', disabled: at >= last });
      if (r.group) list.push({ id: 'to:', label: t('sb72.m.noGroup'), sep: true });
      (draft.groups ?? []).filter((g) => g.id !== r.group).forEach((g, i) => list.push({ id: 'to:' + g.id, label: t('sb72.m.toGroup', { name: E.groupName(g.label, L) }), sep: !r.group && i === 0 }));
      if ((draft.groups ?? []).length < G.maxGroups) list.push({ id: 'new', label: t('sb72.m.newGroup'), icon: 'plus', sep: !r.group && !(draft.groups ?? []).length });
      list.push({ id: 'start', label: t('sb72.m.start'), checked: E.isStart(draft, it) || (it.id === 'home' && !draft.start), sep: true });
      if (E.canHide(it, full)) list.push({ id: 'hide', label: t('sb72.m.hide'), icon: 'eyeOff' });
      menu = { x, y, back, label: t('sb72.menu', { name: nameOf(it) }), items: list, run: (id) => {
        if (id === 'rename') void startRename(r);
        else if (id === 'icon') void openPicker(it);
        else if (id === 'usual') change(E.usualLook(draft, it));
        else if (id === 'up' || id === 'down') { moveBy(r, id === 'up' ? -1 : 1); void focusPart(r.key, 'grip'); }
        else if (id === 'new') newGroup(it);
        else if (id.startsWith('to:')) { const g = id.slice(3) || null; const next = E.place(items, draft, it.id, { group: g, index: 1e9 }, full); change(next, sayPlace(it, next)); void focusPart(r.key, 'grip'); }
        else if (id === 'start') change(E.setStart(draft, E.isStart(draft, it) ? null : it, full));
        else if (id === 'hide') hide(it, true);
      } };
    } else {
      const groups = draft.groups ?? [], gi = groups.findIndex((g) => g.id === r.group);
      menu = { x, y, back, label: t('sb72.menu', { name: groupOf(r.group) }), items: [
        { id: 'rename', label: t('sb72.m.rename'), icon: 'edit' },
        { id: 'up', label: t('sb72.m.up'), icon: 'send', disabled: gi <= 0, sep: true }, { id: 'down', label: t('sb72.m.down'), icon: 'arrowDown', disabled: gi >= groups.length - 1 },
        { id: 'remove', label: t('sb72.m.removeGroup'), icon: 'close', sep: true },
      ], run: (id) => {
        if (id === 'rename') void startRename(r);
        else if (id === 'remove') removeGroup(r.group);
        else { moveBy(r, id === 'up' ? -1 : 1); void focusPart(r.key, 'grip'); }
      } };
    }
  }
  function menuAt(e: MouseEvent, r: Row) {
    if ((e.target as HTMLElement).closest('input')) return; // the field keeps the system's own menu
    e.preventDefault(); e.stopPropagation();
    openMenu(r, e.clientX, e.clientY, document.activeElement as HTMLElement | null);
  }
  function menuFrom(btn: HTMLElement, r: Row) {
    const b = btn.getBoundingClientRect();
    openMenu(r, getComputedStyle(btn).direction === 'rtl' ? b.right : b.left, b.bottom + 4, btn);
  }
  function closeMenu() { const back = menu?.back; menu = null; if (back?.isConnected) back.focus(); }

  // ---------- keyboard ----------
  function rowKey(e: KeyboardEvent, r: Row) {
    const tg = e.target as HTMLElement;
    if (tg.tagName === 'INPUT' || tg.closest('.picker')) return;
    const k = matchKey(e, 'sidebar-edit');
    if (k === 'sidebar-edit.up' || k === 'sidebar-edit.down') { e.preventDefault(); e.stopPropagation(); moveBy(r, k === 'sidebar-edit.up' ? -1 : 1); }
    else if (k === 'sidebar-edit.rename') { e.preventDefault(); void startRename(r); }
    else if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && !e.altKey && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      // Plain arrows walk the rows, staying on the same control.
      e.preventDefault(); e.stopPropagation();
      const d = e.key === 'ArrowUp' ? -1 : 1;
      const all = [...(root?.querySelectorAll<HTMLElement>('[data-key]') ?? [])], at = all.findIndex((x) => x.dataset.key === r.key);
      const to = all[at + d], part = tg.dataset.part ?? 'grip';
      (to?.querySelector<HTMLElement>(`[data-part="${part}"]`) ?? to?.querySelector<HTMLElement>('[data-part]'))?.focus();
    } else if (e.key === 'ContextMenu') { e.preventDefault(); menuFrom(tg, r); }
  }
  function rootKey(e: KeyboardEvent) {
    const typing = (e.target as HTMLElement).tagName === 'INPUT';
    if (e.key === 'Escape' && !menu) {
      e.preventDefault(); e.stopPropagation();
      if (typing) return; // the field took it (nameKey): it only ends the renaming
      if (picking) { const id = picking; picking = null; void focusPart('i:' + id, 'icon'); } else void done();
    } else if (!typing && matchKey(e, 'sidebar-edit') === 'sidebar-edit.undo') { e.preventDefault(); e.stopPropagation(); undoLast(); }
  }

  // ---------- dragging (pointer events; no library) ----------
  // The row that is held lifts and follows the pointer (a group's heading carries its pages along).
  // The other rows slide apart to open the place where it would land, which is marked, and the
  // group it would join lights up. Letting go applies the move with the rows already in place; the
  // held row settles from where it was let go. Escape puts everything back.
  type Box = { key: string; kind: 'item' | 'head'; group: string | null; top: number; h: number };
  type Held = { r: Row; x: number; y: number; cy: number; on: boolean; ins: HTMLElement[]; moving: Set<string>; scroll0: number; boxes: Box[]; raf: number };
  const ROW_GAP = 1; // the list's own gap between rows (`.rows`)
  let held: Held | null = null;
  let drag = $state.raw<{ key: string; kind: 'item' | 'head'; slot: number; target: string | null; gapTop: number; gapH: number } | null>(null);
  /** How far each row that is not held has slid to make room, by row key. */
  let shifts = $state.raw<Record<string, number>>({});
  /** Rows take their new places without gliding (they are already there): set while a drop lands. */
  let landing = $state(false);

  function down(e: PointerEvent, r: Row) {
    if (e.button !== 0 || held) return;
    const tg = e.target as HTMLElement;
    if (tg.closest('input, .picker')) return;
    if (e.pointerType === 'touch' && !tg.closest('.grip')) return; // a finger scrolls the list; only the handle drags
    held = { r, x: e.clientX, y: e.clientY, cy: e.clientY, on: false, ins: [], moving: new Set(), scroll0: 0, boxes: [], raf: 0 };
    window.addEventListener('pointermove', moveDrag);
    window.addEventListener('pointerup', upDrag);
    window.addEventListener('pointercancel', cancelDrag);
    window.addEventListener('keydown', keyDrag, true);
  }
  async function beginDrag() {
    if (!held || !listEl) return;
    held.on = true;
    if (editing || picking || menu) { commitRename(); picking = null; menu = null; await tick(); }
    if (!held || !listEl) return;
    const lr = listEl.getBoundingClientRect(), r = held.r;
    held.boxes = view.rows.map((x) => { const b = rowEl(x.key)!.getBoundingClientRect(); return { key: x.key, kind: x.kind, group: x.group, top: b.top - lr.top + listEl!.scrollTop, h: b.height }; });
    held.moving = new Set(view.rows.filter((x) => x.key === r.key || (r.kind === 'head' && x.group === r.group)).map((x) => x.key));
    held.ins = [...held.moving].map((k) => rowEl(k)!.querySelector<HTMLElement>('.in')!);
    held.scroll0 = listEl.scrollTop;
    paintDrag();
  }
  function moveDrag(e: PointerEvent) {
    if (!held) return;
    held.cy = e.clientY;
    if (!held.on) { if (Math.hypot(e.clientX - held.x, e.clientY - held.y) >= 5) void beginDrag(); return; }
    e.preventDefault();
    paintDrag();
    if (!held.raf) held.raf = requestAnimationFrame(edgeScroll);
  }
  /** Near the top or bottom edge of a list that scrolls, the list follows the pointer. */
  function edgeScroll() {
    if (!held?.on || !listEl) return;
    held.raf = 0;
    const lr = listEl.getBoundingClientRect(), near = 26;
    const v = held.cy < lr.top + near ? -Math.min(10, (lr.top + near - held.cy) / 3) : held.cy > lr.bottom - near ? Math.min(10, (held.cy - lr.bottom + near) / 3) : 0;
    if (!v) return;
    const was = listEl.scrollTop;
    listEl.scrollTop = was + v;
    if (listEl.scrollTop !== was) { paintDrag(); held.raf = requestAnimationFrame(edgeScroll); }
  }
  function paintDrag() {
    if (!held?.on || !listEl || !held.boxes.length) return;
    const { r, boxes, moving } = held;
    const lr = listEl.getBoundingClientRect();
    const y = held.cy - lr.top + listEl.scrollTop;
    const by = held.cy - held.y + listEl.scrollTop - held.scroll0;
    for (const el of held.ins) el.style.transform = `translateY(${by}px)`;
    const others = boxes.filter((b) => !moving.has(b.key)), mine = boxes.filter((b) => moving.has(b.key));
    let slot: number, at: number, target: string | null = null;
    if (r.kind === 'item') {
      slot = at = E.slotAt(others.map((b) => b.top + b.h / 2), y);
      target = E.placeAt(view.rows.filter((x) => x.key !== r.key), slot).group;
    } else {
      // A group moves as a block: the gaps are between the other groups, and after the last one.
      const blocks = (draft.groups ?? []).filter((g) => g.id !== r.group).map((g) => { const b = others.filter((x) => x.group === g.id); return { first: b[0].key, top: b[0].top, bottom: b[b.length - 1].top + b[b.length - 1].h }; });
      slot = E.slotAt(blocks.map((b) => (b.top + b.bottom) / 2), y);
      at = slot < blocks.length ? others.findIndex((b) => b.key === blocks[slot].first) : others.length;
    }
    // Where every row would sit if the held ones were let go here.
    const next: Record<string, number> = {};
    let top = boxes[0].top, gapTop = 0;
    for (const b of [...others.slice(0, at), ...mine, ...others.slice(at)]) { if (b === mine[0]) gapTop = top; if (!moving.has(b.key) && Math.abs(top - b.top) > 0.5) next[b.key] = top - b.top; top += b.h + ROW_GAP; }
    shifts = next;
    drag = { key: r.key, kind: r.kind, slot, target, gapTop, gapH: mine.reduce((n, b) => n + b.h + ROW_GAP, -ROW_GAP) };
  }
  function endDrag() {
    if (!held) return;
    if (held.raf) cancelAnimationFrame(held.raf);
    window.removeEventListener('pointermove', moveDrag);
    window.removeEventListener('pointerup', upDrag);
    window.removeEventListener('pointercancel', cancelDrag);
    window.removeEventListener('keydown', keyDrag, true);
    held = null; drag = null;
  }
  /** The held rows glide from where they were let go to where they now belong. */
  async function settleRows(ins: HTMLElement[], apply: () => void) {
    const from = ins.map((el) => el.getBoundingClientRect().top);
    apply();
    await tick();
    ins.forEach((el, i) => {
      el.style.transform = '';
      const d = from[i] - el.getBoundingClientRect().top;
      if (d && !still() && el.animate) el.animate([{ transform: `translateY(${d}px)` }, { transform: 'none' }], { duration: MOVE_MS, easing: 'cubic-bezier(.33, 1, .68, 1)' });
    });
  }
  function upDrag() {
    if (!held) return;
    const h = held, d = drag;
    endDrag();
    if (!h.on || !d) return;
    // The click that ends a drag is not a click on the name or the eye under the pointer.
    const swallow = (ev: MouseEvent) => { ev.stopPropagation(); ev.preventDefault(); };
    window.addEventListener('click', swallow, { capture: true, once: true });
    setTimeout(() => window.removeEventListener('click', swallow, true), 0);
    landing = true;
    void settleRows(h.ins, () => {
      shifts = {};
      if (h.r.kind === 'item') {
        const to = E.placeAt(view.rows.filter((x) => x.key !== h.r.key), d.slot);
        const next = E.place(items, draft, h.r.item.id, to, full);
        change(next, sayPlace(h.r.item, next));
      } else {
        const next = E.moveGroup(items, draft, h.r.group, d.slot, full);
        change(next, t('sb72.a.group', { name: groupOf(h.r.group), n: d.slot + 1, of: (draft.groups ?? []).length }));
      }
    }).then(() => requestAnimationFrame(() => (landing = false)));
  }
  /** Let go of nothing: the rows slide back and the held ones return to where they came from. */
  function cancelDrag() {
    if (!held) return;
    const h = held;
    endDrag();
    if (h.on) void settleRows(h.ins, () => { shifts = {}; });
  }
  function keyDrag(e: KeyboardEvent) { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); cancelDrag(); } }

  onMount(() => {
    const r = first && view.rows.find((x) => x.kind === 'item' && x.item.id === first!.id);
    if (r && r.kind === 'item') { if (first!.act === 'rename') void startRename(r); else void openPicker(r.item); }
    else root?.querySelector<HTMLElement>('.done')?.focus({ preventScroll: true });
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="sbe" class:dragging={!!drag} class:landing bind:this={root} role="group" aria-label={t('sbe.title')} onkeydown={rootKey}>
  <div class="bar">
    <h2 class="grow ell">{t('sbe.title')}</h2>
    <HelpTip text={t('sb72.help', { up: keys('sidebar-edit.up'), down: keys('sidebar-edit.down') })} label={t('sb72.helpLabel')} size={13} />
    <button class="ib" disabled={!undoN} aria-label={t('sb72.undo')} title={t('sb72.undo')} onclick={undoLast}><Icon name="undo" size={15} /></button>
    <button class="btn primary done" aria-busy={saving} onclick={done}>{t('sb72.done')}</button>
  </div>
  <div class="list" bind:this={listEl} role="list" aria-label={t('shell.mainNav')}>
    <div class="rows">
      {#each view.rows as r (r.key)}
        <div class="er" class:head={r.kind === 'head'} class:lift={!!drag && (drag.key === r.key || (drag.kind === 'head' && r.kind === 'item' && 'g:' + r.group === drag.key))}
          class:target={!!drag && drag.kind === 'item' && r.kind === 'head' && drag.target === r.group} data-key={r.key} role="listitem"
          animate:flip={{ duration: still() || landing ? 0 : MOVE_MS, easing: cubicOut }}>
          <div class="in" style:transform={shifts[r.key] ? `translateY(${shifts[r.key]}px)` : undefined}>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="line" onpointerdown={(e) => down(e, r)} onkeydown={(e) => rowKey(e, r)} oncontextmenu={(e) => menuAt(e, r)}>
            {#if r.kind === 'item'}
              {@const it = r.item}
              {@const name = nameOf(it)}
              <button class="grip" data-part="grip" aria-label={t('sb72.grip', { name })} aria-describedby="sbe-keys" title={t('sb72.grip', { name })}><Icon name="grip" size={14} /></button>
              {#if E.canRestyle(it, full)}
                <button class="ic" data-part="icon" aria-label={t('sb72.icon', { name })} title={t('sb72.m.icon')} aria-expanded={picking === it.id} onclick={() => openPicker(it)}><Icon name={it.icon} /></button>
                {#if editing === r.key}
                  <input class="nm" bind:value={text} placeholder={it.base} maxlength="40" dir="auto" aria-label={t('sb72.nameField', { name: it.base })} onkeydown={nameKey} onblur={() => commitRename()} />
                {:else}
                  <button class="name" data-part="name" aria-label={t('sb72.rename', { name })} title={t('sb72.m.rename')} onclick={() => startRename(r)}><span class="ell" dir="auto">{name}</span></button>
                {/if}
              {:else}
                <span class="ic"><Icon name={it.icon} /></span>
                <span class="name fixed"><span class="ell" dir="auto">{name}</span></span>
              {/if}
              {#if it.custom}<span class="mark" aria-hidden="true"><Icon name="plugin" size={12} /></span>{/if}
              <button class="ib more" data-part="more" aria-haspopup="menu" aria-label={t('sb72.more', { name })} onclick={(e) => menuFrom(e.currentTarget, r)}><Icon name="more" size={15} /></button>
              {#if E.isStart(draft, it)}<span class="mark start" title={t('sb72.startMark')}><Icon name="pin" size={13} /><span class="sr-only">({t('sb72.startMark')})</span></span>{/if}
              {#if E.canHide(it, full)}
                <button class="ib" data-part="eye" aria-label={t('sb72.hide', { name })} title={t('sb72.hide', { name })} onclick={() => hide(it, true)}><Icon name="eye" size={15} /></button>
              {:else}
                <span class="ib lock" title={t('sb72.locked')}><Icon name="lock" size={13} /><span class="sr-only">({t('sb72.locked')})</span></span>
              {/if}
            {:else}
              {@const name = groupOf(r.group)}
              <button class="grip" data-part="grip" aria-label={t('sb72.gripGroup', { name })} aria-describedby="sbe-keys" title={t('sb72.gripGroup', { name })}><Icon name="grip" size={14} /></button>
              {#if editing === r.key}
                <input class="nm" bind:value={text} maxlength="40" dir="auto" aria-label={t('sb72.groupField')} onkeydown={nameKey} onblur={() => commitRename()} />
              {:else}
                <button class="name" data-part="name" aria-label={t('sb72.rename', { name })} title={t('sb72.m.rename')} onclick={() => startRename(r)}><span class="ell" dir="auto">{name}</span></button>
              {/if}
              <button class="ib more" data-part="more" aria-haspopup="menu" aria-label={t('sb72.more', { name })} onclick={(e) => menuFrom(e.currentTarget, r)}><Icon name="more" size={15} /></button>
              <button class="ib" data-part="eye" aria-label={t('sb72.groupRemove', { name })} title={t('sb72.groupRemove', { name })} onclick={() => removeGroup(r.group)}><Icon name="close" size={14} /></button>
            {/if}
          </div>
          {#if r.kind === 'head' && !r.count}<div class="empty">{t('sb72.groupEmpty')}</div>{/if}
          {#if r.kind === 'item' && picking === r.item.id}
            {@const it = r.item}
            <div class="picker" role="group" aria-label={t('sb72.icons', { name: nameOf(it) })}>
              <button class="usual" aria-pressed={!draft.icons?.[it.id]} onclick={() => pickIcon(it, '')}>{t('sbe.iconUsual')}</button>
              {#each G.icons as ic (ic)}
                <button class="pi" aria-label={iconName(ic)} title={iconName(ic)} aria-pressed={draft.icons?.[it.id] === ic} onclick={() => pickIcon(it, ic)}><Icon name={ic} size={15} /></button>
              {/each}
            </div>
          {/if}
          </div>
        </div>
      {/each}
      {#if drag}<div class="drop" style:transform={`translateY(${drag.gapTop}px)`} style:height={drag.gapH + 'px'}></div>{/if}
    </div>
    {#if view.hidden.length}
      <div class="hh">{t('sb72.hidden')}</div>
      {#each view.hidden as it (it.id)}
        {@const name = nameOf(it)}
        <div class="er off" data-key={'i:' + it.id} role="listitem" animate:flip={{ duration: still() ? 0 : MOVE_MS, easing: cubicOut }}>
          <div class="line">
            <span class="grip none"></span>
            <span class="ic"><Icon name={it.icon} /></span>
            <span class="name fixed"><span class="ell" dir="auto">{name}</span></span>
            <button class="ib" data-part="eye" aria-label={t('sb72.show', { name })} title={t('sb72.show', { name })} onclick={() => hide(it, false)}><Icon name="eyeOff" size={15} /></button>
          </div>
        </div>
      {/each}
    {/if}
  </div>
  {#if problem}<p class="err" role="alert"><strong>{t('sb72.saveFailed')}</strong> <span dir="auto">{problem}</span></p>{/if}
  <div class="foot">
    <button class="q newgroup" disabled={(draft.groups ?? []).length >= G.maxGroups} title={(draft.groups ?? []).length >= G.maxGroups ? t('sb72.groupLimit', { n: G.maxGroups }) : undefined} onclick={() => newGroup()}><Icon name="plus" size={13} /> {t('sb72.newGroup')}</button>
    <span class="grow"></span>
    <button class="q" disabled={!Object.keys(E.cleanNav(draft, full)).length} onclick={reset}>{t('sb72.reset')}</button>
  </div>
  <span class="sr-only" id="sbe-keys">{t('sb72.keys', { up: keys('sidebar-edit.up'), down: keys('sidebar-edit.down') })}</span>
  <div class="sr-only" role="status" aria-live="polite">{announce}</div>
</div>
{#if menu}<NavMenu x={menu.x} y={menu.y} label={menu.label} items={menu.items} pick={menu.run} close={closeMenu} />{/if}

<style>
  /* The editor sits where the list sat and reads as the same list, lifted onto a faint panel so it
     is plain that the sidebar is being edited. Rows keep the height and type of the usual entries. */
  .sbe { flex: 0 1 auto; min-height: 150px; display: flex; flex-direction: column; margin: 8px -4px; padding: 4px;
    border-radius: var(--radius-lg); background: var(--fill); border: var(--border-w) solid var(--hairline); font-size: 13px; }
  .bar { flex: none; display: flex; align-items: center; gap: 2px; padding: 2px 2px 4px; padding-inline-start: 8px; }
  .bar h2 { margin: 0; font-size: var(--fs-xs); font-weight: 600; color: var(--muted); letter-spacing: 0; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .done { font-size: var(--fs-xs); padding: 3px 10px; margin-inline-start: 4px; }
  .list { flex: 1 1 auto; min-height: 0; overflow-y: auto; }
  .rows { position: relative; display: flex; flex-direction: column; gap: 1px; }

  .er { position: relative; }
  .line { display: flex; align-items: center; gap: 1px; height: 30px; border-radius: 6px; padding-inline-end: 2px; user-select: none; -webkit-user-select: none; }
  .line:hover, .line:focus-within { background: var(--fill); }
  .grip { font: inherit; flex: none; width: 18px; height: 26px; display: grid; place-items: center; padding: 0; border: 0; border-radius: 5px; background: none; color: var(--muted); opacity: .5; cursor: grab; touch-action: none; }
  .grip.none { cursor: default; }
  .line:hover .grip, .grip:focus-visible { opacity: 1; }
  .ic { font: inherit; flex: none; width: 26px; height: 26px; display: grid; place-items: center; padding: 0; border: 0; border-radius: 6px; background: none; color: var(--muted); }
  button.ic { cursor: pointer; }
  button.ic:hover, button.ic[aria-expanded="true"] { background: var(--fill-2); color: var(--fg); }
  .name { font: inherit; font-size: 13px; flex: 1; min-width: 0; display: flex; align-items: center; height: 24px; padding: 0 5px; margin-inline-start: 1px; border: 0; border-radius: 5px; background: none; color: var(--fg); text-align: start; }
  button.name { cursor: text; }
  /* The name is a field in waiting: an edge on hover says it can be typed in. */
  button.name:hover { box-shadow: inset 0 0 0 1px var(--control-border, var(--hairline)); background: var(--bg); }
  .nm { flex: 1; min-width: 0; height: 24px; padding: 0 5px; margin-inline-start: 1px; font-size: 13px; border-radius: 5px; user-select: text; -webkit-user-select: text; }
  .mark { flex: none; display: inline-flex; color: var(--muted); opacity: .8; margin-inline-end: 3px; }
  .mark.start { color: var(--accent); opacity: 1; }
  .ib { font: inherit; flex: none; width: 24px; height: 24px; display: grid; place-items: center; padding: 0; border: 0; border-radius: 6px; background: none; color: var(--muted); cursor: pointer; }
  .ib:hover:not(:disabled) { background: var(--fill-2); color: var(--fg); }
  .ib:disabled { opacity: .35; cursor: default; }
  .ib.lock { cursor: default; opacity: .55; }
  .ib.lock:hover { background: none; color: var(--muted); }
  /* "More" stays out of the way until the row is pointed at or holds the focus. */
  .more { opacity: 0; }
  .line:hover .more, .line:focus-within .more { opacity: 1; }
  .grip:focus-visible, .ic:focus-visible, .name:focus-visible, .ib:focus-visible, .q:focus-visible, .pi:focus-visible, .usual:focus-visible { outline-offset: -2px; }

  .head .line { height: 26px; margin-top: 8px; }
  .head .name { font-size: var(--fs-xxs, 11px); font-weight: 600; letter-spacing: .02em; color: var(--muted); }
  .head.target .name { color: var(--accent); }
  .empty { margin: 1px 4px 3px; margin-inline-start: 20px; padding: 5px 8px; border: 1px dashed var(--control-border, var(--hairline)); border-radius: 6px; font-size: var(--fs-xs); color: var(--muted); }
  .head.target .empty { border-color: var(--accent); color: var(--on-tint); background: var(--tint); }

  /* Dragging: the held row lifts off the list and the others slide apart; a soft outline marks
     the place it would land in. */
  .in { transition: transform 170ms cubic-bezier(.33, 1, .68, 1); }
  .lift .in, .landing .in { transition: none; }
  .dragging { cursor: grabbing; }
  .dragging .line { pointer-events: none; }
  .dragging .line:hover { background: none; }
  .lift { z-index: 3; }
  .lift .line, .dragging .lift .line:hover { background: var(--panel); box-shadow: var(--shadow-lg), 0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent); }
  .lift .grip { opacity: 1; }
  .lift .more { opacity: 0 !important; }
  .drop { position: absolute; top: 0; inset-inline: 0; border-radius: 6px; background: color-mix(in srgb, var(--accent) 9%, transparent); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 40%, transparent); pointer-events: none;
    transition: transform 170ms cubic-bezier(.33, 1, .68, 1); }

  .hh { font-size: var(--fs-xxs, 11px); font-weight: 600; letter-spacing: .02em; color: var(--muted); margin-top: 8px; padding: 8px 8px 2px; border-top: 1px solid var(--hairline); }
  .off .name, .off .ic { color: var(--muted); }
  .off .ic { opacity: .7; }

  .picker { display: grid; grid-template-columns: repeat(auto-fill, minmax(28px, 1fr)); gap: 2px; margin: 2px 2px 6px; padding: 6px; border-radius: var(--radius);
    background: var(--panel); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow); }
  .pi, .usual { font: inherit; border: 0; background: none; color: var(--fg); border-radius: 6px; cursor: pointer; }
  .pi { height: 28px; display: grid; place-items: center; padding: 0; color: var(--muted); }
  .usual { grid-column: 1 / -1; text-align: start; font-size: var(--fs-xs); padding: 4px 6px; margin-bottom: 2px; }
  .pi:hover, .usual:hover { background: var(--fill); color: var(--fg); }
  .pi[aria-pressed="true"], .usual[aria-pressed="true"] { background: var(--tint); color: var(--on-tint); }

  .err { flex: none; margin: 4px 6px 0; font-size: var(--fs-xs); color: var(--danger); white-space: pre-line; }
  .foot { flex: none; display: flex; align-items: center; gap: 4px; margin-top: 4px; padding-top: 4px; border-top: 1px solid var(--hairline); }
  .q { font: inherit; font-size: var(--fs-xs); display: inline-flex; align-items: center; gap: 4px; padding: 4px 7px; border: 0; border-radius: 6px; background: none; color: var(--muted); cursor: pointer; }
  .q:hover:not(:disabled) { background: var(--fill-2); color: var(--fg); }
  .q:disabled { opacity: .45; cursor: default; }

  @media (prefers-reduced-motion: reduce) { .drop, .in { transition: none; } }
  :global([data-motion="reduce"]) .drop, :global([data-motion="reduce"]) .in { transition: none; }
  /* High contrast: no shadows there, so the held row gets a firm edge instead. */
  :global([data-theme="contrast"]) .lift .line { outline: 2px solid var(--accent); outline-offset: -2px; }
</style>
