<script lang="ts">
  // The step's conversation, beside its documents. A compact reading of the same chat the Chats
  // view shows (the same service calls, the same merging of live events: views/chat/merge.ts):
  // what was said, the answer being written, and a box to reply. Everything else a chat can do
  // (files, options, the steps the assistant took) is one click away in the full chat.
  import { onMount, tick, untrack } from 'svelte';
  import { ArrowUp, Square, ExternalLink, MessagesSquare } from '@lucide/svelte';
  import { api, on, post, ApiError } from '../../lib/api';
  import { md, mdClick } from '../../lib/md';
  import { t } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import { go } from '../../lib/store.svelte';
  import { composerKey } from '../../lib/composer/keys';
  import StreamText from '../chat/StreamText.svelte';
  import { liveLine } from '../chat/steps';
  import { Fetches, applyEvent, applyPartial, emptyStream, type Effects, type Stream } from '../chat/merge';
  import type { Chat, DaemonEvent } from '../../lib/types';

  let { chatId, placeholder = '', emptyTitle, emptyBody }: { chatId: string; placeholder?: string; emptyTitle: string; emptyBody: string } = $props();

  let chat = $state<Chat | null>(null);
  let phase = $state<'loading' | 'ok' | 'missing' | 'error'>('loading');
  let stream = $state<Stream>(emptyStream());
  let text = $state('');
  let sending = $state(false);
  let scroller = $state<HTMLElement>();
  let stick = true;
  const fetches = new Fetches();
  const id = untrack(() => chatId);

  /** What is shown: what people said and what the assistant answered. The steps in between stay in the full chat. */
  const shown = $derived((chat?.messages ?? []).filter((m) => (m.role === 'user' || m.role === 'assistant' || (m.role === 'error' && m.tool?.code !== 'sessionLost')) && m.text?.trim()));

  async function load(first = false) {
    const ticket = fetches.begin();
    try {
      const c = await api<Chat>(`/chats/${encodeURIComponent(id)}`);
      if (!fetches.accept(ticket)) return;
      chat = c;
      applyPartial(c, stream, c.partial);
      for (const e of fetches.replay()) onEvent(e, true);
      phase = 'ok';
      down(first);
    } catch (e) {
      if (ticket < fetches.newest || chat) return;
      phase = e instanceof ApiError && e.status === 404 ? 'missing' : 'error';
    } finally { fetches.end(); }
  }

  function onEvent(e: DaemonEvent, again = false) {
    if (!chat) return;
    const fx: Effects = applyEvent(chat, stream, e, { again, loading: fetches.loading, sending });
    if (fx.scroll) down(fx.scroll === 'force');
    if (fx.reload) load();
  }

  async function down(force = false) {
    if (!force && !stick) return;
    await tick();
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
    stick = true;
  }
  function onscroll() { if (scroller) stick = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 24; }

  onMount(() => {
    load(true);
    return on((e) => {
      if (e.type === 'resync') { load(); return; }
      if (e.chatId !== id && e.chat?.id !== id) return;
      fetches.hold(e);
      if (chat) onEvent(e);
    });
  });

  async function send() {
    const words = text.trim();
    if (!words || !chat || sending || chat.running) return;
    sending = true;
    chat.running = true;
    text = '';
    try { await post(`/chats/${encodeURIComponent(id)}/send`, { text: words, attachments: [], mentions: [], options: chat.options }); }
    catch (e) { text = words; chat.running = false; failed(t('chat3.sendFailed'), e); }
    finally { sending = false; }
  }
  async function stop() {
    try { await post(`/chats/${encodeURIComponent(id)}/stop`); } catch (e) { failed(t('chat3.stopFailed'), e); }
  }
  function key(e: KeyboardEvent) {
    if (composerKey(e, false) === 'send') { e.preventDefault(); send(); }
  }
</script>

<div class="chat">
  <div class="top row">
    <span class="grow muted sm">{t('mado.chat.about')}</span>
    <button type="button" class="more" onclick={() => go({ kind: 'chat', id })}>{t('mado.chat.openFull')} <ExternalLink size={12} aria-hidden="true" /></button>
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events -->
  <div class="log" bind:this={scroller} {onscroll} onclick={mdClick} role="log" aria-label={t('mado.chat.label')} tabindex="-1">
    {#if phase === 'loading'}
      <p class="muted center" role="status">{t('mado.loading')}</p>
    {:else if phase !== 'ok' || !chat}
      <p class="muted center">{t(phase === 'missing' ? 'mado.chat.missing' : 'mado.chat.failed')}</p>
    {:else}
      {#if !shown.length && !chat.running}
        <div class="empty"><MessagesSquare size={24} strokeWidth={1.5} aria-hidden="true" /><b>{emptyTitle}</b><span class="muted">{emptyBody}</span></div>
      {/if}
      {#each shown as m (m.id)}
        {#if m.role === 'user'}<div class="msg me" dir="auto">{m.text}</div>
        {:else if m.role === 'assistant'}<div class="msg ai md">{@html md(m.text)}</div>
        {:else}<div class="msg err" role="alert" dir="auto">{m.text}</div>{/if}
      {/each}
      {#if stream.text}<div class="msg ai md" aria-busy="true"><StreamText text={stream.text} onrender={() => down()} /></div>{/if}
      {#if chat.running && !stream.text}
        <div class="working muted" role="status"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span> {liveLine(chat.messages, !!stream.thinking)}</div>
      {/if}
    {/if}
  </div>
  {#if phase === 'ok' && chat}
    <form class="box" onsubmit={(e) => { e.preventDefault(); send(); }}>
      <label class="sr-only" for="mado-chat-{id}">{t('mado.chat.boxLabel')}</label>
      <textarea id="mado-chat-{id}" bind:value={text} rows="2" dir="auto" placeholder={placeholder || t('mado.chat.placeholder')} onkeydown={key}></textarea>
      {#if chat.running}
        <button type="button" class="btn sendb" onclick={stop} aria-label={t('stop')} title={t('stop')}><Square size={14} /></button>
      {:else}
        <button type="submit" class="btn primary sendb" disabled={!text.trim() || sending} aria-label={t('send')} title={t('send')}><ArrowUp size={16} /></button>
      {/if}
    </form>
  {/if}
</div>

<style>
  .chat { display: flex; flex-direction: column; min-height: 0; height: 100%; gap: 8px; }
  .sm { font-size: var(--fs-xs); }
  .log { flex: 1; min-height: 120px; overflow: auto; display: flex; flex-direction: column; gap: 10px; padding: 4px 2px; overscroll-behavior: contain; }
  .msg { font-size: 13.5px; line-height: 1.55; overflow-wrap: anywhere; animation: mado-msg .25s var(--ease) both; }
  @keyframes mado-msg { from { opacity: 0; transform: translateY(6px); } }
  .me { align-self: flex-end; max-width: 88%; background: var(--tint); color: var(--on-tint); padding: 7px 11px; border-radius: 14px; border-end-end-radius: 4px; white-space: pre-wrap; }
  .ai :global(p:last-child) { margin-bottom: 0; }
  .ai :global(h1), .ai :global(h2), .ai :global(h3) { font-size: 14.5px; font-weight: 600; margin: 10px 0 6px; letter-spacing: 0; }
  .err { color: var(--danger); font-size: 13px; }
  .working { font-size: 13px; display: flex; gap: 8px; align-items: center; }
  .dots { display: inline-flex; gap: 3px; }
  .dots i { width: 5px; height: 5px; border-radius: 99px; background: currentColor; animation: mado-dot 1.2s infinite ease-in-out; }
  .dots i:nth-child(2) { animation-delay: .15s; } .dots i:nth-child(3) { animation-delay: .3s; }
  @keyframes mado-dot { 0%, 80%, 100% { opacity: .25; } 40% { opacity: 1; } }
  .center { text-align: center; padding: 24px 0; }
  .empty { margin: auto 0; display: grid; justify-items: center; gap: 6px; text-align: center; padding: 24px 12px; color: var(--muted); }
  .empty b { color: var(--fg); }
  .empty span { font-size: 13px; max-width: 34ch; }
  .box { position: relative; display: flex; gap: 6px; align-items: flex-end; }
  .box textarea { resize: none; min-height: 44px; max-height: 160px; border-radius: 14px; padding-inline-end: 44px; field-sizing: content; }
  .sendb { position: absolute; inset-inline-end: 6px; bottom: 6px; width: 32px; height: 32px; padding: 0; border-radius: 99px; }
</style>
