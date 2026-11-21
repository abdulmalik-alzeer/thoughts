<script lang="ts">
  // One step's workspace: what it is for, what can be done now (write it, send changes, approve,
  // reopen), its documents and diagrams in tabs, and beside them the step's chat and its comments.
  import { untrack } from 'svelte';
  import { Sparkles, Send, Check, Undo2, Lock, MessageSquareText, MessagesSquare, StickyNote, RotateCw, FileWarning } from '@lucide/svelte';
  import { t, tn } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import { toast } from '../../lib/store.svelte';
  import { confirmDialog } from '../../lib/dialog.svelte';
  import { approval } from '../../lib/consent';
  import DesignStep from '../../lib/mado/design/DesignStep.svelte';
  import CommentsPanel, { type Described, type Draft } from '../../lib/mado/comments/CommentsPanel.svelte';
  import { DOC_FILES, GRAPH_FILES, changeCount, commentsOfStep, commentsOnDesign } from '../../lib/mado/comments/batch';
  import { canApprove, canReopen, canRun, isDrafted, isEditable, type MadoStore } from '../../lib/mado/store.svelte';
  import type { DocName } from '../../lib/mado/api';
  import type { Comment, CommentStatus, CommentTarget, GraphName, StepId } from '../../lib/mado/types';
  import { statusKey, tabKey, tabsOf } from './steps';
  import DocPane from './DocPane.svelte';
  import GraphPane from './GraphPane.svelte';
  import TaskList from './TaskList.svelte';
  import StepChat from './StepChat.svelte';
  import ReviewStep from './ReviewStep.svelte';
  import BuildStep from './BuildStep.svelte';

  let { store, step: stepProp, tab = $bindable('') }: { store: MadoStore; step: StepId; /** The open tab, remembered by the view per step. */ tab?: string } = $props();

  // One workspace shows one step for as long as it lives (the view makes a new one per step).
  const step = untrack(() => stepProp);
  const status = $derived(store.status(step));
  const chatId = $derived(store.step(step).chatId ?? '');
  const tabs = $derived(tabsOf(step));
  const active = $derived(tabs.find((x) => x.id === tab) ?? tabs[0]);
  const comments = $derived(commentsOfStep(store.comments, step));
  const openCount = $derived(comments.filter((c) => c.status === 'open').length);
  const changes = $derived(changeCount(store.comments, store.pendingOps, step));
  const drafting = $derived(status === 'drafting');
  const locked = $derived(status === 'locked');
  const editable = $derived(isEditable(status));
  const canComment = $derived(!locked && !drafting);
  const wide = $derived(step === 'review' || step === 'build');

  let side = $state<'chat' | 'comments'>('chat');
  let draft = $state<Draft | null>(null);
  let focusId = $state<string | null>(null);
  let note = $state('');
  let showNote = $state(false);
  let busy = $state('');

  // ---------- what a comment is about, in plain words ----------
  const docOf = (file: string) => (Object.keys(DOC_FILES) as DocName[]).find((n) => DOC_FILES[n] === file);
  const graphOf = (file: string) => (Object.keys(GRAPH_FILES) as GraphName[]).find((n) => GRAPH_FILES[n] === file);
  function describe(target: CommentTarget): Described {
    switch (target.kind) {
      case 'text': {
        const name = docOf(target.file);
        const where = name ? t(`mado.tab.${name}`) : target.file;
        return { where: target.heading ? `${where} › ${target.heading}` : where, quote: target.quote.replace(/\s+/g, ' ').trim().slice(0, 200) };
      }
      case 'node': {
        const name = graphOf(target.file);
        const node = name ? store.graphs[name]?.nodes.find((n) => n.id === target.nodeId) : undefined;
        return { where: name ? t(`mado.tab.${name}`) : target.file, quote: node?.label ?? target.nodeId };
      }
      case 'edge': {
        const name = graphOf(target.file);
        const g = name ? store.graphs[name] : undefined;
        const edge = g?.edges.find((e) => e.id === target.edgeId);
        const label = (id: string) => g?.nodes.find((n) => n.id === id)?.label ?? id;
        return { where: `${name ? t(`mado.tab.${name}`) : target.file} · ${t('mado.comment.link')}`, quote: edge ? `${label(edge.from)} → ${label(edge.to)}${edge.label ? ` (${edge.label})` : ''}` : target.edgeId };
      }
      case 'task': return { where: t('mado.comment.task'), quote: store.plan?.tasks.find((x) => x.id === target.taskId)?.title ?? target.taskId };
      case 'token': return { where: t('mado.comment.token'), quote: target.tokenPath };
      case 'screen': return { where: t('mado.comment.screen'), quote: target.screenId };
    }
  }

  function startComment(target: CommentTarget, label = '') {
    if (!canComment) return;
    draft = { target, label };
    side = 'comments';
  }
  async function addComment(body: string) {
    if (!draft) return;
    try { const made = await store.addComment(draft.target, body); draft = null; focusId = made?.id ?? null; }
    catch (e) { failed(t('mado.comment.addFailed'), e); throw e; }
  }
  const guard = (key: string, run: () => Promise<void>) => run().catch((e) => failed(t(key), e));
  const editComment = (id: string, body: string) => guard('mado.comment.saveFailed', () => store.editComment(id, { body }));
  const setStatus = (id: string, s: CommentStatus) => guard('mado.comment.saveFailed', () => store.editComment(id, { status: s }));
  async function removeComment(id: string) {
    if (!(await confirmDialog({ title: t('mado.comment.deleteTitle'), body: t('mado.comment.deleteBody'), confirmLabel: t('delete'), danger: true }))) return;
    await guard('mado.comment.deleteFailed', () => store.removeComment(id));
  }
  function pick(id: string) { focusId = id; side = 'comments'; }
  function locate(c: Comment) {
    focusId = c.id;
    const k = c.target;
    const to = k.kind === 'text' ? `doc:${docOf(k.file)}` : k.kind === 'node' || k.kind === 'edge' ? `graph:${graphOf(k.file)}` : k.kind === 'task' ? 'tasks' : 'design';
    if (tabs.some((x) => x.id === to)) tab = to;
  }
  const focusTask = $derived.by(() => { const c = comments.find((x) => x.id === focusId); return c?.target.kind === 'task' ? c.target.taskId : null; });

  // ---------- the step's actions ----------
  async function act(what: string, run: () => Promise<unknown>, failKey: string) {
    if (busy) return;
    busy = what;
    try { await run(); }
    catch (e) { failed(t(failKey), e); }
    finally { busy = ''; }
  }
  const run = () => act('run', async () => { await store.run(step, note); note = ''; showNote = false; side = 'chat'; }, 'mado.act.runFailed');
  const send = () => act('send', async () => {
    if (await store.sendChanges(step, note)) { note = ''; showNote = false; side = 'chat'; toast(t('mado.act.sent'), { kind: 'success' }); }
  }, 'mado.act.sendFailed');
  async function approve() {
    if (busy) return;
    if (changes > 0 && !(await confirmDialog({ title: t('mado.act.unsentTitle'), body: tn('mado.act.unsentBody', changes), confirmLabel: t('mado.act.approveAnyway') }))) return;
    if (step === 'review') return approveReview();
    act('approve', async () => { await store.approve(step); toast(t('mado.act.approved', { step: t(`mado.step.${step}`) }), { kind: 'success' }); }, 'mado.act.approveFailed');
  }
  /** The review is approved as it is NOW: fetched again, every command shown, and the approval
   *  names that very plan — the service refuses it if the plan changed in between. */
  function approveReview() {
    return act('approve', async () => {
      const shown = await store.freshReview();
      if (!shown || !canApprove('review', store.status('review'), shown)) { toast(t('mado.act.fixFirst'), { kind: 'error' }); return; }
      // Approving lets the build run these commands: the desktop app asks in its own dialog, which
      // lists them, and that is the confirmation. Outside it (the browser preview) the page asks,
      // and a real service refuses the approval without the shell's yes.
      const yes = await approval('mado-approve', `${store.projectId}/${shown.planHash}`);
      if (yes === null) return;
      if (yes === undefined) {
        const ok = await confirmDialog({ title: t('mado.act.approveReviewTitle'), body: t('mado.review.commandsWarn'), list: [...shown.commands], note: tn('mado.review.commandCount', shown.commands.length), confirmLabel: t('mado.act.approveReview') });
        if (!ok) return;
      }
      const said = await store.approveReview(shown, yes);
      if (said === 'approved') toast(t('mado.act.approved', { step: t('mado.step.review') }), { kind: 'success' });
      else toast(t(said === 'changed' ? 'mado.act.planChanged' : 'mado.act.fixFirst'), { kind: 'error' });
    }, 'mado.act.approveFailed');
  }
  const reopen = () => act('reopen', () => store.reopen(step), 'mado.act.reopenFailed');
  const runLabel = $derived(step === 'brief' && status === 'ready' && !chatId ? 'mado.act.interview' : status === 'ready' ? 'mado.act.run' : 'mado.act.rerun');
  const strayed = $derived(store.step(step).strayed ?? []);
  const unchecked = $derived(store.step(step).strayUnchecked === true);
  const approvable = $derived(canApprove(step, status, store.review ?? null, strayed, unchecked));
  const dismissStrayed = () => act('dismiss', () => store.dismissStrayed(step), 'mado.strayed.dismissFailed');
</script>

<section class="ws" aria-labelledby="mado-step-title">
  <header class="head">
    <div class="grow titles">
      <div class="row"><h2 id="mado-step-title">{t(`mado.step.${step}`)}</h2><span class="pill st-{status}" role="status">{t(statusKey(step, status))}</span></div>
      <p class="muted what">{t(`mado.stepWhat.${step}`)}</p>
    </div>
    {#if !locked && step !== 'build'}
      <div class="row actions">
        {#if isDrafted(step) && (canRun(step, status) || changes > 0)}
          <button type="button" class="btn ghost" class:on={showNote} aria-expanded={showNote} aria-controls="mado-note" onclick={() => (showNote = !showNote)}><StickyNote size={14} aria-hidden="true" /> {t('mado.act.note')}</button>
        {/if}
        {#if canRun(step, status)}
          <button type="button" class="btn" class:primary={status === 'ready'} onclick={run} disabled={!!busy} aria-busy={busy === 'run'}>
            {#if status === 'ready'}<Sparkles size={14} aria-hidden="true" />{:else}<RotateCw size={13} aria-hidden="true" />{/if} {t(runLabel)}</button>
        {/if}
        {#if isDrafted(step) && status !== 'ready'}
          <button type="button" class="btn" class:primary={changes > 0 && editable} onclick={send} disabled={!!busy || !changes || !editable} aria-busy={busy === 'send'}
            title={!editable && changes ? t('mado.act.sendNeedsReopen') : t('mado.act.sendTip')}>
            <Send size={13} aria-hidden="true" /> {t('mado.act.send')} <span class="pill">{changes}</span></button>
        {/if}
        {#if status === 'approved'}
          {#if canReopen(step, status)}<button type="button" class="btn" onclick={reopen} disabled={!!busy} aria-busy={busy === 'reopen'}><Undo2 size={14} aria-hidden="true" /> {t('mado.act.reopen')}</button>{/if}
        {:else if !drafting && (isDrafted(step) ? status !== 'ready' : true)}
          <button type="button" class="btn" class:primary={approvable && !changes} onclick={approve} disabled={!!busy || !approvable} aria-busy={busy === 'approve'}
            title={strayed.length || unchecked ? t('mado.strayed.approveTip') : step === 'review' && !approvable ? t('mado.act.fixFirst') : undefined}><Check size={15} aria-hidden="true" /> {t('mado.act.approve')}</button>
        {/if}
      </div>
    {/if}
  </header>

  {#if showNote && !locked}
    <div class="noterow" id="mado-note">
      <label for="mado-note-in">{t('mado.act.noteLabel')}</label>
      <input id="mado-note-in" dir="auto" bind:value={note} placeholder={t('mado.act.notePh')} autocomplete="off" maxlength="2000" />
    </div>
  {/if}

  {#if (strayed.length || unchecked) && !drafting}
    <div class="banner warn strayed" role="alert">
      <FileWarning size={15} aria-hidden="true" />
      <div class="grow">
        {#if unchecked}
          <b>{t('mado.strayUnchecked.title')}</b>
          <span>{t('mado.strayUnchecked.body')}</span>
        {/if}
        {#if strayed.length}
        <b>{t('mado.strayed.title')}</b>
        <span>{t('mado.strayed.body')}</span>
        {/if}
        {#if strayed.length}<ul class="strayedlist">{#each strayed.slice(0, 50) as f}<li><code dir="ltr">{f}</code></li>{/each}</ul>{/if}
        {#if strayed.length > 50}<span class="muted">{tn('mado.strayed.more', strayed.length - 50)}</span>{/if}
      </div>
      <button type="button" class="btn" onclick={dismissStrayed} disabled={!!busy} aria-busy={busy === 'dismiss'}>{t('mado.strayed.dismiss')}</button>
    </div>
  {/if}

  {#if drafting}
    <div class="banner busy" role="status">
      <span class="bartrack" aria-hidden="true"><i></i></span>
      <b>{t('mado.drafting.title')}</b> <span>{t('mado.drafting.body')}</span>
    </div>
  {:else if status === 'stale'}
    <div class="banner warn" role="status"><RotateCw size={15} aria-hidden="true" /> <span><b>{t('mado.stale.title')}</b> {t('mado.stale.body')}</span></div>
  {:else if status === 'approved' && step !== 'build'}
    <div class="banner ok" role="status"><Check size={15} aria-hidden="true" /> <span>{t('mado.approved.body')}</span></div>
  {/if}

  {#if locked}
    <div class="lockedbox">
      <Lock size={26} strokeWidth={1.5} aria-hidden="true" />
      <b>{t('mado.locked.title')}</b>
      <span class="muted">{t(`mado.locked.${step}`)}</span>
    </div>
  {:else if step === 'review'}
    <ReviewStep review={store.review} plan={store.plan} />
  {:else if step === 'build'}
    <BuildStep {store} />
  {:else}
    <div class="cols" class:wide>
      <div class="main">
        {#if tabs.length > 1}
          <div class="seg tabs" role="tablist" aria-label={t('mado.tabs.label')}>
            {#each tabs as x (x.id)}
              <button type="button" class="segb" role="tab" id="mado-tab-{x.id}" aria-selected={active?.id === x.id} aria-controls="mado-pane" tabindex={active?.id === x.id ? 0 : -1} onclick={() => (tab = x.id)}
                onkeydown={(e) => {
                  const i = tabs.findIndex((y) => y.id === active?.id);
                  const d = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
                  if (!d) return;
                  e.preventDefault();
                  const dir = e.key.startsWith('ArrowL') || e.key.startsWith('ArrowR') ? (getComputedStyle(e.currentTarget).direction === 'rtl' ? -d : d) : d;
                  const to = tabs[(i + dir + tabs.length) % tabs.length];
                  tab = to.id;
                  queueMicrotask(() => document.getElementById(`mado-tab-${to.id}`)?.focus());
                }}>{t(tabKey(x))}{#if x.kind === 'graph' && store.opsOf(x.name).length}<span class="pill">{store.opsOf(x.name).length}</span>{/if}</button>
            {/each}
          </div>
        {/if}
        <div class="pane" id="mado-pane" role={tabs.length > 1 ? 'tabpanel' : undefined} aria-labelledby={tabs.length > 1 && active ? `mado-tab-${active.id}` : undefined}>
          {#if active?.kind === 'doc'}
            {#key active.name}<DocPane {store} name={active.name} canEdit={editable} {canComment} {focusId} oncomment={startComment} onpick={pick} />{/key}
          {:else if active?.kind === 'graph'}
            {#key active.name}<GraphPane {store} name={active.name} canEdit={editable} {canComment} oncomment={startComment} />{/key}
          {:else if active?.kind === 'tasks'}
            <TaskList plan={store.plan} {comments} {canComment} focusId={focusTask} oncomment={startComment} />
          {:else if active?.kind === 'design'}
            <DesignStep projectId={store.projectId} comments={commentsOnDesign(store.comments)} locked={!editable} version={store.designVersion} oncomment={(target) => startComment(target)} />
          {/if}
        </div>
      </div>

      <aside class="side" aria-label={t('mado.side.label')}>
        <div class="seg" role="tablist" aria-label={t('mado.side.label')}>
          <button type="button" class="segb" role="tab" aria-selected={side === 'chat'} onclick={() => (side = 'chat')}><MessagesSquare size={14} aria-hidden="true" /> {t('mado.side.chat')}</button>
          <button type="button" class="segb" role="tab" aria-selected={side === 'comments'} onclick={() => (side = 'comments')}><MessageSquareText size={14} aria-hidden="true" /> {t('mado.side.comments')}{#if openCount}<span class="pill">{openCount}</span>{/if}</button>
        </div>
        <div class="sidebody" class:chat={side === 'chat'}>
          {#if side === 'chat'}
            {#if chatId}
              {#key chatId}<StepChat {chatId} placeholder={t(step === 'brief' ? 'mado.chat.answerPh' : 'mado.chat.placeholder')} emptyTitle={t('mado.chat.emptyTitle')} emptyBody={t('mado.chat.emptyBody')} />{/key}
            {:else}
              <div class="nochat">
                <MessagesSquare size={24} strokeWidth={1.5} aria-hidden="true" />
                <b>{t(step === 'brief' ? 'mado.chat.briefTitle' : 'mado.chat.noneTitle')}</b>
                <span class="muted">{t(step === 'brief' ? 'mado.chat.briefBody' : 'mado.chat.noneBody')}</span>
                {#if canRun(step, status)}<button type="button" class="btn primary" onclick={run} disabled={!!busy}><Sparkles size={14} aria-hidden="true" /> {t(runLabel)}</button>{/if}
              </div>
            {/if}
          {:else}
            <CommentsPanel {comments} {draft} {canComment} {describe} {focusId} onadd={addComment} oncancel={() => (draft = null)} onedit={editComment} onremove={removeComment} onstatus={setStatus} onlocate={locate} />
          {/if}
        </div>
      </aside>
    </div>
  {/if}
</section>

<style>
  .ws { display: grid; gap: 16px; animation: mado-step .3s var(--ease) backwards; }
  @keyframes mado-step { from { opacity: 0; transform: translateY(8px); } }
  .head { display: flex; gap: 16px; align-items: flex-start; flex-wrap: wrap; }
  .titles { min-width: 240px; }
  h2 { margin: 0; font-size: 22px; font-weight: 600; letter-spacing: -.02em; }
  .what { margin: 4px 0 0; max-width: 70ch; font-size: 13.5px; }
  .actions { flex-wrap: wrap; justify-content: flex-end; }
  .btn.on { background: var(--fill-2); }
  .pill.st-review, .pill.st-ready { background: var(--tint); color: var(--on-tint); }
  .pill.st-approved { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--ok); }
  .pill.st-stale { background: color-mix(in srgb, var(--danger) 14%, transparent); color: var(--danger); }
  :global([data-theme="contrast"]) .pill[class*="st-"] { background: var(--panel-2); color: var(--fg); border: var(--border-w) solid var(--border); }
  .noterow { display: grid; gap: 4px; animation: mado-step .2s var(--ease) backwards; }
  .noterow label { margin: 0; }
  .banner { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; padding: 10px 14px; border-radius: var(--radius-lg); font-size: 13.5px; position: relative; overflow: hidden; }
  .banner :global(svg) { flex: none; }
  .banner.busy { background: var(--tint); color: var(--on-tint); padding-top: 13px; }
  .banner.warn { background: color-mix(in srgb, var(--danger) 10%, transparent); }
  .banner.warn :global(svg) { color: var(--danger); }
  .banner.ok { background: var(--fill); color: var(--muted); }
  .banner.ok :global(svg) { color: var(--ok); }
  :global([data-theme="contrast"]) .banner { background: var(--panel); color: var(--fg); border: var(--border-w) solid var(--border); }
  .bartrack { position: absolute; inset: 0 0 auto 0; height: 3px; background: color-mix(in srgb, var(--accent) 18%, transparent); }
  .bartrack i { position: absolute; top: 0; bottom: 0; width: 34%; border-radius: 3px; background: var(--accent); animation: mado-slide 1.6s ease-in-out infinite; }
  @keyframes mado-slide { 0% { inset-inline-start: -34%; } 100% { inset-inline-start: 100%; } }
  .cols { display: grid; grid-template-columns: minmax(0, 1fr) minmax(300px, 380px); gap: 20px; align-items: start; }
  .main { min-width: 0; display: grid; gap: 14px; }
  .tabs { justify-self: start; max-width: 100%; overflow-x: auto; }
  .pane { min-width: 0; padding: 20px 22px; border-radius: var(--radius-lg); background: var(--panel); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow); }
  .side { position: sticky; top: 12px; display: flex; flex-direction: column; gap: 12px; height: calc(100vh - 86px); min-height: 360px; padding: 14px; border-radius: var(--radius-lg); background: var(--panel); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow); }
  .side > .seg { align-self: flex-start; }
  .sidebody { flex: 1; min-height: 0; overflow: auto; display: flex; flex-direction: column; }
  .sidebody.chat { overflow: hidden; }
  .sidebody > :global(*) { flex: 1; min-height: 0; }
  .nochat, .lockedbox { display: grid; justify-items: center; align-content: center; gap: 8px; text-align: center; padding: 28px 14px; color: var(--muted); }
  .nochat b, .lockedbox b { color: var(--fg); font-size: 15px; }
  .nochat span, .lockedbox span { max-width: 40ch; font-size: 13.5px; }
  .nochat .btn { margin-top: 6px; }
  .lockedbox { padding: 64px 24px; border: 1px dashed var(--border); border-radius: var(--radius-lg); }
  @container main (max-width: 900px) {
    .cols { grid-template-columns: 1fr; }
    .side { position: static; height: auto; max-height: 70vh; }
  }
  .strayed { align-items: flex-start; }
  .strayed .grow { display: grid; gap: 4px; min-width: 0; }
  .strayedlist { margin: 4px 0 0; padding: 0; list-style: none; display: grid; gap: 2px; max-height: 160px; overflow: auto; }
  .strayedlist code { font-size: var(--fs-xs); overflow-wrap: anywhere; }
</style>
