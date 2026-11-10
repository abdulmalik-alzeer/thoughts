<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onMount, tick } from 'svelte';
  import { api, del, on, patch, post } from '../lib/api';
  import { md } from '../lib/md';
  import { app, go, refreshChats, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';

  let { id, projects }: { id: string; projects: any[] } = $props();

  let chat = $state<any>(null);
  let streaming = $state('');
  let thinking = $state('');
  let text = $state('');
  let files = $state<{ name: string; data: string }[]>([]);
  let mentions = $state<string[]>([]);
  let opts = $state<any>({ models: [], efforts: [], permissionModes: [] });
  let tree = $state<string[]>([]);
  let mentionQ = $state<string | null>(null);
  let note = $state('');
  let scroller: HTMLElement;
  let input: HTMLTextAreaElement;

  async function load() {
    chat = await api(`/chats/${id}`);
    chat.options ??= {};
    await scrollDown();
  }

  async function scrollDown() {
    await tick();
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  }

  onMount(() => {
    load();
    api('/ai/claude/options').then((o) => (opts = o)).catch(() => {});
    api('/library/tree').then((x: any[]) => (tree = x.map((f) => f.path).filter((p) => p.endsWith('.md')))).catch(() => {});
    return on((e) => {
      if (e.chatId !== id && e.chat?.id !== id) return;
      if (!chat) return;
      if (e.type === 'chat.message') {
        if (!chat.messages.some((m: any) => m.id === e.message.id)) chat.messages.push(e.message);
        if (e.message.role === 'assistant') streaming = '';
        if (e.message.role !== 'user') thinking = '';
        scrollDown();
      } else if (e.type === 'chat.delta') {
        streaming += e.text;
        scrollDown();
      } else if (e.type === 'chat.thinking') {
        thinking += e.text;
      } else if (e.type === 'chat.done') {
        streaming = ''; thinking = '';
        load();
      } else if (e.type === 'chat.updated') {
        chat.running = e.chat.running;
        chat.title = e.chat.title;
      }
    });
  });

  async function send() {
    if (!text.trim() || chat.running) return;
    const body = { text, attachments: files, mentions, options: chat.options };
    text = ''; files = []; mentions = [];
    chat.running = true;
    try { await post(`/chats/${id}/send`, body); }
    catch (e: any) { toast(e.message); chat.running = false; }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); send(); }
    if (e.key === 'Escape') mentionQ = null;
  }

  function oninput() {
    const before = text.slice(0, input.selectionStart);
    const m = before.match(/@([\w\-./]*)$/);
    mentionQ = m ? m[1] : null;
  }

  function pickMention(p: string) {
    const pos = input.selectionStart;
    const before = text.slice(0, pos).replace(/@([\w\-./]*)$/, '@' + p + ' ');
    text = before + text.slice(pos);
    if (!mentions.includes(p)) mentions.push(p);
    mentionQ = null;
    input.focus();
  }

  const mentionHits = $derived(mentionQ === null ? [] : tree.filter((p) => p.toLowerCase().includes(mentionQ!.toLowerCase())).slice(0, 8));

  async function attach(e: Event) {
    for (const f of (e.target as HTMLInputElement).files || []) {
      const data = await new Promise<string>((res) => { const r = new FileReader(); r.onload = () => res(r.result as string); r.readAsDataURL(f); });
      files.push({ name: f.name, data });
    }
  }

  async function saveOpts() {
    await patch(`/chats/${id}`, { options: chat.options });
  }

  async function moveTo(pid: string) {
    await patch(`/chats/${id}`, { projectId: pid || null });
    chat.projectId = pid || null;
    refreshChats();
  }

  async function rename() {
    const title = prompt('Chat title', chat.title);
    if (title) { await patch(`/chats/${id}`, { title }); chat.title = title; refreshChats(); }
  }

  async function remove() {
    if (!confirm('Delete this chat? Its transcript is removed from this computer.')) return;
    await del(`/chats/${id}`);
    await refreshChats();
    go({ kind: 'chats' });
  }

  async function applyWrap() {
    try { await post(`/wrap/${id}/apply`, { note }); note = ''; chat.running = true; }
    catch (e: any) { toast(e.message); }
  }

  function toolOpen(m: any) { m._open = !m._open; }
</script>

{#if chat}
  <div class="wrap">
    <header class="row">
      <button class="title grow ell" onclick={rename} title="Rename">{chat.title}</button>
      {#if chat.kind === 'chat'}
        <select value={chat.projectId ?? ''} onchange={(e) => moveTo((e.target as HTMLSelectElement).value)} title="Group">
          <option value="">{t('library')}</option>
          {#each projects as p}<option value={p.id}>{p.name}</option>{/each}
        </select>
      {:else}
        <span class="pill accent">{chat.kind}</span>
      {/if}
      {#if chat.costUsd > 0}<span class="pill" title="API-equivalent cost">${chat.costUsd.toFixed(2)}</span>{/if}
      <button class="btn ghost" onclick={remove} title={t('delete')}><Icon name="trash" /></button>
    </header>

    {#if chat.kind === 'customize'}
      <div class="banner">This chat edits the app itself (your app layer). Every change is committed — use {t('rollback')} to undo anything.</div>
    {/if}

    <div class="msgs scroll" bind:this={scroller}>
      {#each chat.messages as m (m.id)}
        {#if m.role === 'user'}
          <div class="msg user"><div class="bubble" dir="auto">{m.text}
            {#if m.attachments?.length}<div class="muted sm att"><Icon name="attach" size={12} /> {m.attachments.map((a: string) => a.split('/').pop()).join(', ')}</div>{/if}
            {#if m.tool?.mentions}<div class="muted sm">@ {m.tool.mentions.join(', ')}</div>{/if}
          </div></div>
        {:else if m.role === 'assistant'}
          <div class="msg ai md" dir="auto">{@html md(m.text)}</div>
        {:else if m.role === 'tool'}
          <button class="tool" onclick={() => toolOpen(m)}><Icon name="tool" size={13} /> <span class="ell">{m.text}</span></button>
          {#if m._open}<pre class="sm">{JSON.stringify(m.tool?.input, null, 2)}</pre>{/if}
        {:else if m.role === 'tool_result'}
          {#if m._open}<pre class="sm">{m.text}</pre>{/if}
        {:else if m.role === 'error'}
          <div class="msg err"><Icon name="warning" size={14} /> {m.text}</div>
        {:else}
          <div class="msg sys muted">{m.text}</div>
        {/if}
      {/each}
      {#if thinking}<div class="msg sys muted ell think"><Icon name="thinking" size={13} /> {thinking.slice(-160)}</div>{/if}
      {#if streaming}<div class="msg ai md">{@html md(streaming)}</div>{/if}
      {#if chat.running && !streaming}<div class="msg sys muted">…</div>{/if}
    </div>

    {#if chat.kind === 'wrap' && !chat.running && chat.meta?.phase === 'preview' && chat.messages.some((m: any) => m.role === 'assistant')}
      <div class="approve card row">
        <input class="grow" placeholder="Optional changes to the plan…" bind:value={note} />
        <button class="btn primary" onclick={applyWrap}><Icon name="check" size={15} /> Approve & apply</button>
      </div>
    {/if}
    {#if chat.kind === 'reorganize' && !chat.running && chat.meta?.phase === 'review'}
      <div class="approve card row"><span class="grow">The reorganized library is ready on branch <code>{chat.meta.branch}</code>.</span>
        <button class="btn primary" onclick={() => go({ kind: 'reorganize', id })}>Review diff</button></div>
    {/if}

    {#if !['wrap', 'reorganize', 'setup'].includes(chat.kind)}
      <div class="composer card">
        {#if mentionHits.length}
          <div class="mentions">{#each mentionHits as p}<button onclick={() => pickMention(p)}>{p}</button>{/each}</div>
        {/if}
        {#if files.length}<div class="row sm">{#each files as f, i}<span class="pill"><Icon name="attach" size={12} /> {f.name} <button class="x" onclick={() => files.splice(i, 1)}><Icon name="close" size={12} /></button></span>{/each}</div>{/if}
        <textarea bind:this={input} bind:value={text} {oninput} onkeydown={keydown} rows="3" placeholder={t('typeMessage')}></textarea>
        <div class="row controls">
          <label class="btn ghost" title={t('attach')}><Icon name="attach" /><input type="file" multiple hidden onchange={attach} /></label>
          <select bind:value={chat.options.model} onchange={saveOpts} title={t('model')}>
            <option value={null}>{t('model')}: {t('default')}</option>
            {#each opts.models as m}<option value={m}>{m}</option>{/each}
            {#if chat.options.model && !opts.models.includes(chat.options.model)}<option value={chat.options.model}>{chat.options.model}</option>{/if}
          </select>
          {#if opts.efforts.length}
            <select bind:value={chat.options.effort} onchange={saveOpts} title={t('effort')}>
              <option value={null}>{t('effort')}: {t('default')}</option>
              {#each opts.efforts as m}<option value={m}>{m}</option>{/each}
            </select>
          {/if}
          {#if opts.permissionModes.length}
            <select bind:value={chat.options.permissionMode} onchange={saveOpts} title={t('permission')}>
              <option value={null}>{t('permission')}: {t('default')}</option>
              {#each opts.permissionModes as m}<option value={m}>{m}</option>{/each}
            </select>
          {/if}
          <span class="grow"></span>
          {#if chat.running}
            <button class="btn danger" onclick={() => post(`/chats/${id}/stop`)}><Icon name="stop" size={13} /> {t('stop')}</button>
          {:else}
            <button class="btn primary send" onclick={send} disabled={!text.trim()} title="{t('send')} (⌘↩)"><Icon name="send" size={16} stroke={2.25} /></button>
          {/if}
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .wrap { display: flex; flex-direction: column; height: calc(100vh - 26px); padding: 0 24px 16px; max-width: 980px; margin: 0 auto; }
  header { padding-bottom: 8px; border-bottom: var(--border-w) solid var(--border); }
  header select { width: auto; }
  .title { all: unset; font-weight: 600; font-size: 16px; cursor: text; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .banner { margin-top: 8px; font-size: 12px; padding: 6px 10px; border-radius: var(--radius); background: var(--panel-2); border: var(--border-w) dashed var(--accent); }
  .msgs { flex: 1; padding: 16px 0; display: flex; flex-direction: column; gap: 10px; }
  .msg.user { display: flex; justify-content: flex-end; }
  .send { width: 32px; height: 32px; padding: 0; border-radius: 50%; }
  .think, .msg.err, .att { display: flex; gap: 6px; align-items: center; }
  .composer { border-radius: 16px; }
  .bubble { background: var(--accent); color: var(--accent-fg); padding: 9px 14px; border-radius: var(--radius); max-width: 78%; white-space: pre-wrap; box-shadow: var(--shadow); border-radius: 16px; }
  .bubble .muted { color: inherit; opacity: .8; }
  .msg.ai { max-width: 100%; }
  .msg.err { color: var(--danger); white-space: pre-wrap; font-size: 13px; }
  .msg.sys { font-size: 12px; }
  .tool { all: unset; cursor: pointer; font-family: var(--mono); font-size: 12px; color: var(--muted); display: flex; gap: 6px; max-width: 100%; }
  .tool:hover { color: var(--accent); }
  .composer { position: relative; padding: 8px; }
  .composer textarea { border: 0; resize: none; background: transparent; outline: none; }
  .controls select { width: auto; font-size: 12px; padding: 4px 6px; }
  .controls label { margin: 0; color: var(--fg); font-size: 14px; }
  .mentions { position: absolute; bottom: 100%; inset-inline-start: 8px; background: var(--panel); border: var(--border-w) solid var(--border); border-radius: var(--radius); display: flex; flex-direction: column; min-width: 320px; z-index: 5; }
  .mentions button { all: unset; padding: 5px 10px; cursor: pointer; font-family: var(--mono); font-size: 12px; }
  .mentions button:hover { background: var(--accent); color: var(--accent-fg); }
  .approve { margin-bottom: 8px; }
  .sm { font-size: 12px; }
  .x { all: unset; cursor: pointer; }
</style>
