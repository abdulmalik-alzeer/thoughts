<script lang="ts">
  // The note editor: a plain text box with writing aids for people who do not know Markdown. A
  // toolbar and shortcuts (text style, bold, italic, link, bulleted / numbered lists, checklists,
  // indent, pictures), lists that continue on Enter and end on an empty item, Tab / Shift+Tab to move
  // list items in and out, pictures pasted or dropped in (saved beside the note), a web address pasted
  // over selected words makes a link, and the formatted view beside it scrolls in step.
  //
  // What is saved stays plain Markdown as the user would have typed it: each action is ONE native
  // edit of the lines it is about (so Undo and Redo work as usual), the note details and code are left
  // alone, and the file's own line endings are kept (the text box only knows "\n"). The text changes
  // themselves are plain functions in editing.ts, checked by editing.test.ts (npm test).
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import { Link, List, ListOrdered, ListTodo, ListIndentDecrease, ListIndentIncrease, ImagePlus } from '@lucide/svelte';
  import { md, mdClick, resolveLink } from '../../lib/md';
  import { zoomImages } from '../../lib/zoomImages';
  import { post } from '../../lib/api';
  import { t, tn, lang } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import { fmtSize, notify, rawBlobUrl, readAsDataURL } from '../../lib/content/util';
  import { isMac, isolatedKeys, tip } from '../../lib/shortcuts';
  import { matchKey, ariaKeys as ariaOf, iso as isoK, label as keyName } from '../../lib/keys/registry';
  import { trapFocus } from '../../lib/focus';
  import * as E from './editing';
  import DictateButton from '../../lib/voice/DictateButton.svelte';

  let { value = $bindable(''), path, plain = false, preview = false, label }: {
    /** The file's text, with its own line endings. */
    value: string;
    /** The note being edited (pictures are saved beside it and resolved from it). */
    path: string;
    /** A text file that is not a note (CSV, JSON, plain text): no writing aids, no formatted view. */
    plain?: boolean;
    preview?: boolean;
    label: string;
  } = $props();

  const uid = `ed${Math.random().toString(36).slice(2, 8)}`;
  let ta = $state<HTMLTextAreaElement>();
  let pv = $state<HTMLElement>();
  let picker = $state<HTMLInputElement>();
  let toolsEl = $state<HTMLElement>();
  /** What the text box shows ("\n" line breaks): drives the formatted view and the toolbar. */
  let shown = $state('');
  let kind = $state<{ heading: number; list: E.ListKind | null; plain: boolean }>({ heading: 0, list: null, plain: false });
  let status = $state('');
  let adding = $state(0);
  let dropping = $state(false);
  let linking = $state<{ s: number; e: number; label: string; address: string; old: boolean } | null>(null);
  const aids = $derived(!plain);
  const showPv = $derived(preview && !plain);

  // ---------- the text box and `value` ----------

  // `value` changed from outside (opened, restored, reloaded): show it, keeping the caret where it was.
  $effect(() => {
    const v = value, el = ta;
    if (!el) return;
    untrack(() => {
      const lf = E.toLF(v);
      if (el.value === lf) { if (shown !== lf) shown = lf; return; }
      const [s, e] = [el.selectionStart, el.selectionEnd];
      el.value = lf;
      shown = lf;
      el.setSelectionRange(Math.min(s, lf.length), Math.min(e, lf.length));
      dirtyAnchors = true;
    });
  });

  function onInput() {
    if (!ta) return;
    const lf = ta.value;
    shown = lf;
    value = E.restoreEol(value, lf);
    dirtyAnchors = true;
    queueKind();
  }

  /** Make `r` the text as one native edit (one Undo step), then select what it says. */
  function apply(r: E.Change | null) {
    const el = ta;
    if (!r || !el) return;
    const d = E.diff(el.value, r.text);
    el.focus({ preventScroll: true });
    if (d.from !== d.to || d.insert) {
      el.setSelectionRange(d.from, d.to);
      let ok = false;
      try { ok = document.execCommand(d.insert ? 'insertText' : 'delete', false, d.insert); } catch { /* not supported */ }
      if (!ok || el.value !== r.text) {
        // A web view without editing commands: the same change, without its own Undo step.
        el.value = r.text;
        el.dispatchEvent(new Event('input', { bubbles: true }));
      }
    }
    el.setSelectionRange(r.start, r.end);
    queueKind();
  }
  const sel = () => [ta!.selectionStart, ta!.selectionEnd] as const;

  // ---------- what the caret is on (for the toolbar) ----------

  let kindFrame = 0;
  function queueKind() {
    if (kindFrame || plain) return;
    kindFrame = requestAnimationFrame(() => { kindFrame = 0; if (ta) kind = E.lineKind(ta.value, ta.selectionStart); });
  }
  function onSelectionChange() { if (ta && document.activeElement === ta) queueKind(); }

  // ---------- keys ----------

  let composing = false;
  /** Escape was the last key: the next Tab leaves the text box instead of indenting. */
  let tabLeaves = false;
  /** A command's keys, isolated so they read correctly inside Arabic text. */
  const keyLabel = (id: string) => isolatedKeys('docs.editor.' + id);
  const keyTip = (label: string, id: string) => tip(label, 'docs.editor.' + id);
  const ariaKeys = (id: string) => ariaOf('docs.editor.' + id);

  function onKeydown(e: KeyboardEvent) {
    // Never while an input method (Chinese, Japanese, Korean…) is composing: its Enter picks a word.
    if (e.isComposing || composing || e.keyCode === 229) return;
    if (['Shift', 'Control', 'Alt', 'Meta', 'CapsLock'].includes(e.key)) return;
    const leave = tabLeaves;
    tabLeaves = e.key === 'Escape';
    if (!aids || !ta) return;
    const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
    const [s, en] = sel();
    if (e.key === 'Tab' && !mod && !e.altKey && !e.ctrlKey && !e.metaKey) {
      if (leave) return;
      const r = E.indent(ta.value, s, en, e.shiftKey ? -1 : 1);
      if (r) { e.preventDefault(); apply(r); }
      return;
    }
    if (e.key === 'Enter' && !mod && !e.altKey && !e.shiftKey && !e.ctrlKey && !e.metaKey) {
      const r = E.enter(ta.value, s, en);
      if (r) { e.preventDefault(); apply(r); }
      return;
    }
    if (e.altKey && e.getModifierState?.('AltGraph')) return;
    // The keys are the user's (Settings › Keyboard, lib/keys/registry.ts); physical keys, so they
    // also work with an Arabic keyboard.
    const id = matchKey(e, 'docs.editor');
    if (!id) return;
    const level = /^docs\.editor\.heading(\d)$/.exec(id);
    if (level) { e.preventDefault(); heading(+level[1]); return; }
    const k = ({ 'docs.editor.list.number': 'number', 'docs.editor.list.bullet': 'bullet', 'docs.editor.list.task': 'task' } as const)[id as 'docs.editor.list.number'];
    if (k) { e.preventDefault(); list(k); return; }
    if (id === 'docs.editor.bold') { e.preventDefault(); inline('**'); }
    else if (id === 'docs.editor.italic') { e.preventDefault(); inline('*'); }
    // The link key on selected words (or inside a link) makes a link; otherwise ⌘K still opens the palette.
    else if (id === 'docs.editor.link' && (s !== en || E.linkAt(ta.value, s, en))) { e.preventDefault(); openLink(); }
  }

  // ---------- toolbar actions ----------

  function inline(mark: '**' | '*') { if (ta) { const [s, e] = sel(); apply(E.toggleInline(ta.value, s, e, mark)); } }
  function list(k: E.ListKind) { if (ta) { const [s, e] = sel(); apply(E.toggleList(ta.value, s, e, k)); } }
  function heading(level: number): boolean {
    if (!ta) return false;
    const [s, e] = sel();
    const r = E.setHeading(ta.value, s, e, level);
    if (r) apply(r); else ta.focus();
    return !!r;
  }
  function shift(dir: 1 | -1) {
    if (!ta) return;
    const [s, e] = sel();
    const r = E.indent(ta.value, s, e, dir);
    if (r) apply(r);
    else { ta.focus(); if (!kind.list) say(t('edtr.listOnly')); }
  }

  function openLink() {
    if (!ta) return;
    let [s, e] = sel();
    const v = ta.value;
    if (E.plainZone(v, s)) return;
    const old = E.linkAt(v, s, e);
    if (!old && s === e) { const w = E.wordAt(v, s); if (w) [s, e] = w; }
    const words = old ? old.label : v.slice(s, e);
    if (!old && words.includes('\n')) { notify(t('edtr.linkOneLine'), { kind: 'info' }); return; }
    const url = !old && E.isUrl(words);
    linking = { s, e, label: url ? '' : words, address: old ? old.target : url ? words.trim() : '', old: !!old };
  }
  async function closeLink(how: 'set' | 'unlink' | null) {
    const l = linking;
    linking = null;
    await tick();
    if (!l || !how || !ta) { ta?.focus(); return; }
    ta.setSelectionRange(l.s, l.e);
    apply(E.setLink(ta.value, l.s, l.e, how === 'unlink' ? '' : l.label, how === 'unlink' ? '' : l.address));
  }

  // ---------- pictures ----------

  const SHOWN_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/webp'];
  const PICTURE_MAX = 15 * 1024 * 1024;
  const isPicture = (f: File) => f.type.startsWith('image/') || /\.(png|jpe?g|gif|webp|bmp|tiff?|heic|heif|avif)$/i.test(f.name);
  /** Names a pasted picture gets from the system ("image.png"): replaced by a dated one. */
  const GENERIC = /^(image|img|blob|untitled|clipboard|pasted[\s_-]*(image|graphic)?|picture)?([\s_-]*\d+)?$/i;

  function pictureName(f: File): string {
    const stem = f.name.replace(/\.[a-z0-9]{2,5}$/i, '').trim();
    if (stem && !GENERIC.test(stem)) return f.name;
    const d = new Date(), p = (n: number) => String(n).padStart(2, '0');
    return `picture-${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
  }
  function altFor(f: File): string {
    const stem = f.name.replace(/\.[a-z0-9]{2,5}$/i, '').trim();
    return stem && !GENERIC.test(stem) ? stem.replace(/[_-]+/g, ' ').trim() : t('edtr.pictureAlt');
  }

  /** A picture in a kind every system shows; others the web view can read (HEIC, TIFF…) become PNG. */
  async function shownKind(f: File): Promise<Blob> {
    if (SHOWN_TYPES.includes(f.type) || (!f.type && /\.(png|jpe?g|gif|webp)$/i.test(f.name))) return f;
    try {
      const bmp = await createImageBitmap(f);
      const c = document.createElement('canvas');
      c.width = bmp.width; c.height = bmp.height;
      c.getContext('2d')!.drawImage(bmp, 0, 0);
      bmp.close?.();
      const b = await new Promise<Blob | null>((res) => c.toBlob(res, 'image/png'));
      if (b) return b;
    } catch { /* the web view cannot read it either */ }
    throw new Error(t('edtr.pictureKind'));
  }

  /** Save the pictures beside the note and link them where the caret was. */
  async function addPictures(files: File[]) {
    const el = ta;
    const pics = files.filter(isPicture);
    if (!el || !pics.length) return;
    if (pics.length < files.length) notify(t('edtr.notPictures'), { kind: 'info' });
    const before = el.value, [s0, e0] = sel();
    adding += pics.length;
    say(tn('edtr.adding', adding));
    const blocks: string[] = [];
    for (const f of pics) {
      try {
        const blob = await shownKind(f);
        if (blob.size > PICTURE_MAX) throw new Error(t('edtr.pictureLarge', { size: fmtSize(blob.size) }));
        const data = (await readAsDataURL(blob)).split(',').pop() || '';
        const r = await post<{ link: string }>('/library/picture', { note: path, name: pictureName(f), dataBase64: data });
        blocks.push(E.pictureMarkdown(altFor(f), r.link));
      } catch (err) { failed(t('edtr.pictureFailed'), err); }
      finally { adding--; }
    }
    if (!adding) say('');
    if (!blocks.length || !ta?.isConnected) return;
    // Where the caret was when the picture came in; where it is now if the text changed meanwhile.
    const [s, e] = ta.value === before ? [s0, e0] : sel();
    apply(E.insertBlocks(ta.value, s, e, blocks));
    say(tn('edtr.added', blocks.length));
  }

  function onPaste(e: ClipboardEvent) {
    const dt = e.clipboardData;
    if (!aids || !dt || !ta) return;
    const text = dt.getData('text/plain');
    // Some web views list a pasted picture only among the items.
    const files = dt.files?.length ? Array.from(dt.files)
      : Array.from(dt.items ?? []).filter((i) => i.kind === 'file').map((i) => i.getAsFile()).filter((f): f is File => !!f);
    // A spreadsheet or a word processor copies a picture of the cells along with their text: the text
    // wins. Files copied from the file manager come with their names as the text.
    const names = files.map((f) => f.name);
    const onlyNames = !text.trim() || text.split(/\r?\n/).every((l) => !l.trim() || names.includes(l.trim()) || names.some((n) => l.trim().endsWith('/' + n)));
    if (files.some(isPicture) && onlyNames) { e.preventDefault(); addPictures(files); return; }
    const [s, en] = sel();
    const r = E.pasteLink(ta.value, s, en, text);
    if (r) { e.preventDefault(); apply(r); }
  }

  // Pictures dragged onto the text go into the note; anything else is left to the file list's import.
  let dragDepth = 0;
  const pictureDrag = (e: DragEvent) => {
    const dt = e.dataTransfer;
    if (!aids || !dt || !Array.from(dt.types ?? []).includes('Files')) return false;
    const items = Array.from(dt.items ?? []).filter((i) => i.kind === 'file');
    return items.length > 0 && items.every((i) => i.type.startsWith('image/'));
  };
  function onDragEnter(e: DragEvent) { if (!pictureDrag(e)) return; e.preventDefault(); e.stopPropagation(); dragDepth++; dropping = true; }
  function onDragOver(e: DragEvent) { if (!pictureDrag(e)) return; e.preventDefault(); e.stopPropagation(); if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'; }
  function onDragLeave(e: DragEvent) { if (!pictureDrag(e)) return; e.stopPropagation(); dragDepth = Math.max(0, dragDepth - 1); if (!dragDepth) dropping = false; }
  function onDrop(e: DragEvent) {
    if (!dropping) return;
    e.preventDefault(); e.stopPropagation();
    dragDepth = 0; dropping = false;
    addPictures(Array.from(e.dataTransfer?.files ?? []));
  }
  function pick(e: Event) { const el = e.target as HTMLInputElement; const f = Array.from(el.files ?? []); el.value = ''; addPictures(f); }

  let statusTimer: ReturnType<typeof setTimeout> | undefined;
  function say(msg: string) {
    clearTimeout(statusTimer);
    status = msg;
    if (msg && !adding) statusTimer = setTimeout(() => (status = ''), 4000);
  }

  // ---------- the formatted view ----------

  let pvHtml = $state('');
  let pvTimer: ReturnType<typeof setTimeout> | undefined;
  /** Pictures in the note are library files: fetched once each, shown from a local address. */
  const pics = new Map<string, string>();
  const fetching = new Set<string>();
  $effect(() => {
    const txt = shown, on = showPv;
    if (!on) return;
    clearTimeout(pvTimer);
    pvTimer = setTimeout(() => untrack(() => { pvHtml = withPictures(md(txt)); afterRender(); }), untrack(() => pvHtml) ? 120 : 0);
  });

  function withPictures(html: string): string {
    // `html` is md()'s sanitised output. Parsed in a document of its own, where nothing loads or runs.
    const doc = new DOMParser().parseFromString(`<!doctype html><body>${html}`, 'text/html');
    for (const img of Array.from(doc.body.querySelectorAll('img'))) {
      const src = img.getAttribute('src') || '';
      if (!src || /^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(src)) continue;
      const p = resolveLink(path, src);
      const u = pics.get(p);
      if (u) img.setAttribute('src', u);
      else { img.removeAttribute('src'); img.setAttribute('data-pic', p); }
    }
    return doc.body.innerHTML;
  }

  async function afterRender() {
    await tick();
    if (!pv) return;
    for (const img of Array.from(pv.querySelectorAll<HTMLImageElement>('img[data-pic]'))) {
      const p = img.getAttribute('data-pic')!;
      if (pics.has(p)) { img.src = pics.get(p)!; continue; }
      if (fetching.has(p)) continue;
      fetching.add(p);
      rawBlobUrl(p).then((u) => {
        fetching.delete(p);
        if (!u) return; // missing picture
        if (destroyed) { URL.revokeObjectURL(u); return; }
        pics.set(p, u);
        for (const i of Array.from(pv?.querySelectorAll<HTMLImageElement>('img[data-pic]') ?? [])) if (i.getAttribute('data-pic') === p) i.src = u;
      });
    }
    dirtyAnchors = true;
    follow();
  }

  // ---------- scrolling in step ----------
  // Each block of the formatted view is paired with the line it starts on; scrolling either side
  // moves the other to the same place between the two nearest pairs.

  let anchors: { e: number; p: number }[] = [];
  let dirtyAnchors = true;
  let expectTa: number | null = null, expectPv: number | null = null;

  const FITS: Record<string, (el: Element) => boolean> = {
    heading: (el) => /^H[1-6]$/.test(el.tagName),
    paragraph: (el) => el.tagName === 'P',
    list: (el) => el.tagName === 'UL' || el.tagName === 'OL',
    code: (el) => el.classList.contains('md-code'),
    table: (el) => el.classList.contains('md-table'),
    blockquote: (el) => el.tagName === 'BLOCKQUOTE',
    hr: (el) => el.tagName === 'HR',
  };

  /** How far down the text box each of `lines` starts, wrapped lines included (measured on a hidden copy). */
  function lineTops(text: string, lines: number[]): number[] {
    const el = ta!, cs = getComputedStyle(el);
    const m = document.createElement('div');
    for (const k of ['fontFamily', 'fontSize', 'fontWeight', 'fontStyle', 'lineHeight', 'letterSpacing', 'wordSpacing', 'tabSize', 'textTransform',
      'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'direction', 'unicodeBidi', 'textAlign', 'wordBreak', 'overflowWrap'] as const) {
      (m.style as any)[k] = (cs as any)[k];
    }
    const padX = (parseFloat(cs.paddingLeft) || 0) + (parseFloat(cs.paddingRight) || 0);
    Object.assign(m.style, { position: 'fixed', visibility: 'hidden', pointerEvents: 'none', top: '0', left: '0', whiteSpace: 'pre-wrap', boxSizing: 'content-box',
      height: 'auto', border: '0', width: Math.max(1, el.clientWidth - padX) + 'px' });
    const want = new Set(lines), marks = new Map<number, HTMLElement>();
    let buf = '';
    text.split('\n').forEach((l, i, all) => {
      if (want.has(i)) {
        if (buf) m.append(buf);
        buf = '';
        const s = document.createElement('span');
        m.append(s);
        marks.set(i, s);
      }
      buf += l + (i < all.length - 1 ? '\n' : '');
    });
    m.append(buf + '\u200b');
    document.body.append(m);
    const pad = parseFloat(cs.paddingTop) || 0;
    const tops = lines.map((i) => (marks.get(i)?.offsetTop ?? 0) - pad);
    m.remove();
    return tops;
  }

  function buildAnchors() {
    dirtyAnchors = false;
    anchors = [];
    if (!ta || !pv) return;
    const eMax = ta.scrollHeight - ta.clientHeight, pMax = pv.scrollHeight - pv.clientHeight;
    if (eMax <= 0 || pMax <= 0) return;
    const kids = Array.from(pv.children) as HTMLElement[];
    const pairs: { line: number; el: HTMLElement }[] = [];
    let j = 0;
    for (const b of E.blockStarts(shown)) {
      const fits = FITS[b.kind];
      if (!fits) continue; // raw HTML may show as several blocks or none: no pair
      for (let k = j; k < Math.min(kids.length, j + 4); k++) if (fits(kids[k])) { pairs.push({ line: b.line, el: kids[k] }); j = k + 1; break; }
    }
    const tops = lineTops(shown, pairs.map((x) => x.line));
    const pad = parseFloat(getComputedStyle(pv).paddingTop) || 0;
    const pts = [{ e: 0, p: 0 }];
    pairs.forEach((x, i) => {
      const e = tops[i], p = x.el.offsetTop - pad, last = pts[pts.length - 1];
      if (e > last.e && p > last.p && e < eMax && p < pMax) pts.push({ e, p });
    });
    pts.push({ e: eMax, p: pMax });
    anchors = pts;
  }

  function across(v: number, from: 'e' | 'p', to: 'e' | 'p'): number {
    if (dirtyAnchors) buildAnchors();
    const a = anchors;
    if (a.length < 2) return 0;
    let k = 0;
    while (k < a.length - 2 && v > a[k + 1][from]) k++;
    const span = a[k + 1][from] - a[k][from] || 1;
    const f = Math.min(1, Math.max(0, (v - a[k][from]) / span));
    return a[k][to] + f * (a[k + 1][to] - a[k][to]);
  }

  /** A picture came in and moved what is below it. */
  let loadFrame = 0;
  function pictureLoaded() {
    dirtyAnchors = true;
    if (!loadFrame) loadFrame = requestAnimationFrame(() => { loadFrame = 0; follow(); });
  }

  /** The formatted view follows the text box. */
  function follow() {
    if (!showPv || !ta || !pv) return;
    const y = across(ta.scrollTop, 'e', 'p');
    if (Math.abs(pv.scrollTop - y) < 1) return;
    pv.scrollTop = y;
    expectPv = pv.scrollTop;
  }
  function onTaScroll() {
    if (expectTa !== null && ta && Math.abs(ta.scrollTop - expectTa) < 2) { expectTa = null; return; }
    expectTa = null;
    follow();
  }
  function onPvScroll() {
    if (expectPv !== null && pv && Math.abs(pv.scrollTop - expectPv) < 2) { expectPv = null; return; }
    expectPv = null;
    if (!ta || !pv) return;
    const y = across(pv.scrollTop, 'p', 'e');
    if (Math.abs(ta.scrollTop - y) < 1) return;
    ta.scrollTop = y;
    expectTa = ta.scrollTop;
  }

  // ---------- toolbar keyboard (one stop in the Tab order, arrows move along it) ----------

  let active = $state(0);
  const toolItems = () => Array.from(toolsEl?.querySelectorAll<HTMLElement>('[data-tool]') ?? []);
  function toolKey(e: KeyboardEvent) {
    const items = toolItems();
    const i = items.indexOf(e.target as HTMLElement);
    if (i < 0) return;
    const rtl = getComputedStyle(toolsEl!).direction === 'rtl';
    let n = -1;
    if (e.key === 'ArrowRight') n = rtl ? i - 1 : i + 1;
    else if (e.key === 'ArrowLeft') n = rtl ? i + 1 : i - 1;
    else if (e.key === 'Home') n = 0;
    else if (e.key === 'End') n = items.length - 1;
    else return;
    e.preventDefault();
    n = (n + items.length) % items.length;
    active = n;
    items[n].focus();
  }
  const keep = (e: MouseEvent) => { if (document.activeElement === ta) e.preventDefault(); }; // a click keeps the caret in the text

  // ---------- lifetime ----------

  let destroyed = false;
  let ro: ResizeObserver | undefined;
  onMount(() => {
    document.addEventListener('selectionchange', onSelectionChange);
    ro = new ResizeObserver(() => { dirtyAnchors = true; });
    if (ta) {
      ro.observe(ta);
      // Straight into writing: a short (new) note with the caret at its end.
      ta.focus({ preventScroll: true });
      if (ta.value.length < 500) ta.setSelectionRange(ta.value.length, ta.value.length);
      queueKind();
    }
  });
  $effect(() => { const el = pv; if (el && ro) { ro.observe(el); return () => ro?.unobserve(el); } });
  onDestroy(() => {
    destroyed = true;
    document.removeEventListener('selectionchange', onSelectionChange);
    ro?.disconnect();
    clearTimeout(pvTimer); clearTimeout(statusTimer);
    if (kindFrame) cancelAnimationFrame(kindFrame);
    if (loadFrame) cancelAnimationFrame(loadFrame);
    for (const u of pics.values()) URL.revokeObjectURL(u);
  });

  /** The note's own direction (from its first letter, note details left out): the toolbar mirrors for a
   *  right-to-left note, and lines with no letters of their own (an empty line, a new "3. ") follow it.
   *  Lines with letters still each take their own direction. */
  const dir = $derived(E.textDir(shown) || (lang() === 'ar' ? 'rtl' : 'ltr'));
  const hint = $derived(t('edtr.hint', { b: keyLabel('bold'), i: keyLabel('italic'), k: keyLabel('link') }));
  /** The link dialog sits on the page itself (the Docs column is a size container). */
  function portal(node: HTMLElement) { document.body.appendChild(node); return { destroy() { node.remove(); } }; }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="ed" class:two={showPv} ondragenter={onDragEnter} ondragover={onDragOver} ondragleave={onDragLeave} ondrop={onDrop}>
  <div class="col">
    {#if aids}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="tools no-print" role="toolbar" aria-label={t('edtr.toolbar')} aria-controls={uid} dir={dir} bind:this={toolsEl} onkeydown={toolKey}
        onfocusin={(e) => { const i = toolItems().indexOf(e.target as HTMLElement); if (i >= 0) active = i; }}>
        <select class="style" data-tool tabindex={active === 0 ? 0 : -1} aria-label={t('edtr.style')} title={t('edtr.style')}
          value={String(Math.min(kind.heading, 4))}
          onchange={(e) => { const el = e.currentTarget; if (!heading(Number(el.value))) el.value = String(Math.min(kind.heading, 4)); }}>
          <option value="0">{t('edtr.normal')}</option>
          <option value="1">{t('edtr.h1')}</option>
          <option value="2">{t('edtr.h2')}</option>
          <option value="3">{t('edtr.h3')}</option>
          {#if kind.heading > 3}<option value="4" disabled>{t('edtr.hSmall')}</option>{/if}
        </select>
        <span class="sep" aria-hidden="true"></span>
        <button type="button" class="tb glyph b" data-tool tabindex={active === 1 ? 0 : -1} onmousedown={keep} onclick={() => inline('**')}
          aria-label={t('edtr.bold')} title={keyTip(t('edtr.bold'), 'bold')} aria-keyshortcuts={ariaKeys('bold')}><span aria-hidden="true">{t('edtr.boldGlyph')}</span></button>
        <button type="button" class="tb glyph i" data-tool tabindex={active === 2 ? 0 : -1} onmousedown={keep} onclick={() => inline('*')}
          aria-label={t('edtr.italic')} title={keyTip(t('edtr.italic'), 'italic')} aria-keyshortcuts={ariaKeys('italic')}><span aria-hidden="true">{t('edtr.italicGlyph')}</span></button>
        <button type="button" class="tb" data-tool tabindex={active === 3 ? 0 : -1} onmousedown={keep} onclick={openLink}
          aria-label={t('edtr.link')} title={keyTip(t('edtr.link'), 'link')} aria-keyshortcuts={ariaKeys('link')} aria-haspopup="dialog"><Link size={16} strokeWidth={1.9} /></button>
        <span class="sep" aria-hidden="true"></span>
        <button type="button" class="tb" data-tool tabindex={active === 4 ? 0 : -1} onmousedown={keep} onclick={() => list('bullet')} aria-pressed={kind.list === 'bullet'}
          aria-label={t('edtr.bullets')} title={keyTip(t('edtr.bullets'), 'list.bullet')}><List size={17} strokeWidth={1.9} class="flip-rtl" /></button>
        <button type="button" class="tb" data-tool tabindex={active === 5 ? 0 : -1} onmousedown={keep} onclick={() => list('number')} aria-pressed={kind.list === 'number'}
          aria-label={t('edtr.numbers')} title={keyTip(t('edtr.numbers'), 'list.number')}><ListOrdered size={17} strokeWidth={1.9} class="flip-rtl" /></button>
        <button type="button" class="tb" data-tool tabindex={active === 6 ? 0 : -1} onmousedown={keep} onclick={() => list('task')} aria-pressed={kind.list === 'task'}
          aria-label={t('edtr.checklist')} title={keyTip(t('edtr.checklist'), 'list.task')}><ListTodo size={17} strokeWidth={1.9} class="flip-rtl" /></button>
        <button type="button" class="tb" data-tool tabindex={active === 7 ? 0 : -1} onmousedown={keep} onclick={() => shift(-1)} aria-disabled={!kind.list}
          aria-label={t('edtr.outdent')} title={`${t('edtr.outdent')} (${isoK(keyName('Shift+Tab'))})`}><ListIndentDecrease size={17} strokeWidth={1.9} class="flip-rtl" /></button>
        <button type="button" class="tb" data-tool tabindex={active === 8 ? 0 : -1} onmousedown={keep} onclick={() => shift(1)} aria-disabled={!kind.list}
          aria-label={t('edtr.indent')} title={`${t('edtr.indent')} (${isoK(keyName('Tab'))})`}><ListIndentIncrease size={17} strokeWidth={1.9} class="flip-rtl" /></button>
        <span class="sep" aria-hidden="true"></span>
        <button type="button" class="tb" data-tool tabindex={active === 9 ? 0 : -1} onclick={() => { if (!adding) picker?.click(); }} aria-disabled={adding > 0}
          aria-label={t('edtr.picture')} title={t('edtr.pictureTip')}><ImagePlus size={17} strokeWidth={1.9} /></button>
        <DictateButton target={() => ta} tool tabindex={active === 10 ? 0 : -1} place="below" />
        <span class="status" role="status" aria-live="polite">{status}</span>
        <input type="file" accept="image/png,image/jpeg,image/gif,image/webp,image/*" multiple hidden bind:this={picker} onchange={pick} />
      </div>
      <p class="sr-only" id="{uid}-hint">{hint}</p>
    {/if}
    <textarea id={uid} class="editor" class:prose={aids} bind:this={ta} dir={aids ? dir : 'auto'} spellcheck={aids} aria-label={label}
      aria-describedby={aids ? `${uid}-hint` : undefined} oninput={onInput} onkeydown={onKeydown} onpaste={onPaste}
      onkeyup={queueKind} onmouseup={queueKind} onfocus={queueKind} onscroll={onTaScroll}
      oncompositionstart={() => (composing = true)} oncompositionend={() => (composing = false)}></textarea>
    {#if dropping}<div class="dropzone" aria-hidden="true"><ImagePlus size={26} /><span>{t('edtr.dropPicture')}</span></div>{/if}
  </div>
  {#if showPv}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
    <article class="md pv scroll" dir="auto" aria-label={t('docs.preview')} bind:this={pv} onscroll={onPvScroll}
      onloadcapture={pictureLoaded}
      onclick={(e) => { if (!mdClick(e) && (e.target as HTMLElement).closest('a')) e.preventDefault(); }} use:zoomImages>{@html pvHtml}</article>
  {/if}
</div>

{#if linking}
  <div class="modal-bg top" role="presentation" use:portal onclick={(e) => { if (e.target === e.currentTarget) closeLink(null); }}>
    <div class="modal dlg" role="dialog" aria-modal="true" aria-labelledby="{uid}-lt" use:trapFocus={{ onEscape: () => closeLink(null) }}>
    <form class="dlgf" onsubmit={(e) => { e.preventDefault(); if (linking?.address.trim()) closeLink('set'); }}>
      <h2 id="{uid}-lt">{t(linking.old ? 'edtr.linkEdit' : 'edtr.linkAdd')}</h2>
      <label class="fld"><span>{t('edtr.linkText')}</span><input bind:value={linking.label} dir="auto" /></label>
      <label class="fld"><span>{t('edtr.linkAddress')}</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value={linking.address} dir="ltr" placeholder={t('edtr.linkPh')} autofocus autocomplete="off" spellcheck="false" aria-describedby="{uid}-lh" />
      </label>
      <p class="muted sm" id="{uid}-lh">{t('edtr.linkHint')}</p>
      <div class="row end">
        {#if linking.old}<button type="button" class="btn danger" onclick={() => closeLink('unlink')}>{t('edtr.unlink')}</button>{/if}
        <span class="grow"></span>
        <button type="button" class="btn" onclick={() => closeLink(null)}>{t('cancel')}</button>
        <button type="submit" class="btn primary" disabled={!linking.address.trim()}>{t(linking.old ? 'edtr.linkSave' : 'edtr.linkOk')}</button>
      </div>
    </form>
    </div>
  </div>
{/if}

<style>
  .ed { display: grid; grid-template-columns: minmax(0, 1fr); gap: 16px; height: calc(100vh - 110px); min-height: 320px; }
  .ed.two { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
  .col { position: relative; display: flex; flex-direction: column; gap: 6px; min-height: 0; min-width: 0; }
  .tools { display: flex; flex-wrap: wrap; align-items: center; gap: 2px; padding: 3px; border: var(--border-w) solid var(--border); border-radius: var(--radius); background: var(--panel); }
  .tb { font: inherit; color: var(--fg); background: none; border: 0; min-width: 32px; height: 32px; padding: 0 6px; border-radius: 7px; display: inline-grid; place-items: center; cursor: pointer; flex: none; }
  .tb:hover:not(:disabled) { background: var(--fill); }
  .tb[aria-pressed='true'] { background: var(--tint); color: var(--on-tint); }
  .tb[aria-disabled='true'], .tb:disabled { color: var(--muted); }
  .tb:disabled { cursor: default; opacity: .55; }
  .glyph { font-size: 16px; line-height: 1; }
  .glyph.b { font-weight: 800; }
  .glyph.i { font-style: italic; font-family: Georgia, 'Times New Roman', var(--font-ar, serif); font-size: 17px; }
  .style { font: inherit; font-size: var(--fs-sm); height: 32px; padding: 0 8px; max-width: 12em; border-radius: 7px; }
  .sep { width: 1px; align-self: stretch; margin: 5px 4px; background: var(--border); flex: none; }
  .status { font-size: var(--fs-xs); color: var(--muted); padding-inline: 6px; }
  .status:empty { padding: 0; }
  /* Each line finds its own direction, so an Arabic note behind English note details is still
     written right-to-left, line by line. */
  .editor { flex: 1; min-height: 120px; width: 100%; font-family: var(--mono); font-size: 13px; line-height: 1.55; resize: none; unicode-bidi: plaintext; text-align: start; }
  /* Notes are written in the reading font; plain text files keep their columns. */
  .editor.prose { font-family: inherit; font-size: 15px; line-height: 1.65; }
  .pv { position: relative; height: 100%; padding: 8px 14px; border-radius: var(--radius); border: var(--border-w) solid var(--border); overflow-wrap: anywhere; }
  .dropzone { position: absolute; inset: 0; z-index: 3; pointer-events: none; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px;
    font-weight: 500; color: var(--accent); border: 2px dashed var(--accent); border-radius: var(--radius-lg, var(--radius)); background: color-mix(in srgb, var(--bg) 88%, transparent); text-align: center; padding: 12px; }
  @container docs (max-width: 900px) { .ed.two { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(220px, 1fr) minmax(160px, 1fr); } }
  .dlg { max-width: 480px; }
  .dlgf { display: grid; gap: 12px; }
  .dlg h2 { margin: 0; font-size: 17px; }
  .fld { display: grid; gap: 4px; font-size: var(--fs-sm); }
  .fld input { width: 100%; }
  .dlg p { margin: 0; }
  .sm { font-size: var(--fs-xs); }
  .end { gap: 8px; margin-top: 4px; flex-wrap: wrap; align-items: center; }
</style>
