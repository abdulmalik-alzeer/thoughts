<script lang="ts">
  // Review panel for a staged change to the customisation layer: nothing here is loaded into the app
  // until the user chooses Try it or Keep.
  //
  // The whole panel is drawn inside a shadow root (lib/layer/shield.ts) with its own fixed styles, so
  // the staged CSS under review can never restyle, hide or relabel it. Classes used here are defined
  // in shield.ts, not in theme.css.
  import Icon from '../lib/Icon.svelte';
  import { app, go, toast } from '../lib/store.svelte';
  import { t, tn } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { shield } from '../lib/layer/shield';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { closeReview, discardPending, dryLoadStaged, keepPending, permText, stagedItems, tryPending, whenText, type LayerItem } from '../lib/layer';

  const p = $derived(app.pending);
  const files = $derived(p?.files ?? []);
  const outside = $derived(p?.outsideChanges ?? []);
  const problems = $derived((Array.isArray(p?.problems) ? p!.problems! : []).filter((x) => x && typeof x.message === 'string'));
  let busy = $state('');
  // Unattended runs need an explicit yes: never pre-ticked, reset whenever the staged change is replaced.
  let allowWorkflows = $state(false);
  const canAllow = $derived((p?.origin ?? 'customize') === 'customize');
  $effect(() => { void p?.summary; void p?.files; void p?.signature; allowWorkflows = false; });

  // What the staged add-ons and pages may do (read as data), then a test load of each in a sealed frame.
  let items = $state<LayerItem[] | null>(null);
  let tested = $state(false);
  let run = 0;
  $effect(() => {
    void p?.signature; void p?.summary;
    if (!p || p.running) return;
    const mine = ++run;
    items = null; tested = false;
    stagedItems().then((x) => { if (mine === run && !tested) items = x; });
    dryLoadStaged().then((x) => { if (mine === run) { items = x; tested = true; } });
  });
  const runnable = $derived((items ?? []).filter((x) => !x.unsupported));
  const unsupported = $derived((items ?? []).filter((x) => x.unsupported));
  const loadErrors = $derived(runnable.filter((x) => x.error));
  // Anything the user should read before deciding: then Keep is not the pre-focused button.
  const caution = $derived(!!(p?.pluginChanged || p?.workflowChanged || outside.length || problems.length));

  async function act(key: string, fn: () => Promise<unknown>, failKey: string) {
    if (busy) return;
    busy = key;
    try { await fn(); } catch (e) { failed(t(failKey), e); }
    finally { busy = ''; }
  }
  const tryIt = () => act('try', async () => { await tryPending(); toast(t('layer.previewOn'), { key: 'layer' }); }, 'layer.tryFailed');
  const keep = () => { if (!problems.length) act('keep', () => keepPending({ approveWorkflows: canAllow && allowWorkflows && !!p?.workflowChanged }), 'layer3.keepFailed'); };
  async function discard() {
    if (!(await confirmDialog({ title: t('layer.discardTitle'), body: t('layer.discardBody'), confirmLabel: t('layer.discard'), danger: true }))) { app.showReview = true; return; }
    await act('discard', async () => { await discardPending(); toast(t('layer.discarded'), { key: 'layer' }); }, 'layer.discardFailed');
  }
  function watch() { const id = p?.chatId; closeReview(); if (id) go({ kind: 'chat', id }); }
</script>

{#if p}
<div class="no-print" use:shield={{ box: 'position:fixed;inset:0;z-index:60', trap: true, onEscape: closeReview, onBackdrop: closeReview }}>
<div class="bg" data-backdrop>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="rv-title" tabindex="-1">
    <div class="row">
      <h1 class="grow row" id="rv-title"><Icon name="palette" size={22} /> {t('layer.reviewTitle')}</h1>
      <span class="pill">{t('layer.origin.' + (p.origin ?? 'customize'))}</span>
    </div>

    {#if p.running}
      <p role="status" class="row"><span class="spin" aria-hidden="true"></span> {t('layer.running')}</p>
      <div class="row end">
        <button class="btn" onclick={closeReview}>{t('close')}</button>
        {#if p.chatId}<button class="btn primary" data-autofocus onclick={watch}>{t('layer.watch')}</button>{/if}
      </div>
    {:else}
      <p class="muted">{t('layer.reviewIntro')}</p>
      <h2 id="rv-what">{t('layer.whatChanges')}</h2>
      <p class="sum" dir="auto" tabindex="-1" aria-labelledby="rv-what rv-sum" id="rv-sum" data-autofocus={caution ? '' : undefined}>{p.summary?.trim() || t('layer.noSummary')}</p>

      {#if problems.length}
        <div class="warn bad" role="alert">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b>{t('layer3.problemsTitle')}</b>
            <p>{t('layer3.problemsBody')}</p>
            <ul>{#each problems as pr}<li>{#if pr.path}<bdi class="mono">{pr.path}</bdi> — {/if}<span dir="auto">{pr.message}</span></li>{/each}</ul>
          </div>
        </div>
      {/if}

      {#if p.pluginChanged || runnable.length}
        <div class="warn" role="group" aria-labelledby="rv-asks">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b id="rv-asks">{p.pluginChanged ? t('layer.pluginWarnTitle') : t('layer3.asksTitle')}</b>
            {#if p.pluginChanged}<p>{t('layer.pluginWarn')}</p>{/if}
            {#if items === null}
              <p class="muted" role="status">{t('shell.loading')}</p>
            {:else}
              {#each runnable as pl (pl.kind + ':' + pl.name)}
                <p class="pl"><span class="pill">{t('layer3.kind.' + pl.kind)}</span> <bdi>{pl.name}</bdi>{#if pl.description} <span class="sub muted" dir="auto">— {pl.description}</span>{/if}</p>
                <p>{pl.permissions.length ? t('layer.pluginAsks') : t('layer.pluginAsksNothing')}</p>
                {#if pl.permissions.length}
                  <ul>
                    {#each pl.permissions as x}
                      <li>{permText(x)}
                        {#if x === 'library:write'}
                          {#if pl.write.length}<br />{t('layer3.writeIn')} {#each pl.write as w, i}{i ? ' · ' : ''}<bdi class="mono">{w}</bdi>{/each}
                          {:else}<br /><span class="muted">{t('layer3.writeNone')}</span>{/if}
                        {/if}
                      </li>
                    {/each}
                  </ul>
                {/if}
              {/each}
              <p class="muted" role="status">{tested ? (loadErrors.length ? '' : t('layer3.checkedOk')) : t('layer3.checking')}</p>
            {/if}
          </div>
        </div>
      {/if}
      {#if loadErrors.length}
        <div class="warn bad" role="alert">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b>{t('layer3.loadErrTitle')}</b>
            <p>{t('layer3.loadErrBody')}</p>
            <ul>{#each loadErrors as x (x.kind + ':' + x.name)}<li><bdi class="mono">{x.name}</bdi> — <bdi>{x.error}</bdi></li>{/each}</ul>
          </div>
        </div>
      {/if}
      {#if unsupported.length}
        <div class="warn" role="note">
          <Icon name="info" size={16} />
          <div class="grow">
            <b>{t('layer3.unsupportedTitle')}</b>
            <p>{t('layer3.unsupportedBody')}</p>
            <ul>{#each unsupported as x (x.kind + ':' + x.name)}<li><bdi class="mono">{x.name}</bdi> — <span dir="auto">{x.message}</span></li>{/each}</ul>
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
      {#if problems.length}<p class="err" id="rv-blocked">{t('layer3.keepBlocked')}</p>{/if}
      <div class="row end">
        <button class="btn ghost" disabled={!!busy} onclick={closeReview}>{t('layer.later')}</button>
        <span class="grow"></span>
        <button class="btn danger" disabled={!!busy} onclick={discard}>{t('layer.discard')}</button>
        <button class="btn" disabled={!!busy || app.safeMode} aria-busy={busy === 'try'} onclick={tryIt}><Icon name="eye" size={15} /> {t('layer.try')}</button>
        <button class="btn primary" data-autofocus={caution ? undefined : ''} disabled={!!busy || problems.length > 0} aria-describedby={problems.length ? 'rv-blocked' : undefined} aria-busy={busy === 'keep'} onclick={keep}><Icon name="check" size={15} /> {t('layer.keep')}</button>
      </div>
    {/if}
  </div>
</div>
</div>
{/if}
