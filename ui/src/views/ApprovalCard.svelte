<script lang="ts">
  // One thing the assistant asks permission for, in the chat: what it wants to do, in plain words,
  // why the chat's mode did not let it do so on its own, and Allow once / Always for this chat /
  // Deny. The assistant waits for the answer; unanswered, the request is refused when its time
  // runs out. Once answered, the card stays as a one-line record of what was decided.
  import Icon from '../lib/Icon.svelte';
  import { post } from '../lib/api';
  import { lang, t, tn } from '../lib/i18n';
  import { failed } from '../lib/errors';

  let { m, chatId, running }: { m: any; chatId: string; running: boolean } = $props();

  const info = $derived(m.tool ?? {});
  const card = $derived(info.card ?? {});
  const status = $derived(String(info.status ?? 'waiting'));
  // A card the service left waiting when it stopped (the run is over) is no longer waiting.
  const live = $derived(status === 'waiting' && running);
  const shownStatus = $derived(status === 'waiting' && !running ? 'ended' : status);
  const WHAT = ['command', 'change', 'open', 'look', 'connected', 'tool'];
  const what = $derived(WHAT.includes(card.what) ? card.what : 'tool');
  const WHY = ['command', 'planCommand', 'change', 'plan', 'outside', 'read', 'connected', 'other', 'strict'];
  const why = $derived(WHY.includes(card.why) ? card.why : 'other');
  const modeName = $derived.by(() => { const k = `chat3.perm.${card.mode}`; const s = t(k); return s === k ? String(card.mode ?? '') : s; });
  const target = $derived(String(card.target || (what === 'tool' ? card.tool ?? '' : '')));

  let open = $state(false);
  let busy = $state(false);
  let now = $state(Date.now());
  $effect(() => {
    if (!live) return;
    const h = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(h);
  });
  const left = $derived(Math.max(0, (Date.parse(info.expiresAt ?? '') || 0) - now));
  const waitingText = $derived(left < 60_000 ? t('appr.waitingSoon') : tn('appr.waiting', Math.ceil(left / 60_000)));

  async function reply(answer: 'once' | 'always' | 'deny') {
    if (busy) return;
    busy = true;
    // The card itself changes when the service has the answer (it sends the card again).
    try { await post(`/chats/${chatId}/approvals/${m.id}`, { answer }); }
    catch (e) { failed(t('appr.failed'), e); }
    finally { busy = false; }
  }

  const ISOLATE = [String.fromCharCode(0x2068), String.fromCharCode(0x2069)];
</script>

<div class="approval" class:live class:strict={card.strict} role="group" aria-labelledby="appr-{m.id}">
  <div class="head">
    <Icon name={card.strict ? 'warning' : 'shield'} size={14} />
    {#if live}
      <p class="title" id="appr-{m.id}" role="alert">{t(`appr.title.${what}`)}</p>
    {:else}
      <p class="title" id="appr-{m.id}">{t(`appr.asked.${what}`)}</p>
    {/if}
  </div>
  {#if target}
    {#if what === 'command'}
      <pre class="target cmd" dir="ltr">{target}</pre>
    {:else}
      <p class="target"><bdi>{target}</bdi></p>
    {/if}
  {/if}
  {#if live}
    <p class="why" dir="auto">{t(`appr.why.${why}`, { mode: modeName })}</p>
    {#if card.strict}
      <p class="warn" dir="auto"><Icon name="warning" size={13} /> <span>{t('appr.strictWarn', { folders: (card.private ?? []).map((f: string) => ISOLATE[0] + f + ISOLATE[1]).join(lang() === 'ar' ? '، ' : ', ') })}</span></p>
    {/if}
    <div class="row acts">
      <button class="btn primary" disabled={busy} onclick={() => reply('once')}>{t('appr.allowOnce')}</button>
      {#if card.always}
        <button class="btn" disabled={busy} onclick={() => reply('always')} title={t(`appr.alwaysTip.${card.always}`)}>{t('appr.allowAlways')}</button>
      {/if}
      <button class="btn danger" disabled={busy} onclick={() => reply('deny')}>{t('appr.deny')}</button>
      <span class="grow"></span>
      <span class="muted left" role="timer" aria-live="off">{waitingText}</span>
    </div>
  {:else}
    <p class="done" class:no={shownStatus !== 'once' && shownStatus !== 'always'} dir="auto">
      <Icon name={shownStatus === 'once' || shownStatus === 'always' ? 'check' : 'close'} size={13} /> <span>{t(`appr.status.${shownStatus}`)}</span>
    </p>
  {/if}
  <button class="more" onclick={() => (open = !open)} aria-expanded={open}>
    <Icon name={open ? 'down' : 'right'} size={12} class={open ? '' : 'flip-rtl'} /> {t(open ? 'appr.hideDetails' : 'appr.showDetails')}
  </button>
  {#if open}<pre class="detail" dir="ltr">{card.tool}{'\n'}{JSON.stringify(card.detail ?? {}, null, 2)}</pre>{/if}
</div>

<style>
  .approval { font-size: var(--fs-sm); padding: 8px 12px; border-radius: var(--radius); border: var(--border-w) dashed var(--border); display: flex; flex-direction: column; align-items: stretch; gap: 6px; min-width: 0; }
  .approval.live { border-style: solid; border-color: var(--accent); background: var(--panel-2); }
  .approval.live.strict { border-color: var(--danger); }
  .head { display: flex; gap: 6px; align-items: center; }
  .title { margin: 0; font-weight: 600; }
  .strict .head > :global(.icon), .warn > :global(.icon) { color: var(--danger); flex: none; }
  .target { margin: 0; overflow-wrap: anywhere; font-family: var(--mono); font-size: var(--fs-xs); }
  .cmd { padding: 6px 8px; border-radius: 7px; background: var(--fill); white-space: pre-wrap; max-height: 180px; overflow: auto; }
  .why { margin: 0; }
  .warn { margin: 0; display: flex; gap: 6px; align-items: flex-start; color: var(--danger); }
  .acts { flex-wrap: wrap; gap: 6px; }
  .left { font-size: var(--fs-xs); }
  .done { margin: 0; display: flex; gap: 6px; align-items: center; color: var(--muted); }
  .done.no > :global(.icon) { color: var(--danger); }
  .more { align-self: flex-start; font: inherit; font-size: var(--fs-xs); color: var(--muted); background: none; border: 0; border-radius: 6px; padding: 2px 4px; margin-inline-start: -4px; cursor: pointer; display: inline-flex; align-items: center; gap: 4px; }
  .more:hover { color: var(--fg); background: var(--fill); }
  .detail { margin: 0; padding: 6px 8px; border-radius: 7px; background: var(--fill); font-size: var(--fs-xs); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 260px; overflow: auto; }
  @media print { .acts, .more, .left { display: none !important; } }
</style>
