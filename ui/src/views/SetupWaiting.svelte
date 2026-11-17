<script lang="ts">
  // A library made before an assistant was connected (FEAT-08): its setup waits. Everywhere but the
  // setup chat itself, a quiet banner says what is waiting, and offers the setup once the assistant
  // is connected. "Not now" hides it until the app is opened again.
  import Icon from '../lib/Icon.svelte';
  import { post } from '../lib/api';
  import { app, go, refreshChats } from '../lib/store.svelte';
  import { failed } from '../lib/errors';
  import { t } from '../lib/i18n';
  import { isReady, isSignedOut, primaryOf, statusOf } from '../lib/assistantReady';

  const KEY = 'thoughts.setupWaiting.hidden';
  const read = () => { try { return sessionStorage.getItem(KEY) ?? ''; } catch { return ''; } };
  let hidden = $state(read());

  const waiting = $derived(app.chats.find((c: any) => c.kind === 'setup' && c.phase === 'waiting' && !c.running) ?? null);
  // The setup runs on the program chosen in Settings › Assistant (Claude Code or Codex): that one
  // has to be connected.
  const program = $derived(primaryOf(app.settings));
  const st = $derived(statusOf(app.status, program));
  const connected = $derived(isReady(st, program));
  // Installed but signed out: the app's own sign-in banner already says what to do.
  const signedOutShown = $derived(isSignedOut(st, program));
  // Not over the setup chat itself, nor over Settings, where the assistant is being connected.
  const here = $derived((app.view.kind === 'chat' && app.view.id === waiting?.id) || app.view.kind === 'settings');
  // A hidden "connect" banner comes back once the assistant is connected: that is news.
  const show = $derived(!!waiting && !here && (connected ? hidden !== 'ready' : !signedOutShown && hidden !== 'wait'));

  function hide() {
    hidden = connected ? 'ready' : 'wait';
    try { sessionStorage.setItem(KEY, hidden); } catch { /* private mode: hidden for this window only */ }
  }

  let starting = $state(false);
  async function run() {
    if (!waiting || starting) return;
    starting = true;
    const id = waiting.id;
    try { await post(`/onboarding/setup/${id}`); await refreshChats().catch(() => {}); go({ kind: 'chat', id }); }
    catch (e) { failed(t('sb.wait.failed'), e); }
    finally { starting = false; }
  }
</script>

{#if show}
  <div class="waitbar no-print" role="status">
    <Icon name="ai" size={16} />
    <span class="grow">{t(connected ? 'sb.wait.bannerReady' : 'sb.wait.banner')}</span>
    {#if connected}
      <button class="btn primary" disabled={starting} aria-busy={starting} onclick={run}>{starting ? t('sb.wait.starting') : t('sb.wait.run')}</button>
      <button class="btn" onclick={hide}>{t('sb.wait.later')}</button>
    {:else}
      <button class="btn primary" onclick={() => go({ kind: 'settings', id: 'ai' })}>{t('sb.wait.connect')}</button>
      <button class="iconbtn" aria-label={t('sb.wait.hide')} title={t('sb.wait.hide')} onclick={hide}><Icon name="close" size={14} /></button>
    {/if}
  </div>
{/if}

<style>
  /* Like the app's other banners above the view. */
  .waitbar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin: 4px 28px 0; padding: 10px 14px; border-radius: var(--radius-lg); background: var(--panel); border: var(--border-w) solid var(--accent-2); font-size: 13px; }
  .waitbar > :global(.icon) { flex: none; color: var(--accent-2); }
  .waitbar .grow { min-width: 220px; }
  @media (max-width: 640px) { .waitbar { margin-inline: 12px; } }
</style>
