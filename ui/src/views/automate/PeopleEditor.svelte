<script lang="ts">
  // The People list: the people the user writes to, kept as ordinary notes in the library
  // (`people/`, written by the background service). Shown as a card on the Automate tab, and as a
  // step after the Email writing routine is kept (`step`), where adding someone is offered at once.
  import Icon from '../../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { on, post } from '../../lib/api';
  import { toast } from '../../lib/store.svelte';
  import { t } from '../../lib/i18n';
  import { errCode } from '../../lib/errors';
  import { confirmDialog } from '../../lib/dialog.svelte';
  import { emptyPerson, loadPeople, personBody, personLine, personProblem, samePerson, type Person } from '../../lib/automate';

  let { step = false }: { step?: boolean } = $props();

  let people = $state<Person[]>([]);
  let priv = $state(false);
  let phase = $state<'loading' | 'ok' | 'error' | 'nolib'>('loading');
  let loadError = $state('');
  let editing = $state<{ base: Person | null; p: Person } | null>(null);
  let saveError = $state('');
  let saving = $state(false);
  let removing = $state('');
  let tried = $state(false);
  const uid = Math.random().toString(36).slice(2, 8);
  const fid = (x: string) => `au-pp-${uid}-${x}`;

  async function load(first = false) {
    try {
      const r = await loadPeople();
      people = r.people; priv = r.private; phase = 'ok';
      // In the step after the routine, an empty list opens the form straight away.
      if (first && step && !people.length && !editing) add();
    } catch (e: any) {
      if (errCode(e) === 'no_library' || /library/i.test(errCode(e))) phase = 'nolib';
      else { phase = 'error'; loadError = String(e?.message ?? e ?? ''); }
    }
  }
  onMount(() => {
    load(true);
    return on((e) => { if (e?.type === 'library.changed' || e?.type === 'resync' || e?.type === 'library.switched') load(); });
  });

  function add() { editing = { base: null, p: emptyPerson() }; saveError = ''; tried = false; focusName(); }
  function edit(p: Person) { editing = { base: p, p: { ...p } }; saveError = ''; tried = false; focusName(); }
  function focusName() { queueMicrotask(() => document.getElementById(fid('name'))?.focus()); }

  const problem = $derived(editing ? personProblem(editing.p) : '');
  const changed = $derived(!!editing && !samePerson(editing.p, editing.base ?? emptyPerson()));

  async function cancel() {
    if (editing && changed && !(await confirmDialog({ title: t('auto-ui.ppl.discardTitle'), confirmLabel: t('auto-ui.ed.discard'), danger: true }))) return;
    editing = null; saveError = '';
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (!editing || saving) return;
    tried = true;
    if (problem) return;
    saving = true; saveError = '';
    const base = editing.base;
    try {
      await post('/people/save', { ...(base ? { slug: base.slug, hash: base.hash } : {}), person: personBody(editing.p) });
      toast(t(base ? 'auto-ui.ppl.saved' : 'auto-ui.ppl.added', { name: editing.p.name.trim() }), { kind: 'success' });
      editing = null;
      await load();
    } catch (err: any) {
      saveError = String(err?.message ?? err ?? '');
      if (errCode(err) === 'stale' || errCode(err) === 'not_found') { editing = null; toast(saveError, { kind: 'error' }); await load(); }
    } finally { saving = false; }
  }

  async function remove(p: Person) {
    if (!(await confirmDialog({ title: t('auto-ui.ppl.removeTitle', { name: p.name }), body: t('auto-ui.ppl.removeBody'), confirmLabel: t('auto-ui.ppl.remove'), danger: true }))) return;
    removing = p.slug ?? '';
    try { await post('/people/remove', { slug: p.slug, hash: p.hash }); toast(t('auto-ui.ppl.removed', { name: p.name })); await load(); }
    catch (err: any) { toast(t('auto-ui.ppl.removeFailed') + ' ' + String(err?.message ?? ''), { kind: 'error' }); await load(); }
    finally { removing = ''; }
  }

  const FIELDS = [
    ['email', 'email'], ['role', 'text'], ['organisation', 'text'], ['relationship', 'text'], ['language', 'text'], ['tone', 'text'],
  ] as const;
</script>

{#if phase === 'loading'}<p class="muted sm" role="status">{t('shell.loading')}</p>
{:else if phase === 'nolib'}<p class="muted sm">{t('auto-ui.blocked.noLibrary')}</p>
{:else if phase === 'error'}
  <p class="err" role="alert">{t('auto-ui.ppl.loadFailed')} <bdi>{loadError}</bdi> <button type="button" class="btn ghost" onclick={() => load()}>{t('shell.retry')}</button></p>
{:else}
  {#if priv}
    <p class="note row" role="note"><Icon name="lock" size={15} /> <span>{t('auto-ui.ppl.private')}</span></p>
  {/if}
  {#if people.length}
    <ul class="ppl">
      {#each people as p (p.slug)}
        <li class="row wrap">
          <div class="grow who">
            <b dir="auto">{p.name}</b>
            {#if personLine(p) || p.email}
              <span class="muted sm">{#if personLine(p)}<bdi>{personLine(p)}</bdi>{/if}{#if personLine(p) && p.email} · {/if}{#if p.email}<bdi dir="ltr">{p.email}</bdi>{/if}</span>
            {/if}
          </div>
          <button type="button" class="btn ghost" disabled={!!editing} onclick={() => edit(p)} aria-label={t('auto-ui.ppl.editOne', { name: p.name })}><Icon name="edit" size={14} /> {t('auto-ui.ppl.edit')}</button>
          <button type="button" class="btn ghost danger" disabled={!!editing || removing === p.slug} onclick={() => remove(p)} aria-label={t('auto-ui.ppl.removeOne', { name: p.name })}>{t('auto-ui.ppl.remove')}</button>
        </li>
      {/each}
    </ul>
  {:else if !editing}
    <p class="muted sm">{t('auto-ui.ppl.empty')}</p>
  {/if}

  {#if editing}
    <form class="form" onsubmit={save} aria-labelledby={fid('h')} novalidate>
      <h4 id={fid('h')}>{editing.base ? t('auto-ui.ppl.editOne', { name: editing.base.name }) : t('auto-ui.ppl.new')}</h4>
      <label for={fid('name')}>{t('auto-ui.ppl.f.name')}</label>
      <input id={fid('name')} dir="auto" maxlength="120" bind:value={editing.p.name} aria-invalid={tried && problem === 'auto-ui.ppl.needName'} autocomplete="off" />
      <div class="grid">
        {#each FIELDS as [f, type] (f)}
          <div>
            <label for={fid(f)}>{t('auto-ui.ppl.f.' + f)}</label>
            <input id={fid(f)} {type} dir={f === 'email' ? 'ltr' : 'auto'} maxlength={f === 'email' ? 254 : 200} bind:value={editing.p[f]}
              placeholder={t('auto-ui.ppl.ph.' + f)} autocomplete="off" aria-invalid={f === 'email' && tried && problem === 'auto-ui.ppl.badEmail'} />
          </div>
        {/each}
      </div>
      <label for={fid('notes')}>{t('auto-ui.ppl.f.notes')}</label>
      <textarea id={fid('notes')} dir="auto" rows="3" bind:value={editing.p.notes} placeholder={t('auto-ui.ppl.ph.notes')}></textarea>
      {#if editing.base?.extra?.length}<p class="muted sm">{t('auto-ui.ppl.extra')}</p>{/if}
      {#if tried && problem}<p class="err" role="alert">{t(problem)}</p>{/if}
      {#if saveError}<p class="err" role="alert"><bdi>{saveError}</bdi></p>{/if}
      <div class="row end">
        <button type="button" class="btn" disabled={saving} onclick={cancel}>{t('cancel')}</button>
        <button type="submit" class="btn primary" disabled={saving || (!!editing.base && !changed)} aria-busy={saving}><Icon name="check" size={15} /> {t('auto-ui.ppl.save')}</button>
      </div>
    </form>
  {:else}
    <button type="button" class="btn gap" onclick={add}><Icon name="plus" size={15} /> {t(people.length ? 'auto-ui.ppl.addAnother' : 'auto-ui.ppl.add')}</button>
  {/if}
{/if}

<style>
  .sm { font-size: var(--fs-xs); }
  .wrap { flex-wrap: wrap; }
  .gap { margin-top: 10px; }
  .ppl { list-style: none; margin: 0; padding: 0; }
  .ppl li { padding: 8px 0; border-bottom: 1px solid var(--hairline); gap: 6px; align-items: center; font-size: 13px; }
  .ppl li:last-child { border-bottom: 0; }
  .who { display: grid; gap: 1px; min-width: 160px; }
  .note { gap: 8px; align-items: flex-start; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); font-size: 13px; margin: 0 0 8px; }
  .note :global(.icon) { flex: none; color: var(--accent-2); margin-top: 2px; }
  .form { display: grid; gap: 4px; margin-top: 10px; padding: 10px 12px; border-radius: var(--radius); background: var(--fill); }
  .form h4 { margin: 0 0 4px; font-size: 13.5px; font-weight: 600; }
  .form p { margin: 0; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 4px 12px; }
  .grid > div { display: grid; gap: 4px; }
  .end { justify-content: flex-end; gap: 8px; margin-top: 8px; }
  .err { color: var(--danger); font-size: var(--fs-sm); }
</style>
