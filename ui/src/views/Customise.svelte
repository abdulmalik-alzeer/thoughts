<script lang="ts">
  // Settings → Customisations: ask for a change, review what is waiting, history, sharing, workflows, app updates.
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { t, fmtDate } from '../lib/i18n';
  import { checkPending, exportPack, importPack, openReview, startCustomise, whenText } from '../lib/layer';
  import { checkAppUpdate } from '../lib/updates';

  let { show }: { show: (...keys: string[]) => boolean } = $props();

  let text = $state('');
  let busy = $state<Record<string, boolean>>({});
  let workflows = $state<any[]>([]);
  let wfState = $state<'loading' | 'ok' | 'error' | 'none'>('loading');
  let wfError = $state('');
  let picker = $state<HTMLInputElement>();

  async function run(key: string, failKey: string, fn: () => Promise<unknown>) {
    if (busy[key]) return;
    busy[key] = true;
    try { await fn(); } catch (e: any) { toast(t(failKey), { kind: 'error', detail: e?.message ?? String(e) }); }
    finally { busy[key] = false; }
  }

  async function loadWorkflows() {
    try { const r = await api('/workflows'); workflows = Array.isArray(r) ? r : []; wfState = 'ok'; }
    catch (e: any) { if (e?.status === 404) wfState = 'none'; else { wfState = 'error'; wfError = e?.message ?? String(e); } }
  }
  onMount(() => {
    loadWorkflows();
    checkPending(false);
    return on((e) => { if (e.type === 'layer.changed' || e.type === 'layer.pending' || e.type === 'resync') loadWorkflows(); });
  });

  const ask = () => run('ask', 'layer.askFailed', async () => { if (await startCustomise(text)) text = ''; });
  const doExport = () => run('export', 'layer.exportFailed', async () => {
    const r = await exportPack();
    if (r === 'saved') toast(t('layer.exported'), { kind: 'success' });
    else if (r === 'copied') toast(t('layer.exportCopied'));
  });
  function pick(e: Event) {
    const el = e.target as HTMLInputElement;
    const f = el.files?.[0];
    el.value = '';
    if (f) run('import', 'layer.importFailed', () => importPack(f));
  }

  async function allow(w: any) {
    const ok = await confirmDialog({ title: t('layer.wf.allowTitle', { name: w.name }), body: `${whenText(w.trigger)}\n\n${w.prompt ?? ''}`.trim(), note: t('layer.wf.allowNote'), confirmLabel: t('layer.wf.allow') });
    if (ok) await run('wf:' + w.name, 'layer.wf.failed', async () => { await post('/workflows/approve', { name: w.name }); await loadWorkflows(); });
  }
  const disable = (w: any) => run('wf:' + w.name, 'layer.wf.failed', async () => { await post('/workflows/disable', { name: w.name }); await loadWorkflows(); toast(t('layer.wf.disabled', { name: w.name })); });
  const when = (d: any) => (d ? fmtDate(d, { dateStyle: 'medium', timeStyle: 'short' }) : t('layer.wf.never'));
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
    <div class="row gap-top wrap">
      <button type="button" class="btn" onclick={() => (app.showRollback = true)}><Icon name="rollback" size={15} /> {t('layer.history')}…</button>
      <span class="muted sm grow">{t('layer.historyHint')}</span>
    </div>
  </div>
{/if}

{#if show('layer.share')}
  <div class="card">
    <h3>{t('layer.share')}</h3>
    <p class="muted sm">{t('layer.shareHint')}</p>
    <div class="row wrap">
      <button type="button" class="btn" disabled={busy.export} onclick={doExport}><Icon name="download" size={15} /> {t('layer.export')}</button>
      <button type="button" class="btn" disabled={busy.import} onclick={() => picker?.click()}><Icon name="upload" size={15} /> {t('layer.import')}</button>
      <input type="file" accept=".json,application/json" hidden bind:this={picker} onchange={pick} />
    </div>
    <p class="muted sm gap-top">{t('layer.importHint')}</p>
  </div>
{/if}

{#if show('layer.workflows')}
  <div class="card">
    <h3>{t('layer.workflows')}</h3>
    <p class="muted sm">{t('layer.workflowsHint')}</p>
    {#if wfState === 'loading'}<p class="muted sm" role="status">{t('shell.loading')}</p>
    {:else if wfState === 'error'}<p class="err" role="alert">{t('layer.wf.loadFailed')} <bdi>{wfError}</bdi> <button type="button" class="btn ghost" onclick={loadWorkflows}>{t('shell.retry')}</button></p>
    {:else if !workflows.length}<p class="muted sm">{t('layer.wf.empty')}</p>
    {:else}
      {#each workflows as w (w.name)}
        <div class="wf">
          <div class="row wrap">
            <b class="grow" dir="auto">{w.name}</b>
            {#if w.approved === false}
              <span class="pill">{t('layer.wf.notAllowed')}</span>
              <button type="button" class="btn" disabled={busy['wf:' + w.name]} onclick={() => allow(w)}>{t('layer.wf.allow')}</button>
            {:else}
              <button type="button" class="btn ghost" disabled={busy['wf:' + w.name]} onclick={() => disable(w)}>{t('layer.wf.turnOff')}</button>
            {/if}
          </div>
          <div class="muted sm">{whenText(w.trigger)}</div>
          <div class="muted sm">{t('layer.wf.last', { when: when(w.lastRun) })}{#if w.nextRun && w.approved !== false} · {t('layer.wf.next', { when: when(w.nextRun) })}{/if}</div>
        </div>
      {/each}
    {/if}
  </div>
{/if}

{#if show('layer.appUpdates')}
  <div class="card">
    <h3>{t('layer.appUpdates')}</h3>
    <p class="muted sm">{t('layer.appUpdatesHint')}</p>
    <button type="button" class="btn" onclick={() => checkAppUpdate(true)}><Icon name="sync" size={15} /> {t('layer.upd.check')}</button>
  </div>
{/if}

<style>
  h3 { font-size: 14px; font-weight: 600; margin: 0 0 6px; }
  .card { margin-bottom: 12px; }
  .wrap { flex-wrap: wrap; }
  .gap-top { margin-top: 10px; }
  .sm { font-size: 12px; }
  .pend { margin-top: 10px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); font-size: 13px; }
  .pend :global(.icon) { flex: none; color: var(--accent-2); }
  .wf { padding: 10px 0; border-bottom: 1px solid var(--hairline); }
  .wf:last-child { border-bottom: 0; }
  .err { color: var(--danger); font-size: 12.5px; margin: 4px 0 0; }
</style>
