<script lang="ts">
  // Settings › Customisations › App updates: the choices an app update left, all on one screen.
  // For each part of the app that both the user and the new version changed: what the new version
  // changes (read by the service from the files), then Keep mine / Use the new version / Let the
  // assistant merge. A part the user kept their own version of stays listed, so the new version can
  // still be taken (or merged) later.
  import { onMount } from 'svelte';
  import HelpTip from '../lib/HelpTip.svelte';
  import { on } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { fxGroups } from '../lib/effects';
  import { openReview } from '../lib/layer';
  import { drafts, openDraft } from '../lib/drafts.svelte';
  import { layerUpdateStatus, onUpdates, resolveLayerUpdate, type UpdateQuestion } from '../lib/updates';

  let status = $state<any>(null);
  let busy = $state('');
  const list = (v: unknown): UpdateQuestion[] => (Array.isArray(v) ? v.filter((c: any) => c && typeof c.path === 'string') : []);
  const open = $derived(list(status?.conflicts));
  const kept = $derived(list(status?.kept));

  const refresh = () => layerUpdateStatus().then((s) => { status = s; }, () => {});
  onMount(() => {
    refresh();
    const offUpd = onUpdates((s) => { status = s; });
    const offEv = on((e) => { if (e?.type === 'layer.pending' || e?.type === 'layer.changed') refresh(); });
    return () => { offUpd(); offEv(); };
  });

  const name = (path: string) => { const k = 'ldb.upd.file.' + path, s = t(k as any); return s !== k ? s : path; };

  async function choose(c: UpdateQuestion, choice: 'keep' | 'new' | 'merge') {
    if (busy) return;
    if (choice === 'new' && !(await confirmDialog({ title: t('ldb.upd.useNewTitle', { name: name(c.path) }), body: t('ldb.upd.useNewBody'), confirmLabel: t('layer.upd.useNew') }))) return;
    busy = c.path + ':' + choice;
    try {
      await resolveLayerUpdate(c.path, choice);
      if (choice === 'keep') toast(t('ldb.upd.kept'), { key: 'upd' });
      if (choice === 'new') toast(t('ldb.upd.usedNew'), { kind: 'success', key: 'upd' });
      await refresh();
    } catch (e) { failed(t('layer.upd.resolveFailed'), e); }
    finally { busy = ''; }
  }

  /** The merged version waiting for review: open it (it may be the open change, or set aside). */
  const mergeDraft = (path: string) => drafts.list.find((d) => d.origin === 'update' && d.resolves === path);
  const mergeOpen = (path: string) => !!app.pending?.exists && !app.pending.running && app.pending.origin === 'update' && app.pending.resolves === path;
  async function reviewMerge(path: string) {
    const d = mergeDraft(path);
    try { if (d) await openDraft(d.id); else openReview(); } catch (e) { failed(t('ldb.openFailed'), e); }
  }
</script>

{#snippet question(c: UpdateQuestion, isOpen: boolean)}
  {@const groups = Array.isArray(c.changes) ? fxGroups(c.changes) : null}
  <li>
    <b dir="auto">{name(c.path)}</b>
    <p class="muted sm">{t('ldb.upd.newBrings')}</p>
    {#if groups === null}<p class="sm">{t('ldb.upd.unknown')}</p>
    {:else if !groups.length}<p class="sm">{t('ldb.upd.nothingNew')}</p>
    {:else}
      <ul class="fx">
        {#each groups.flatMap((g) => g.lines) as l (l.key)}
          <li class="sm"><span dir="auto">{l.text}</span>
            {#each l.detail as d}<div class="muted" dir="auto">{d}</div>{/each}
            {#if l.quote !== undefined}<div class="quote" dir="auto">{l.quote}</div>{/if}
            {#if l.may?.length}<div class="muted" dir="auto">{l.mayTitle} {l.may.join(t('ldb.listSep'))}</div>{/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if c.merging}
      <div class="row wrap gap">
        <span class="sm grow" role="status">{mergeOpen(c.path) || mergeDraft(c.path) ? t('ldb.upd.mergeReady') : t('ldb.upd.merging')}</span>
        {#if mergeOpen(c.path) || mergeDraft(c.path)}<button type="button" class="btn" onclick={() => reviewMerge(c.path)}>{t('ldb.open')}</button>{/if}
      </div>
    {:else}
      <div class="row wrap gap">
        {#if isOpen}<button type="button" class="btn primary" disabled={!!busy} aria-busy={busy === c.path + ':keep'} onclick={() => choose(c, 'keep')}>{t('layer.upd.keepMine')}</button>{/if}
        <button type="button" class="btn" disabled={!!busy} aria-busy={busy === c.path + ':new'} onclick={() => choose(c, 'new')}>{t('layer.upd.useNew')}</button>
        <button type="button" class="btn" disabled={!!busy} aria-busy={busy === c.path + ':merge'} onclick={() => choose(c, 'merge')}>{t('layer.upd.merge')}</button>
      </div>
    {/if}
  </li>
{/snippet}

{#if open.length || kept.length}
  <div class="upd" role="group" aria-labelledby="ldb-upd">
    <p id="ldb-upd"><b>{t('ldb.upd.title')}</b> <HelpTip label={t('czh.upd')}>
      {#if open.length}<span>{t('ldb.upd.intro')}</span>{/if}
      <span>{t('layer.upd.conflictNote')}</span>
    </HelpTip></p>
    {#if open.length}
      <ul class="qs">{#each open as c (c.path)}{@render question(c, true)}{/each}</ul>
    {/if}
    {#if kept.length}
      <p class="muted sm">{t('ldb.upd.keptIntro')}</p>
      <ul class="qs">{#each kept as c (c.path)}{@render question(c, false)}{/each}</ul>
    {/if}
  </div>
{/if}

<style>
  .upd { margin-top: 12px; padding-top: 10px; border-top: 1px solid var(--hairline); display: grid; gap: 6px; }
  .upd p { margin: 0; }
  .sm { font-size: var(--fs-xs); }
  .qs { list-style: none; margin: 0; padding: 0; display: grid; gap: 12px; }
  .qs > li { display: grid; gap: 4px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); }
  .fx { margin: 0; padding-inline-start: 18px; display: grid; gap: 2px; }
  .quote { white-space: pre-wrap; overflow-wrap: anywhere; border-inline-start: 3px solid var(--hairline); padding: 0 8px; margin-top: 2px; }
  .gap { gap: 8px; margin-top: 4px; }
  .wrap { flex-wrap: wrap; }
</style>
