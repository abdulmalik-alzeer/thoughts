<script lang="ts">
  // Building a Routine or a Helper from a suggestion: the draft (the app's own template at once, or
  // one the assistant writes after the person agreed to that one request), an editor, then a review
  // of exactly what will be saved. Keep is the only thing that writes; the service does the writing.
  import Icon from '../../lib/Icon.svelte';
  import { post } from '../../lib/api';
  import { t } from '../../lib/i18n';
  import { failed } from '../../lib/errors';
  import { trapFocus } from '../../lib/focus';
  import { confirmDialog } from '../../lib/dialog.svelte';
  import { draftFrom, draftReady, draftUnsafe, emptyDraft, saveBody, tidyDraft, toBody, type Draft } from '../../lib/automate';

  let { kind, title, what, request, initial = null, onkept, onclose }: {
    kind: 'skill' | 'agent'; title: string; what: string; request: string;
    /** The app's own template, shown at once; null = the assistant is asked for a draft first. */
    initial?: Draft | null;
    onkept: () => void; onclose: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let draft = $state<Draft>(initial ? { ...initial } : emptyDraft());
  // svelte-ignore state_referenced_locally
  let phase = $state<'ask' | 'drafting' | 'edit' | 'review'>(initial ? 'edit' : 'ask');
  let draftError = $state('');
  let saving = $state(false);
  // What the editor started from: closing with anything else asks first.
  // svelte-ignore state_referenced_locally
  let start = $state<Draft>(initial ? { ...initial } : emptyDraft());

  const k = (x: string) => `auto-ui.ed.${kind}.${x}`;
  const id = (x: string) => `au-ed-${x}`;
  const unsafe = $derived(draftUnsafe(draft));
  const ready = $derived(draftReady(draft));
  const shown = $derived(tidyDraft(draft));
  const touched = $derived(JSON.stringify(tidyDraft(draft)) !== JSON.stringify(tidyDraft(start)));

  async function ask() {
    if (phase === 'drafting') return;
    phase = 'drafting'; draftError = '';
    try {
      const d = draftFrom(await post(kind === 'skill' ? '/routines/draft' : '/agents/draft', { request }), kind);
      if (!d.title.trim()) d.title = title;
      draft = d; start = { ...d }; phase = 'edit';
    } catch (e: any) {
      draftError = String(e?.message ?? e ?? '');
      phase = 'ask';
    }
  }

  async function close() {
    if (saving || phase === 'drafting') return;
    if ((phase === 'edit' || phase === 'review') && touched) {
      if (!(await confirmDialog({ title: t(k('discardTitle')), confirmLabel: t('auto-ui.ed.discard'), danger: true }))) return;
    }
    onclose();
  }

  async function keep() {
    if (saving) return;
    saving = true;
    try {
      await post(kind === 'skill' ? '/routines/save' : '/agents/save', saveBody(draft, kind));
      onkept();
    } catch (e) { failed(t(k('saveFailed')), e); }
    finally { saving = false; }
  }
</script>

<div class="modal-bg no-print" role="presentation" use:toBody onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="modal ed" role="dialog" aria-modal="true" aria-labelledby={id('h')} tabindex="-1" use:trapFocus={{ onEscape: close }}>
    {#if phase === 'ask' || phase === 'drafting'}
      <h2 id={id('h')} dir="auto">{title}</h2>
      <p dir="auto">{what}</p>
      <p class="muted sm">{t(k('askIntro'))}</p>
      <p class="muted sm">{t('auto-ui.ed.askCost')}</p>
      {#if draftError}<p class="err" role="alert"><bdi>{draftError}</bdi></p>{/if}
      {#if phase === 'drafting'}<p class="row status" role="status"><span class="spin" aria-hidden="true"></span> {t('auto-ui.ed.drafting')}</p>{/if}
      <div class="row end">
        <button type="button" class="btn" onclick={close} disabled={phase === 'drafting'}>{t('cancel')}</button>
        <button type="button" class="btn primary" onclick={ask} disabled={phase === 'drafting'} aria-busy={phase === 'drafting'}>
          <Icon name="wrap" size={15} /> {draftError ? t('auto-ui.tryAgain') : t('auto-ui.ed.askGo')}
        </button>
      </div>
    {:else if phase === 'edit'}
      <h2 id={id('h')}>{t(k('editTitle'))}</h2>
      <p class="muted sm">{t(k('editHint'))}</p>
      <label for={id('name')}>{t(k('name'))}</label>
      <input id={id('name')} dir="auto" maxlength="80" bind:value={draft.title} />
      <label for={id('when')}>{t(k('when'))}</label>
      <input id={id('when')} dir="auto" maxlength="500" bind:value={draft.description} aria-describedby={id('when-hint')} />
      <p id={id('when-hint')} class="muted sm">{t(k('whenHint'))}</p>
      <label for={id('body')}>{t(k('body'))}</label>
      <textarea id={id('body')} dir="auto" rows="12" bind:value={draft.body}></textarea>
      {#if unsafe}<p class="err" role="alert">{t(k('unsafe'))}</p>{/if}
      {#if !ready}<p class="muted sm">{t('auto-ui.ed.needAll')}</p>{/if}
      <div class="row end">
        <button type="button" class="btn" onclick={close}>{t('cancel')}</button>
        <button type="button" class="btn primary" disabled={!ready || unsafe} onclick={() => (phase = 'review')}>{t('auto-ui.ed.review')}</button>
      </div>
    {:else}
      <h2 id={id('h')}>{t(k('reviewTitle'))}</h2>
      <p class="muted sm">{t(k('reviewHint'))}</p>
      <div class="field"><div class="lbl">{t(k('name'))}</div><div dir="auto">{shown.title}</div></div>
      <div class="field"><div class="lbl">{t(k('when'))}</div><div dir="auto">{shown.description}</div></div>
      <div class="field"><div class="lbl">{t(k('body'))}</div><div class="body" dir="auto">{shown.body}</div></div>
      <div class="row end">
        <button type="button" class="btn" disabled={saving} onclick={() => (phase = 'edit')}>{t('auto-ui.ed.back')}</button>
        <button type="button" class="btn primary" disabled={saving} aria-busy={saving} onclick={keep}><Icon name="check" size={15} /> {t('auto-ui.ed.keep')}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .ed { max-width: 640px; width: 100%; display: grid; gap: 4px; max-height: 90vh; overflow: auto; }
  .ed h2 { margin: 0 0 6px; font-size: 17px; }
  .ed p { margin: 0 0 4px; }
  .ed textarea { min-height: 200px; font-size: 13px; line-height: 1.5; }
  .sm { font-size: var(--fs-xs); }
  .end { justify-content: flex-end; gap: 8px; margin-top: 10px; flex-wrap: wrap; }
  .status { gap: 8px; align-items: center; font-size: 13px; }
  .field { margin-top: 8px; display: grid; gap: 2px; font-size: 13px; }
  .lbl { font-size: var(--fs-xs); font-weight: 600; color: var(--muted); }
  .body { white-space: pre-wrap; background: var(--fill); border-radius: var(--radius); padding: 6px 8px; max-height: 45vh; overflow: auto; line-height: 1.5; overflow-wrap: anywhere; }
  .err { color: var(--danger); font-size: var(--fs-sm); }
  .spin { width: 14px; height: 14px; border-radius: 50%; border: 2px solid var(--fill-2); border-top-color: var(--accent); animation: au-spin .7s linear infinite; flex: none; }
  @keyframes au-spin { to { transform: rotate(360deg); } }
</style>
