<script lang="ts">
  import HelpTip from '../lib/HelpTip.svelte';
  // Customise › Automate: "Suggest automations". Only when the person presses the button, the
  // background service sends their own assistant a short summary of how they use Thoughts (shown
  // word for word under "See what will be sent") and gets back a few suggestions: a Routine, a
  // Helper, a Connection or a New page. Each is built through a review the app already has, or
  // turned down for good. Nothing here runs in the background or pops up anywhere.
  import Icon from '../lib/Icon.svelte';
  import { onMount, tick } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { app, go, refreshChats, toast } from '../lib/store.svelte';
  import { fmtDate, fmtMoney, t, tn } from '../lib/i18n';
  import { errCode, failed } from '../lib/errors';
  import { trapFocus } from '../lib/focus';
  import {
    blockedKey, chatDraft, draftFrom, loadState, normGuide, normState, phaseOf, toBody,
    type AutoState, type Draft, type Guide, type Suggestion,
  } from '../lib/automate';
  import SuggestionCard from './automate/SuggestionCard.svelte';
  import DraftEditor from './automate/DraftEditor.svelte';
  import ConnectionGuide from './automate/ConnectionGuide.svelte';
  import PeopleEditor from './automate/PeopleEditor.svelte';

  let st = $state<AutoState | null>(null);
  let loadError = $state('');
  // "Suggest again" shows the explanation (and what is sent) before anything is asked.
  let again = $state(false);
  let starting = $state(false);
  let announce = $state('');
  let previewOpen = $state(false);
  let previewText = $state('');
  let previewState = $state<'idle' | 'loading' | 'ok' | 'error'>('idle');
  let previewError = $state('');
  let busyId = $state('');
  let editor = $state<{ s: Suggestion; kind: 'skill' | 'agent'; initial: Draft | null; request: string; what: string; people: boolean } | null>(null);
  let guide = $state<{ s: Suggestion; guide: Guide } | null>(null);
  let walking = $state(false);
  let peopleStep = $state(false);
  let listHead = $state<HTMLElement | null>(null);

  const phase = $derived(st ? phaseOf(st, again) : 'loading');
  const why = $derived(st ? blockedKey(st) : '');
  const usage = $derived(app.status?.usage);
  const spendLine = $derived(typeof usage?.monthCostUsd === 'number' && usage?.limitUsd
    ? t('runc.spend.used', { spent: fmtMoney(usage.monthCostUsd), limit: fmtMoney(usage.limitUsd), percent: usage.percent ?? 0 }) : '');

  async function load() {
    const was = st?.running ?? false;
    try {
      const s = await loadState();
      st = s; loadError = '';
      if (was && !s.running) announce = s.error ? s.error : s.suggestions.length ? tn('auto-ui.readyN', s.suggestions.length) : t('auto-ui.nothing.title');
    } catch (e: any) { loadError = String(e?.message ?? e ?? ''); }
  }
  onMount(() => {
    load();
    return on((e) => {
      if (e?.type === 'automations.changed' || e?.type === 'resync') load();
      if (e?.type === 'library.switched') { again = false; previewOpen = false; previewState = 'idle'; load(); }
      // A person added, changed or removed (or any library edit) changes what would be sent.
      if (e?.type === 'library.changed' && previewOpen && previewState === 'ok') loadPreview(true);
    });
  });

  async function suggest() {
    if (!st?.canSuggest || starting) return;
    starting = true;
    try {
      await post('/automations/suggest');
      again = false; previewOpen = false; announce = t('auto-ui.run.title');
      st = { ...st, running: true, canSuggest: false, error: null };
      // The service may already be done (a quick answer): read the state again.
      load();
    } catch (e) { failed(t('auto-ui.suggestFailed'), e); load(); }
    finally { starting = false; }
  }

  async function togglePreview() {
    previewOpen = !previewOpen;
    if (previewOpen) await loadPreview();
  }
  /** `quiet`: a refresh while it is open; the text on screen stays until the new one is in. */
  let previewSeq = 0;
  async function loadPreview(quiet = false) {
    const n = ++previewSeq;
    if (!quiet) { previewState = 'loading'; previewError = ''; }
    try { const r = await api('/automations/preview'); if (n === previewSeq && previewOpen) { previewText = String(r?.text ?? ''); previewState = 'ok'; } }
    catch (e: any) { if (n === previewSeq && !quiet) { previewError = String(e?.message ?? e ?? ''); previewState = 'error'; } }
  }

  function gone(e: unknown, s: Suggestion) {
    if (errCode(e) === 'not_found') { toast(t('auto-ui.gone', { title: s.title })); load(); return true; }
    return false;
  }

  async function dismiss(s: Suggestion) {
    if (busyId) return;
    busyId = s.id;
    try {
      st = normState(await post(`/automations/${encodeURIComponent(s.id)}/dismiss`));
      toast(t('auto-ui.dismissed', { title: s.title }), { key: 'auto-ui-dismiss', action: { label: t('auto-ui.undo'), run: () => restore(s) } });
      await tick();
      listHead?.focus();
    } catch (e) { if (!gone(e, s)) failed(t('auto-ui.dismissFailed'), e); }
    finally { busyId = ''; }
  }
  async function restore(s: Suggestion) {
    try { st = normState(await post(`/automations/${encodeURIComponent(s.id)}/restore`)); again = false; }
    catch (e) { failed(t('auto-ui.undoFailed'), e); load(); }
  }

  async function build(s: Suggestion) {
    if (busyId) return;
    busyId = s.id;
    try {
      const r = await post(`/automations/${encodeURIComponent(s.id)}/build`);
      if (s.kind === 'tab') {
        const chat = typeof r?.chat === 'string' ? r.chat : '';
        await refreshChats().catch(() => {});
        load();
        if (chat) go({ kind: 'chat', id: chat });
      } else if (s.kind === 'skill' || s.kind === 'agent') {
        const initial = s.kind === 'skill' && r?.draft ? draftFrom(r.draft, 'skill') : null;
        editor = { s, kind: s.kind, initial, request: String(r?.request ?? s.request ?? ''), what: String(r?.what ?? s.what), people: s.kind === 'skill' && r?.people === true };
      } else {
        const g = normGuide(r?.guide);
        if (!g) throw new Error(t('auto-ui.gd.empty'));
        guide = { s, guide: g };
      }
    } catch (e) { if (!gone(e, s)) failed(t('auto-ui.buildFailed'), e); }
    finally { busyId = ''; }
  }

  /** The routine, helper or guide is done with: the suggestion leaves the list for good. */
  async function markBuilt(s: Suggestion) {
    try { st = normState(await post(`/automations/${encodeURIComponent(s.id)}/built`)); }
    catch { load(); /* it stays in the list; pressing Build it again is harmless */ }
  }

  async function kept() {
    if (!editor) return;
    const { s, kind, people } = editor;
    editor = null;
    toast(t(kind === 'skill' ? 'auto-ui.ed.skill.kept' : 'auto-ui.ed.agent.kept'), { kind: 'success' });
    await markBuilt(s);
    if (people) peopleStep = true;
  }

  async function walk() {
    if (!guide || walking) return;
    walking = true;
    try {
      // An ordinary chat, started by the app (so it never inherits a "skip all questions" default).
      const c = await post('/chats', { title: guide.guide.title });
      try { localStorage.setItem(`thoughts.chatdraft:${c.id}`, chatDraft(guide.guide.chatPrompt)); } catch { /* storage unavailable: an empty chat */ }
      await refreshChats().catch(() => {});
      toast(t('auto-ui.gd.opened'), { key: 'auto-ui-walk' });
      guide = null;
      go({ kind: 'chat', id: c.id });
    } catch (e) { failed(t('auto-ui.gd.walkFailed'), e); }
    finally { walking = false; }
  }
  async function guideDone() {
    if (!guide) return;
    const s = guide.s;
    guide = null;
    await markBuilt(s);
    toast(t('auto-ui.gd.doneToast'), { kind: 'success' });
  }

  const when = (iso: string) => fmtDate(iso, { day: 'numeric', month: 'short' });
</script>

<p class="sr-only" role="status" aria-live="polite">{announce}</p>

{#if !st}
  {#if loadError}
    <div class="card"><p class="err" role="alert">{t('auto-ui.loadFailed')} <bdi>{loadError}</bdi></p>
      <button type="button" class="btn" onclick={load}><Icon name="retry" size={15} /> {t('shell.retry')}</button></div>
  {:else}
    <div class="card"><p class="muted sm" role="status">{t('shell.loading')}</p></div>
  {/if}
{:else}
  {#if phase === 'before'}
    <section class="card before" aria-labelledby="au-h">
      <h3 id="au-h">{t('auto-ui.before.title')} <HelpTip label={t('czh.auto.before')} text={t('auto-ui.before.lead')} /></h3>
      <h4>{t('auto-ui.before.sentTitle')}</h4>
      <p class="sm">{t('auto-ui.before.sent')}</p>
      <p class="sm">{t('auto-ui.before.notSent')}</p>
      <p class="sm cost"><Icon name="usage" size={14} /> <span>{t('auto-ui.before.cost')}{#if spendLine} <bdi>{spendLine}</bdi>{/if}</span></p>
      <button type="button" class="btn ghost peek" aria-expanded={previewOpen} aria-controls="au-preview" onclick={togglePreview}>
        <Icon name={previewOpen ? 'down' : 'right'} size={14} class={previewOpen ? '' : 'flip-rtl'} /> {t('auto-ui.before.see')}
      </button>
      {#if previewOpen}
        <div id="au-preview" class="preview">
          {#if previewState === 'loading'}<p class="muted sm" role="status">{t('shell.loading')}</p>
          {:else if previewState === 'error'}<p class="err" role="alert">{t('auto-ui.before.seeFailed')} <bdi>{previewError}</bdi></p>
          {:else}
            <p class="muted sm">{t('auto-ui.before.seeHint')}</p>
            <!-- A scrolling box must be reachable by keyboard to be read. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <pre tabindex="0" role="region" aria-label={t('auto-ui.before.see')}>{previewText}</pre>
          {/if}
        </div>
      {/if}
      <div class="row go">
        <button type="button" class="btn primary" disabled={!st.canSuggest || starting} aria-busy={starting} aria-describedby={why ? 'au-why' : undefined} onclick={suggest}>
          <Icon name="wrap" size={16} /> {t('auto-ui.suggest')}
        </button>
        {#if again && (st.suggestions.length || st.generatedAt)}
          <button type="button" class="btn ghost" onclick={() => (again = false)}>{t('auto-ui.back')}</button>
        {/if}
      </div>
      {#if why}<p id="au-why" class="sm reason">{t(why)}</p>{/if}
    </section>
  {:else if phase === 'running'}
    <section class="card run" aria-labelledby="au-h" aria-busy="true">
      <div class="row status"><span class="spin" aria-hidden="true"></span><h3 id="au-h">{t('auto-ui.run.title')} <HelpTip label={t('czh.auto.run')} text={t('auto-ui.run.hint')} /></h3></div>
    </section>
  {:else if phase === 'error'}
    <section class="card" aria-labelledby="au-h">
      <h3 id="au-h">{t('auto-ui.err.title')}</h3>
      <p class="err" role="alert"><bdi>{st.error}</bdi></p>
      <div class="row go">
        <button type="button" class="btn primary" disabled={!st.canSuggest || starting} aria-describedby={why ? 'au-why' : undefined} onclick={suggest}><Icon name="retry" size={15} /> {t('auto-ui.tryAgain')}</button>
        <button type="button" class="btn ghost" onclick={() => (again = true)}>{t('auto-ui.err.what')}</button>
      </div>
      <p class="muted sm">{t('auto-ui.before.cost')}</p>
      {#if why}<p id="au-why" class="sm reason">{t(why)}</p>{/if}
    </section>
  {:else if phase === 'nothing' || phase === 'done'}
    <section class="card" aria-labelledby="au-h">
      <h3 id="au-h">{t(phase === 'nothing' ? 'auto-ui.nothing.title' : 'auto-ui.done.title')}{#if phase === 'done'} <HelpTip label={t('czh.auto.done')} text={t('auto-ui.done.hint')} />{/if}</h3>
      {#if phase === 'nothing' && st.nothing}<p class="sm" dir="auto">{st.nothing}</p>{/if}
      <div class="row meta">
        {#if st.generatedAt}<span class="muted sm">{t('auto-ui.on', { date: when(st.generatedAt) })}</span>{/if}
        <button type="button" class="btn" onclick={() => (again = true)}><Icon name="sync" size={15} /> {t('auto-ui.again')}</button>
      </div>
    </section>
  {:else}
    <div class="row meta head">
      <h3 class="grow" tabindex="-1" bind:this={listHead}>{t('auto-ui.list.title')} <HelpTip label={t('czh.auto.list')} text={t('auto-ui.list.hint')} /></h3>
      {#if st.generatedAt}<span class="muted sm">{t('auto-ui.on', { date: when(st.generatedAt) })}</span>{/if}
      <button type="button" class="btn" onclick={() => (again = true)}><Icon name="sync" size={15} /> {t('auto-ui.again')}</button>
    </div>
    {#if st.error}<p class="err sm" role="alert">{t('auto-ui.list.lastFailed')} <bdi>{st.error}</bdi></p>{/if}
    {#each st.suggestions as s (s.id)}
      <SuggestionCard {s} busy={busyId === s.id} locked={!!busyId && busyId !== s.id} onbuild={() => build(s)} ondismiss={() => dismiss(s)} />
    {/each}
  {/if}

  {#if st.blocked !== 'noLibrary'}
    <section class="card" aria-labelledby="au-ppl-h">
      <h3 id="au-ppl-h">{t('auto-ui.ppl.title')} <HelpTip text={t('auto-ui.ppl.hint')} /></h3>
      {#if !peopleStep}<PeopleEditor />{/if}
    </section>
  {/if}
{/if}

{#if editor}
  {#key editor.s.id}
    <DraftEditor kind={editor.kind} title={editor.s.title} what={editor.what} request={editor.request} initial={editor.initial}
      onkept={kept} onclose={() => (editor = null)} />
  {/key}
{/if}

{#if guide}
  <ConnectionGuide guide={guide.guide} busy={walking} onwalk={walk} ondone={guideDone} onclose={() => (guide = null)} />
{/if}

{#if peopleStep}
  <div class="modal-bg no-print" role="presentation" use:toBody>
    <div class="modal pp" role="dialog" aria-modal="true" aria-labelledby="au-pp-h" aria-describedby="au-pp-d" tabindex="-1" use:trapFocus={{ onEscape: () => (peopleStep = false) }}>
      <h2 id="au-pp-h">{t('auto-ui.ppl.stepTitle')}</h2>
      <p id="au-pp-d" class="muted sm">{t('auto-ui.ppl.stepHint')}</p>
      <PeopleEditor step />
      <div class="row end">
        <button type="button" class="btn primary" onclick={() => (peopleStep = false)}>{t('auto-ui.ppl.stepDone')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .card { margin-bottom: 12px; }
  h3 { font-size: 14px; font-weight: 600; margin: 0 0 6px; }
  h4 { font-size: 13px; font-weight: 600; margin: 12px 0 4px; }
  .sm { font-size: var(--fs-xs); }
  .before p { margin: 0 0 6px; line-height: 1.5; }
  .cost { display: flex; gap: 6px; align-items: flex-start; }
  .cost :global(.icon) { flex: none; margin-top: 2px; color: var(--accent-2); }
  .peek { padding-inline: 6px; margin-inline-start: -6px; }
  .preview { margin: 4px 0 8px; }
  .preview pre { white-space: pre-wrap; overflow-wrap: anywhere; font-family: var(--mono); font-size: 12px; line-height: 1.5; background: var(--fill); border-radius: var(--radius); padding: 8px 10px; max-height: 320px; overflow: auto; margin: 4px 0 0; direction: ltr; text-align: left; }
  .go { gap: 8px; flex-wrap: wrap; margin-top: 10px; }
  .reason { margin: 8px 0 0; color: var(--muted); }
  .status { gap: 10px; align-items: center; }
  .status h3 { margin: 0; }
  .meta { gap: 10px; align-items: center; flex-wrap: wrap; }
  .head { margin: 2px 0 10px; }
  .head h3 { margin: 0; outline: none; }
  .err { color: var(--danger); font-size: var(--fs-sm); margin: 0 0 8px; }
  .spin { width: 16px; height: 16px; border-radius: 50%; border: 2px solid var(--fill-2); border-top-color: var(--accent); animation: au-spin .7s linear infinite; flex: none; }
  @keyframes au-spin { to { transform: rotate(360deg); } }
  .pp { max-width: 640px; width: 100%; display: grid; gap: 6px; max-height: 90vh; overflow: auto; }
  .pp h2 { margin: 0; font-size: 17px; }
  .pp p { margin: 0 0 4px; }
  .end { justify-content: flex-end; margin-top: 10px; }
</style>
