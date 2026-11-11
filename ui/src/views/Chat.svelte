<script lang="ts" module>
  type Attachment = { name: string; data: string; size: number; image: boolean; /** bytes were lost on restart: must be attached again */ missing?: boolean };
  // Kept for the session, per chat: attachments still waiting in the composer (their bytes cannot
  // go into localStorage) and the files of the last message sent (so Retry can send them again).
  const pendingFiles = new Map<string, Attachment[]>();
  const lastSent = new Map<string, { text: string; files: Attachment[] }>();
  const DRAFT_MAX = 20000;
  const draftKey = (id: string) => `thoughts.chatdraft:${id}`;
</script>

<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onMount, tick } from 'svelte';
  import { api, del, on, patch, post, ApiError } from '../lib/api';
  import { md, mdClick } from '../lib/md';
  import { app, costVisible, go, refreshChats } from '../lib/store.svelte';
  import { t, tn, fmtMoney } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { signIn } from '../lib/auth';
  import { normalizeCapture } from '../lib/capture';
  import { confirmDialog, promptDialog } from '../lib/dialog.svelte';
  import { openReview } from '../lib/layer';
  import { MAX_FILE_BYTES, copyText, fmtSize, isImageName, notify, readAsDataURL } from '../lib/content/util';

  let { id, projects }: { id: string; projects: any[] } = $props();


  let chat = $state<any>(null);
  let loadState = $state<'loading' | 'ok' | 'missing' | 'error'>('loading');
  let loadError = $state('');
  let streaming = $state('');
  let thinking = $state('');
  let text = $state('');
  let files = $state<Attachment[]>([]);
  let mentions = $state<string[]>([]);
  let opts = $state<any>({ models: [], efforts: [], permissionModes: [] });
  let tree = $state<string[]>([]);
  let mentionQ = $state<string | null>(null);
  let mentionIdx = $state(0);
  let note = $state('');
  let openTools = $state<Record<string, boolean>>({});
  let stick = $state(true);      // user is at (or near) the bottom, so new output may scroll the view
  let unseen = $state(false);    // new output arrived while the user was reading further up
  let dragging = $state(false);
  let dragDepth = 0;
  let scroller = $state<HTMLElement>();
  let input = $state<HTMLTextAreaElement>();
  let escHint = $state(false);   // Esc was pressed once while the assistant is working
  let escTimer: ReturnType<typeof setTimeout> | undefined;
  let announce = $state('');     // one polite line for screen readers when an answer completes
  let announceTimer: ReturnType<typeof setTimeout> | undefined;

  // ---- draft: what was typed (and which files were waiting) survives chat switches and restarts ----
  {
    let d: any = null;
    try { d = JSON.parse(localStorage.getItem(draftKey(id)) || 'null'); } catch { /* no draft */ }
    if (d && typeof d === 'object') {
      text = String(d.text ?? '').slice(0, DRAFT_MAX);
      mentions = Array.isArray(d.mentions) ? d.mentions.filter((p: any) => typeof p === 'string').slice(0, 30) : [];
      const mem = pendingFiles.get(id) ?? [];
      const meta: any[] = Array.isArray(d.files) ? d.files.slice(0, 20) : [];
      files = meta.map((m) => mem.find((f) => f.name === m.name && f.size === m.size) ?? { name: String(m.name ?? ''), size: Number(m.size) || 0, image: !!m.image, data: '', missing: true });
    }
  }
  $effect(() => {
    const draft = { text: text.slice(0, DRAFT_MAX), mentions: [...mentions], files: files.map((f) => ({ name: f.name, size: f.size, image: f.image })) };
    const live = files.filter((f) => !f.missing).map((f) => ({ ...f }));
    if (live.length) pendingFiles.set(id, live); else pendingFiles.delete(id);
    try {
      if (draft.text || draft.mentions.length || draft.files.length) localStorage.setItem(draftKey(id), JSON.stringify(draft));
      else localStorage.removeItem(draftKey(id));
    } catch { /* storage full or unavailable: the draft just isn't kept */ }
  });

  // A mention lives only as long as its "@path" is still in the text.
  $effect(() => {
    const keep = mentions.filter((p) => text.includes('@' + p));
    if (keep.length !== mentions.length) mentions = keep;
  });

  // The box grows with the text up to about 40% of the window, then scrolls.
  function grow() {
    if (!input) return;
    input.style.height = 'auto';
    const max = Math.round(window.innerHeight * 0.4);
    input.style.height = Math.min(input.scrollHeight + 2, max) + 'px';
    input.style.overflowY = input.scrollHeight + 2 > max ? 'auto' : 'hidden';
  }
  $effect(() => { text; input; grow(); });

  function say(msg: string) {
    clearTimeout(announceTimer);
    announce = msg;
    announceTimer = setTimeout(() => (announce = ''), 4000);
  }

  async function load(initial = false) {
    if (initial) loadState = 'loading';
    try {
      const c = await api(`/chats/${id}`);
      c.options ??= {};
      // "Ask before changes" no longer exists: an older chat that stored it is read-only.
      if (c.options.permissionMode === 'default') c.options.permissionMode = 'plan';
      chat = c;
      savedPerm = c.options.permissionMode ?? null;
      loadState = 'ok';
      // Seen: the "cut off" marker stays on screen for this visit but no longer follows the chat around.
      if (c.interrupted && initial) post(`/chats/${id}/ack-interrupted`).catch(() => {});
      await scrollDown(initial);
      if (initial) {
        const missing = files.filter((f) => f.missing).length;
        if (missing) notify(tn('chat3.reattachNote', missing), { kind: 'info', key: 'reattach:' + id });
        // Only a brand-new chat takes the cursor; otherwise the shell puts focus on the title.
        if (!c.messages.length && !c.running && !['wrap', 'reorganize', 'setup'].includes(c.kind)) { await tick(); input?.focus(); }
      }
    } catch (e: any) {
      if (chat && !initial) return failed(t('chat3.refreshFailed'), e); // keep what is on screen; a refresh failed
      loadError = e?.message ?? String(e);
      loadState = e instanceof ApiError && e.status === 404 ? 'missing' : 'error';
    }
  }

  async function scrollDown(force = false) {
    if (!force && !stick) { unseen = true; return; }
    await tick();
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
    stick = true; unseen = false;
  }

  function onscroll() {
    if (!scroller) return;
    stick = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 80;
    if (stick) unseen = false;
  }

  onMount(() => {
    load(true);
    window.addEventListener('resize', grow);
    api('/ai/claude/options').then((o) => (opts = { models: [], efforts: [], permissionModes: [], ...o })).catch(() => {});
    api('/library/tree?exclude=private').then((x: any[]) => (tree = x.map((f) => f.path).filter((p) => p.endsWith('.md')))).catch(() => {});
    const off = on((e) => {
      if (e.type === 'resync') { load(); return; }
      if (e.chatId !== id && e.chat?.id !== id) return;
      if (!chat) return;
      if (e.type === 'chat.message') {
        if (!chat.messages.some((m: any) => m.id === e.message.id)) chat.messages.push(e.message);
        if (e.message.role === 'assistant') streaming = '';
        if (e.message.role !== 'user') thinking = '';
        scrollDown(e.message.role === 'user');
      } else if (e.type === 'chat.delta') {
        streaming += e.text;
        scrollDown();
      } else if (e.type === 'chat.thinking') {
        thinking += e.text;
      } else if (e.type === 'chat.done') {
        streaming = ''; thinking = '';
        escHint = false;
        say(t('chat3.answerDone'));
        load();
      } else if (e.type === 'chat.updated') {
        chat.running = e.chat.running;
        chat.title = e.chat.title;
      }
    });
    return () => { off(); window.removeEventListener('resize', grow); clearTimeout(escTimer); clearTimeout(announceTimer); };
  });

  async function sendBody(body: any, failKey = 'chat3.sendFailed') {
    if (body.options?.permissionMode === 'default') body.options = { ...body.options, permissionMode: 'plan' };
    chat.running = true;
    stick = true;
    try { await post(`/chats/${id}/send`, body); return true; }
    catch (e: any) {
      // The service refuses files in private folders (or ones that no longer exist) as mentions.
      if (e instanceof ApiError && e.status === 400 && body.mentions?.length) notify(t('layer.mentionRejected'), { kind: 'error', detail: e.message });
      else failed(t(failKey), e);
      chat.running = false;
      return false;
    }
  }

  async function send() {
    if (!canSend || chat.running) return;
    const keep = { text, files: [...files], mentions: [...mentions] };
    const ready = files.filter((f) => !f.missing);
    // Files may go on their own; the assistant still needs a sentence to answer.
    const words = text.trim() ? normalizeCapture(text) : tn('chat3.seeAttached', ready.length);
    const body = { text: words, attachments: ready.map(({ name, data }) => ({ name, data })), mentions: [...mentions], options: chat.options };
    text = ''; files = []; mentions = []; mentionQ = null;
    // Nothing the user typed is lost when sending fails.
    if (!(await sendBody(body))) { text = keep.text; files = keep.files; mentions = keep.mentions; }
    else lastSent.set(id, { text: words, files: ready.map((f) => ({ ...f })) });
  }
  const canSend = $derived(!!text.trim() || files.some((f) => !f.missing));

  const lastUser = $derived(chat ? [...chat.messages].reverse().find((m: any) => m.role === 'user') : null);
  const lastAnswerId = $derived(chat ? [...chat.messages].reverse().find((m: any) => m.role === 'assistant' || m.role === 'error')?.id : null);
  const canCompose = $derived(!!chat && !['wrap', 'reorganize', 'setup'].includes(chat.kind));

  async function retry() {
    if (!lastUser || chat.running) return;
    // The files of that message go again too (they are kept in memory for this session).
    const prev = lastSent.get(id);
    const again = prev && prev.text === lastUser.text ? prev.files : [];
    if (!again.length && lastUser.attachments?.length) notify(t('chat3.retryNoFiles'), { kind: 'info' });
    await sendBody({ text: lastUser.text, attachments: again.map(({ name, data }) => ({ name, data })), mentions: lastUser.tool?.mentions ?? [], options: chat.options }, 'chat3.retryFailed');
  }

  async function stop() {
    escHint = false;
    try { await post(`/chats/${id}/stop`); } catch (e) { failed(t('chat3.stopFailed'), e); }
  }

  async function copyMsg(s: string) {
    const ok = await copyText(s);
    notify(t(ok ? 'chat.copied' : 'chat.copyFailed'), { kind: ok ? 'info' : 'error' });
  }

  function keydown(e: KeyboardEvent) {
    if (mentionHits.length) {
      if (e.key === 'ArrowDown') { e.preventDefault(); mentionIdx = (mentionIdx + 1) % mentionHits.length; return; }
      if (e.key === 'ArrowUp') { e.preventDefault(); mentionIdx = (mentionIdx - 1 + mentionHits.length) % mentionHits.length; return; }
      if ((e.key === 'Enter' || e.key === 'Tab') && !e.isComposing) { e.preventDefault(); pickMention(mentionHits[mentionIdx]); return; }
    }
    if (e.key === 'Escape') {
      if (mentionQ !== null) { e.preventDefault(); mentionQ = null; }
      else if (chat?.running) {
        // One stray Esc must not throw away an answer: the second press within 2 seconds stops it.
        e.preventDefault();
        if (escHint) { clearTimeout(escTimer); stop(); }
        else { escHint = true; clearTimeout(escTimer); escTimer = setTimeout(() => (escHint = false), 2000); }
      }
      return;
    }
    // Enter sends, Shift+Enter makes a new line (Cmd/Ctrl+Enter also sends). Never while an IME is composing.
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); }
  }

  function oninput() {
    const before = text.slice(0, input!.selectionStart);
    const m = before.match(/@([\p{L}\p{N}_\-./]*)$/u);
    mentionQ = m ? m[1] : null;
    mentionIdx = 0;
  }

  function pickMention(p: string) {
    const pos = input!.selectionStart;
    const before = text.slice(0, pos).replace(/@([\p{L}\p{N}_\-./]*)$/u, '@' + p + ' ');
    text = before + text.slice(pos);
    if (!mentions.includes(p)) mentions.push(p);
    mentionQ = null;
    input!.focus();
  }

  function dropMention(p: string) {
    text = text.split('@' + p + ' ').join('').split('@' + p).join('');
    mentions = mentions.filter((x) => x !== p);
    input?.focus();
  }

  const mentionHits = $derived(mentionQ === null ? [] : tree.filter((p) => p.toLowerCase().includes(mentionQ!.toLowerCase())).slice(0, 8));

  async function addFiles(list: Iterable<File>) {
    for (const f of list) {
      if (f.size > MAX_FILE_BYTES) { notify(t('chat.tooLarge', { name: f.name, size: fmtSize(f.size) }), { kind: 'error' }); continue; }
      try {
        const data = await readAsDataURL(f);
        const name = f.name || t('chat.pastedName');
        const att = { name, data, size: f.size, image: f.type.startsWith('image/') || isImageName(name) };
        // Attaching a file again fills the placeholder a restored draft left for it.
        const gap = files.findIndex((x) => x.missing && x.name === name);
        if (gap >= 0) files[gap] = att; else files.push(att);
      } catch { notify(t('chat.readFailed', { name: f.name }), { kind: 'error' }); }
    }
  }

  async function attach(e: Event) {
    const el = e.target as HTMLInputElement;
    await addFiles(el.files || []);
    el.value = '';
  }

  function onpaste(e: ClipboardEvent) {
    const fs = [...(e.clipboardData?.files || [])];
    if (!fs.length) return;
    e.preventDefault();
    addFiles(fs);
  }

  const hasFiles = (e: DragEvent) => [...(e.dataTransfer?.types || [])].includes('Files');
  function dragenter(e: DragEvent) { if (!canCompose || !hasFiles(e)) return; e.preventDefault(); dragDepth++; dragging = true; }
  function dragover(e: DragEvent) { if (canCompose && hasFiles(e)) e.preventDefault(); }
  function dragleave(e: DragEvent) { if (!hasFiles(e)) return; dragDepth = Math.max(0, dragDepth - 1); if (!dragDepth) dragging = false; }
  function drop(e: DragEvent) {
    if (!canCompose || !hasFiles(e)) return;
    e.preventDefault();
    dragDepth = 0; dragging = false;
    addFiles(e.dataTransfer?.files || []);
    input?.focus();
  }

  async function saveOpts(confirmBypass = false): Promise<boolean> {
    try { await patch(`/chats/${id}`, { options: chat.options, confirmBypass }); return true; }
    catch (e) { failed(t('chat3.optionsFailed'), e); return false; }
  }

  // The permission mode as the service has it. "Bypass permissions" is only switched on after the
  // user agreed to the warning (the service refuses it otherwise).
  const BYPASS = 'bypassPermissions';
  let savedPerm: string | null = null;
  async function permChanged() {
    const want = chat.options.permissionMode ?? null;
    const turnsOn = want === BYPASS && savedPerm !== BYPASS;
    if (turnsOn) {
      const yes = await confirmDialog({
        title: t('perm.bypass.title'), body: `${t('perm.bypass.body')}\n\n${t('perm.bypass.chat')}`,
        note: t('perm.bypass.note'), confirmLabel: t('perm.bypass.confirm'), danger: true,
      });
      if (!yes) { chat.options.permissionMode = savedPerm; return; }
    }
    if (await saveOpts(turnsOn)) savedPerm = want;
    else chat.options.permissionMode = savedPerm;
  }

  async function moveTo(pid: string) {
    try {
      await patch(`/chats/${id}`, { projectId: pid || null });
      chat.projectId = pid || null;
      refreshChats().catch(() => {});
    } catch (e) { failed(t('chat3.moveFailed'), e); }
  }

  async function rename() {
    const title = (await promptDialog({ title: t('chat.renameTitle'), value: chat.title, placeholder: t('chat.renamePlaceholder'), confirmLabel: t('save') }))?.trim();
    if (!title || title === chat.title) return;
    try { await patch(`/chats/${id}`, { title }); chat.title = title; refreshChats().catch(() => {}); } catch (e) { failed(t('chat3.renameFailed'), e); }
  }

  async function remove() {
    if (!(await confirmDialog({ title: t('chat.deleteTitle'), body: t('chat.deleteBody'), confirmLabel: t('delete'), danger: true }))) return;
    try {
      await del(`/chats/${id}`);
      try { localStorage.removeItem(draftKey(id)); } catch { /* ignore */ }
      pendingFiles.delete(id); lastSent.delete(id);
      await refreshChats().catch(() => {});
      go({ kind: 'chats' });
    } catch (e) { failed(t('chat3.deleteFailed'), e); }
  }

  async function applyWrap() {
    try { await post(`/wrap/${id}/apply`, { note }); note = ''; chat.running = true; }
    catch (e) { failed(t('chat3.wrapApplyFailed'), e); }
  }

  let undoing = $state(false);
  async function undoPartialWrap() {
    if (undoing) return;
    if (!(await confirmDialog({ title: t('layer.wrap.undoTitle'), body: t('layer.wrap.undoBody'), confirmLabel: t('layer.wrap.undo') }))) return;
    undoing = true;
    try { await post(`/wrap/${id}/rollback`); notify(t('layer.wrap.undone'), { kind: 'success' }); await load(); }
    catch (e) { failed(t('chat3.wrapUndoFailed'), e); }
    finally { undoing = false; }
  }
  const phase = $derived(chat?.meta?.phase as string | undefined);

  const resultFor = $derived.by(() => {
    const map: Record<string, any> = {};
    for (const m of chat?.messages ?? []) if (m.role === 'tool_result' && m.tool?.toolUseId) map[m.tool.toolUseId] = m;
    return map;
  });
  const toolKey = (m: any) => String(m.tool?.id ?? m.id);

  /** Translated label when we have one, the raw value otherwise (models, future modes). */
  const label = (prefix: string, v: string) => { const k = `${prefix}.${v}`; const s = t(k); return s === k ? v : s; };
  const usd = fmtMoney;
  // Claude Code's modes, as far as the service allows them. Bypass is for ordinary chats only.
  const permModes = $derived((opts.permissionModes as string[]).filter((m) => m === 'acceptEdits' || m === 'plan' || (m === BYPASS && chat?.kind === 'chat')));
</script>

{#if loadState === 'loading'}
  <div class="wrap" aria-busy="true">
    <span class="sr" role="status">{t('chat.loading')}</span>
    <div class="skel line title-sk"></div>
    <div class="skel-col">
      <div class="skel bubble-sk end"></div>
      <div class="skel line w80"></div><div class="skel line w60"></div><div class="skel line w70"></div>
      <div class="skel bubble-sk end short"></div>
      <div class="skel line w60"></div><div class="skel line w40"></div>
    </div>
  </div>
{:else if loadState !== 'ok' || !chat}
  <div class="wrap state" role="alert">
    <Icon name={loadState === 'missing' ? 'chats' : 'warning'} size={28} />
    <h2>{t(loadState === 'missing' ? 'chat.missingTitle' : 'chat.errorTitle')}</h2>
    <p class="muted">{t(loadState === 'missing' ? 'chat.missingBody' : 'chat.errorBody')}</p>
    {#if loadState === 'error' && loadError}<p class="muted detail" dir="auto">{loadError}</p>{/if}
    <div class="row">
      <button class="btn" onclick={() => go({ kind: 'chats' })}><Icon name="arrow" size={14} class="flip-rtl back" /> {t('chat.backToChats')}</button>
      {#if loadState === 'error'}<button class="btn primary" onclick={() => load(true)}><Icon name="retry" size={14} /> {t('chat.tryAgain')}</button>{/if}
    </div>
  </div>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="wrap" ondragenter={dragenter} ondragover={dragover} ondragleave={dragleave} ondrop={drop}>
    <header class="row">
      <h1 class="h grow"><button class="title ell" onclick={rename} title={t('chat.rename')} aria-label={t('chat.renameAria', { title: chat.title })} dir="auto">{chat.title}</button></h1>
      {#if chat.kind === 'chat'}
        <select value={chat.projectId ?? ''} onchange={(e) => moveTo((e.target as HTMLSelectElement).value)} title={t('chat.group')} aria-label={t('chat.group')}>
          <option value="">{t('library')}</option>
          {#each projects as p}<option value={p.id}>{p.name}</option>{/each}
        </select>
      {:else}
        <span class="pill accent">{label('chat.kind', chat.kind)}</span>
      {/if}
      {#if chat.costUsd > 0 && costVisible()}
        <button class="pill cost" type="button" title={t('chat.costTip')} aria-label="{t('chat.costLabel', { cost: usd(chat.costUsd) })}. {t('chat.costTip')}"
          onclick={() => notify(t('chat.costTip'), { kind: 'info' })}><bdi>{usd(chat.costUsd)}</bdi> <Icon name="info" size={12} /></button>
      {/if}
      <button class="btn ghost" onclick={remove} title={t('chat.deleteChat')} aria-label={t('chat.deleteChat')}><Icon name="trash" /></button>
    </header>

    {#if chat.kind === 'customize'}
      <div class="banner row">
        <span class="grow">{t('layer.chatBanner')}</span>
        {#if app.pending?.exists && !app.pending.running}<button class="btn primary" onclick={openReview}>{t('layer.review')}</button>{/if}
      </div>
    {/if}
    {#if chat.interrupted}
      <div class="banner row" role="status"><Icon name="warning" size={14} /> <span class="grow">{t('layer.chat.interrupted')}</span></div>
    {/if}

    <div class="msgs-wrap">
      <div class="msgs scroll" bind:this={scroller} {onscroll} role="group" aria-label={t('chat.conversation')}>
        {#each chat.messages as m (m.id)}
          {#if m.role === 'user'}
            <div class="msg user">
              <div class="bubble" dir="auto">{m.text}
                {#if m.attachments?.length}<div class="sm att"><Icon name="attach" size={12} /> <bdi>{m.attachments.map((a: string) => a.split('/').pop()).join(', ')}</bdi></div>{/if}
                {#if m.tool?.mentions?.length}<div class="sm att"><bdi>@ {m.tool.mentions.join(', ')}</bdi></div>{/if}
              </div>
              <div class="acts">
                <button class="act" onclick={() => copyMsg(m.text)} aria-label={t('chat.copyMessage')} title={t('chat.copyMessage')}><Icon name="copy" size={14} /></button>
              </div>
            </div>
          {:else if m.role === 'assistant'}
            <div class="msg ai">
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <div class="md" dir="auto" onclick={mdClick}>{@html md(m.text)}</div>
              <div class="acts" class:show={m.id === lastAnswerId}>
                <button class="act" onclick={() => copyMsg(m.text)} aria-label={t('chat.copyMessage')} title={t('chat.copyMessage')}><Icon name="copy" size={14} /><span>{t('chat.copy')}</span></button>
                {#if m.id === lastAnswerId && !chat.running && lastUser && canCompose}
                  <button class="act" onclick={retry} title={t('chat.retryTip')}><Icon name="retry" size={14} /><span>{t('chat.retry')}</span></button>
                {/if}
              </div>
            </div>
          {:else if m.role === 'tool'}
            {@const k = toolKey(m)}
            {@const res = resultFor[k]}
            <div class="toolrow">
              <button class="tool" onclick={() => (openTools[k] = !openTools[k])} aria-expanded={!!openTools[k]} title={t(openTools[k] ? 'chat.hideDetails' : 'chat.showDetails')}>
                <Icon name="tool" size={13} /> <span class="ell" dir="auto">{m.text}</span>
                {#if res?.tool?.isError}<span class="pill bad">{t('chat.toolFailed')}</span>{/if}
                <Icon name={openTools[k] ? 'down' : 'right'} size={13} class={openTools[k] ? '' : 'flip-rtl'} />
              </button>
              {#if openTools[k]}
                <div class="tooldetail">
                  <div class="eyebrow">{t('chat.toolInput')}</div>
                  <pre class="sm" dir="ltr">{JSON.stringify(m.tool?.input, null, 2)}</pre>
                  <div class="eyebrow">{t('chat.toolResult')}</div>
                  {#if res}<pre class="sm" dir="auto">{res.text || t('chat.toolEmpty')}</pre>
                  {:else}<p class="muted sm">{t(chat.running ? 'chat.toolPending' : 'chat.toolNoResult')}</p>{/if}
                </div>
              {/if}
            </div>
          {:else if m.role === 'tool_result'}
            <!-- shown inside its tool call above -->
          {:else if m.role === 'error'}
            <div class="msg err" role="alert">
              <div class="errline"><Icon name="warning" size={14} /> <span dir="auto">{m.text}</span></div>
              {#if m.tool?.code === 'signedOut' && m.id === lastAnswerId}
                <button class="act show" onclick={signIn}><Icon name="external" size={14} /><span>{t('auth.signIn')}</span></button>
              {/if}
              {#if m.id === lastAnswerId && !chat.running && lastUser && canCompose}
                <button class="act show" onclick={retry}><Icon name="retry" size={14} /><span>{t('chat.tryAgain')}</span></button>
              {/if}
            </div>
          {:else}
            <div class="msg sys muted" dir="auto">{m.text}</div>
          {/if}
        {:else}
          {#if !chat.running}<div class="hint muted"><Icon name="chat" size={22} /><span>{t('chat.emptyHint')}</span></div>{/if}
        {/each}
        {#if thinking}<div class="msg sys muted ell think"><Icon name="thinking" size={13} /> <span class="ell" dir="auto">{thinking.slice(-160)}</span></div>{/if}
        {#if streaming}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="msg ai" aria-busy="true"><div class="md" dir="auto" onclick={mdClick}>{@html md(streaming)}</div></div>
        {/if}
        {#if chat.running && !streaming}<div class="msg sys muted working" role="status"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span> {t('chat.working')}</div>{/if}
      </div>
      <span class="sr" role="status" aria-live="polite">{announce}</span>
      {#if !stick}
        <button class="jump btn" onclick={() => scrollDown(true)}><Icon name="arrowDown" size={14} /> {t(unseen ? 'chat.jumpNew' : 'chat.jumpLatest')}</button>
      {/if}
    </div>

    {#if chat.kind === 'wrap' && !chat.running && chat.meta?.phase === 'preview' && chat.messages.some((m: any) => m.role === 'assistant')}
      <div class="approve card row">
        <input class="grow" placeholder={t('chat.wrapNotePlaceholder')} aria-label={t('chat.wrapNotePlaceholder')} bind:value={note} dir="auto" />
        <button class="btn primary" onclick={applyWrap}><Icon name="check" size={15} /> {t('chat.approveApply')}</button>
      </div>
    {/if}
    {#if chat.kind === 'wrap' && !chat.running && phase === 'failed'}
      <div class="approve card row" role="alert"><Icon name="warning" size={15} /> <span class="grow">{t(chat.meta?.rolledBack === false ? 'layer.wrap.failedKept' : 'layer.wrap.failedRolledBack')}</span>
        {#if chat.meta?.rolledBack === false && chat.meta?.canRollback}<button class="btn" disabled={undoing} onclick={undoPartialWrap}><Icon name="undo" size={14} /> {t('layer.wrap.undo')}</button>{/if}</div>
    {/if}
    {#if chat.kind === 'wrap' && !chat.running && phase === 'interrupted'}
      <div class="approve card row" role="alert"><Icon name="warning" size={15} /> <span class="grow">{t('layer.wrap.interrupted')} {t('chat3.wrapChoose')}</span>
        <button class="btn primary" disabled={undoing} onclick={applyWrap} title={t('chat3.wrapFinishTip')}><Icon name="check" size={14} /> {t('chat3.wrapFinish')}</button>
        {#if chat.meta?.canRollback !== false}<button class="btn" disabled={undoing} onclick={undoPartialWrap}><Icon name="undo" size={14} /> {t('layer.wrap.undo')}</button>{/if}</div>
    {/if}
    {#if chat.kind === 'reorganize' && !chat.running && (phase === 'interrupted' || phase === 'failed')}
      <div class="approve card row" role="alert"><Icon name="warning" size={15} /> <span class="grow">{t(phase === 'failed' ? 'layer.reorg.failed' : 'layer.reorg.interrupted')}</span></div>
    {/if}
    {#if chat.kind === 'reorganize' && !chat.running && chat.meta?.phase === 'review'}
      <div class="approve card row"><span class="grow">{t('chat.reorgReady')}</span>
        <button class="btn primary" onclick={() => go({ kind: 'reorganize', id })}>{t('chat.reviewChanges')}</button></div>
    {/if}

    {#if canCompose}
      <div class="composer card">
        {#if mentionHits.length}
          <div class="mentions" role="listbox" id="mention-list" aria-label={t('chat.mentionList')}>
            {#each mentionHits as p, i}
              <button type="button" role="option" id="mention-{i}" aria-selected={i === mentionIdx} class:sel={i === mentionIdx} tabindex="-1"
                onmousedown={(e) => e.preventDefault()} onclick={() => pickMention(p)} onmouseenter={() => (mentionIdx = i)}><bdi>{p}</bdi></button>
            {/each}
          </div>
        {/if}
        {#if files.length}
          <ul class="files">
            {#each files as f, i (f.name + i)}
              <li class="file" class:gone={f.missing} title={f.missing ? t('chat3.reattachTip') : undefined}>
                {#if f.image && !f.missing}<img src={f.data} alt="" />{:else}<span class="ficon"><Icon name={f.missing ? 'warning' : 'docs'} size={16} /></span>{/if}
                <span class="fmeta"><bdi class="ell fname">{f.name}</bdi><span class="muted">{f.missing ? t('chat3.reattach') : fmtSize(f.size)}</span></span>
                <button class="x" type="button" onclick={() => files.splice(i, 1)} aria-label={t('chat.removeFile', { name: f.name })} title={t('chat.removeFile', { name: f.name })}><Icon name="close" size={13} /></button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if mentions.length}
          <ul class="files chips" aria-label={t('chat3.mentioned')}>
            {#each mentions as p (p)}
              <li class="chip"><span aria-hidden="true">@</span><bdi class="ell">{p}</bdi>
                <button class="x" type="button" onclick={() => dropMention(p)} aria-label={t('chat3.removeMention', { path: p })} title={t('chat3.removeMention', { path: p })}><Icon name="close" size={13} /></button>
              </li>
            {/each}
          </ul>
        {/if}
        <textarea bind:this={input} bind:value={text} {oninput} onkeydown={keydown} {onpaste} rows="2" dir="auto"
          placeholder={t('chat.placeholder')} aria-label={t('chat.messageLabel')}
          aria-controls={mentionHits.length ? 'mention-list' : undefined} aria-activedescendant={mentionHits.length ? `mention-${mentionIdx}` : undefined}></textarea>
        <div class="row controls">
          <label class="btn ghost" title={t('chat.attachTip')}><Icon name="attach" /><span class="sr">{t('chat.attachTip')}</span><input type="file" multiple class="sr" onchange={attach} /></label>
          <select bind:value={chat.options.model} onchange={() => saveOpts()} title={t('model')} aria-label={t('model')}>
            <option value={null}>{t('model')}: {t('default')}</option>
            {#each opts.models as m}<option value={m}>{m}</option>{/each}
            {#if chat.options.model && !opts.models.includes(chat.options.model)}<option value={chat.options.model}>{chat.options.model}</option>{/if}
          </select>
          {#if opts.efforts.length}
            <select bind:value={chat.options.effort} onchange={() => saveOpts()} title={t('effort')} aria-label={t('effort')}>
              <option value={null}>{t('effort')}: {t('default')}</option>
              {#each opts.efforts as m}<option value={m}>{label('chat.effort', m)}</option>{/each}
            </select>
          {/if}
          {#if permModes.length}
            <select bind:value={chat.options.permissionMode} onchange={permChanged} class:bypass={chat.options.permissionMode === BYPASS}
              title={chat.options.permissionMode === BYPASS ? t('perm.bypass.on') : t('chat.permissionTip')} aria-label={t('permission')}>
              <option value={null}>{t('permission')}: {t('default')}</option>
              {#each permModes as m}<option value={m}>{label('chat3.perm', m)}</option>{/each}
            </select>
          {/if}
          <span class="grow"></span>
          <span class="esc muted" role="status" aria-live="polite">{#if escHint}{t('chat3.escHint')}{/if}</span>
          {#if chat.running}
            <button class="btn danger" onclick={stop} title={t('chat.stopTip')}><Icon name="stop" size={13} /> {t('stop')}</button>
          {:else}
            <button class="btn primary send" onclick={send} disabled={!canSend} title={t('chat.sendTip')} aria-label={t('send')}><Icon name="send" size={16} stroke={2.25} /></button>
          {/if}
        </div>
      </div>
    {/if}

    {#if dragging}<div class="dropzone" aria-hidden="true"><Icon name="upload" size={26} /><span>{t('chat.dropHere')}</span></div>{/if}
  </div>
{/if}

<style>
  .wrap { position: relative; display: flex; flex-direction: column; height: calc(100vh - 26px); padding: 0 24px 16px; max-width: 980px; margin: 0 auto; }
  header { padding-bottom: 8px; border-bottom: var(--border-w) solid var(--border); }
  header select { width: auto; }
  .h { margin: 0; font-size: inherit; font-weight: inherit; letter-spacing: 0; line-height: inherit; min-width: 0; display: flex; border-radius: 6px; }
  .title { max-width: 100%; font: inherit; color: inherit; background: none; border: 0; padding: 2px 4px; margin-inline-start: -4px; border-radius: 6px; text-align: start; font-weight: 600; font-size: 16px; cursor: text; }
  .title:hover { background: var(--fill); }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; padding: 0; border: 0; }
  .cost { font: inherit; font-size: var(--fs-xs); font-weight: 500; border: 0; cursor: help; display: inline-flex; align-items: center; gap: 4px; }
  .banner { margin-top: 8px; font-size: var(--fs-sm); padding: 6px 10px; border-radius: var(--radius); background: var(--panel-2); border: var(--border-w) dashed var(--accent); }

  .state { align-items: center; justify-content: center; text-align: center; gap: 6px; }
  .state h2 { margin: 6px 0 0; }
  .state p { margin: 0; max-width: 420px; }
  .state .detail { font-size: var(--fs-xs); font-family: var(--mono); overflow-wrap: anywhere; }
  .state .row { margin-top: 12px; }
  .state :global(.back) { transform: scaleX(-1); }
  :global([dir="rtl"]) .state :global(.back) { transform: none; }

  .skel { border-radius: 10px; background: linear-gradient(90deg, var(--fill), var(--fill-2), var(--fill)); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
  .skel.line { height: 13px; }
  .title-sk { width: 220px; height: 20px; margin: 4px 0 14px; }
  .skel-col { display: flex; flex-direction: column; gap: 12px; padding-top: 18px; }
  .bubble-sk { width: 46%; height: 44px; border-radius: 16px; }
  .bubble-sk.short { width: 30%; }
  .end { align-self: flex-end; }
  .w80 { width: 80%; } .w70 { width: 70%; } .w60 { width: 60%; } .w40 { width: 40%; }
  @keyframes shimmer { to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .skel, .dots i { animation: none; } }

  .msgs-wrap { flex: 1; min-height: 0; position: relative; display: flex; }
  .msgs { flex: 1; padding: 16px 0; display: flex; flex-direction: column; gap: 10px; }
  .msg.user { display: flex; flex-direction: column; align-items: flex-end; gap: 2px; }
  .send { width: 32px; height: 32px; padding: 0; border-radius: 50%; }
  .think, .att, .errline, .working { display: flex; gap: 6px; align-items: center; }
  .think { min-width: 0; }
  .bubble { background: var(--accent); color: var(--accent-fg); padding: 9px 14px; max-width: 78%; white-space: pre-wrap; overflow-wrap: anywhere; box-shadow: var(--shadow); border-radius: 16px; }
  .bubble .att { opacity: .85; margin-top: 4px; }
  .msg.ai { max-width: 100%; min-width: 0; }
  .msg.ai .md { overflow-wrap: anywhere; }
  .msg.err { color: var(--danger); font-size: 13px; display: flex; flex-direction: column; align-items: flex-start; gap: 4px; }
  .errline { white-space: pre-wrap; align-items: flex-start; }
  .msg.sys { font-size: var(--fs-sm); }
  .hint { margin: auto; display: flex; flex-direction: column; align-items: center; gap: 8px; font-size: 13.5px; }

  .acts { display: flex; gap: 2px; opacity: 0; transition: opacity .15s; }
  .msg:hover .acts, .acts:focus-within, .acts.show { opacity: 1; }
  @media (hover: none) { .acts { opacity: 1; } }
  .act { font: inherit; font-size: var(--fs-xs); color: var(--muted); background: none; border: 0; border-radius: 7px; padding: 3px 7px; cursor: pointer; display: inline-flex; align-items: center; gap: 5px; }
  .act:hover { color: var(--fg); background: var(--fill); }
  .act.show { opacity: 1; }

  .toolrow { min-width: 0; }
  .tool { font: inherit; background: none; border: 0; padding: 2px 4px; margin-inline-start: -4px; border-radius: 6px; text-align: start; cursor: pointer; font-family: var(--mono); font-size: var(--fs-xs); color: var(--muted); display: flex; align-items: center; gap: 6px; max-width: 100%; }
  .tool:hover { color: var(--accent); }
  .tooldetail { margin: 4px 0 6px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); display: grid; gap: 4px; min-width: 0; }
  .tooldetail pre { margin: 0 0 4px; max-height: 260px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
  .tooldetail p { margin: 0; }
  .pill.bad { color: var(--danger); }

  .dots { display: inline-flex; gap: 3px; }
  .dots i { width: 5px; height: 5px; border-radius: 50%; background: currentColor; animation: blink 1.2s infinite both; }
  .dots i:nth-child(2) { animation-delay: .2s; } .dots i:nth-child(3) { animation-delay: .4s; }
  @keyframes blink { 0%, 80%, 100% { opacity: .25; } 40% { opacity: 1; } }

  .jump { position: absolute; bottom: 10px; inset-inline-start: 50%; transform: translateX(-50%); border-radius: 99px; box-shadow: var(--shadow-lg); background: var(--panel); border: 1px solid var(--hairline); font-size: var(--fs-sm); z-index: 3; }
  :global([dir="rtl"]) .jump { transform: translateX(50%); }

  .composer { position: relative; padding: 8px; border-radius: 16px; }
  .composer textarea { border: 0; resize: none; background: transparent; box-shadow: none; display: block; min-height: 56px; max-height: 40vh; overflow-y: hidden; }
  .esc { font-size: var(--fs-xs); }
  .composer:focus-within { box-shadow: var(--shadow), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }
  .controls { flex-wrap: wrap; }
  .controls select { width: auto; font-size: var(--fs-xs); padding: 4px 6px; }
  .controls select.bypass { color: var(--danger); border-color: var(--danger); font-weight: 600; }
  .controls label { position: relative; margin: 0; color: var(--fg); font-size: 14px; }
  .controls label:focus-within { box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent); }

  .mentions { position: absolute; bottom: 100%; inset-inline-start: 8px; margin-bottom: 4px; padding: 4px; background: var(--panel); border: var(--border-w) solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg); display: flex; flex-direction: column; min-width: 320px; max-width: calc(100% - 16px); z-index: 5; }
  .mentions button { font: inherit; color: inherit; background: none; border: 0; border-radius: 6px; text-align: start; padding: 5px 10px; cursor: pointer; font-family: var(--mono); font-size: var(--fs-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mentions button.sel { background: var(--accent); color: var(--accent-fg); }

  .files { list-style: none; margin: 0 0 6px; padding: 0; display: flex; flex-wrap: wrap; gap: 6px; }
  .file { display: flex; align-items: center; gap: 8px; padding: 4px 6px 4px 4px; border-radius: 10px; background: var(--fill); max-width: 240px; }
  .file img, .ficon { width: 36px; height: 36px; border-radius: 7px; object-fit: cover; flex: none; }
  .ficon { display: grid; place-items: center; background: var(--fill-2); color: var(--muted); }
  .fmeta { display: flex; flex-direction: column; min-width: 0; font-size: var(--fs-xs); line-height: 1.35; }
  .fname { display: block; font-weight: 500; }
  .file.gone { outline: 1px dashed var(--muted); outline-offset: -1px; }
  .file.gone .fname { color: var(--muted); }
  .chips { margin-bottom: 4px; }
  .chip { display: inline-flex; align-items: center; gap: 4px; max-width: 100%; padding: 0 2px 0 8px; padding-inline: 8px 2px; border-radius: 99px; background: var(--tint); color: var(--on-tint); font-family: var(--mono); font-size: var(--fs-xs); }
  .chip .x { color: inherit; }
  .x { font: inherit; color: var(--muted); background: none; border: 0; border-radius: 6px; padding: 4px; min-width: 24px; min-height: 24px; flex: none; cursor: pointer; display: grid; place-items: center; }
  .x:hover { color: var(--fg); background: var(--fill-2); }

  .dropzone { position: absolute; inset: 0 12px 8px; z-index: 10; pointer-events: none; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; font-weight: 500; color: var(--accent); border: 2px dashed var(--accent); border-radius: var(--radius-lg); background: color-mix(in srgb, var(--bg) 88%, transparent); }

  .approve { margin-bottom: 8px; }
  .sm { font-size: var(--fs-xs); }
</style>
