<script lang="ts">
  // Everything a Wrap offers on its own page, by phase: the plan (approve, discard), a plan that
  // failed (try again), a Wrap cut off while saving (finish, undo, keep), and a saved Wrap (what it
  // changed, undo).
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { on, post } from '../lib/api';
  import { failed } from '../lib/errors';
  import { command, go } from '../lib/store.svelte';
  import { fmtNum, t, tn } from '../lib/i18n';
  import { discardPlan, finishWrap, keepCutOffWrap, undoCutOffWrap, undoSavedWrap } from '../lib/wrap';

  let { chat, id, reload }: { chat: any; id: string; reload: () => Promise<unknown> } = $props();

  let note = $state('');
  let busy = $state(false);
  const meta = $derived(chat?.meta ?? {});
  const phase = $derived(meta.phase as string | undefined);
  // Only a plan that was written to the end can be approved.
  const planReady = $derived(phase === 'preview' && meta.previewOk === true && !chat.running);
  // (A plan whose run has just ended is briefly neither: the service says which a moment later.)
  const planFailed = $derived(phase === 'preview_failed');
  const skipped = $derived(Array.isArray(meta.skipped) ? meta.skipped.length : 0);
  const changed = $derived((Array.isArray(meta.changed) ? meta.changed : []) as { path: string; change: string }[]);
  const total = $derived(Math.max(Number(meta.changedCount) || 0, changed.length));

  // The phase can move without a message arriving here: a newer Wrap replaced this plan, the
  // banner was used to finish or undo it, another window discarded it.
  onMount(() => on((e) => { if (e.type === 'wrap.changed' && e.chatId === id) reload(); }));

  async function act(fn: () => Promise<unknown>) {
    if (busy) return;
    busy = true;
    try { await fn(); await reload(); } finally { busy = false; }
  }
  const approve = () => act(async () => {
    try { await post(`/wrap/${id}/apply`, { note }); note = ''; chat.running = true; }
    catch (e) { failed(t('chat3.wrapApplyFailed'), e); }
  });
</script>

{#if phase === 'preparing' || (phase === 'preview' && chat.running)}
  <div class="wp card row" role="status">
    <span class="grow muted">{t('wrapa.preparing')}</span>
    <button class="btn" disabled={busy} onclick={() => act(() => discardPlan(id))} title={t('wrapa.discardTip')}><Icon name="close" size={14} /> {t('wrapa.discard')}</button>
  </div>
{:else if planReady}
  <div class="wp card">
    <p class="hint muted">{t('wrapa.planHint')}{#if skipped} {tn('wrapa.skipped', skipped)}{/if}</p>
    <div class="row">
      <input class="grow" placeholder={t('chat.wrapNotePlaceholder')} aria-label={t('chat.wrapNotePlaceholder')} bind:value={note} dir="auto"
        onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); approve(); } }} />
      <button class="btn" disabled={busy} onclick={() => act(() => discardPlan(id))} title={t('wrapa.discardTip')}>{t('wrapa.discard')}</button>
      <button class="btn primary" disabled={busy} aria-busy={busy} onclick={approve}><Icon name="check" size={15} /> {t('chat.approveApply')}</button>
    </div>
  </div>
{:else if planFailed}
  <div class="wp card row" role="alert"><Icon name="warning" size={15} /> <span class="grow">{t('wrapa.planFailed')}</span>
    <button class="btn primary" onclick={() => command('wrap.start')}><Icon name="retry" size={14} /> {t('wrapa.tryAgain')}</button>
  </div>
{:else if chat.running}
  <!-- being saved: the conversation above shows the work -->
{:else if phase === 'interrupted'}
  <div class="wp card" role="alert">
    <p class="hint"><Icon name="warning" size={15} /> <span>{t('wrapa.banner')}</span></p>
    <div class="row acts">
      <button class="btn primary" disabled={busy} onclick={() => act(() => finishWrap(id))} title={t('wrapa.finishTip')}><Icon name="check" size={14} /> {t('wrapa.finish')}</button>
      {#if meta.canRollback !== false}<button class="btn" disabled={busy} onclick={() => act(() => undoCutOffWrap(id))} title={t('wrapa.undoTip')}><Icon name="undo" size={14} /> {t('wrapa.undo')}</button>{/if}
      <button class="btn" disabled={busy} onclick={() => act(() => keepCutOffWrap(id))} title={t('wrapa.keepTip')}>{t('wrapa.keep')}</button>
    </div>
    {#if meta.canRollback === false}<p class="hint muted">{t('wrapa.noUndo')}</p>{/if}
  </div>
{:else if phase === 'failed'}
  <div class="wp card row" role="status"><Icon name="warning" size={15} />
    <span class="grow">{t(meta.kept ? 'wrapa.keptNote' : meta.rolledBack === false ? 'layer.wrap.failedKept' : 'layer.wrap.failedRolledBack')}</span>
    {#if !meta.kept && meta.rolledBack !== false}<button class="btn" onclick={() => command('wrap.start')}><Icon name="retry" size={14} /> {t('wrapa.tryAgain')}</button>{/if}
  </div>
{:else if phase === 'done' && meta.saved !== undefined}
  <div class="wp card">
    <div class="row">
      <b class="grow">{total ? tn('wrapa.changed', total) : t('wrapa.changedNone')}</b>
      {#if meta.saved && meta.commit}<button class="btn" disabled={busy} onclick={() => act(() => undoSavedWrap(id))}><Icon name="undo" size={14} /> {t('wrapa.undoWrap')}</button>{/if}
    </div>
    {#if changed.length}
      <ul class="docs scroll">
        {#each changed as c (c.path)}
          <li>
            <span class="pill" class:gone={c.change === 'deleted'}>{t('wrapa.change.' + (['added', 'deleted'].includes(c.change) ? c.change : 'edited'))}</span>
            {#if c.change === 'deleted'}<bdi class="ell" dir="ltr">{c.path}</bdi>
            {:else}<button class="doc ell" onclick={() => go({ kind: 'docs', path: c.path })} aria-label={t('wrapa.openDoc', { path: c.path })}><bdi dir="ltr">{c.path}</bdi></button>{/if}
          </li>
        {/each}
        {#if total > changed.length}<li class="muted">{t('wrapa.more', { n: fmtNum(total - changed.length) })}</li>{/if}
      </ul>
    {/if}
  </div>
{:else if phase === 'superseded' || phase === 'discarded' || phase === 'undone'}
  <div class="wp card row muted" role="status"><Icon name="info" size={15} />
    <span class="grow">{t(phase === 'superseded' ? 'wrapa.superseded' : phase === 'discarded' ? 'wrapa.discardedNote' : 'wrapa.undoneNote')}</span>
    {#if phase === 'undone'}<button class="btn" onclick={() => command('wrap.start')}><Icon name="wrap" size={14} /> {t('wrap')}</button>{/if}
  </div>
{/if}

<style>
  .wp { margin-bottom: 8px; }
  .wp.row { align-items: center; }
  .hint { margin: 0 0 8px; font-size: var(--fs-sm); display: flex; gap: 8px; align-items: flex-start; }
  .hint :global(.icon) { flex: none; margin-top: 2px; color: var(--accent-2); }
  .acts { flex-wrap: wrap; }
  .acts + .hint { margin: 8px 0 0; }
  .docs { list-style: none; margin: 8px 0 0; padding: 0; display: grid; gap: 4px; max-height: 24vh; font-size: var(--fs-sm); }
  .docs li { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .docs .pill { flex: none; }
  .docs .pill.gone { opacity: 0.7; }
  .doc { font: inherit; color: var(--accent); background: none; border: 0; padding: 2px 0; cursor: pointer; text-align: start; text-decoration: underline; text-underline-offset: 3px; min-width: 0; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
