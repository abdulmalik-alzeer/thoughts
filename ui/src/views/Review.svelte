<script lang="ts">
  // Review panel for a staged change to the customisation layer: nothing here is loaded into the app
  // until the user chooses Try it or Keep.
  import Icon from '../lib/Icon.svelte';
  import { app, go, toast } from '../lib/store.svelte';
  import { t, tn } from '../lib/i18n';
  import { trapFocus } from '../lib/focus';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { closeReview, discardPending, keepPending, stagedPermissions, tryPending, whenText } from '../lib/layer';

  const p = $derived(app.pending);
  const files = $derived(p?.files ?? []);
  const outside = $derived(p?.outsideChanges ?? []);
  let busy = $state('');
  // Unattended runs need an explicit yes: never pre-ticked, reset whenever the staged change is replaced.
  let allowWorkflows = $state(false);
  const canAllow = $derived((p?.origin ?? 'customize') === 'customize');
  $effect(() => { void p?.summary; void p?.files; allowWorkflows = false; });
  let perms = $state<{ name: string; permissions: string[] }[] | null>(null);

  $effect(() => {
    if (p?.pluginChanged && !p.running) { perms = null; stagedPermissions().then((x) => (perms = x)); }
  });

  async function act(key: string, fn: () => Promise<void>, failKey: string) {
    if (busy) return;
    busy = key;
    try { await fn(); } catch (e: any) { toast(t(failKey), { kind: 'error', detail: e?.message ?? String(e) }); }
    finally { busy = ''; }
  }
  const tryIt = () => act('try', async () => { await tryPending(); toast(t('layer.previewOn'), { key: 'layer' }); }, 'layer.tryFailed');
  const keep = () => act('keep', () => keepPending({ approveWorkflows: canAllow && allowWorkflows && !!p?.workflowChanged }), 'layer.keepFailed');
  async function discard() {
    if (!(await confirmDialog({ title: t('layer.discardTitle'), body: t('layer.discardBody'), confirmLabel: t('layer.discard'), danger: true }))) { app.showReview = true; return; }
    await act('discard', async () => { await discardPending(); toast(t('layer.discarded'), { key: 'layer' }); }, 'layer.discardFailed');
  }
  function watch() { const id = p?.chatId; closeReview(); if (id) go({ kind: 'chat', id }); }
</script>

{#if p}
<div class="modal-bg no-print" role="presentation" onclick={(e) => e.target === e.currentTarget && closeReview()}>
  <div class="modal rv" role="dialog" aria-modal="true" aria-labelledby="rv-title" tabindex="-1" use:trapFocus={{ onEscape: closeReview }}>
    <div class="row">
      <h1 class="grow row" id="rv-title"><Icon name="palette" size={22} /> {t('layer.reviewTitle')}</h1>
      <span class="pill">{t('layer.origin.' + (p.origin ?? 'customize'))}</span>
    </div>

    {#if p.running}
      <p role="status" class="row"><span class="spin" aria-hidden="true"></span> {t('layer.running')}</p>
      <div class="row end">
        <button class="btn" onclick={closeReview}>{t('close')}</button>
        {#if p.chatId}<button class="btn primary" onclick={watch}>{t('layer.watch')}</button>{/if}
      </div>
    {:else}
      <p class="muted">{t('layer.reviewIntro')}</p>
      <h2>{t('layer.whatChanges')}</h2>
      <p class="sum" dir="auto">{p.summary?.trim() || t('layer.noSummary')}</p>

      {#if p.pluginChanged}
        <div class="warn" role="note">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b>{t('layer.pluginWarnTitle')}</b>
            <p>{t('layer.pluginWarn')}</p>
            {#if perms === null}
              <p class="muted" role="status">{t('shell.loading')}</p>
            {:else}
              {#each perms as pl (pl.name)}
                <p class="pl"><bdi>{pl.name}</bdi> — {pl.permissions.length ? t('layer.pluginAsks') : t('layer.pluginAsksNothing')}</p>
                {#if pl.permissions.length}<ul>{#each pl.permissions as x}<li>{t('layer.perm.' + x)}</li>{/each}</ul>{/if}
              {/each}
            {/if}
          </div>
        </div>
      {/if}
      {#if outside.length}
        <div class="warn" role="note">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b>{t('layer.outsideTitle')}</b>
            <p>{t('layer.outside')}</p>
            <ul>{#each outside as f}<li><bdi>{f}</bdi></li>{/each}</ul>
          </div>
        </div>
      {/if}

      {#if p.workflowChanged}
        <div class="warn" role="group" aria-labelledby="rv-wf">
          <Icon name="clock" size={16} />
          <div class="grow">
            <b id="rv-wf">{t('layer.wfWarnTitle')}</b>
            <p>{t('layer.wfWarn')}</p>
            {#each p.workflows ?? [] as w (w.name)}
              <p class="pl"><span dir="auto">{w.name}</span> — {whenText(w.trigger)}</p>
              <pre dir="auto">{w.prompt}</pre>
              {#if (w as any).valid === false}<p class="muted">{t('layer.wfInvalid')}</p>{/if}
            {/each}
            {#if canAllow}
              <label class="allow"><input type="checkbox" bind:checked={allowWorkflows} /> {t('layer.wfAllow')}</label>
              <p class="muted">{t('layer.wfAllowHint')}</p>
            {:else}
              <p class="muted">{t('layer.wfAllowLater')}</p>
            {/if}
          </div>
        </div>
      {/if}

      <h2>{tn('layer.filesChanged', files.length)}</h2>
      <ul class="files">
        {#each files as f (f.path)}
          <li><span class="pill st {f.status}">{t('layer.status.' + f.status)}</span> <bdi>{f.path}</bdi></li>
        {/each}
      </ul>

      {#if app.safeMode}<p class="muted">{t('layer.safeNoTry')}</p>{/if}
      <div class="row end">
        <button class="btn ghost" disabled={!!busy} onclick={closeReview}>{t('layer.later')}</button>
        <span class="grow"></span>
        <button class="btn danger" disabled={!!busy} onclick={discard}>{t('layer.discard')}</button>
        <button class="btn" disabled={!!busy || app.safeMode} aria-busy={busy === 'try'} onclick={tryIt}><Icon name="eye" size={15} /> {t('layer.try')}</button>
        <!-- svelte-ignore a11y_autofocus -->
        <button class="btn primary" autofocus disabled={!!busy} aria-busy={busy === 'keep'} onclick={keep}><Icon name="check" size={15} /> {t('layer.keep')}</button>
      </div>
    {/if}
  </div>
</div>
{/if}

<style>
  .rv { max-width: 640px; display: grid; gap: 10px; }
  .rv h1, .rv h2, .rv p, .rv ul { margin: 0; }
  .rv h2 { font-size: 14px; margin-top: 6px; }
  .sum { white-space: pre-wrap; overflow-wrap: anywhere; }
  .warn { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border-radius: var(--radius); border: var(--border-w) solid var(--accent-2); background: var(--fill); font-size: 13px; }
  .warn > :global(.icon) { flex: none; margin-top: 2px; color: var(--accent-2); }
  .warn p { margin-top: 4px; }
  .warn ul { padding-inline-start: 20px; margin-top: 2px; }
  .pl { font-weight: 500; }
  .warn pre { margin: 4px 0 0; max-height: 160px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12.5px; padding: 8px; text-align: start; }
  .allow { display: flex; gap: 8px; align-items: center; margin: 8px 0 0; font-size: 13px; font-weight: 600; color: var(--fg); }
  .files { list-style: none; padding: 0; display: grid; gap: 4px; font-size: 13px; max-height: 180px; overflow: auto; }
  .files bdi { font-family: var(--mono); font-size: 12.5px; overflow-wrap: anywhere; }
  .st { font-size: 12px; }
  .st.added { color: var(--ok); }
  .st.deleted { color: var(--danger); }
  .end { flex-wrap: wrap; margin-top: 8px; }
  .spin { width: 14px; height: 14px; border-radius: 50%; border: 2px solid var(--fill-2); border-top-color: var(--accent); animation: rvspin .7s linear infinite; flex: none; }
  @keyframes rvspin { to { transform: rotate(360deg); } }
</style>
