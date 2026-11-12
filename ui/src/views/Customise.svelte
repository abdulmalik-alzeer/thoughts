<script lang="ts">
  // Settings → Customisations: ask for a change, review what is waiting, history, sharing, automatic tasks, routines, app updates.
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { app, toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { t, tn } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { trapFocus } from '../lib/focus';
  import { checkPending, exportPack, layerItems, openReview, packChoices, permText, readPack, stagePack, startCustomise } from '../lib/layer';
  import AutomaticTasks from './AutomaticTasks.svelte';
  import Routines from './Routines.svelte';
  import WaitingChanges from './WaitingChanges.svelte';
  import UpdateChoices from './UpdateChoices.svelte';
  import { checkAppUpdate } from '../lib/updates';

  let { show }: { show: (...keys: string[]) => boolean } = $props();

  let text = $state('');
  let busy = $state<Record<string, boolean>>({});
  let picker = $state<HTMLInputElement>();

  async function run(key: string, failKey: string, fn: () => Promise<unknown>) {
    if (busy[key]) return;
    busy[key] = true;
    try { await fn(); } catch (e) { failed(t(failKey), e); }
    finally { busy[key] = false; }
  }

  onMount(() => { checkPending(false); });

  const ask = () => run('ask', 'layer.askFailed', async () => { if (await startCustomise(text)) text = ''; });
  function exported(r: 'saved' | 'copied' | 'cancelled') {
    if (r === 'saved') toast(t('layer.exported'), { kind: 'success' });
    else if (r === 'copied') toast(t('layer.exportCopied'));
  }
  const doExport = () => run('export', 'layer.exportFailed', async () => exported(await exportPack()));

  // A feature pack: the user ticks which add-ons, pages, tasks and files go in.
  let choices = $state<Awaited<ReturnType<typeof packChoices>> | null>(null);
  let chosen = $state<Record<string, boolean>>({});
  const nChosen = $derived(Object.values(chosen).filter(Boolean).length);
  const chooseExport = () => run('choose', 'layer3.exportListFailed', async () => {
    const list = await packChoices();
    if (!list.length) { toast(t('layer3.exportNothing')); return; }
    chosen = {};
    choices = list;
  });
  const exportChosen = () => run('export', 'layer.exportFailed', async () => {
    const paths = Object.entries(chosen).filter(([, v]) => v).map(([k]) => k);
    if (!paths.length) return;
    const r = await exportPack(paths);
    choices = null;
    exported(r);
  });

  /** A pack's own words (its name, its description) as one plain line: no hidden text, nothing that
   *  reorders it, no quotation marks that could end the quotation it is shown in. (The review that
   *  follows shows the service's stricter reading, with anything invisible written out.) */
  function packWords(v: unknown, max: number): string {
    return String(v ?? '')
      .replace(/[\u0000-\u001f\u007f-\u009f\u00ad\u180e\u200b\u2028-\u202e\u2060-\u206f\ufeff]|[\u{e0000}-\u{e0fff}]/gu, ' ')
      .replace(/["“”«»]/g, '').replace(/\s+/g, ' ').trim().slice(0, max);
  }

  function pick(e: Event) {
    const el = e.target as HTMLInputElement;
    const f = el.files?.[0];
    el.value = '';
    if (!f) return;
    run('import', 'layer.importFailed', async () => {
      const r = await readPack(f);
      if (r.unsupported) { toast(t('layer3.packNewer'), { kind: 'error' }); return; }
      // Say what the pack is before anything is staged; the full review still follows. What the
      // pack says about itself comes last and in quotation marks: it is its maker's claim, not the app's.
      const [name, says] = [packWords(r.name, 80), packWords(r.description, 400)];
      const ok = await confirmDialog({
        title: name ? `${t('layer3.importTitle')} “${name}”` : t('layer3.importTitle'),
        body: [t(r.kind === 'feature' ? 'layer3.importFeature' : 'layer3.importFull'), tn('layer3.importFiles', r.files.length), says ? `${t('lda.packSaysIntro')}\n“${says}”` : ''].filter(Boolean).join('\n\n'),
        list: r.files.slice(0, 30),
        note: t('layer3.importNote'),
        confirmLabel: t('layer3.importGo'),
      });
      if (ok) await stagePack(r.pack);
    });
  }

  const installed = $derived(layerItems());
  const notRunning = $derived(installed.filter((x) => x.unsupported));

</script>

{#if show('layer.customise', 'layer.history')}
  <div class="card">
    <h3>{t('layer.customise')}</h3>
    <p class="muted sm">{t('layer.askHint')}</p>
    <form class="row wrap" onsubmit={(e) => { e.preventDefault(); ask(); }}>
      <input class="grow" dir="auto" bind:value={text} aria-label={t('layer.customise')} placeholder={t('set.customizePh')} />
      <button type="submit" class="btn primary" disabled={!text.trim() || busy.ask}>{busy.ask ? t('set.starting') : t('layer.askGo')}</button>
    </form>
    {#if app.pending?.exists}
      <div class="row pend" role="status">
        <Icon name={app.pending.running ? 'clock' : 'info'} size={15} />
        <span class="grow">{t(app.pending.running ? 'layer.running' : 'layer.pendingWaiting')}</span>
        <button type="button" class="btn" onclick={openReview}>{t('layer.review')}</button>
      </div>
    {/if}
    <WaitingChanges />
    <div class="row gap-top wrap">
      <button type="button" class="btn" onclick={() => (app.showRollback = true)}><Icon name="rollback" size={15} /> {t('layer3.historyDots')}</button>
      <span class="muted sm grow">{t('layer.historyHint')}</span>
    </div>
  </div>
{/if}

{#if show('layer.customise', 'layer.history') && installed.length}
  <div class="card">
    <h3>{t('layer3.installedTitle')}</h3>
    <ul class="inst">
      {#each installed as x (x.kind + ':' + x.name)}
        <li>
          <div class="row wrap"><span class="pill">{t('layer3.kind.' + x.kind)}</span> <bdi class="grow">{x.name}</bdi>{#if x.unsupported}<span class="pill">{t('layer3.notLoadedTitle')}</span>{/if}</div>
          {#if x.description}<div class="muted sm" dir="auto">{x.description}</div>{/if}
          {#if x.unsupported}
            <div class="sm" role="note" dir="auto">{x.message}</div>
          {:else if x.permissions.length}
            <div class="muted sm">{x.permissions.map(permText).join(' · ')}{#if x.write.length} — {t('layer3.writeIn')} {#each x.write as w, i}{i ? ' · ' : ''}<bdi>{w}</bdi>{/each}{/if}</div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if notRunning.length}<p class="muted sm gap-top">{t('layer3.unsupportedBody')}</p>{/if}
  </div>
{/if}

{#if show('layer.share')}
  <div class="card">
    <h3>{t('layer.share')}</h3>
    <p class="muted sm">{t('layer.shareHint')}</p>
    <div class="row wrap">
      <button type="button" class="btn" disabled={busy.export} onclick={doExport}><Icon name="download" size={15} /> {t('layer3.exportAll')}</button>
      <button type="button" class="btn" disabled={busy.export || busy.choose} onclick={chooseExport}>{t('layer3.exportSome')}</button>
      <button type="button" class="btn" disabled={busy.import} onclick={() => picker?.click()}><Icon name="upload" size={15} /> {t('layer.import')}</button>
      <input type="file" accept=".json,application/json" hidden bind:this={picker} onchange={pick} />
    </div>
    <p class="muted sm gap-top">{t('layer.importHint')}</p>
  </div>
{/if}

{#if choices}
  <div class="modal-bg no-print" role="presentation" onclick={(e) => e.target === e.currentTarget && (choices = null)}>
    <div class="modal pick" role="dialog" aria-modal="true" aria-labelledby="pk-title" tabindex="-1" use:trapFocus={{ onEscape: () => (choices = null) }}>
      <h2 id="pk-title">{t('layer3.exportChoose')}</h2>
      <p class="muted sm">{t('layer3.exportChooseHint')}</p>
      <ul class="inst">
        {#each choices as c (c.path)}
          <li><label class="ck"><input type="checkbox" bind:checked={chosen[c.path]} /> <span class="pill">{t('layer3.kind.' + c.kind)}</span> <span class="grow" dir="auto">{c.label}</span> <bdi class="muted sm">{c.path}</bdi></label></li>
        {/each}
      </ul>
      <div class="row end">
        <button type="button" class="btn" onclick={() => (choices = null)}>{t('cancel')}</button>
        <button type="button" class="btn primary" disabled={!nChosen || busy.export} onclick={exportChosen}><Icon name="download" size={15} /> {t('layer3.exportSelected')}</button>
      </div>
    </div>
  </div>
{/if}

{#if show('layer.workflows')}
  <div class="card">
    <h3>{t('layer.workflows')}</h3>
    <p class="muted sm">{t('layer.workflowsHint')}</p>
    <AutomaticTasks />
  </div>
{/if}

{#if show('auto.routines')}
  <div class="card">
    <h3>{t('auto.routines')}</h3>
    <p class="muted sm">{t('auto.routinesHint')}</p>
    <Routines />
  </div>
{/if}

{#if show('layer.appUpdates')}
  <div class="card">
    <h3>{t('layer.appUpdates')}</h3>
    <p class="muted sm">{t('layer.appUpdatesHint')}</p>
    <button type="button" class="btn" onclick={() => checkAppUpdate(true)}><Icon name="sync" size={15} /> {t('layer.upd.check')}</button>
    <UpdateChoices />
  </div>
{/if}

<style>
  h3 { font-size: 14px; font-weight: 600; margin: 0 0 6px; }
  .card { margin-bottom: 12px; }
  .wrap { flex-wrap: wrap; }
  .gap-top { margin-top: 10px; }
  .sm { font-size: var(--fs-xs); }
  .pend { margin-top: 10px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); font-size: 13px; }
  .pend :global(.icon) { flex: none; color: var(--accent-2); }
  .inst { list-style: none; margin: 0; padding: 0; }
  .inst li { padding: 8px 0; border-bottom: 1px solid var(--hairline); display: grid; gap: 2px; font-size: 13px; }
  .inst li:last-child { border-bottom: 0; }
  .pick { max-width: 520px; display: grid; gap: 10px; }
  .pick h2 { margin: 0; font-size: 17px; }
  .pick p { margin: 0; }
  .ck { display: flex; align-items: center; gap: 8px; margin: 0; font-size: 13px; font-weight: 400; color: var(--fg); cursor: pointer; min-height: 24px; }
  .end { justify-content: flex-end; }
</style>
