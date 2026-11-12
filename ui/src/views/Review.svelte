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
  import { post } from '../lib/api';
  import { checkLayerUpdates } from '../lib/updates';
  import { closeReview, discardPending, dryLoadStaged, keepPending, permText, stagedItems, tryPending, whenText, type LayerItem } from '../lib/layer';

  const p = $derived(app.pending);
  const files = $derived(p?.files ?? []);
  const outside = $derived(p?.outsideChanges ?? []);
  const problems = $derived((Array.isArray(p?.problems) ? p!.problems! : []).filter((x) => x && typeof x.message === 'string'));
  let busy = $state('');
  // Unattended runs need an explicit yes: never pre-ticked, reset whenever the staged change is replaced.
  let allowWorkflows = $state(false);
  const origin = $derived(String(p?.origin ?? 'customize'));
  const canAllow = $derived(origin === 'customize');
  // The app changed underneath this change and the two cannot be combined: it can only be discarded.
  const stale = $derived(p?.stale === true);
  // The assistant was stopped or failed before finishing: Keep needs an explicit yes, never pre-ticked.
  const incomplete = $derived(p?.incomplete === true);
  let keepIncomplete = $state(false);
  // What the staged style sheet adds, as the service read it: { kind, flags }.
  const css = $derived(p?.css && typeof p.css === 'object' ? (p.css as { kind?: string; flags?: unknown }) : null);
  const cssFlags = $derived((Array.isArray(css?.flags) ? css!.flags : []).filter((f): f is string => typeof f === 'string' && ['text', 'overlay', 'dialogs', 'hides', 'remote'].includes(f)));
  // What an imported pack says about itself: the pack's words, shown as a quotation.
  const packSays = $derived(origin === 'import' && typeof p?.pack?.description === 'string' ? p.pack.description.trim() : '');
  const outsidePrivate = $derived(Number(p?.outsidePrivate) > 0 ? Number(p!.outsidePrivate) : 0);
  $effect(() => { void p?.summary; void p?.files; void p?.signature; allowWorkflows = false; keepIncomplete = false; });

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
  // Why the new parts were not test-run at all (customisations are off, or this change froze the app before).
  const notTested = $derived(runnable.find((x) => x.skipped)?.skipped ?? '');
  // Anything the user should read before deciding: then Keep is not the pre-focused button.
  const caution = $derived(!!(p?.pluginChanged || p?.workflowChanged || outside.length || outsidePrivate || problems.length || stale || incomplete || packSays || cssFlags.length || css?.kind === 'unreadable'));
  const blocked = $derived(problems.length > 0 || stale || (incomplete && !keepIncomplete));

  async function act(key: string, fn: () => Promise<unknown>, failKey: string) {
    if (busy) return;
    busy = key;
    try { await fn(); } catch (e) { failed(t(failKey), e); }
    finally { busy = ''; }
  }
  const tryIt = () => act('try', async () => { if (await tryPending()) toast(t('layer.previewOn'), { key: 'layer' }); }, 'layer.tryFailed');
  const keep = () => {
    if (blocked) return;
    act('keep', async () => {
      // The yes to an unfinished change is recorded for exactly what was reviewed, then it is kept.
      if (incomplete) await post('/layer/pending/accept', { signature: p?.signature ?? '' });
      await keepPending({ approveWorkflows: canAllow && allowWorkflows && !!p?.workflowChanged });
    }, 'layer3.keepFailed');
  };
  async function discard() {
    if (!(await confirmDialog({ title: t('layer.discardTitle'), body: t('layer.discardBody'), confirmLabel: t('layer.discard'), danger: true }))) { app.showReview = true; return; }
    const wasUpdate = origin === 'update';
    await act('discard', async () => { await discardPending(); toast(t('layer.discarded'), { key: 'layer' }); }, 'layer.discardFailed');
    // A discarded merge of an app update leaves its question open: ask it again now.
    if (wasUpdate && !app.pending) checkLayerUpdates();
  }
  function watch() { const id = p?.chatId; closeReview(); if (id) go({ kind: 'chat', id }); }
</script>

{#if p}
<div class="no-print" use:shield={{ box: 'position:fixed;inset:0;z-index:60', trap: true, onEscape: closeReview, onBackdrop: closeReview }}>
<div class="bg" data-backdrop>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="rv-title" tabindex="-1">
    <div class="row">
      <h1 class="grow row" id="rv-title"><Icon name="palette" size={22} /> {t('layer.reviewTitle')}</h1>
      <span class="pill">{origin === 'manual' ? t('lda.origin.manual') : t('layer.origin.' + origin)}</span>
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

      {#if packSays}
        <div class="warn" role="group" aria-labelledby="rv-pack">
          <Icon name="info" size={16} />
          <div class="grow">
            <b id="rv-pack">{t('lda.packSays')}</b>
            <p class="sum" dir="auto">“{packSays}”</p>
            <p class="muted">{t('lda.packSaysNote')}</p>
          </div>
        </div>
      {/if}

      {#if stale}
        <div class="warn bad" role="alert">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b>{t('lda.staleTitle')}</b>
            <p>{t('lda.staleBody')}</p>
          </div>
        </div>
      {/if}

      {#if incomplete}
        <div class="warn bad" role="alert">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b>{t('lda.incompleteTitle')}</b>
            <p>{t('lda.incompleteBody')}</p>
            {#if !stale && !problems.length}<label class="allow"><input type="checkbox" bind:checked={keepIncomplete} /> {t('lda.incompleteAccept')}</label>{/if}
          </div>
        </div>
      {/if}

      {#if css}
        {#if css.kind === 'unreadable'}
          <div class="warn bad" role="group" aria-labelledby="rv-css">
            <Icon name="warning" size={16} />
            <div class="grow">
              <b id="rv-css">{t('lda.css.unreadableTitle')}</b>
              <p>{t('lda.css.unreadableBody')}</p>
            </div>
          </div>
        {:else if cssFlags.length}
          <div class="warn" role="group" aria-labelledby="rv-css">
            <Icon name="warning" size={16} />
            <div class="grow">
              <b id="rv-css">{t('lda.css.warnTitle')}</b>
              <p>{t('lda.css.warnBody')}</p>
              <ul>{#each cssFlags as f (f)}<li>{t('lda.css.flag.' + f)}</li>{/each}</ul>
            </div>
          </div>
        {:else if css.kind === 'colours' || css.kind === 'removes' || css.kind === 'layout'}
          <p><b>{t('lda.css.title')}</b><br /><span class="muted">{t('lda.css.' + css.kind)}</span></p>
        {/if}
      {/if}

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
              <p class="muted" role="status">{tested ? notTested || (loadErrors.length ? '' : t('layer3.checkedOk')) : t('layer3.checking')}</p>
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
      {#if outside.length || outsidePrivate}
        <div class="warn" role="note">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b>{t('layer.outsideTitle')}</b>
            <p>{t('layer.outside')}</p>
            <ul>
              {#each outside as f}<li><bdi>{f}</bdi></li>{/each}
              {#if outsidePrivate}<li>{tn('lda.outsidePrivate', outsidePrivate)}</li>{/if}
            </ul>
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
      {#if blocked}<p class="err" id="rv-blocked">{t(stale ? 'lda.staleKeep' : problems.length ? 'layer3.keepBlocked' : 'lda.incompleteKeep')}</p>{/if}
      <div class="row end">
        <button class="btn ghost" disabled={!!busy} onclick={closeReview}>{t('layer.later')}</button>
        <span class="grow"></span>
        <button class="btn danger" disabled={!!busy} onclick={discard}>{t('layer.discard')}</button>
        <button class="btn" disabled={!!busy || app.safeMode} aria-busy={busy === 'try'} onclick={tryIt}><Icon name="eye" size={15} /> {t('layer.try')}</button>
        <button class="btn primary" data-autofocus={caution ? undefined : ''} disabled={!!busy || blocked} aria-describedby={blocked ? 'rv-blocked' : undefined} aria-busy={busy === 'keep'} onclick={keep}><Icon name="check" size={15} /> {t('layer.keep')}</button>
      </div>
    {/if}
  </div>
</div>
</div>
{/if}
