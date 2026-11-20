<script lang="ts">
  // Editing a message already sent, in its own row. The user's last message is changed in place
  // (the service stops a running answer, replaces the message and its answer, and sends it again);
  // an earlier one goes into a branch — a new chat — and this chat stays as it is. Logic: branch.ts.
  import { onMount, tick, untrack } from 'svelte';
  import Icon from '../../lib/Icon.svelte';
  import { post } from '../../lib/api';
  import { failed, errDetail } from '../../lib/errors';
  import { t, tn } from '../../lib/i18n';
  import Composer from '../../lib/composer/Composer.svelte';
  import { composerKey } from '../../lib/composer/keys';
  import type { TextBox } from '../../lib/composer/textBox';
  import { go, refreshChats } from '../../lib/store.svelte';
  import { notify } from '../../lib/content/util';
  import { editNotes, fileName, madeKey } from './branch';

  let { chatId, chat, m, kind, mentionName, onclose }: {
    chatId: string; chat: any; m: any; kind: 'inplace' | 'branch';
    mentionName: (p: string) => string; onclose: () => void;
  } = $props();

  // (The message as it was when editing began: a later change to it does not reset what is typed.)
  const start = untrack(() => ({ text: String(m.text ?? ''), files: [...(m.attachments ?? [])] as string[], mentions: [...(m.tool?.mentions ?? [])] as string[] }));
  let text = $state(start.text);
  let out = $state<Record<string, boolean>>({});        // files left out, by path
  let dropped = $state<Record<string, boolean>>({});    // mentions left out, by path
  let busy = $state(false);
  let error = $state('');
  let box = $state<TextBox & { element: HTMLElement }>();
  const uid = untrack(() => `edit-${m.id}`);

  const notes = $derived(editNotes(chat, m.id));
  const keptFiles = $derived(start.files.filter((p) => !out[p]));
  const ready = $derived(!!text.trim() || keptFiles.length > 0);

  // The same formatted box as the chat's own (lib/composer); it grows up to 40% of the window.
  onMount(() => { tick().then(() => { box?.focus(); box?.setSelectionRange(text.length, text.length); }); });

  async function submit() {
    if (!ready || busy) return;
    busy = true; error = '';
    const words = text.trim() ? text : tn('chat3.seeAttached', keptFiles.length);
    const body = {
      messageId: m.id, text: words, keepAttachments: keptFiles.map(fileName),
      mentions: start.mentions.filter((p) => !dropped[p]), options: chat.options,
    };
    try {
      if (kind === 'inplace') {
        await post(`/chats/${encodeURIComponent(chatId)}/edit`, body);
        onclose();
      } else {
        const r = await post(`/chats/${encodeURIComponent(chatId)}/branch`, body);
        refreshChats().catch(() => {});
        if (r?.how && r.how !== 'fork' && r.how !== 'none') notify(t(madeKey(r.how, chat.provider)), { kind: 'info', key: 'branch:' + r.id });
        onclose();
        if (r?.id) go({ kind: 'chat', id: r.id });
      }
    } catch (e) {
      error = errDetail(e) || t(kind === 'inplace' ? 'chatfBranch.editFailed' : 'chatfBranch.branchFailed');
      failed(t(kind === 'inplace' ? 'chatfBranch.editFailed' : 'chatfBranch.branchFailed'), e);
    } finally { busy = false; }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); if (!busy) onclose(); return; }
    if (composerKey(e, false) === 'send') { e.preventDefault(); submit(); }
  }
</script>

<div class="edit card" role="group" aria-label={t('chatfBranch.editRegion')} aria-busy={busy}>
  <Composer id={uid} bind:value={text} bind:box label={t('chatfBranch.editLabel')} disabled={busy} onkeydown={keydown}
    describedby="{uid}-note {uid}-keys" class="ebox" />
  {#if start.files.length}
    <div class="chips" role="group" aria-label={t('chatfBranch.files')}>
      {#each start.files as p (p)}
        <button type="button" class="chip" class:off={out[p]} aria-pressed={!out[p]} disabled={busy}
          aria-label={t(out[p] ? 'chatfBranch.putBack' : 'chatfBranch.leaveOut', { name: fileName(p) })}
          title={t(out[p] ? 'chatfBranch.putBack' : 'chatfBranch.leaveOut', { name: fileName(p) })}
          onclick={() => (out[p] = !out[p])}>
          <Icon name="attach" size={12} /><bdi class="ell">{fileName(p)}</bdi>
          {#if out[p]}<span class="muted">· {t('chatfBranch.leftOut')}</span>{:else}<Icon name="close" size={11} />{/if}
        </button>
      {/each}
    </div>
  {/if}
  {#if start.mentions.length}
    <div class="chips" role="group" aria-label={t('chatfBranch.mentions')}>
      {#each start.mentions as p (p)}
        <button type="button" class="chip" class:off={dropped[p]} aria-pressed={!dropped[p]} disabled={busy}
          aria-label={t(dropped[p] ? 'chatfBranch.putBack' : 'chatfBranch.leaveOut', { name: mentionName(p) })}
          title={t(dropped[p] ? 'chatfBranch.putBack' : 'chatfBranch.leaveOut', { name: mentionName(p) })}
          onclick={() => (dropped[p] = !dropped[p])}>
          @<bdi class="ell">{mentionName(p)}</bdi>
          {#if dropped[p]}<span class="muted">· {t('chatfBranch.leftOut')}</span>{:else}<Icon name="close" size={11} />{/if}
        </button>
      {/each}
    </div>
  {/if}
  <div class="notes" id="{uid}-note">
    {#if kind === 'branch'}
      <p class="lead"><Icon name="git" size={13} /> <span>{t('chatfBranch.noteEarlier')}</span></p>
    {:else}
      <p>{t('chatfBranch.noteLast')}{#if chat.running} {t('chatfBranch.noteRunning')}{/if}</p>
    {/if}
    {#if notes.saved}<p class="muted">{t('chatfBranch.noteSaved')}</p>{/if}
    {#if kind === 'inplace' && notes.changes}<p class="muted">{t('chatfBranch.noteChanges')}</p>{/if}
  </div>
  {#if error}<p class="err" role="alert" dir="auto">{error}</p>{/if}
  <div class="row foot">
    <span class="muted sm grow" id="{uid}-keys">{t('chatfBranch.keysHint')}</span>
    <button type="button" class="btn" onclick={onclose} disabled={busy}>{t('chatfBranch.cancel')}</button>
    <button type="button" class="btn primary" onclick={submit} disabled={!ready || busy}>
      {#if busy}<span class="dots" aria-hidden="true"><i></i><i></i><i></i></span> {t('chatfBranch.sending')}
      {:else}<Icon name={kind === 'branch' ? 'git' : 'send'} size={13} /> {t(kind === 'branch' ? 'chatfBranch.sendBranch' : 'chatfBranch.sendAgain')}{/if}
    </button>
  </div>
  {#if !ready}<span class="sr-only" role="status">{t('chatfBranch.empty')}</span>{/if}
</div>

<style>
  .edit { width: min(78%, 640px); align-self: flex-end; display: flex; flex-direction: column; gap: 8px; padding: 10px 12px; border: var(--border-w, 1px) solid var(--accent); border-radius: 16px; }
  .edit :global(.ebox) { --composer-min: 44px; color: var(--fg); background: var(--bg); border: var(--border-w, 1px) solid var(--border); border-radius: var(--radius); padding: 2px 6px; }
  .edit :global(.ebox:focus-within) { outline: 2px solid var(--accent); outline-offset: 1px; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .chip { font: inherit; font-size: var(--fs-xs); display: inline-flex; align-items: center; gap: 5px; max-width: 100%; padding: 3px 8px; border-radius: 999px; border: var(--border-w, 1px) solid var(--border); background: var(--fill); color: var(--fg); cursor: pointer; }
  .chip:hover:not(:disabled) { border-color: var(--accent); }
  .chip.off { background: none; text-decoration: line-through; color: var(--muted); }
  .chip .ell { max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .notes { display: grid; gap: 4px; font-size: var(--fs-sm); }
  .notes p { margin: 0; }
  .notes .lead { display: flex; gap: 6px; align-items: flex-start; }
  .err { margin: 0; color: var(--danger); font-size: var(--fs-sm); white-space: pre-wrap; }
  .foot { gap: 8px; align-items: center; flex-wrap: wrap; }
  .sm { font-size: var(--fs-xs); }
  .dots { display: inline-flex; gap: 2px; }
  .dots i { width: 4px; height: 4px; border-radius: 50%; background: currentColor; opacity: .6; animation: blink 1s infinite; }
  .dots i:nth-child(2) { animation-delay: .15s; } .dots i:nth-child(3) { animation-delay: .3s; }
  @keyframes blink { 50% { opacity: .15; } }
  @media (prefers-reduced-motion: reduce) { .dots i { animation: none; } }
  @media (max-width: 640px) { .edit { width: 100%; } }
</style>
