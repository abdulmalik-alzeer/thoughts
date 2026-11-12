<script lang="ts">
  // Settings → Customisations → Automatic tasks: what each one does, how it last ended, Run now,
  // allow / turn off, and its time and spending limits.
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { go, toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { t, fmtDate, fmtMoney } from '../lib/i18n';
  import { errCode, failed } from '../lib/errors';
  import { whenText } from '../lib/layer';
  import { runTask } from '../lib/automatic';

  let tasks = $state<any[]>([]);
  let phase = $state<'loading' | 'ok' | 'error' | 'none'>('loading');
  let error = $state('');
  let busy = $state<Record<string, boolean>>({});
  // Limits being edited, per task file.
  let edits = $state<Record<string, { minutes: number; budget: number }>>({});

  async function load() {
    try {
      const r = await api('/workflows');
      tasks = Array.isArray(r) ? r : [];
      for (const w of tasks) if (!edits[w.file]) edits[w.file] = { minutes: w.limits?.minutes ?? 20, budget: w.limits?.budgetUsd ?? 2 };
      phase = 'ok';
    } catch (e: any) {
      if (e?.status === 404) phase = 'none';
      else { phase = 'error'; error = e?.message ?? String(e); }
    }
  }
  onMount(() => {
    load();
    return on((e) => {
      if (['workflows.changed', 'layer.changed', 'layer.pending', 'resync'].includes(e.type)) load();
      else if (e.type === 'chat.done' && tasks.some((w) => w.chatId === e.chatId)) load();
    });
  });

  async function act(w: any, failKey: string, fn: () => Promise<unknown>) {
    if (busy[w.file]) return;
    busy[w.file] = true;
    try { await fn(); }
    catch (e) {
      // The task changed since the list was read: show the new version rather than act on the old one.
      if (errCode(e) === 'stale') { await load(); toast(t('auto.allowStale'), { kind: 'error' }); }
      else failed(t(failKey), e);
    } finally { busy[w.file] = false; }
  }

  const limitNote = (w: any) => w.budgetSupported
    ? t('auto.runNote', { minutes: w.limits?.minutes ?? 20, amount: fmtMoney(w.limits?.budgetUsd ?? 2) })
    : t('auto.runNoteTime', { minutes: w.limits?.minutes ?? 20 });

  async function allow(w: any) {
    const ok = await confirmDialog({ title: t('layer.wf.allowTitle', { name: w.name }), body: `${whenText(w.trigger)}\n\n${w.prompt ?? ''}`.trim(), note: `${t('layer.wf.allowNote')} ${limitNote(w)}`, confirmLabel: t('layer.wf.allow') });
    if (ok) await act(w, 'layer.wf.failed', async () => { await post('/workflows/approve', { file: w.file, hash: w.hash }); await load(); });
  }
  const turnOff = (w: any) => act(w, 'layer.wf.failed', async () => { await post('/workflows/disable', { file: w.file }); await load(); toast(t('layer.wf.disabled', { name: w.name })); });

  async function runNow(w: any) {
    const ok = await confirmDialog({ title: t('auto.runTitle', { name: w.name }), body: `${t('auto.runBody')}\n\n${w.prompt ?? ''}`.trim(), note: limitNote(w), confirmLabel: t('auto.runNow') });
    if (!ok || busy[w.file]) return;
    busy[w.file] = true;
    try { await runTask(w); await load(); } finally { busy[w.file] = false; }
  }

  const limitsValid = (l: { minutes: number; budget: number }) => Number.isInteger(l?.minutes) && l.minutes >= 1 && l.minutes <= 240 && Number.isFinite(l?.budget) && l.budget >= 0.1 && l.budget <= 100;
  const limitsChanged = (w: any) => { const l = edits[w.file]; return !!l && (l.minutes !== w.limits?.minutes || l.budget !== w.limits?.budgetUsd); };
  const saveLimits = (w: any) => act(w, 'auto.limitsFailed', async () => {
    const l = edits[w.file];
    await post('/workflows/limits', { file: w.file, minutes: l.minutes, budgetUsd: l.budget });
    delete edits[w.file];
    await load();
    toast(t('auto.limitsSaved'), { kind: 'success' });
  });

  const when = (d: any) => (d ? fmtDate(d, { dateStyle: 'medium', timeStyle: 'short' }) : t('layer.wf.never'));
  const outcomeText = (r: any) => {
    const k = 'auto.out.' + (r?.outcome ?? 'failed');
    const s = t(k) === k ? t('auto.out.failed') : t(k);
    return r?.manual ? `${s} (${t('auto.byHand')})` : s;
  };
  const bad = (r: any) => !!r && r.outcome !== 'ok' && r.outcome !== 'stopped';
  function openChat(r: any) {
    if (r?.chatExists === false) { toast(t('auto.chatGone')); return; }
    if (r?.chatId) go({ kind: 'chat', id: r.chatId });
  }
</script>

{#if phase === 'loading'}<p class="muted sm" role="status">{t('shell.loading')}</p>
{:else if phase === 'error'}<p class="err" role="alert">{t('layer.wf.loadFailed')} <bdi>{error}</bdi> <button type="button" class="btn ghost" onclick={load}>{t('shell.retry')}</button></p>
{:else if !tasks.length}<p class="muted sm">{t('layer.wf.empty')}</p>
{:else}
  {#each tasks as w (w.file)}
    {@const b = !!busy[w.file]}
    <div class="wf">
      <div class="row wrap">
        <b class="grow" dir="auto">{w.name}</b>
        {#if w.running}<span class="pill accent"><Icon name="clock" size={12} /> {t('auto.running')}</span>
        {:else if w.approved !== false && w.skipped}<span class="pill">{t('auto.badge.skipped')}</span>
        {:else if bad(w.last)}<span class="pill bad"><Icon name="warning" size={12} /> {t('auto.badge.failed')}</span>{/if}
        {#if w.approved === false}<span class="pill">{t('layer.wf.notAllowed')}</span>{/if}
        {#if w.running && w.chatId}
          <button type="button" class="btn ghost" onclick={() => go({ kind: 'chat', id: w.chatId })}>{t('auto.openRun')}</button>
        {:else}
          <button type="button" class="btn" disabled={b} onclick={() => runNow(w)}>{t('auto.runNow')}</button>
        {/if}
        {#if w.approved === false}
          <button type="button" class="btn" disabled={b} onclick={() => allow(w)}>{t('layer.wf.allow')}</button>
        {:else}
          <button type="button" class="btn ghost" disabled={b} onclick={() => turnOff(w)}>{t('layer.wf.turnOff')}</button>
        {/if}
      </div>
      {#if w.duplicateName}<div class="sm note" role="note">{t('auto.sameName')}</div>{/if}
      <div class="muted sm">{whenText(w.trigger)}{#if w.nextRun && w.approved !== false} · {t('layer.wf.next', { when: when(w.nextRun) })}{/if}</div>
      <div class="sm" class:muted={!bad(w.last)} class:warn={bad(w.last)}>
        {#if w.last}
          {t('auto.lastResult', { when: when(w.last.ended ?? w.last.started), outcome: outcomeText(w.last) })}
          {#if w.last.chatId && w.last.chatExists !== false} <button type="button" class="link" onclick={() => openChat(w.last)}>{t('auto.openRun')}</button>{/if}
        {:else if w.lastRun}{t('layer.wf.last', { when: when(w.lastRun) })}
        {:else}{t('auto.lastNever')}{/if}
      </div>
      {#if w.retryAt}<div class="sm muted">{t('auto.retryAt', { when: fmtDate(w.retryAt, { timeStyle: 'short' }) })}</div>{/if}
      {#if w.skipped && w.approved !== false}<div class="sm warn">{t('auto.skipped', { when: when(w.skipped) })}</div>{/if}
      <details>
        <summary class="sm">{t('auto.what')}</summary>
        <div class="prompt" dir="auto">{w.prompt}</div>
        {#if (w.history ?? []).length > 1}
          <h4>{t('auto.history')}</h4>
          <ul class="hist">
            {#each w.history as r, i (i)}
              <li class="sm" class:warn={bad(r)}>
                {t('auto.historyRow', { when: when(r.ended ?? r.started), outcome: outcomeText(r) })}{#if r.costUsd > 0} · {t('auto.cost', { amount: fmtMoney(r.costUsd) })}{/if}
                {#if r.chatId && r.chatExists !== false} <button type="button" class="link" onclick={() => openChat(r)}>{t('auto.openRun')}</button>{/if}
              </li>
            {/each}
          </ul>
        {/if}
        <h4>{t('auto.limits')}</h4>
        <p class="muted sm">{t('auto.limitsHint')}</p>
        {#if edits[w.file]}
          <form class="row wrap limits" onsubmit={(e) => { e.preventDefault(); if (limitsValid(edits[w.file]) && limitsChanged(w)) saveLimits(w); }}>
            <div><label for={'m-' + w.file}>{t('auto.minutes')}</label><input id={'m-' + w.file} type="number" min="1" max="240" step="1" bind:value={edits[w.file].minutes} /></div>
            {#if w.budgetSupported}
              <div><label for={'b-' + w.file}>{t('auto.budget')}</label><input id={'b-' + w.file} type="number" min="0.1" max="100" step="0.1" bind:value={edits[w.file].budget} /></div>
            {/if}
            <button type="submit" class="btn" disabled={b || !limitsValid(edits[w.file]) || !limitsChanged(w)}>{t('auto.saveLimits')}</button>
          </form>
          {#if !limitsValid(edits[w.file])}<p class="err">{t('auto.limitsInvalid')}</p>{/if}
        {/if}
        {#if !w.budgetSupported}<p class="muted sm">{t('auto.noBudget')}</p>{/if}
      </details>
    </div>
  {/each}
{/if}

<style>
  .sm { font-size: var(--fs-xs); }
  .wrap { flex-wrap: wrap; }
  .wf { padding: 10px 0; border-bottom: 1px solid var(--hairline); display: grid; gap: 3px; }
  .wf:last-child { border-bottom: 0; }
  .warn { color: var(--danger); }
  .note { color: var(--fg); }
  .pill.bad { color: var(--danger); }
  .pill :global(.icon) { vertical-align: -2px; }
  details { margin-top: 4px; }
  summary { cursor: pointer; color: var(--muted); width: fit-content; }
  .prompt { white-space: pre-wrap; font-size: 13px; background: var(--fill); border-radius: var(--radius); padding: 8px 10px; margin: 6px 0; max-height: 240px; overflow: auto; }
  h4 { font-size: 12px; font-weight: 600; margin: 10px 0 2px; }
  .hist { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
  .limits { align-items: flex-end; }
  .limits input { width: 120px; }
  .link { background: none; border: 0; padding: 0; color: var(--accent-2); cursor: pointer; font: inherit; text-decoration: underline; }
  .err { color: var(--danger); font-size: var(--fs-sm); margin: 4px 0 0; }
</style>
