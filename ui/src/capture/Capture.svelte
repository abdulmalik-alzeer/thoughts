<script lang="ts">
  // The quick capture box. Type, press Enter: a todo goes to the general todo list (a day said at
  // the end becomes its due date), a note goes to the inbox. Saved straight into the library by the
  // background service (`POST /api/capture`), with no assistant run and nothing to wrap. What is
  // typed is kept while the box is away (and across a restart) until it is saved.
  import { onMount, tick } from 'svelte';
  import { api, post } from '../lib/api';
  import { app, cachedLook } from '../lib/store.svelte';
  import { fmtDate, t } from '../lib/i18n';
  import { os } from '../lib/quickKeys';

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

  /** The look and language of the app (customised themes fall back to the standard one here). */
  function paintLook(s: any) {
    const root = document.documentElement;
    const dark = s.mode === 'dark' || (s.mode !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
    root.dataset.theme = ['saudi', 'contrast', 'tech'].includes(s.theme) ? s.theme : 'saudi';
    root.dataset.mode = dark ? 'dark' : 'light';
    root.lang = s.language === 'ar' ? 'ar' : 'en';
    root.dir = s.language === 'ar' ? 'rtl' : 'ltr';
    document.title = t('nativeb.cap.title');
  }

  /** Reach the service and read the settings. True when the box can save. */
  async function connect(): Promise<boolean> {
    if (phase !== 'ready') phase = 'connecting';
    try {
      const s = await api('/settings');
      app.settings = s;
      paintLook(s);
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

  /** The box came up again: a fresh start after a save, any text a link brought, the cursor in place. */
  async function reopened() {
    cancelHide();
    if (saved) { saved = null; status = null; }
    applyPrefill(await invoke<Prefill>('capture_take').catch(() => null));
    connect();
    focusBox();
  }

  // Until the settings arrive, the words follow the language the window was last shown in.
  if (!app.settings) { const look = cachedLook(); if (look?.language) app.settings = { ...look }; }

  onMount(() => {
    let off: (() => void) | undefined;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      off = await listen('capture-open', reopened);
    })().catch(() => {});
    // Shown as soon as it is drawn: typing can start while the service is still being reached.
    invoke<Prefill>('capture_ready').then(applyPrefill).catch(() => {});
    focusBox();
    connect();
    return () => off?.();
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
    invoke('capture_open_main', s ? { view: s.kind === 'todo' ? 'todos' : 'note', path: s.path } : { view: 'home' }).catch(() => {});
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
  const iso = (k: string) => `\u2066${k}\u2069`;
  const keyHint = $derived(t('nativeb.cap.keys', {
    enter: iso(mac ? '↩' : 'Enter'), newline: iso(mac ? '⇧↩' : 'Shift+Enter'), esc: iso(mac ? 'esc' : 'Esc'),
  }));
</script>

<svelte:window onkeydown={onWindowKey} />

<main class="cap" class:mac aria-labelledby="cap-title" onpointerenter={() => (hover = true)} onpointerleave={() => (hover = false)}>
  <header data-tauri-drag-region>
    <h1 id="cap-title" data-tauri-drag-region>{t('nativeb.cap.title')}</h1>
    {#if phase !== 'setup'}
      <div class="seg" role="radiogroup" aria-label={t('nativeb.cap.kind')} tabindex="-1" onkeydown={kindKeys}>
        {#each KINDS as k (k)}
          <button type="button" class="segb" class:on={kind === k} role="radio" data-kind={k} aria-checked={kind === k} tabindex={kind === k ? 0 : -1} onclick={() => pick(k)}>{t('nativeb.cap.' + k)}</button>
        {/each}
      </div>
    {/if}
  </header>

  {#if phase === 'setup'}
    <section class="setup">
      <h2>{t('nativeb.cap.setupTitle')}</h2>
      <p class="muted">{t('nativeb.cap.setupBody')}</p>
      <button type="button" class="btn primary" onclick={openInApp}>{t('nativeb.cap.openApp')}</button>
    </section>
  {:else}
    <label for="cap-text" class="sr-only">{t('nativeb.cap.label')}</label>
    <textarea
      id="cap-text" bind:this={box} bind:value={text} dir={text.trim() ? 'auto' : undefined} rows="3" spellcheck="true" maxlength="4000"
      placeholder={t(kind === 'todo' ? 'nativeb.cap.phTodo' : 'nativeb.cap.phNote')}
      aria-describedby="cap-hint cap-status" onkeydown={onBoxKey}
    ></textarea>

    <footer>
      <div class="info">
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
          <p id="cap-status" class="status muted" role="status">{phase === 'connecting' ? t('nativeb.cap.connecting') : t(kind === 'todo' ? 'nativeb.cap.hintTodo' : 'nativeb.cap.hintNote')}</p>
        {/if}
        <p id="cap-hint" class="keys muted">{keyHint}</p>
      </div>
      <button type="button" class="btn primary" disabled={saving} aria-busy={saving} onclick={save}>{t(saving ? 'nativeb.cap.saving' : 'nativeb.cap.save')}</button>
    </footer>
  {/if}
</main>

<style>
  :global(html), :global(body) { height: 100%; overflow: hidden; background: var(--panel); }
  .cap { height: 100%; display: flex; flex-direction: column; gap: 10px; padding: 10px 16px 14px; }
  header { display: flex; align-items: center; gap: 12px; min-height: 30px; -webkit-user-select: none; user-select: none; }
  /* The window's own buttons sit at the top left on a Mac, whatever the language. */
  .mac header { padding-left: 64px; }
  h1 { font-size: 13px; font-weight: 600; margin: 0; color: var(--muted); letter-spacing: 0; flex: 1; }
  textarea {
    flex: 1; min-height: 0; width: 100%; resize: none; font: inherit; font-size: 16px; line-height: 1.5; color: var(--fg);
    background: var(--bg); border: var(--border-w) solid var(--control-border); border-radius: var(--radius); padding: 10px 12px;
  }
  textarea:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  footer { display: flex; align-items: flex-end; gap: 12px; }
  .info { flex: 1; min-width: 0; display: grid; gap: 2px; }
  .status { margin: 0; font-size: 13px; display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; }
  .status.ok { color: var(--ok); font-weight: 500; }
  .status.error { color: var(--danger); font-weight: 500; }
  .detail { margin: 0; font-size: var(--fs-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .keys { margin: 0; font-size: var(--fs-xs); }
  .link { font: inherit; font-weight: 600; color: var(--accent); background: none; border: 0; padding: 0; cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .setup { flex: 1; display: grid; align-content: center; justify-items: start; gap: 8px; }
  .setup h2 { margin: 0; font-size: 17px; }
  .setup p { margin: 0; }
</style>
