<script lang="ts">
  // Find in the open note or chat (⌘F / Ctrl+F): every match highlighted, the current one scrolled
  // into view, Enter / Shift+Enter (or the arrows) to step, a count. Matching folds case and Arabic
  // spellings like the rest of search. It searches what is on screen: the rendered note or chat, or
  // the text being edited (`editor`).
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import Icon from './Icon.svelte';
  import { t, fmtNum } from './i18n';
  import { fold } from './CommandPalette.svelte';
  import { foldMap, registerFind, takeFind } from './search';
  import { isMac } from './shortcuts';

  let { root, editor, where = '', anchors = '' }: {
    /** The element whose text is searched. */
    root: () => HTMLElement | null | undefined;
    /** While editing: the text box to search instead. */
    editor?: () => HTMLTextAreaElement | null | undefined;
    /** `chat:<id>` / `doc:<path>`: picks up a search result that was just opened here. */
    where?: string;
    /** Selector for the messages (in order), so a result can land on its message. */
    anchors?: string;
  } = $props();

  let open = $state(false);
  let query = $state('');
  let count = $state(0);
  let cur = $state(-1);
  let input = $state<HTMLInputElement>();
  let barEl = $state<HTMLElement>();
  let anchorEl = $state<HTMLElement>();
  let boxes = $state<{ x: number; y: number; w: number; h: number }[]>([]);

  let ranges: Range[] = [];
  let spans: [number, number][] = [];
  let textMode = false;
  let before: HTMLElement | null = null;
  const canHighlight = typeof CSS !== 'undefined' && 'highlights' in CSS && typeof (window as any).Highlight === 'function';

  const label = $derived(count ? t('srch.findCount', { n: fmtNum(cur + 1), total: fmtNum(count) }) : query.trim() ? t('srch.findNone') : '');
  const mod = isMac ? '⌘' : 'Ctrl+';
  /** Keys isolated so they read correctly inside Arabic text. */
  const iso = (k: string) => String.fromCharCode(0x2066) + k + String.fromCharCode(0x2069);

  function ensureStyle() {
    if (!canHighlight || document.getElementById('thoughts-find-style')) return;
    const st = document.createElement('style');
    st.id = 'thoughts-find-style';
    // Classic find colours: readable on day, night and high-contrast themes alike.
    st.textContent = '::highlight(thoughts-find){background-color:rgba(255,204,0,.45);color:inherit}'
      + '::highlight(thoughts-find-current){background-color:#ff9632;color:#000}';
    document.head.appendChild(st);
  }

  // ---------- matching ----------

  /** The whole phrase; when it is nowhere, each of its words. */
  function needles(q: string): string[][] {
    const phrase = fold(q.trim()).replace(/\s+/g, ' ');
    if (!phrase) return [];
    const words = [...new Set(phrase.split(' ').filter(Boolean))];
    return words.length > 1 ? [[phrase], words] : [[phrase]];
  }

  function spansIn(s: string, list: string[]): [number, number][] {
    const out: [number, number][] = [];
    for (const n of list) for (let i = s.indexOf(n); i >= 0; i = s.indexOf(n, i + n.length)) out.push([i, i + n.length]);
    out.sort((a, b) => a[0] - b[0]);
    const merged: [number, number][] = [];
    for (const r of out) { const l = merged[merged.length - 1]; if (l && r[0] < l[1]) l[1] = Math.max(l[1], r[1]); else merged.push([...r]); }
    return merged;
  }

  function textNodes(el: HTMLElement): Text[] {
    const out: Text[] = [];
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, {
      acceptNode(n) {
        const p = n.parentElement;
        if (!p || !n.nodeValue?.trim()) return NodeFilter.FILTER_REJECT;
        if (p.closest('script, style, textarea, input, select, .find-bar, [aria-hidden="true"]')) return NodeFilter.FILTER_REJECT;
        if (!p.getClientRects().length) return NodeFilter.FILTER_REJECT; // not shown (a closed detail)
        return NodeFilter.FILTER_ACCEPT;
      },
    });
    for (let n = walk.nextNode(); n; n = walk.nextNode()) out.push(n as Text);
    return out;
  }

  function findDom(el: HTMLElement, q: string): Range[] {
    const nodes = textNodes(el);
    let s = '';
    const nodeOf: number[] = [], offOf: number[] = [];
    nodes.forEach((n, i) => {
      const m = foldMap(n.nodeValue!);
      s += m.s;
      for (const f of m.from) { nodeOf.push(i); offOf.push(f); }
    });
    for (const list of needles(q)) {
      const found = spansIn(s, list);
      if (!found.length) continue;
      return found.map(([a, b]) => {
        const r = document.createRange();
        const endNode = nodes[nodeOf[b - 1]], endOff = offOf[b - 1];
        const code = endNode.nodeValue!.charCodeAt(endOff);
        r.setStart(nodes[nodeOf[a]], offOf[a]);
        r.setEnd(endNode, endOff + (code >= 0xd800 && code <= 0xdbff ? 2 : 1));
        return r;
      });
    }
    return [];
  }

  function findText(value: string, q: string): [number, number][] {
    const m = foldMap(value);
    for (const list of needles(q)) {
      const found = spansIn(m.s, list);
      if (found.length) return found.map(([a, b]) => [m.from[a], m.from[b - 1] + 1]);
    }
    return [];
  }

  // ---------- showing ----------

  function scroller(el: HTMLElement | null): HTMLElement | null {
    for (let e = el; e; e = e.parentElement) {
      const o = getComputedStyle(e).overflowY;
      if ((o === 'auto' || o === 'scroll') && e.scrollHeight > e.clientHeight) return e;
    }
    return null;
  }

  function paint() {
    if (canHighlight) {
      const reg = (CSS as any).highlights;
      const H = (window as any).Highlight;
      if (!open || textMode || !ranges.length) { reg.delete('thoughts-find'); reg.delete('thoughts-find-current'); }
      else {
        reg.set('thoughts-find', new H(...ranges));
        const c = new H(...(cur >= 0 ? [ranges[cur]] : []));
        c.priority = 1;
        reg.set('thoughts-find-current', c);
      }
    } else {
      // Older system web views: an outline over the current match.
      boxes = open && !textMode && cur >= 0 && ranges[cur]
        ? Array.from(ranges[cur].getClientRects()).map((b) => ({ x: b.left - 1, y: b.top - 1, w: b.width + 2, h: b.height + 2 })) : [];
    }
  }

  function show() {
    if (cur < 0) return;
    if (textMode) {
      const ed = editor?.();
      if (!ed) return;
      const [a, b] = spans[cur];
      ed.setSelectionRange(a, b);
      const line = ed.value.slice(0, a).split('\n').length - 1;
      const lh = parseFloat(getComputedStyle(ed).lineHeight) || 20;
      ed.scrollTop = Math.max(0, line * lh - ed.clientHeight / 3);
      return;
    }
    const r = ranges[cur];
    const rect = r.getBoundingClientRect();
    const sc = scroller(root() ?? null);
    if (sc) {
      const b = sc.getBoundingClientRect();
      if (rect.top < b.top + 70 || rect.bottom > b.bottom - 20) sc.scrollTop += rect.top - b.top - b.height / 3;
    } else if (rect.top < 70 || rect.bottom > innerHeight - 20) window.scrollBy(0, rect.top - innerHeight / 3);
    requestAnimationFrame(paint);
  }

  /** The first match at or below the top of what is on screen. */
  function firstVisible(): number {
    if (textMode || !ranges.length) return 0;
    const sc = scroller(root() ?? null);
    const top = sc ? sc.getBoundingClientRect().top : 0;
    const i = ranges.findIndex((r) => r.getBoundingClientRect().bottom >= top);
    return i < 0 ? 0 : i;
  }

  function refresh(keep: boolean) {
    const q = query.trim();
    const ed = editor?.() ?? null;
    textMode = !!ed;
    ranges = []; spans = [];
    if (q && ed) spans = findText(ed.value, q);
    else if (q) { const el = root(); if (el) ranges = findDom(el, q); }
    count = textMode ? spans.length : ranges.length;
    if (!count) cur = -1;
    else if (keep && cur >= 0) cur = Math.min(cur, count - 1);
    else cur = firstVisible();
    paint();
    if (!keep) show();
  }

  function step(d: number) {
    if (!count) return;
    cur = (cur + d + count) % count;
    paint();
    show();
  }

  // ---------- open / close ----------

  let observer: MutationObserver | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** The note or chat changes while the bar is open (new output, another file): look again. */
  function watch() {
    observer?.disconnect();
    const host = anchorEl?.parentElement;
    if (!host || !open) return;
    observer = new MutationObserver((recs) => {
      if (recs.every((r) => barEl?.contains(r.target))) return;
      clearTimeout(timer);
      timer = setTimeout(() => { if (open) refresh(true); }, 300);
    });
    observer.observe(host, { subtree: true, childList: true, characterData: true });
  }
  function onEditorInput() { clearTimeout(timer); timer = setTimeout(() => { if (open) refresh(true); }, 200); }

  /** Open with the text that was selected (or `q`). False when there is nothing here to search. */
  function openBar(q?: string): boolean {
    if (!root() && !editor?.()) return false;
    const sel = q ?? (window.getSelection()?.toString().trim() || '');
    if (sel && sel.length <= 200 && !sel.includes('\n')) query = sel;
    if (!open) before = document.activeElement as HTMLElement | null;
    open = true;
    ensureStyle();
    tick().then(() => { input?.focus(); input?.select(); watch(); editor?.()?.addEventListener('input', onEditorInput); refresh(false); });
    return true;
  }

  function close() {
    open = false;
    observer?.disconnect();
    clearTimeout(timer);
    editor?.()?.removeEventListener('input', onEditorInput);
    paint();
    boxes = [];
    const ed = editor?.();
    if (ed && textMode && cur >= 0) { ed.focus(); ed.setSelectionRange(spans[cur][0], spans[cur][1]); }
    else if (before?.isConnected) before.focus();
  }

  let inputTimer: ReturnType<typeof setTimeout> | undefined;
  function onInput() { clearTimeout(inputTimer); inputTimer = setTimeout(() => refresh(false), 80); }

  function onKey(e: KeyboardEvent) {
    if (e.isComposing) return;
    const modKey = isMac ? e.metaKey : e.ctrlKey;
    if (e.key === 'Enter' || e.key === 'F3' || (modKey && e.code === 'KeyG')) { e.preventDefault(); clearTimeout(inputTimer); if (!count && query.trim()) refresh(false); else step(e.shiftKey ? -1 : 1); }
    else if (e.key === 'ArrowDown' && !e.shiftKey) { e.preventDefault(); step(1); }
    else if (e.key === 'ArrowUp' && !e.shiftKey) { e.preventDefault(); step(-1); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); }
  }

  function onScroll() { if (open && !canHighlight && cur >= 0) requestAnimationFrame(paint); }

  let unregister: (() => void) | undefined;
  let handoff: ReturnType<typeof setInterval> | undefined;
  /** A search result was opened here: show its words (on its message, for a chat) once its content is in. */
  function pickUp(w: string) {
    const p = w ? takeFind(w) : null;
    if (!p) return;
    clearInterval(handoff);
    query = p.query;
    const shownBefore = root()?.textContent ?? '';
    let tries = 0;
    handoff = setInterval(() => {
      const el = root();
      if (++tries > 30) { clearInterval(handoff); return; }
      // Wait for the new content: for a moment the view may still show what was open before.
      if (!el || !el.textContent?.trim() || (el.textContent === shownBefore && tries < 7)) return;
      clearInterval(handoff);
      open = true;
      ensureStyle();
      tick().then(() => {
        watch();
        refresh(true);
        if (p.msg != null && anchors && count) {
          const target = el.querySelectorAll(anchors)[p.msg];
          const i = target ? ranges.findIndex((r) => target.contains(r.startContainer)) : -1;
          cur = i >= 0 ? i : 0;
        } else if (count) cur = 0;
        paint();
        show();
      });
    }, 150);
  }

  onMount(() => {
    unregister = registerFind({ open: openBar });
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onScroll);
  });
  // The view can stay on screen while what it shows changes (Docs follows the route): check each time.
  $effect(() => { const w = where; untrack(() => pickUp(w)); });
  onDestroy(() => {
    unregister?.();
    clearInterval(handoff);
    clearTimeout(timer); clearTimeout(inputTimer);
    observer?.disconnect();
    editor?.()?.removeEventListener('input', onEditorInput);
    window.removeEventListener('scroll', onScroll, true);
    window.removeEventListener('resize', onScroll);
    if (canHighlight) { (CSS as any).highlights.delete('thoughts-find'); (CSS as any).highlights.delete('thoughts-find-current'); }
  });
</script>

<span bind:this={anchorEl} hidden></span>
{#if open}
  <div class="find-bar no-print" role="search" aria-label={t('srch.findLabel')} bind:this={barEl}>
    <Icon name="search" size={14} />
    <input bind:this={input} bind:value={query} oninput={onInput} onkeydown={onKey} dir="auto"
      placeholder={t('srch.findPh')} aria-label={t('srch.findPh')} autocomplete="off" spellcheck="false" />
    <span class="count" role="status" aria-live="polite">{label}</span>
    <button type="button" class="fb" onclick={() => step(-1)} disabled={!count} aria-label={t('srch.findPrev')} title={`${t('srch.findPrev')} (${iso('⇧↩')})`}><Icon name="down" size={15} class="find-up" /></button>
    <button type="button" class="fb" onclick={() => step(1)} disabled={!count} aria-label={t('srch.findNext')} title={`${t('srch.findNext')} (${iso('↩')} · ${iso(mod + 'G')})`}><Icon name="down" size={15} /></button>
    <button type="button" class="fb" onclick={close} aria-label={t('srch.findClose')} title={`${t('srch.findClose')} (Esc)`}><Icon name="close" size={15} /></button>
    {#each boxes as b}<span class="find-box" aria-hidden="true" style="left:{b.x}px;top:{b.y}px;width:{b.w}px;height:{b.h}px"></span>{/each}
  </div>
{/if}

<style>
  .find-bar { position: absolute; top: 8px; inset-inline-end: 16px; z-index: 9; display: flex; align-items: center; gap: 4px;
    padding: 4px 4px 4px 10px; padding-inline: 10px 4px; background: var(--panel); border: var(--border-w, 1px) solid var(--hairline);
    border-radius: 10px; box-shadow: var(--shadow-lg); color: var(--muted); max-width: calc(100% - 32px); }
  .find-bar input { width: 200px; min-width: 0; flex: 1; background: transparent; border: 0; box-shadow: none; padding: 4px 2px; font-size: 14px; color: var(--fg); }
  .count { font-size: var(--fs-xs); white-space: nowrap; min-width: 4.5em; text-align: center; font-variant-numeric: tabular-nums; }
  .fb { font: inherit; background: none; border: 0; width: 28px; height: 28px; display: grid; place-items: center; border-radius: 6px; color: var(--fg); cursor: pointer; flex: none; }
  .fb:hover:not(:disabled) { background: var(--fill); }
  .fb:disabled { opacity: .4; cursor: default; }
  .fb :global(.find-up) { transform: rotate(180deg); }
  .find-box { position: fixed; pointer-events: none; border: 2px solid #ff9632; border-radius: 3px; background: rgba(255, 150, 50, .25); }
  @media (max-width: 560px) { .find-bar input { width: 120px; } }
</style>
