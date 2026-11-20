<script lang="ts">
  // The quick capture box. Type, press Enter: a todo goes to the general todo list (a day said at
  // the end becomes its due date), a note goes to the inbox. Saved straight into the library by the
  // background service (`POST /api/capture`), with no assistant run and nothing to wrap. What is
  // typed is kept while the box is away (and across a restart) until it is saved.
  //
  // It is a slim floating bar, like a launcher: one line to type in, which grows as the text
  // wraps (up to a few lines, then it scrolls), and a line under it only when there is something
  // to say (saved, not saved, offline…). The window is exactly as big as the bar (`capture_fit`):
  // on a Mac it is clear, and the page draws the rounded bar and its shadow inside a small margin.
  // It wears the app's look — theme (also those added through Customise), Day / Night, language,
  // zoom — and follows it while the app runs (see look.ts).
  import { onMount, tick } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { a11y, app, applyTheme, cachedLook } from '../lib/store.svelte';
  import { fmtDate, t } from '../lib/i18n';
  import { isolated as iso, os } from '../lib/quickKeys';
  import Icon from '../lib/Icon.svelte';
  import { applyTokens, themeFor, themeTokens } from './look';

  type Kind = 'todo' | 'note';
  type Prefill = { text?: string; kind?: Kind | null } | null;
  type Saved = { kind: Kind; text: string; path?: string; undo: () => Promise<unknown> };

  const mac = os() === 'macos';
  const KINDS: Kind[] = ['todo', 'note'];
  const KIND_KEY = 'thoughts.capture.kind';
  const DRAFT_KEY = 'thoughts.capture.draft';
  const store = {
    get(k: string): string | null { try { return localStorage.getItem(k); } catch { return null; } },
    set(k: string, v: string | null) { try { if (v === null) localStorage.removeItem(k); else localStorage.setItem(k, v); } catch { /* private mode */ } },
  };

  let text = $state(store.get(DRAFT_KEY) ?? '');
  let kind = $state<Kind>(store.get(KIND_KEY) === 'note' ? 'note' : 'todo');
  let phase = $state<'connecting' | 'ready' | 'setup' | 'offline'>('connecting');
  let saving = $state(false);
  let status = $state<{ tone: 'ok' | 'error' | 'info'; text: string; detail?: string } | null>(null);
  let saved = $state<Saved | null>(null);
  let box: HTMLTextAreaElement | undefined = $state();
  let bar: HTMLElement | undefined = $state();
  /** Themes added through Customise (their tokens are in force once read). */
  let custom: { id?: unknown }[] = [];
  /** The customisations were read at least once (until then a theme added there is not missing,
   *  only not known yet). */
  let themesRead = false;
  let hover = false;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  // Typing "note:" / «ملاحظة:» or "todo:" / «مهمة:» picks the kind (the service drops the word).
  const NOTE = /^\s*(?:note|ملاحظ[ةه])\s*[:：]/i;
  const TODO = /^\s*(?:todo|مهم[ةه])\s*[:：]/i;
  $effect(() => { if (NOTE.test(text)) kind = 'note'; else if (TODO.test(text)) kind = 'todo'; });
  $effect(() => { store.set(DRAFT_KEY, text.trim() ? text : null); });
  $effect(() => { store.set(KIND_KEY, kind); });

  async function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>(cmd, args);
  }

  /** The look of the app, painted the way the app window paints it (lib/store `applyTheme`). */
  function paintLook() {
    if (!app.settings) return;
    applyTheme();
    // A theme that is not there (removed in Customise) would leave the box without colours: the
    // standard one stands in. Only on screen here; the choice itself stays as the user made it.
    if (themesRead) document.documentElement.dataset.theme = themeFor(app.settings.theme, custom);
    document.title = t('nativeb.cap.title');
    fitSoon();
  }

  /** The themes added through Customise: their tokens only (look.ts), read again when they change. */
  async function loadThemes() {
    try {
      const m = await api('/layer');
      custom = Array.isArray(m?.themes) ? m.themes : [];
      applyTokens(themeTokens(String(m?.css ?? '')));
      themesRead = true;
    } catch {
      // Not reached: what was read last stays in force.
    }
    paintLook();
  }

  /** Reach the service and read the settings. True when the box can save. */
  async function connect(): Promise<boolean> {
    if (phase !== 'ready') phase = 'connecting';
    try {
      const s = await api('/settings');
      app.settings = s;
      paintLook();
      const ok = !!(s?.onboarded && s?.libraryPath);
      phase = ok ? 'ready' : 'setup';
      if (status?.text === t('nativeb.cap.offline')) status = null;
      return ok;
    } catch {
      phase = 'offline';
      status = { tone: 'error', text: t('nativeb.cap.offline') };
      return false;
    }
  }

  function applyPrefill(p: Prefill) {
    if (!p) return;
    const add = String(p.text ?? '').trim();
    if (add && text.trim() !== add) text = text.trim() ? text.replace(/\s+$/, '') + '\n' + add : add;
    if (p.kind === 'todo' || p.kind === 'note') kind = p.kind;
    if (add) status = { tone: 'info', text: t('nativeb.cap.fromLink') };
  }

  async function focusBox() {
    await tick();
    box?.focus();
    box?.setSelectionRange(text.length, text.length);
  }

  // ----- the window's size follows the bar -----
  /** The bar's width, and the clear margin around it for its shadow (Mac only), in CSS pixels. */
  const BAR_W = 640;
  const MARGIN = mac ? 20 : 0;
  /** The text box never grows past this many lines; then it scrolls. */
  const MAX_LINES = 6;
  const zoom = () => Math.min(200, Math.max(50, Math.round(Number(a11y().zoom) || 100))) / 100;
  let asked = '';

  /** Ask the shell for a window `barH` CSS pixels of bar tall (in points: the zoom is applied). */
  function askFit(barH: number): Promise<unknown> {
    const z = zoom();
    const width = Math.ceil((BAR_W + 2 * MARGIN) * z), height = Math.ceil((barH + 2 * MARGIN) * z);
    const key = width + 'x' + height;
    if (key === asked) return Promise.resolve();
    asked = key;
    return invoke('capture_fit', { width, height }).catch(() => { asked = ''; });
  }
  function fitNow() { if (bar) return askFit(bar.getBoundingClientRect().height); }
  function fitSoon() { requestAnimationFrame(() => { fitNow(); }); }

  /** The height the text needs, within the limit (CSS pixels). */
  function textHeight(el: HTMLTextAreaElement): number {
    const cs = getComputedStyle(el);
    const line = parseFloat(cs.lineHeight) || 24;
    const pad = (parseFloat(cs.paddingTop) || 0) + (parseFloat(cs.paddingBottom) || 0);
    const was = el.style.height;
    el.style.height = '0px';
    const need = el.scrollHeight;
    el.style.height = was;
    return Math.min(Math.max(need, line + pad), line * MAX_LINES + pad);
  }

  /** Grow or shrink the text box to its text without the box ever being cut off: when it grows the
   *  window grows first (what does not fit yet scrolls inside the box for that moment); when it
   *  shrinks the box shrinks first and the window follows (the clear part below shows nothing). */
  let sizing = Promise.resolve();
  function sizeText() {
    sizing = sizing.then(async () => {
      const el = box;
      if (!el || !bar) return;
      const now = el.getBoundingClientRect().height;
      const next = textHeight(el);
      if (Math.abs(next - now) < 0.5) return;
      const barH = bar.getBoundingClientRect().height - now + next;
      if (next > now) await askFit(barH);
      el.style.height = next + 'px';
      if (next < now) await askFit(barH);
    }).catch(() => {});
  }
  $effect(() => { void text; void phase; tick().then(sizeText); });

  /** The box came up again: a fresh start after a save, any text a link brought, the cursor in place. */
  async function reopened() {
    cancelHide();
    if (saved) { saved = null; status = null; }
    applyPrefill(await invoke<Prefill>('capture_take').catch(() => null));
    connect().then(() => loadThemes());
    focusBox();
    fitSoon();
  }

  // Until the settings arrive, the words follow the language the window was last shown in.
  if (!app.settings) { const look = cachedLook(); if (look?.language) app.settings = { ...look }; }

  onMount(() => {
    let off: (() => void) | undefined;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      off = await listen('capture-open', reopened);
    })().catch(() => {});
    // The look changes while the app runs: a new theme, Day / Night, language or zoom chosen in the
    // app, or a theme added or edited through Customise.
    const offEvents = on((e) => {
      if (e?.type === 'settings.changed' || e?.type === 'resync') connect();
      if (e?.type === 'layer.changed' || e?.type === 'resync') loadThemes();
    });
    // Day / Night following the system: the store paints it again, and the stand-in theme with it.
    const scheme = matchMedia('(prefers-color-scheme: dark)');
    const onScheme = () => paintLook();
    scheme.addEventListener('change', onScheme);
    // Anything else that changes the bar's height (a status line, the setup notice, the zoom).
    const ro = new ResizeObserver(() => fitNow());
    if (bar) ro.observe(bar);
    // Shown once it wears the app's look and has its size, or after a moment at the latest:
    // typing can start while the service is still being reached.
    const looked = connect().then(() => loadThemes());
    Promise.race([looked, new Promise((r) => setTimeout(r, 400))])
      .then(() => tick())
      .then(() => fitNow())
      .finally(() => invoke<Prefill>('capture_ready').then(applyPrefill).catch(() => {}));
    focusBox();
    return () => { off?.(); offEvents(); ro.disconnect(); scheme.removeEventListener('change', onScheme); };
  });

  // ----- saving -----
  function dueText(due: string): string {
    const day = (n: number) => { const d = new Date(); d.setDate(d.getDate() + n); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`; };
    if (due === day(0)) return t('nativeb.cap.today');
    if (due === day(1)) return t('nativeb.cap.tomorrow');
    return fmtDate(due + 'T12:00:00', { weekday: 'long', day: 'numeric', month: 'long' });
  }

  async function save() {
    if (saving) return;
    const body = text.trim();
    if (!body) { status = { tone: 'info', text: t('nativeb.cap.empty') }; focusBox(); return; }
    if (phase !== 'ready' && !(await connect())) return;
    saving = true;
    status = null;
    const k = kind;
    try {
      const r = await post('/capture', { kind: k, text: body });
      saved = k === 'todo'
        ? { kind: k, text: body, undo: () => post('/todos/delete', { file: r.file, line: r.line, text: r.text }) }
        : { kind: k, text: body, path: r?.path, undo: () => post('/library/delete', { path: r.path }) };
      text = '';
      const msg = k === 'note' ? t('nativeb.cap.noteSaved') : r?.due ? t('nativeb.cap.todoDue', { date: dueText(String(r.due)) }) : t('nativeb.cap.todoSaved');
      status = { tone: 'ok', text: msg };
      laterHide();
    } catch (e: any) {
      status = { tone: 'error', text: t('nativeb.cap.failed'), detail: String(e?.message ?? e ?? '').slice(0, 300) };
      // Back in front if the user had already clicked away: the text was not saved.
      invoke('capture_show').catch(() => {});
      focusBox();
    } finally { saving = false; }
  }

  async function undo() {
    const s = saved;
    if (!s) return;
    cancelHide();
    try {
      await s.undo();
      saved = null;
      text = s.text;
      kind = s.kind;
      status = { tone: 'info', text: t('nativeb.cap.undone') };
      focusBox();
    } catch (e: any) {
      status = { tone: 'error', text: t('nativeb.cap.undoFailed'), detail: String(e?.message ?? e ?? '').slice(0, 300) };
    }
  }

  function openInApp() {
    const s = saved;
    cancelHide();
    invoke('capture_open_main', { view: s ? (s.kind === 'todo' ? 'todos' : 'inbox') : 'home' }).catch(() => {});
  }

  // After a save the box goes away by itself, unless the pointer or the keyboard is on it.
  function laterHide() {
    cancelHide();
    hideTimer = setTimeout(() => {
      if (hover || document.activeElement?.closest('.done')) { laterHide(); return; }
      close();
    }, 1800);
  }
  function cancelHide() { clearTimeout(hideTimer); hideTimer = undefined; }
  function close() { cancelHide(); invoke('capture_hide', { giveBack: true }).catch(() => {}); }

  function onBoxKey(e: KeyboardEvent) {
    if (e.isComposing || e.keyCode === 229) return;
    if (e.key === 'Enter' && !e.shiftKey && !e.altKey) { e.preventDefault(); save(); return; }
    if (saved && !text && e.key.toLowerCase() === 'z' && (mac ? e.metaKey : e.ctrlKey) && !e.shiftKey) { e.preventDefault(); undo(); return; }
    cancelHide();
  }
  function onWindowKey(e: KeyboardEvent) {
    if (e.isComposing) return;
    if (e.key === 'Escape') { e.preventDefault(); close(); }
  }

  function pick(k: Kind) { kind = k; focusBox(); }
  function kindKeys(e: KeyboardEvent) {
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) return;
    e.preventDefault();
    kind = kind === 'todo' ? 'note' : 'todo';
    (e.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-kind="${kind}"]`)?.focus();
  }

  // Each key is isolated, so it keeps its own order inside an Arabic sentence.
  const keyHint = $derived(t('nativeb.cap.keys', {
    enter: iso(mac ? '↩' : 'Enter'), newline: iso(mac ? '⇧↩' : 'Shift+Enter'), esc: iso(mac ? 'esc' : 'Esc'),
  }));
</script>

<svelte:window onkeydown={onWindowKey} />

<main class="cap" class:mac aria-labelledby="cap-title">
  <h1 id="cap-title" class="sr-only">{t('nativeb.cap.title')}</h1>
  <!-- The bar can be dragged by any part that is not a control. -->
  <div class="bar" bind:this={bar} data-tauri-drag-region role="group" aria-labelledby="cap-title"
    onpointerenter={() => (hover = true)} onpointerleave={() => (hover = false)}>
    {#if phase === 'setup'}
      <div class="row setup" data-tauri-drag-region>
        <div class="setup-text" data-tauri-drag-region>
          <h2 data-tauri-drag-region>{t('nativeb.cap.setupTitle')}</h2>
          <p class="muted" data-tauri-drag-region>{t('nativeb.cap.setupBody')}</p>
        </div>
        <button type="button" class="btn primary" onclick={openInApp}>{t('nativeb.cap.openApp')}</button>
      </div>
    {:else}
      <div class="row" data-tauri-drag-region>
        <div class="seg" role="radiogroup" aria-label={t('nativeb.cap.kind')} tabindex="-1" onkeydown={kindKeys}>
          {#each KINDS as k (k)}
            <button type="button" class="segb" class:on={kind === k} role="radio" data-kind={k} aria-checked={kind === k} tabindex={kind === k ? 0 : -1}
              title={t(k === 'todo' ? 'nativeb.cap.hintTodo' : 'nativeb.cap.hintNote')} onclick={() => pick(k)}>{t('nativeb.cap.' + k)}</button>
          {/each}
        </div>
        <label for="cap-text" class="sr-only">{t('nativeb.cap.label')}</label>
        <textarea
          id="cap-text" bind:this={box} bind:value={text} dir={text.trim() ? 'auto' : undefined} rows="1" spellcheck="true" maxlength="4000"
          placeholder={t(kind === 'todo' ? 'nativeb.cap.phTodoShort' : 'nativeb.cap.phNoteShort')}
          aria-describedby="cap-hint cap-status" onkeydown={onBoxKey}
        ></textarea>
        <button type="button" class="send" disabled={saving} aria-busy={saving} aria-label={t(saving ? 'nativeb.cap.saving' : 'nativeb.cap.save')} title={keyHint} onclick={save}>
          <Icon name="send" size={16} stroke={2.25} />
        </button>
      </div>
      <p id="cap-hint" class="sr-only">{t(kind === 'todo' ? 'nativeb.cap.phTodo' : 'nativeb.cap.phNote')} {keyHint}</p>
      <div class="below" aria-live="polite">
        {#if status}
          <p id="cap-status" class="status {status.tone}" class:done={!!saved} role={status.tone === 'error' ? 'alert' : 'status'}>
            <span>{status.text}</span>
            {#if saved}
              <button type="button" class="link" onclick={undo}>{t('nativeb.cap.undo')}</button>
              <button type="button" class="link" onclick={openInApp}>{t('nativeb.cap.open')}</button>
            {:else if phase === 'offline'}
              <button type="button" class="link" onclick={connect}>{t('nativeb.cap.retry')}</button>
            {/if}
          </p>
          {#if status.detail}<p class="detail muted" dir="auto">{status.detail}</p>{/if}
        {:else}
          <p id="cap-status" class="sr-only" role="status">{phase === 'connecting' ? t('nativeb.cap.connecting') : t(kind === 'todo' ? 'nativeb.cap.hintTodo' : 'nativeb.cap.hintNote')}</p>
        {/if}
      </div>
    {/if}
  </div>
</main>

<style>
  /* The window is exactly the bar (plus, on a Mac, a clear margin for its shadow). Nothing here
     depends on the window's height, so the bar never reflows while the window is being resized. */
  :global(html), :global(body) { margin: 0; overflow: hidden; background: var(--panel); }
  :global(html:has(.cap.mac)), :global(body:has(.cap.mac)) { background: transparent; }
  .cap { padding: 0; }
  .cap.mac { padding: 20px; }
  .bar {
    width: 100%; display: grid; gap: 0; background: var(--panel); color: var(--fg);
    border: var(--border-w) solid var(--hairline); padding: 10px 10px 10px 12px;
  }
  .mac .bar { border-radius: 16px; box-shadow: var(--shadow-lg); }
  :global([dir="rtl"]) .bar { padding: 10px 12px 10px 10px; }
  .row { display: flex; align-items: flex-start; gap: 10px; min-width: 0; }
  .seg { flex: none; display: inline-flex; padding: 2px; gap: 2px; border-radius: 999px; background: var(--fill); margin-top: 3px; }
  .segb {
    font: inherit; font-size: var(--fs-xs); font-weight: 600; line-height: 1; color: var(--muted); background: transparent;
    border: 0; border-radius: 999px; padding: 6px 10px; cursor: pointer;
  }
  .segb.on { background: var(--tint); color: var(--on-tint); }
  .segb:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  textarea {
    flex: 1; min-width: 0; height: 36px; resize: none; font: inherit; font-size: 16px; line-height: 24px; color: var(--fg);
    background: transparent; border: 0; padding: 6px 2px; margin: 0; overflow-y: auto; scrollbar-width: thin;
  }
  textarea::placeholder { color: var(--muted); opacity: 1; }
  /* The bar is the text box: its keyboard focus shows on the whole bar. */
  textarea:focus-visible { outline: none; }
  .bar:has(textarea:focus-visible) { border-color: color-mix(in srgb, var(--accent) 55%, var(--hairline)); }
  .send {
    flex: none; width: 32px; height: 32px; margin-top: 2px; border-radius: 999px; border: 0; cursor: pointer;
    display: grid; place-items: center; background: var(--accent); color: var(--accent-fg);
  }
  .send:disabled { opacity: .5; cursor: default; }
  .send:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .below { display: grid; gap: 2px; padding-inline-start: 2px; }
  .below:has(.status) { padding-top: 6px; margin-top: 8px; border-top: 1px solid var(--hairline); }
  .status { margin: 0; font-size: 13px; display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; }
  .status.ok { color: var(--ok); font-weight: 500; }
  .status.error { color: var(--danger); font-weight: 500; }
  .status.info { color: var(--muted); }
  .detail { margin: 0; font-size: var(--fs-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .link { font: inherit; font-weight: 600; color: var(--accent); background: none; border: 0; padding: 0; cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .setup { align-items: center; }
  .setup-text { flex: 1; min-width: 0; display: grid; gap: 2px; }
  .setup h2 { margin: 0; font-size: 14px; font-weight: 600; }
  .setup p { margin: 0; font-size: 13px; }
</style>
