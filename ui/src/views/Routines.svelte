<script lang="ts">
  import HelpTip from '../lib/HelpTip.svelte';
  // Settings → Customisations → Routines: the step-by-step routines the assistant follows. Change one
  // or make a new one — by hand or from an assistant draft — then review exactly what will be saved
  // and Keep it. The background service writes the routine; the assistant itself never can.
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { t } from '../lib/i18n';
  import { errCode, failed } from '../lib/errors';
  import { trapFocus } from '../lib/focus';

  type Draft = { title: string; description: string; steps: string };
  type Routine = Draft & { id: string; hash: string; comesWithApp: boolean; unchanged: boolean; extra: string[] };

  let list = $state<Routine[]>([]);
  let phase = $state<'loading' | 'ok' | 'error' | 'nolib'>('loading');
  let error = $state('');
  // The routine being edited (`base` null for a new one), and whether the review is showing.
  let editing = $state<{ base: Routine | null; draft: Draft } | null>(null);
  let reviewing = $state(false);
  let request = $state('');
  let busy = $state<Record<string, boolean>>({});

  async function load() {
    try { const r = await api('/routines'); list = Array.isArray(r) ? r : []; phase = 'ok'; }
    catch (e: any) {
      if (errCode(e) === 'no_library') phase = 'nolib';
      else { phase = 'error'; error = e?.message ?? String(e); }
    }
  }
  onMount(() => {
    load();
    return on((e) => { if (e.type === 'library.changed' || e.type === 'resync') load(); });
  });

  const empty = (): Draft => ({ title: '', description: '', steps: '' });
  const pick = (r: Routine): Draft => ({ title: r.title, description: r.description, steps: r.steps });
  const same = (a: Draft, b: Draft) => a.title.trim() === b.title.trim() && a.description.trim() === b.description.trim() && a.steps.trim() === b.steps.trim();

  function open(r: Routine | null) { editing = { base: r, draft: r ? pick(r) : empty() }; reviewing = false; request = ''; }
  async function close() {
    if (editing && !same(editing.draft, editing.base ? pick(editing.base) : empty())) {
      if (!(await confirmDialog({ title: t('auto.rt.discardTitle'), confirmLabel: t('auto.rt.discard'), danger: true }))) return;
    }
    editing = null; reviewing = false;
  }

  // The same rule the background service applies: nothing that would run a program.
  const runsCommands = (s: string) => s.includes('!`') || s.split('\n').some((l) => { const x = l.trimStart(); return x.startsWith('!') && !x.startsWith('![') && x.length > 1; });
  const problem = $derived.by(() => {
    if (!editing) return '';
    const d = editing.draft;
    if ([d.title, d.description, d.steps].some(runsCommands)) return t('auto.rt.unsafe');
    return '';
  });
  const complete = $derived(!!editing && !!editing.draft.title.trim() && !!editing.draft.description.trim() && !!editing.draft.steps.trim());
  const changed = $derived(!!editing && !same(editing.draft, editing.base ? pick(editing.base) : empty()));

  async function run(key: string, failKey: string, fn: () => Promise<unknown>) {
    if (busy[key]) return;
    busy[key] = true;
    try { await fn(); }
    catch (e) {
      if (errCode(e) === 'stale') { toast(String((e as any)?.message ?? ''), { kind: 'error' }); editing = null; reviewing = false; await load(); }
      else failed(t(failKey), e);
    } finally { busy[key] = false; }
  }

  const draftIt = () => run('draft', 'auto.rt.draftFailed', async () => {
    if (!editing || !request.trim()) return;
    const current = editing.base || editing.draft.steps.trim() ? editing.draft : undefined;
    const d = await post<Draft>('/routines/draft', { request, current });
    if (editing && d) { editing.draft = { title: d.title || editing.draft.title, description: d.description || editing.draft.description, steps: d.steps || editing.draft.steps }; request = ''; }
  });

  const keep = () => run('keep', 'auto.rt.saveFailed', async () => {
    if (!editing) return;
    await post('/routines/save', { id: editing.base?.id ?? null, hash: editing.base?.hash ?? null, routine: editing.draft });
    editing = null; reviewing = false;
    toast(t('auto.rt.kept'), { kind: 'success' });
    await load();
  });

  async function remove(r: Routine) {
    if (!(await confirmDialog({ title: t('auto.rt.removeTitle', { name: r.title }), body: t('auto.rt.removeBody'), confirmLabel: t('auto.rt.remove'), danger: true }))) return;
    await run('rm:' + r.id, 'auto.rt.removeFailed', async () => { await post('/routines/remove', { id: r.id, hash: r.hash }); toast(t('auto.rt.removed')); await load(); });
  }

  // A line-by-line comparison of the steps, for the review.
  type Line = { kind: 'same' | 'add' | 'del'; text: string };
  function diff(a: string, b: string): Line[] {
    const x = a ? a.split('\n') : [], y = b ? b.split('\n') : [];
    if (x.length * y.length > 400_000) return [...x.map((text) => ({ kind: 'del' as const, text })), ...y.map((text) => ({ kind: 'add' as const, text }))];
    const n = x.length, m = y.length;
    const L: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) L[i][j] = x[i] === y[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
    const out: Line[] = [];
    let i = 0, j = 0;
    while (i < n && j < m) {
      if (x[i] === y[j]) { out.push({ kind: 'same', text: x[i] }); i++; j++; }
      else if (L[i + 1][j] >= L[i][j + 1]) out.push({ kind: 'del', text: x[i++] });
      else out.push({ kind: 'add', text: y[j++] });
    }
    while (i < n) out.push({ kind: 'del', text: x[i++] });
    while (j < m) out.push({ kind: 'add', text: y[j++] });
    return out;
  }
  const tidy = (d: Draft): Draft => ({ title: d.title.split(/\s+/).join(' ').trim(), description: d.description.split(/\s+/).join(' ').trim(), steps: d.steps.trim() });
</script>

{#if phase === 'loading'}<p class="muted sm" role="status">{t('shell.loading')}</p>
{:else if phase === 'nolib'}<p class="muted sm">{t('auto.rt.noLibrary')}</p>
{:else if phase === 'error'}<p class="err" role="alert">{t('auto.rt.loadFailed')} <bdi>{error}</bdi> <button type="button" class="btn ghost" onclick={load}>{t('shell.retry')}</button></p>
{:else}
  {#if !list.length}<p class="muted sm">{t('auto.rt.empty')}</p>{/if}
  <ul class="rts">
    {#each list as r (r.id)}
      <li>
        <div class="row wrap">
          <b class="grow" dir="auto">{r.title}</b>
          {#if r.comesWithApp}<span class="pill">{t(r.unchanged ? 'auto.rt.fromApp' : 'auto.rt.changedByYou')}</span>{/if}
          <button type="button" class="btn ghost" onclick={() => open(r)}><Icon name="edit" size={14} /> {t('auto.rt.edit')}</button>
          {#if !r.comesWithApp}<button type="button" class="btn ghost danger" disabled={busy['rm:' + r.id]} onclick={() => remove(r)}>{t('auto.rt.remove')}</button>{/if}
        </div>
        {#if r.description}<div class="muted sm" dir="auto">{r.description}</div>{/if}
      </li>
    {/each}
  </ul>
  <button type="button" class="btn gap" onclick={() => open(null)}><Icon name="plus" size={15} /> {t('auto.rt.new')}</button>
{/if}

{#if editing}
  {@const before = editing.base ? tidy(pick(editing.base)) : null}
  {@const after = tidy(editing.draft)}
  <div class="modal-bg no-print" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
    <div class="modal ed" role="dialog" aria-modal="true" aria-labelledby="rt-title" tabindex="-1" use:trapFocus={{ onEscape: close }}>
      {#if !reviewing}
        <h2 id="rt-title">{editing.base ? editing.base.title : t('auto.rt.new')}</h2>
        <form class="ask row wrap" onsubmit={(e) => { e.preventDefault(); draftIt(); }}>
          <label for="rt-ask" class="full">{t('auto.rt.ask')}</label>
          <input id="rt-ask" class="grow" dir="auto" bind:value={request} placeholder={t('auto.rt.askPh')} />
          <button type="submit" class="btn" disabled={!request.trim() || busy.draft}>{busy.draft ? t('auto.rt.drafting') : t('auto.rt.askGo')}</button>
          <p class="muted sm full">{t('auto.rt.draftCost')}</p>
        </form>
        <label for="rt-name">{t('auto.rt.title')}</label>
        <input id="rt-name" dir="auto" maxlength="80" bind:value={editing.draft.title} />
        <label for="rt-when">{t('auto.rt.when')}</label>
        <input id="rt-when" dir="auto" maxlength="500" bind:value={editing.draft.description} aria-describedby="rt-when-hint" />
        <p id="rt-when-hint" class="muted sm">{t('auto.rt.whenHint')}</p>
        <label for="rt-steps">{t('auto.rt.steps')}</label>
        <textarea id="rt-steps" dir="auto" rows="12" bind:value={editing.draft.steps}></textarea>
        {#if problem}<p class="err" role="alert">{problem}</p>{/if}
        <div class="row end">
          <button type="button" class="btn" onclick={close}>{t('cancel')}</button>
          <button type="button" class="btn primary" disabled={!complete || !changed || !!problem || busy.draft} onclick={() => (reviewing = true)}>{t('auto.rt.review')}</button>
        </div>
      {:else}
        <h2 id="rt-title">{t('auto.rt.reviewTitle')} <HelpTip text={t('auto.rt.reviewHint')} /></h2>
        {#if editing.base?.extra?.length}<p class="sm note" role="note">{t('auto.rt.extra')}</p>{/if}
        {#each [['auto.rt.title', 'title'], ['auto.rt.when', 'description']] as [label, key] (key)}
          {@const a = before ? (before as any)[key] : ''}
          {@const b = (after as any)[key]}
          <div class="field">
            <div class="lbl">{t(label)}{#if before && a === b} <span class="muted">· {t('auto.rt.unchangedPart')}</span>{/if}</div>
            {#if before && a !== b}<div class="del" dir="auto"><span class="sr">{t('auto.rt.before')}: </span>{a}</div>{/if}
            <div class={before && a !== b ? 'add' : ''} dir="auto">{#if before && a !== b}<span class="sr">{t('auto.rt.after')}: </span>{/if}{b}</div>
          </div>
        {/each}
        <div class="field">
          <div class="lbl">{t('auto.rt.steps')}</div>
          <div class="steps" dir="auto">
            {#each diff(before?.steps ?? '', after.steps) as l, i (i)}
              <div class={before ? l.kind : 'same'}><span class="mark" aria-hidden="true">{before ? (l.kind === 'add' ? '+' : l.kind === 'del' ? '−' : ' ') : ''}</span>{#if before && l.kind !== 'same'}<span class="sr">{t(l.kind === 'add' ? 'auto.rt.after' : 'auto.rt.before')}: </span>{/if}{l.text || ' '}</div>
            {/each}
          </div>
        </div>
        <div class="row end">
          <button type="button" class="btn" onclick={() => (reviewing = false)}>{t('auto.rt.backToEdit')}</button>
          <button type="button" class="btn primary" disabled={busy.keep} onclick={keep}><Icon name="check" size={15} /> {t('auto.rt.keep')}</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .sm { font-size: var(--fs-xs); }
  .wrap { flex-wrap: wrap; }
  .gap { margin-top: 10px; }
  .rts { list-style: none; margin: 0; padding: 0; }
  .rts li { padding: 8px 0; border-bottom: 1px solid var(--hairline); display: grid; gap: 2px; font-size: 13px; }
  .rts li:last-child { border-bottom: 0; }
  .ed { max-width: 640px; width: 100%; display: grid; gap: 4px; max-height: 90vh; overflow: auto; }
  .ed h2 { margin: 0 0 6px; font-size: 17px; }
  .ed p { margin: 0; }
  .ed textarea { min-height: 200px; font-size: 13px; line-height: 1.5; }
  .ask { padding: 8px 10px; border-radius: var(--radius); background: var(--fill); margin-bottom: 4px; }
  .ask label { margin-top: 0; }
  .full { flex-basis: 100%; }
  .end { justify-content: flex-end; margin-top: 10px; }
  .field { margin-top: 8px; display: grid; gap: 2px; font-size: 13px; }
  .lbl { font-size: var(--fs-xs); font-weight: 600; color: var(--muted); }
  .steps { white-space: pre-wrap; background: var(--fill); border-radius: var(--radius); padding: 6px 8px; max-height: 45vh; overflow: auto; font-size: 13px; line-height: 1.5; }
  .mark { display: inline-block; width: 1.2em; color: var(--muted); }
  .add { background: color-mix(in srgb, var(--success, #2a9d5b) 16%, transparent); }
  .del { background: color-mix(in srgb, var(--danger) 14%, transparent); text-decoration: line-through; text-decoration-color: color-mix(in srgb, var(--danger) 60%, transparent); }
  .note { color: var(--fg); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  .err { color: var(--danger); font-size: var(--fs-sm); margin: 4px 0 0; }
</style>
