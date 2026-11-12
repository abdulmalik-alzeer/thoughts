<script lang="ts">
  // Everything a Wrap offers on its own page, by phase: the chats to choose from, the plan (which
  // chats it covers and what they said, change it, discard it, approve it), a plan that failed (try
  // again), the changes waiting in a copy of the library (look at them, change them, keep or discard
  // them), a Wrap cut off while saving (finish, undo, keep), and a saved Wrap (what it changed, undo).
  import Icon from '../lib/Icon.svelte';
  import DocChanges from '../lib/DocChanges.svelte';
  import { onMount, untrack } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { errCode, errDetail, failed } from '../lib/errors';
  import { app, command, go, refreshWrap, toast } from '../lib/store.svelte';
  import { choiceDialog, confirmDialog } from '../lib/dialog.svelte';
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
  const planFailed = $derived(phase === 'preview_failed' && !chat.running);
  const skipped = $derived(Array.isArray(meta.skipped) ? meta.skipped.length : 0);
  const changed = $derived((Array.isArray(meta.changed) ? meta.changed : []) as { path: string; change: string }[]);
  const total = $derived(Math.max(Number(meta.changedCount) || 0, changed.length));
  // With a history, the changes are written into a copy and shown before anything is kept.
  const reviewed = $derived(app.settings?.keepHistory !== false || String(app.settings?.sync?.mode ?? '').startsWith('git'));

  // ---- the chats this Wrap covers, and what was said in them ----
  type Offer = { id: string; title: string };
  const inPlan = $derived(new Set<string>(Array.isArray(meta.chatIds) ? meta.chatIds : []));
  const unread = $derived(new Set<string>((Array.isArray(meta.skipped) ? meta.skipped : []).map((s: any) => s.id)));
  const offered = $derived((Array.isArray(meta.offered) ? meta.offered : []) as Offer[]);
  let docs = $state<any>(null);
  let docsState = $state<'idle' | 'loading' | 'failed'>('idle');
  let docsFor = '';
  let picked = $state<Record<string, boolean>>({});
  let edits = $state<Record<string, string>>({});
  let shownSummary = $state<Record<string, boolean>>({});
  // A new plan (or a new Wrap on this page) starts from what the plan covers.
  let seen = '';
  $effect(() => {
    const key = `${id}|${phase}|${meta.previewAt ?? ''}|${[...inPlan].join(',')}`;
    if (key === seen) return;
    seen = key;
    untrack(() => {
      picked = Object.fromEntries(offered.map((o) => [o.id, phase === 'choosing' || inPlan.has(o.id)]));
      edits = {};
      if (phase !== 'review') note = '';
      // What was shown belongs to the phase before.
      docs = null; docsState = 'idle'; docsFor = '';
    });
  });
  // A Wrap from before chats could be chosen covers what its plan covers.
  const chosen = $derived(offered.length ? offered.filter((o) => picked[o.id]).map((o) => o.id) : [...inPlan]);
  const corrected = $derived(Object.fromEntries(Object.entries(edits).map(([k, v]) => [k, v.trim()])
    .filter(([k, v]) => v && picked[k] && v !== String(meta.summaries?.[k] ?? '').trim())));
  const choiceChanged = $derived(chosen.length !== inPlan.size || chosen.some((c) => !inPlan.has(c)));
  // The plan no longer matches what is ticked or what the summaries say: it must be made again first.
  const stale = $derived(phase === 'preview' && offered.length > 0 && (choiceChanged || Object.keys(corrected).length > 0));

  // The phase can move without a message arriving here: a newer Wrap replaced this plan, the
  // banner was used to finish or undo it, another window discarded it.
  onMount(() => on((e) => { if (e.type === 'wrap.changed' && e.chatId === id) reload(); }));

  async function act(fn: () => Promise<unknown>) {
    if (busy) return;
    busy = true;
    try { await fn(); await reload(); } finally { busy = false; }
  }
  const makePlan = (withNote: boolean, ids = chosen) => act(async () => {
    try {
      await post(`/wrap/${id}/plan`, { chatIds: ids, summaries: corrected, note: withNote ? note.trim() : '' });
      note = ''; chat.running = true;
    } catch (e) { failed(t('wrapb.planFailed'), e); }
  });
  const tryAgain = () => {
    const ids = inPlan.size ? [...inPlan] : offered.map((o) => o.id);
    if (ids.length) makePlan(false, ids); else command('wrap.start');
  };
  const approve = () => act(async () => {
    try { await post(`/wrap/${id}/apply`, { note }); note = ''; chat.running = true; }
    catch (e) { failed(t('chat3.wrapApplyFailed'), e); }
  });

  // ---- the changes: waiting for review, or made by a saved Wrap ----
  async function loadDocs() {
    docsState = 'loading';
    try { docs = await api(`/wrap/${id}/changes`); docsState = 'idle'; }
    catch { docs = null; docsState = 'failed'; }
  }
  // Changes waiting for review are shown straight away (and again after each revision).
  $effect(() => {
    const key = phase === 'review' && !chat.running ? `${id}|${meta.reviewCount}|${chat.messageCount ?? chat.messages?.length}` : '';
    if (!key || key === docsFor) return;
    docsFor = key;
    untrack(loadDocs);
  });
  const rework = () => act(async () => {
    try { await post(`/wrap/${id}/apply`, { note: note.trim() }); note = ''; chat.running = true; }
    catch (e) { failed(t('wrapb.reworkFailed'), e); }
  });
  async function discardChanges() {
    if (!(await confirmDialog({ title: t('wrapb.discard.title'), body: t('wrapb.discard.body'), confirmLabel: t('wrapb.discard.confirm'), danger: true }))) return;
    await act(async () => {
      try { await post(`/wrap/${id}/discard`); toast(t('wrapb.discarded'), { key: 'wrap' }); refreshWrap().catch(() => {}); }
      catch (e) { failed(t('wrapa.discardFailed'), e); }
    });
  }
  async function keep() {
    let clash: string[] | null = null;
    await act(async () => {
      try { await post(`/wrap/${id}/save`); toast(t('wrapb.kept'), { kind: 'success', key: 'wrap' }); refreshWrap().catch(() => {}); }
      catch (e) {
        if (errCode(e) === 'WRAP_CONFLICT') clash = errDetail(e).split(';').map((f) => f.trim()).filter(Boolean);
        else failed(t('wrapb.keepFailed'), e);
      }
    });
    if (!clash) return;
    // The same lines were changed in the library since: write the changes again on top of that, or drop them.
    const choice = await choiceDialog({
      title: t('wrapb.clash.title'), body: t('wrapb.clash.body'), list: clash, cancelLabel: t('wrapb.clash.later'),
      choices: [{ id: 'again', label: t('wrapb.clash.again'), primary: true }, { id: 'discard', label: t('wrapb.discard.confirm'), danger: true }],
    });
    if (choice === 'again') await act(async () => {
      try { await post(`/wrap/${id}/apply`, { note: '' }); chat.running = true; }
      catch (e) { failed(t('wrapb.reworkFailed'), e); }
    });
    else if (choice === 'discard') await act(async () => {
      try { await post(`/wrap/${id}/discard`); toast(t('wrapb.discarded'), { key: 'wrap' }); refreshWrap().catch(() => {}); }
      catch (e) { failed(t('wrapa.discardFailed'), e); }
    });
  }
</script>

{#snippet chatList(withSummaries: boolean)}
  <ul class="chats" aria-label={t('wrapb.chats.label')}>
    {#each offered as o (o.id)}
      {@const summary = meta.summaries?.[o.id]}
      <li>
        <div class="row">
          <label class="pick grow"><input type="checkbox" bind:checked={picked[o.id]} disabled={busy} /> <span class="ell" dir="auto">{o.title}</span></label>
          {#if unread.has(o.id)}<span class="pill">{t('wrapb.chats.unread')}</span>
          {:else if withSummaries && typeof summary === 'string' && picked[o.id]}
            {#if meta.edited?.[o.id]}<span class="pill accent">{t('wrapb.summary.yours')}</span>{/if}
            <button class="btn ghost sm" aria-expanded={!!shownSummary[o.id]} onclick={() => (shownSummary[o.id] = !shownSummary[o.id])}>
              <Icon name={shownSummary[o.id] ? 'down' : 'edit'} size={13} /> {t(shownSummary[o.id] ? 'wrapb.summary.hide' : 'wrapb.summary.show')}</button>
          {:else if withSummaries && picked[o.id] && !inPlan.has(o.id)}<span class="pill">{t('wrapb.chats.added')}</span>{/if}
        </div>
        {#if withSummaries && shownSummary[o.id] && typeof summary === 'string' && picked[o.id]}
          <textarea rows="6" dir="auto" value={edits[o.id] ?? summary} disabled={busy} aria-label={t('wrapb.summary.label', { title: o.title })}
            oninput={(e) => (edits[o.id] = e.currentTarget.value)}></textarea>
          <p class="hint muted">{t('wrapb.summary.hint')}</p>
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

{#if phase === 'choosing'}
  <div class="wp card">
    <p class="hint muted">{t('wrapb.choose.hint')}</p>
    {@render chatList(false)}
    <div class="row acts">
      <span class="grow muted sm">{tn('wrapb.choose.count', chosen.length, { of: fmtNum(offered.length) })}</span>
      <button class="btn" disabled={busy} onclick={() => act(() => discardPlan(id))} title={t('wrapa.discardTip')}>{t('wrapa.discard')}</button>
      <button class="btn primary" disabled={busy || !chosen.length} onclick={() => makePlan(false)}><Icon name="wrap" size={14} /> {t('wrapb.choose.go')}</button>
    </div>
  </div>
{:else if phase === 'preparing' || (phase === 'preview' && chat.running)}
  <div class="wp card row" role="status">
    <span class="grow muted">{t('wrapa.preparing')}</span>
    <button class="btn" disabled={busy} onclick={() => act(() => discardPlan(id))} title={t('wrapa.discardTip')}><Icon name="close" size={14} /> {t('wrapa.discard')}</button>
  </div>
{:else if planReady}
  <div class="wp card">
    <p class="hint muted">{t('wrapa.planHint')} {t(reviewed ? 'wrapb.plan.reviewHint' : 'wrapb.plan.directHint')}</p>
    {#if skipped}<p class="hint muted">{tn('wrapa.skipped', skipped)}</p>{/if}
    {#if offered.length}
      <p class="eyebrow">{t('wrapb.chats.title')}</p>
      {@render chatList(true)}
    {/if}
    {#if stale}<p class="hint" role="status"><Icon name="info" size={15} /> <span>{t('wrapb.plan.stale')}</span></p>{/if}
    <div class="row acts">
      <input class="grow" placeholder={t('wrapb.plan.notePlaceholder')} aria-label={t('wrapb.plan.notePlaceholder')} bind:value={note} dir="auto" disabled={busy}
        onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing && note.trim()) { e.preventDefault(); makePlan(true); } }} />
      <button class="btn" disabled={busy} onclick={() => act(() => discardPlan(id))} title={t('wrapa.discardTip')}>{t('wrapa.discard')}</button>
      <button class="btn" class:primary={stale} disabled={busy || !chosen.length || (!stale && !note.trim())} onclick={() => makePlan(true)}
        title={t('wrapb.plan.reviseTip')}><Icon name="retry" size={14} /> {t(stale ? 'wrapb.plan.update' : 'wrapb.plan.revise')}</button>
      {#if !stale}
        <button class="btn primary" disabled={busy} aria-busy={busy} onclick={approve}><Icon name="check" size={15} /> {t(reviewed ? 'wrapb.plan.approve' : 'chat.approveApply')}</button>
      {/if}
    </div>
  </div>
{:else if planFailed}
  <div class="wp card row" role="alert"><Icon name="warning" size={15} /> <span class="grow">{t('wrapa.planFailed')}</span>
    <button class="btn" disabled={busy} onclick={() => act(() => discardPlan(id))} title={t('wrapa.discardTip')}>{t('wrapa.discard')}</button>
    <button class="btn primary" disabled={busy} onclick={tryAgain}><Icon name="retry" size={14} /> {t('wrapa.tryAgain')}</button>
  </div>
{:else if chat.running}
  <!-- being written or saved: the conversation above shows the work, and the bar below has Stop -->
{:else if phase === 'review'}
  <div class="wp card">
    <p class="hint"><Icon name="eye" size={15} /> <span>{tn('wrapb.review.head', Number(meta.reviewCount) || 0)}</span></p>
    {#if docs?.files?.length}
      <div class="scroll review"><DocChanges files={docs.files} /></div>
    {:else if docsState === 'loading'}
      <p class="muted sm" role="status">{t('shell.loading')}</p>
    {:else if docsState === 'failed'}
      <p class="muted sm" role="alert">{t('wrapb.review.loadFailed')} <button class="btn ghost sm" onclick={loadDocs}><Icon name="retry" size={13} /> {t('shell.retry')}</button></p>
    {:else if docs}
      <p class="muted sm">{t('wrapb.review.none')}</p>
    {/if}
    <div class="row acts">
      <input class="grow" placeholder={t('wrapb.review.notePlaceholder')} aria-label={t('wrapb.review.notePlaceholder')} bind:value={note} dir="auto" disabled={busy}
        onkeydown={(e) => { if (e.key === 'Enter' && !e.isComposing && note.trim()) { e.preventDefault(); rework(); } }} />
      <button class="btn" disabled={busy} onclick={discardChanges}>{t('wrapb.review.discard')}</button>
      <button class="btn" disabled={busy || !note.trim()} onclick={rework} title={t('wrapb.review.changeTip')}><Icon name="edit" size={14} /> {t('wrapb.review.change')}</button>
      <button class="btn primary" disabled={busy} aria-busy={busy} onclick={keep}><Icon name="check" size={15} /> {t('wrapb.review.keep')}</button>
    </div>
  </div>
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
{:else if phase === 'done' && (meta.changesKnown === true || (meta.saved && meta.commit))}
  <!-- Only a list that is known to be complete may say "nothing changed": a library without
       history, or a save that failed, has its outcome in the conversation above instead. -->
  <div class="wp card">
    <div class="row">
      <b class="grow">{meta.changesKnown !== true ? t('wrapa.savedUnlisted') : total ? tn('wrapa.changed', total) : t('wrapa.changedNone')}</b>
      {#if meta.saved && meta.commit}<button class="btn" disabled={busy} onclick={() => act(() => undoSavedWrap(id))}><Icon name="undo" size={14} /> {t('wrapa.undoWrap')}</button>{/if}
    </div>
    {#if docs?.files?.length}
      <div class="scroll review"><DocChanges files={docs.files} canOpen /></div>
    {:else if changed.length}
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
      {#if meta.commit}
        <button class="btn ghost sm lines" disabled={docsState === 'loading'} onclick={loadDocs}><Icon name="eye" size={13} /> {t('wrapb.done.showLines')}</button>
        {#if docsState === 'failed'}<p class="muted sm" role="alert">{t('wrapb.review.loadFailed')}</p>{/if}
      {/if}
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
  .eyebrow { margin: 0 0 4px; }
  .sm { font-size: var(--fs-xs); }
  .btn.sm { padding: 3px 8px; }
  .chats { list-style: none; margin: 0 0 10px; padding: 0; display: grid; gap: 4px; max-height: 34vh; overflow: auto; }
  .chats li { min-width: 0; }
  .chats textarea { width: 100%; margin: 4px 0 2px; font-size: var(--fs-sm); resize: vertical; }
  .chats .hint { margin: 0 0 6px; font-size: var(--fs-xs); }
  .pick { display: flex; align-items: center; gap: 8px; min-width: 0; font-size: var(--fs-sm); cursor: pointer; }
  .pick input { flex: none; }
  .review { max-height: 46vh; margin: 0 0 10px; }
  .lines { margin-top: 8px; }
  .docs { list-style: none; margin: 8px 0 0; padding: 0; display: grid; gap: 4px; max-height: 24vh; font-size: var(--fs-sm); }
  .docs li { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .docs .pill { flex: none; }
  .docs .pill.gone { opacity: 0.7; }
  .doc { font: inherit; color: var(--accent); background: none; border: 0; padding: 2px 0; cursor: pointer; text-align: start; text-decoration: underline; text-underline-offset: 3px; min-width: 0; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
