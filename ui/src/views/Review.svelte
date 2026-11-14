<script lang="ts">
  // Review panel for a staged change to the customisation layer: nothing here is loaded into the app
  // until the user chooses Try it or Keep.
  //
  // It leads with what the change does, item by item, as the service read it from the files
  // (lib/effects.ts words it): new and changed add-ons and pages with the permissions they gain or
  // lose, Home shortcuts with their prompts, the sidebar, todo settings, themes, the look, automatic
  // tasks. The assistant's (or the pack's) own description comes after, labelled as theirs, and the
  // file names last, with the exact differences on request.
  //
  // The whole panel is drawn inside a shadow root (lib/layer/shield.ts) with its own fixed styles, so
  // the staged CSS under review can never restyle, hide or relabel it. Classes used here are defined
  // in shield.ts, or in the style element below that is moved into the shadow root with the panel
  // (page styles never reach it).
  import Icon from '../lib/Icon.svelte';
  import { app, go, toast } from '../lib/store.svelte';
  import { t, tn } from '../lib/i18n';
  import { errDetail, failed } from '../lib/errors';
  import { aboveShield, shield } from '../lib/layer/shield';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { api, post } from '../lib/api';
  import { checkLayerUpdates } from '../lib/updates';
  import { fxGroups, mayDo, type FxLine } from '../lib/effects';
  import { drafts, draftTitle, openDraft, refreshDrafts } from '../lib/drafts.svelte';
  import { closeReview, continueChatting, discardPending, dryLoadStaged, keepPending, stagedItems, tryPending, whenText, type LayerItem } from '../lib/layer';

  const p = $derived(app.pending);
  const files = $derived(p?.files ?? []);
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
  // A new or changed part that did not start when tested: Keep needs an explicit yes too.
  let keepBroken = $state(false);
  // What the staged style sheet adds, as the service read it: { kind, flags }.
  const css = $derived(p?.css && typeof p.css === 'object' ? (p.css as { kind?: string; flags?: unknown }) : null);
  const cssFlags = $derived((Array.isArray(css?.flags) ? css!.flags : []).filter((f): f is string => typeof f === 'string' && ['text', 'overlay', 'dialogs', 'hides', 'remote'].includes(f)));
  // What an imported pack says about itself: the pack's words, shown as a quotation.
  const packSays = $derived(origin === 'import' && typeof p?.pack?.description === 'string' ? p.pack.description.trim() : '');
  $effect(() => { void p?.summary; void p?.files; void p?.signature; allowWorkflows = false; keepIncomplete = false; keepBroken = false; showDiff = false; diff = null; });

  // What it does, item by item. The service's list (`effects`) is the truth; groups are for reading.
  const groups = $derived(fxGroups(p?.effects));
  const changedItems = $derived(new Set(groups.flatMap((g) => g.lines).filter((l) => l.tag === 'added' || l.tag === 'changed').map((l) => l.key)));
  const addsCode = $derived(groups.some((g) => (g.area === 'addon' || g.area === 'page') && g.lines.some((l) => l.tag !== 'removed')));
  const installed = $derived((Array.isArray(p?.installed) ? p!.installed : []).filter((x: any) => x && typeof x.name === 'string') as { area: string; name: string; description?: string; permissions?: string[]; write?: string[]; read?: string[] }[]);
  const strs = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);

  // Files changed outside the staging copy while it was prepared: those the run's own steps name,
  // and the others (most likely the user or another chat).
  const outside = $derived(strs(p?.outsideChanges));
  const byRun = $derived(strs(p?.outsideByRun).filter((f) => outside.includes(f)));
  const notByRun = $derived(outside.filter((f) => !byRun.includes(f)));
  const outsidePrivate = $derived(Number(p?.outsidePrivate) > 0 ? Number(p!.outsidePrivate) : 0);
  const privateByRun = $derived(Math.min(outsidePrivate, Number(p?.outsidePrivateByRun) > 0 ? Number(p!.outsidePrivateByRun) : 0));

  // A test load of each staged add-on and page in a sealed frame. Only the new and changed ones can
  // stop this change from being kept; the others were already in use.
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
  const itemKey = (x: LayerItem) => `${x.kind === 'view' ? 'page' : 'addon'}:${x.name}`;
  const mine = $derived((items ?? []).filter((x) => changedItems.has(itemKey(x))));
  const unsupported = $derived(mine.filter((x) => x.unsupported));
  const loadErrors = $derived(mine.filter((x) => !x.unsupported && x.error));
  const testing = $derived(addsCode && !tested);
  // Why the new parts were not test-run at all (customisations are off, or this change froze the app before).
  const notTested = $derived(mine.filter((x) => !x.unsupported).find((x) => x.skipped)?.skipped ?? '');
  const errorOf = (l: FxLine) => loadErrors.find((x) => itemKey(x) === l.key)?.error ?? '';

  // Anything the user should read before deciding: then Keep is not the pre-focused button.
  const caution = $derived(!!(p?.pluginChanged || p?.workflowChanged || outside.length || outsidePrivate || problems.length || stale || incomplete || packSays || cssFlags.length || css?.kind === 'unreadable' || loadErrors.length));
  const blocked = $derived(problems.length > 0 || stale || (incomplete && !keepIncomplete) || testing || (loadErrors.length > 0 && !keepBroken));
  const blockedText = $derived(stale ? t('lda.staleKeep') : problems.length ? t('layer3.keepBlocked') : incomplete && !keepIncomplete ? t('lda.incompleteKeep') : testing ? t('ldb.testingKeep') : t('ldb.loadErrKeep'));

  // The other changes in progress (each reviewed and kept on its own).
  const others = $derived(drafts.list);
  $effect(() => { void p?.signature; refreshDrafts(); });

  let error = $state('');
  async function act(key: string, fn: () => Promise<unknown>, failKey: string) {
    if (busy) return;
    busy = key;
    // Also said inside the panel: a toast would sit underneath it.
    error = '';
    try { await fn(); } catch (e) { failed(t(failKey), e); const d = errDetail(e); error = d ? `${t(failKey)} ${d}` : t(failKey); }
    finally { busy = ''; }
  }
  const tryIt = () => act('try', async () => { if (await tryPending()) toast(t('lua.previewOn'), { key: 'layer' }); }, 'layer.tryFailed');
  const keep = () => {
    if (blocked) return;
    act('keep', async () => {
      // The yes to an unfinished change is recorded for exactly what was reviewed, then it is kept.
      if (incomplete) await post('/layer/pending/accept', { signature: p?.signature ?? '' });
      await keepPending({ approveWorkflows: canAllow && allowWorkflows && !!p?.workflowChanged });
    }, 'layer3.keepFailed');
  };
  async function discard() {
    if (!(await aboveShield(() => confirmDialog({ title: t('layer.discardTitle'), body: t('layer.discardBody'), confirmLabel: t('layer.discard'), danger: true })))) { app.showReview = true; return; }
    const wasUpdate = origin === 'update';
    await act('discard', async () => { await discardPending(); toast(t('layer.discarded'), { key: 'layer' }); }, 'layer.discardFailed');
    // A discarded merge of an app update leaves its question open: ask it again now.
    if (wasUpdate && !app.pending) checkLayerUpdates();
  }
  function watch() { const id = p?.chatId; closeReview(); if (id) go({ kind: 'chat', id }); }
  const openOther = (id: string) => act('open', () => openDraft(id), 'ldb.openFailed');

  // "Show the exact changes": fetched when asked for, and only shown for the change under review.
  let showDiff = $state(false);
  let diff = $state<{ path: string; status: string; diff: string; cut: boolean }[] | null>(null);
  let diffError = $state('');
  async function toggleDiff() {
    showDiff = !showDiff;
    if (!showDiff || diff) return;
    diffError = '';
    const sig = p?.signature;
    try {
      const r = await api('/layer/pending/details');
      if (r?.signature !== sig) { diffError = t('ldb.diffStale'); return; }
      diff = (Array.isArray(r?.files) ? r.files : []).filter((f: any) => f && typeof f.path === 'string').map((f: any) => ({ path: f.path, status: String(f.status ?? ''), diff: String(f.diff ?? ''), cut: f.cut === true }));
    } catch { diffError = t('ldb.diffFailed'); }
  }
  const lineKind = (l: string) => (l.startsWith('+') ? 'add' : l.startsWith('-') ? 'del' : l.startsWith('@@') ? 'hunk' : '');
  const tagText = (tag: string) => t(tag === 'added' ? 'layer.status.added' : tag === 'removed' ? 'layer.status.deleted' : 'layer.status.modified');
</script>

{#if p}
<div class="no-print" use:shield={{ box: 'position:fixed;inset:0;z-index:60', trap: true, onEscape: closeReview, onBackdrop: closeReview }}>
<style>
  .fx { list-style: none; padding: 0; margin: 0; display: grid; gap: 8px; }
  .fx > li { display: grid; gap: 2px; }
  .fx .head { display: flex; gap: 8px; align-items: baseline; }
  .fx .head .pill { flex: none; }
  .fx .more { padding-inline-start: 18px; margin: 0; font-size: 13px; }
  .fx .more li { margin-top: 1px; }
  .fx .note { font-size: 13px; color: var(--muted); }
  .area { font-size: 12px; font-weight: 600; color: var(--muted); text-transform: none; margin: 4px 0 0; }
  .pill.added { color: var(--ok); }
  .pill.removed { color: var(--danger); }
  .quote { white-space: pre-wrap; overflow-wrap: anywhere; border-inline-start: 3px solid var(--line); padding: 2px 10px; margin: 2px 0 0; font-size: 13px; max-height: 140px; overflow: auto; }
  .bad-line { color: var(--danger); font-size: 13px; }
  details { border: 1px solid var(--line); border-radius: 8px; padding: 6px 10px; }
  summary { cursor: pointer; font-weight: 600; font-size: 13px; }
  details[open] > summary { margin-bottom: 6px; }
  .diff { max-height: 260px; padding: 0; }
  .diff span { display: block; padding: 0 8px; white-space: pre-wrap; }
  .diff .add { background: rgba(27, 110, 60, .14); }
  .diff .del { background: rgba(179, 38, 30, .12); }
  .diff .hunk { color: var(--muted); }
  .others { list-style: none; padding: 0; margin: 0; display: grid; gap: 6px; font-size: 13px; }
  .others li { display: grid; gap: 4px; }
</style>
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

      <h2 id="rv-what">{t('ldb.whatTitle')}</h2>
      <p class="muted" id="rv-what-note" tabindex="-1" data-autofocus={caution ? '' : undefined}>{t('ldb.whatNote')}</p>
      {#if addsCode}<p class="muted">{t('lua.pluginWarn')}</p>{/if}
      {#if groups.length}
        {#each groups as g (g.area)}
          <h3 class="area">{t('ldb.area.' + g.area)}</h3>
          <ul class="fx" aria-label={t('ldb.area.' + g.area)}>
            {#each g.lines as l (l.key)}
              <li>
                <div class="head"><span class="pill {l.tag}">{tagText(l.tag)}</span> <span dir="auto">{l.text}</span></div>
                {#each l.detail as d}<div class="note" dir="auto">{d}</div>{/each}
                {#if l.quote !== undefined}<div class="quote" dir="auto">{l.quote}</div>{/if}
                {#if l.may?.length}
                  <div class="note">{l.mayTitle}</div>
                  <ul class="more">{#each l.may as m}<li dir="auto">{m}</li>{/each}</ul>
                {/if}
                {#if errorOf(l)}<div class="bad-line" role="status">{t('layer3.loadErrTitle')}: <bdi>{errorOf(l)}</bdi></div>{/if}
              </li>
            {/each}
          </ul>
        {/each}
        {#if addsCode}<p class="muted" role="status">{tested ? notTested || (loadErrors.length ? '' : t('layer3.checkedOk')) : t('layer3.checking')}</p>{/if}
      {:else}
        <p class="muted">{t('ldb.whatNothing')}</p>
      {/if}

      {#if loadErrors.length}
        <div class="warn bad" role="alert">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b>{t('layer3.loadErrTitle')}</b>
            <p>{t('layer3.loadErrBody')}</p>
            <ul>{#each loadErrors as x (x.kind + ':' + x.name)}<li><bdi class="mono">{x.name}</bdi> — <bdi>{x.error}</bdi></li>{/each}</ul>
            {#if !stale && !problems.length}<label class="allow"><input type="checkbox" bind:checked={keepBroken} /> {t('ldb.loadErrAccept')}</label>{/if}
          </div>
        </div>
      {/if}

      {#if css && (css.kind === 'unreadable' || cssFlags.length)}
        <div class="warn {css.kind === 'unreadable' ? 'bad' : ''}" role="group" aria-labelledby="rv-css">
          <Icon name="warning" size={16} />
          <div class="grow">
            {#if css.kind === 'unreadable'}
              <b id="rv-css">{t('lda.css.unreadableTitle')}</b>
              <p>{t('lda.css.unreadableBody')}</p>
            {:else}
              <b id="rv-css">{t('lda.css.warnTitle')}</b>
              <p>{t('lda.css.warnBody')}</p>
              <ul>{#each cssFlags as f (f)}<li>{t('lda.css.flag.' + f)}</li>{/each}</ul>
            {/if}
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

      {#if installed.length}
        <details>
          <summary>{t('ldb.installedTitle', { n: installed.length })}</summary>
          <p class="muted">{t('ldb.installedNote')}</p>
          <ul class="fx">
            {#each installed as x (x.area + ':' + x.name)}
              <li>
                <div class="head"><span class="pill">{t(x.area === 'page' ? 'layer3.kind.view' : 'layer3.kind.plugin')}</span> <bdi>{x.name}</bdi></div>
                <ul class="more">{#each mayDo(x.area === 'page' ? 'page' : 'addon', strs(x.permissions), strs(x.write), strs(x.read)) as m}<li dir="auto">{m}</li>{/each}</ul>
              </li>
            {/each}
          </ul>
        </details>
      {/if}

      <h2 id="rv-sum-title">{t('ldb.summary.' + (['import', 'manual', 'update'].includes(origin) ? origin : 'customize'))}</h2>
      <p class="sum" dir="auto" id="rv-sum" aria-labelledby="rv-sum-title rv-sum">{p.summary?.trim() || t('layer.noSummary')}</p>

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

      {#if byRun.length || privateByRun}
        <div class="warn bad" role="note">
          <Icon name="warning" size={16} />
          <div class="grow">
            <b>{t('ldb.outsideRunTitle')}</b>
            <p>{t('ldb.outsideRunBody')}</p>
            <ul>
              {#each byRun as f}<li><bdi>{f}</bdi></li>{/each}
              {#if privateByRun}<li>{tn('lda.outsidePrivate', privateByRun)}</li>{/if}
            </ul>
          </div>
        </div>
      {/if}
      {#if notByRun.length || outsidePrivate > privateByRun}
        <div class="warn" role="note">
          <Icon name="info" size={16} />
          <div class="grow">
            <b>{t('ldb.outsideOtherTitle')}</b>
            <p>{t('ldb.outsideOtherBody')}</p>
            <ul>
              {#each notByRun as f}<li><bdi>{f}</bdi></li>{/each}
              {#if outsidePrivate > privateByRun}<li>{tn('lda.outsidePrivate', outsidePrivate - privateByRun)}</li>{/if}
            </ul>
          </div>
        </div>
      {/if}

      <details>
        <summary>{t('ldb.filesTitle', { n: files.length })}</summary>
        <p class="muted">{t('ldb.filesNote')}</p>
        <ul class="files">
          {#each files as f (f.path)}
            <li><span class="pill st {f.status}">{t('layer.status.' + f.status)}</span> <bdi>{f.path}</bdi></li>
          {/each}
        </ul>
        <div class="row end"><button class="btn" aria-expanded={showDiff} onclick={toggleDiff}>{showDiff ? t('ldb.hideDiff') : t('ldb.showDiff')}</button></div>
        {#if showDiff}
          {#if diffError}<p class="err" role="status">{diffError}</p>
          {:else if !diff}<p class="muted" role="status">{t('shell.loading')}</p>
          {:else}
            {#each diff as f (f.path)}
              <p class="pl"><span class="pill st {f.status}">{t('layer.status.' + f.status)}</span> <bdi class="mono">{f.path}</bdi></p>
              {#if f.diff.trim()}
                <pre class="diff" dir="ltr">{#each f.diff.split('\n') as l, i (i)}<span class={lineKind(l)}>{l || ' '}</span>{/each}</pre>
              {:else}<p class="muted">{t('ldb.diffEmpty')}</p>{/if}
              {#if f.cut}<p class="muted">{t('ldb.diffCut')}</p>{/if}
            {/each}
          {/if}
        {/if}
      </details>

      {#if others.length}
        <div class="warn" role="group" aria-labelledby="rv-others">
          <Icon name="inbox" size={16} />
          <div class="grow">
            <b id="rv-others">{t('ldb.waitingTitle')}</b>
            <p class="muted">{t('ldb.waitingHint')}</p>
            <ul class="others">
              {#each others as o (o.id)}
                <li><span dir="auto">{draftTitle(o)}</span>
                  <span class="row"><span class="pill">{o.origin === 'manual' ? t('lda.origin.manual') : t('layer.origin.' + o.origin)}</span><span class="grow"></span>
                  <button class="btn" disabled={!!busy} onclick={() => openOther(o.id)}>{t('ldb.open')}</button></span></li>
              {/each}
            </ul>
          </div>
        </div>
      {/if}

      {#if app.safeMode}<p class="muted">{t('layer.safeNoTry')}</p>{/if}
      {#if error}<p class="err" role="alert" dir="auto">{error}</p>{/if}
      {#if blocked}<p class="err" id="rv-blocked">{blockedText}</p>{/if}
      <div class="row end">
        <button class="btn ghost" disabled={!!busy} onclick={closeReview}>{t('layer.later')}</button>
        <span class="grow"></span>
        <button class="btn danger" disabled={!!busy} onclick={discard}>{t('layer.discard')}</button>
        {#if p.canContinue}<button class="btn" disabled={!!busy} aria-busy={busy === 'continue'} title={t('r10-cust-continue.hint')} onclick={() => act('continue', continueChatting, 'r10-cust-continue.failed')}><Icon name="chat" size={15} /> {t('r10-cust-continue.button')}</button>{/if}
        <button class="btn" disabled={!!busy || app.safeMode} aria-busy={busy === 'try'} onclick={tryIt}><Icon name="eye" size={15} /> {t('layer.try')}</button>
        <button class="btn primary" data-autofocus={caution ? undefined : ''} disabled={!!busy || blocked} aria-describedby={blocked ? 'rv-blocked' : undefined} aria-busy={busy === 'keep'} onclick={keep}><Icon name="check" size={15} /> {t('layer.keep')}</button>
      </div>
    {/if}
  </div>
</div>
</div>
{/if}
