<script lang="ts">
  // What the first-run setup chat says about itself: that the assistant is at work, that setup did
  // not finish (and how to run it again), or that the library is ready and where to go from there.
  import { onMount } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import SubscriptionRisk from '../lib/SubscriptionRisk.svelte';
  import { on, post } from '../lib/api';
  import { app, command, go } from '../lib/store.svelte';
  import { failed } from '../lib/errors';
  import { t } from '../lib/i18n';
  import { isReady, primaryOf, programName, statusOf } from '../lib/assistantReady';

  let { chat, reload, reply, stop }: { chat: any; reload: () => void; reply: (text: string) => void; stop: () => void } = $props();

  const phase = $derived(String(chat?.meta?.phase ?? ''));
  const running = $derived(chat?.kind === 'setup' && !!chat?.running);
  // "Running" on record while nothing runs: the app was closed or restarted in the middle of setup.
  // (It also reads like that for an instant when a run ends, so wait a moment before saying so.)
  const cutOff = $derived(chat?.kind === 'setup' && !chat?.running && phase === 'running');
  let settled = $state(false);
  $effect(() => {
    if (!cutOff) { settled = false; return; }
    const timer = setTimeout(() => (settled = true), 2000);
    return () => clearTimeout(timer);
  });
  // Only a setup whose answers were kept can be run again (older versions did not keep them).
  const notFinished = $derived(chat?.kind === 'setup' && !chat?.running && !!chat?.meta?.prompt && (phase === 'failed' || (cutOff && settled)));
  // The hand-over: shown until the user says something in this chat.
  const ready = $derived(!!chat?.meta?.setup && phase === 'done' && !chat?.running && (chat?.messages?.length ?? 0) <= Number(chat?.meta?.doneAt ?? 0));
  const first = $derived(chat?.meta?.first?.path ? (chat.meta.first as { path: string; title: string }) : null);
  // Made without an assistant: setup waits until one is connected, then is offered here (FEAT-08).
  const waiting = $derived(chat?.kind === 'setup' && !chat?.running && phase === 'waiting');
  // It runs on the program chosen in Settings › Assistant (Claude Code or Codex).
  const program = $derived(primaryOf(app.settings));
  const connected = $derived(isReady(statusOf(app.status, program), program));

  let starting = $state(false);
  async function again() {
    if (starting) return;
    starting = true;
    try { await post(`/onboarding/setup/${chat.id}`); reload(); }
    catch (e) { failed(t(waiting ? 'sb.wait.failed' : 'sa.fail.failed'), e); }
    finally { starting = false; }
  }

  // The service says when setup has ended (a moment after the run itself): show what it left.
  onMount(() => on((e) => { if (e.type === 'setup.finished' && e.chatId === chat?.id) reload(); }));
</script>

{#if running}
  <div class="setup card" role="status">
    <span class="ic"><Icon name="ai" size={18} /></span>
    <div class="grow">
      <b>{t('sa.run.title')}</b>
      <p class="muted">{t('sa.run.body')}</p>
    </div>
    <div class="side">
      <button type="button" class="btn" onclick={() => command('tour')}>{t('sa.tour')}</button>
      <!-- Stopping keeps everything: the card then offers to run setup again. -->
      <button type="button" class="btn danger" onclick={stop} title={t('chata.stopRun')}><Icon name="stop" size={13} /> {t('stop')}</button>
    </div>
  </div>
{:else if waiting}
  <div class="setup card" role="status">
    <span class="ic"><Icon name="ai" size={18} /></span>
    <div class="grow">
      <b>{t(connected ? 'sb.wait.readyTitle' : 'sb.wait.title')}</b>
      <p class="muted">{connected ? t('sb.wait.readyBody') : t('ux-onboarding.wait.body', { name: programName(program) })}</p>
      {#if !connected && app.settings && !(program === 'claude' && app.settings.ai?.claude?.hasApiToken)}<SubscriptionRisk bind:ack={app.settings.subscriptionRiskAck} />{/if}
    </div>
    {#if connected}
      <button type="button" class="btn primary" disabled={starting} aria-busy={starting} onclick={again}><Icon name="ai" size={14} /> {starting ? t('sb.wait.starting') : t('sb.wait.run')}</button>
    {:else}
      <button type="button" class="btn primary" disabled={!(app.settings as any)?.subscriptionRiskAck && !(program === 'claude' && app.settings?.ai?.claude?.hasApiToken)} onclick={() => go({ kind: 'settings', id: 'ai' })}>{t('sb.wait.connect')}</button>
    {/if}
  </div>
{:else if notFinished}
  <div class="setup card bad" role="alert">
    <span class="ic"><Icon name="warning" size={18} /></span>
    <div class="grow">
      <b>{t('sa.fail.title')}</b>
      <p class="muted">{t('sa.fail.body')}</p>
    </div>
    <button type="button" class="btn primary" disabled={starting} aria-busy={starting} onclick={again}><Icon name="retry" size={14} /> {starting ? t('sa.fail.starting') : t('sa.fail.again')}</button>
  </div>
{:else if ready}
  <div class="setup card done" role="status">
    <span class="ic"><Icon name="check" size={18} /></span>
    <div class="grow">
      <b>{t('sa.done.title')}</b>
      <p class="muted">{t('sa.done.body')}</p>
      <div class="acts">
        <button type="button" class="btn primary" onclick={() => reply(t('sa.done.replyText'))}><Icon name="chat" size={14} /> {t('sa.done.reply')}</button>
        {#if first}
          <button type="button" class="btn" onclick={() => go({ kind: 'docs', path: first.path })}><Icon name="docs" size={14} /> <span class="ell" dir="auto">{t('sa.done.open', { title: first.title })}</span></button>
        {:else}
          <button type="button" class="btn" onclick={() => go({ kind: 'ideas' })}><Icon name="docs" size={14} /> {t('sa.done.library')}</button>
        {/if}
        <button type="button" class="btn" onclick={() => command('tour')}>{t('sa.tour')}</button>
      </div>
      <p class="muted sm">{t('sa.done.hint')}</p>
    </div>
  </div>
{/if}

<style>
  .setup { display: flex; gap: 12px; align-items: flex-start; margin: 8px 0; padding: 12px 14px; }
  .setup p { margin: 2px 0 0; font-size: var(--fs-sm); }
  .setup > .btn, .side { flex: none; align-self: center; }
  .side { display: flex; gap: 8px; }
  .ic { flex: none; width: 32px; height: 32px; border-radius: 10px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); }
  .bad { border-color: var(--danger); }
  .bad .ic { background: color-mix(in srgb, var(--danger) 12%, transparent); color: var(--danger); }
  .done .ic { color: var(--ok); }
  .acts { display: flex; flex-wrap: wrap; gap: 8px; margin: 10px 0 6px; }
  .acts .btn { max-width: 100%; min-width: 0; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sm { font-size: var(--fs-xs); }
  .grow { min-width: 0; }
  @media (max-width: 640px) { .setup { flex-wrap: wrap; } .setup > .btn, .side { align-self: flex-start; } }
</style>
