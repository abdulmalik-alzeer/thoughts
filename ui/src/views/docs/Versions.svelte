<script lang="ts">
  // Earlier versions of one library file: list → preview → restore.
  import { onMount } from 'svelte';
  import Icon from '../../lib/Icon.svelte';
  import DocChanges, { type Doc } from '../../lib/DocChanges.svelte';
  import { api, post } from '../../lib/api';
  import { errCode, errDetail, failed } from '../../lib/errors';
  import { trapFocus } from '../../lib/focus';
  import { md } from '../../lib/md';
  import { zoomImages } from '../../lib/zoomImages';
  import { go } from '../../lib/store.svelte';
  import { t, fmtDate } from '../../lib/i18n';

  let { path, close, restored }: { path: string; close: () => void; restored: () => void } = $props();

  // `path`: the name the file had at that point (it may have been renamed or moved since).
  type Ver = { hash: string; short?: string; date: string; subject?: string; kind?: string; path?: string };
  let list = $state<Ver[] | null>(null);
  let error = $state('');
  let noHistory = $state(false);
  let noGit = $state(false);
  let sel = $state<Ver | null>(null);
  let content = $state<string | null>(null);
  let verError = $state('');
  let verLoading = $state(false);
  let restoring = $state(false);
  let seq = 0;
  // How the chosen version is shown: what differs from the text now, or its whole text.
  let view = $state<'diff' | 'text'>('diff');
  let diff = $state<Doc[] | null>(null);
  let diffError = $state('');
  // Versions are read a page at a time, however many there are: `more` says older ones exist.
  const PAGE = 50;
  let more = $state(false);
  let loadingMore = $state(false);

  const kindLabel = (k?: string) => (k === 'start' ? t('hista.hist.kindStart') : k === 'outside' ? t('libf-sync.hist.kind') : t('docs3.kind.' + (['wrap', 'sync', 'edit', 'assistant'].includes(k ?? '') ? k : 'other')));
  /** What a version's entry says it did. The entry a history starts with is described in the reader's language. */
  const subjectOf = (v: Ver) => (v.kind === 'start' ? t('hista.hist.startSubject') : (v.subject ?? '').replace(/^(edit|wrap|sync|assistant|restore|undo|module|init|reorganize|start):\s*/i, ''));
  const when = (d: string) => { try { return fmtDate(d, { dateStyle: 'medium', timeStyle: 'short' }); } catch { return d; } };
  /** The name a version was saved under, when that is not the file's name now. */
  const earlier = (v: Ver) => (v.path && v.path !== path ? v.path : '');
  /** Where to read a version: under the name it had then. */
  const atOf = (v: Ver) => (earlier(v) ? `&at=${encodeURIComponent(earlier(v))}` : '');

  /** `PAGE` versions from `skip` on (one more is asked for, to know whether older ones exist). */
  async function page(skip: number): Promise<Ver[]> {
    const r = await api(`/library/history?path=${encodeURIComponent(path)}&limit=${PAGE + 1}&skip=${skip}`);
    return Array.isArray(r) ? r : [];
  }

  async function load() {
    error = ''; noHistory = false; noGit = false; list = null; more = false;
    try {
      const got = await page(0);
      list = got.slice(0, PAGE); more = got.length > PAGE;
    } catch (e) {
      if (errCode(e) === 'NO_HISTORY') noHistory = true;
      else if (errCode(e) === 'git_missing') noGit = true;
      else error = errDetail(e) || t('docs3.versionsFailed');
      list = [];
    }
  }
  onMount(load);

  async function older() {
    if (!list || loadingMore) return;
    loadingMore = true;
    try {
      const got = await page(list.length);
      const seen = new Set(list.map((v) => v.hash));
      list = [...list, ...got.slice(0, PAGE).filter((v) => !seen.has(v.hash))];
      more = got.length > PAGE;
    } catch (e) { failed(t('docs3.versionsFailed'), e); }
    finally { loadingMore = false; }
  }

  async function show(v: Ver) {
    const mine = ++seq;
    sel = v; content = null; verError = ''; verLoading = true; diff = null; diffError = '';
    const q = `path=${encodeURIComponent(path)}&hash=${encodeURIComponent(v.hash)}${atOf(v)}`;
    try {
      const [r, d] = await Promise.all([
        api(`/library/version?${q}`),
        api(`/library/compare?${q}`).catch((e) => { if (mine === seq) diffError = errDetail(e) || t('histc.ver.diffFailed'); return null; }),
      ]);
      if (mine === seq) { content = String(r?.content ?? ''); diff = Array.isArray(d?.files) ? d.files : null; }
    } catch (e) {
      if (mine === seq) verError = errDetail(e) || t('docs3.versionFailed');
    } finally { if (mine === seq) verLoading = false; }
  }
  const same = $derived(!!diff && diff.every((f) => !f.hunks?.length && f.detail !== false && !f.binary));

  async function restore() {
    if (!sel || restoring) return;
    restoring = true;
    try {
      // An earlier version of a file that was renamed since goes back where the file is now.
      await post('/library/restore', { path, hash: sel.hash, ...(earlier(sel) ? { at: earlier(sel) } : {}) });
      restored();
      close();
    } catch (e) { failed(t('docs3.restoreFailed'), e); }
    finally { restoring = false; }
  }
</script>

<div class="modal-bg no-print" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
  <div class="modal ver" role="dialog" aria-modal="true" aria-labelledby="ver-title" tabindex="-1" use:trapFocus={{ onEscape: close }}>
    <div class="row head">
      <h2 id="ver-title" class="grow">{t('docs3.versionsOf')} <bdi>{path}</bdi></h2>
      <button class="btn ghost x" onclick={close} aria-label={t('close')} title={t('close')}><Icon name="close" size={16} /></button>
    </div>
    {#if list === null}
      <div class="pad" role="status" aria-busy="true"><div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div></div>
    {:else if noHistory}
      <div class="state" role="status">
        <Icon name="rollback" size={26} />
        <h3>{t('docs3.noHistoryTitle')}</h3>
        <p class="muted">{t('docs3.noHistory')}</p>
        <button class="btn primary" data-autofocus onclick={() => { close(); go({ kind: 'settings', id: 'git' }); }}><Icon name="settings" size={14} /> {t('hista.openSettings')}</button>
      </div>
    {:else if noGit}
      <div class="state" role="status">
        <Icon name="rollback" size={26} />
        <h3>{t('hista.hist.noGitTitle')}</h3>
        <p class="muted">{t('hista.hist.noGitBody')}</p>
        <button class="btn primary" data-autofocus onclick={() => { close(); go({ kind: 'settings', id: 'git' }); }}><Icon name="settings" size={14} /> {t('hista.openSettings')}</button>
      </div>
    {:else if error}
      <div class="state" role="alert">
        <Icon name="warning" size={26} />
        <h3>{t('docs3.versionsFailed')}</h3>
        <p class="muted" dir="auto">{error}</p>
        <button class="btn primary" onclick={load}><Icon name="retry" size={14} /> {t('docs3.retry')}</button>
      </div>
    {:else if !list.length}
      <div class="state"><Icon name="rollback" size={26} /><p class="muted">{t('docs3.versionsEmpty')}</p></div>
    {:else}
      <div class="cols">
        <ul class="vlist scroll" aria-label={t('docs3.versionList')}>
          {#each list as v (v.hash)}
            <li>
              <button class="v" class:active={sel?.hash === v.hash} aria-current={sel?.hash === v.hash ? 'true' : undefined} onclick={() => show(v)}>
                <span class="row"><span class="grow when">{when(v.date)}</span><span class="pill">{kindLabel(v.kind)}</span></span>
                {#if v.subject}<span class="subj" dir="auto">{subjectOf(v)}</span>{/if}
                {#if earlier(v)}<span class="subj was" title={earlier(v)}>{t('hista.ver.was')} <bdi>{earlier(v)}</bdi></span>{/if}
              </button>
            </li>
          {/each}
          {#if more}
            <li class="morerow"><button class="btn sm" disabled={loadingMore} aria-busy={loadingMore} onclick={older}>{t(loadingMore ? 'shell.loading' : 'hista.ver.older')}</button></li>
          {/if}
        </ul>
        <div class="pv scroll" aria-live="polite">
          {#if !sel}
            <p class="muted">{t('docs3.pickVersion')}</p>
          {:else if verLoading}
            <div class="pad" aria-busy="true"><div class="skel"></div><div class="skel w70"></div></div>
          {:else if verError}
            <div role="alert"><p><Icon name="warning" size={14} /> {t('docs3.versionFailed')}</p><p class="muted sm" dir="auto">{verError}</p>
              <button class="btn" onclick={() => sel && show(sel)}><Icon name="retry" size={14} /> {t('docs3.retry')}</button></div>
          {:else if content !== null}
            <div class="seg vtabs" role="tablist" aria-label={t('histc.ver.showAs')}>
              <button class="segb" role="tab" aria-selected={view === 'diff'} onclick={() => (view = 'diff')}>{t('histc.ver.diff')}</button>
              <button class="segb" role="tab" aria-selected={view === 'text'} onclick={() => (view = 'text')}>{t('histc.ver.text')}</button>
            </div>
            {#if view === 'diff'}
              {#if diffError}
                <p class="muted sm" role="alert" dir="auto"><Icon name="warning" size={14} /> {diffError}</p>
              {:else if diff && same}
                <p class="muted"><Icon name="check" size={14} /> {t('histc.ver.same')}</p>
              {:else if diff}
                <p class="muted sm">{t('histc.ver.diffIntro')}</p>
                <DocChanges files={diff} single labels={{ added: t('histc.ver.onlyNow'), removed: t('histc.ver.onlyThen') }} />
              {/if}
            {:else if /\.md$/i.test(path)}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
              <article class="md" dir="auto" onclick={(e) => { if ((e.target as HTMLElement).closest('a')) e.preventDefault(); }} use:zoomImages>{@html md(content)}</article>
            {:else}
              <pre class="plain" dir="auto">{content}</pre>
            {/if}
          {/if}
        </div>
      </div>
      <div class="row foot">
        <span class="grow muted sm">{#if sel}{when(sel.date)} · {kindLabel(sel.kind)}{/if}</span>
        <button class="btn" onclick={close}>{t('close')}</button>
        <button class="btn primary" disabled={!sel || content === null || restoring} onclick={restore}><Icon name="rollback" size={14} /> {t(restoring ? 'docs3.restoring' : 'docs3.restore')}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  /* The dialog sits inside the content column, not the window: sized by it, never by the window's width. */
  .ver { width: min(980px, calc(100% - 32px)); box-sizing: border-box; display: flex; flex-direction: column; gap: 12px; overflow: hidden; container-type: inline-size; }
  .head h2 { margin: 0; font-size: 17px; overflow-wrap: anywhere; }
  .x { padding: 6px 8px; }
  .cols { display: grid; grid-template-columns: minmax(200px, 280px) minmax(0, 1fr); gap: 14px; min-height: 0; flex: 1; height: 60vh; }
  .vlist { list-style: none; margin: 0; padding: 2px; display: flex; flex-direction: column; gap: 2px; }
  .v { font: inherit; color: inherit; background: none; border: 0; width: 100%; text-align: start; cursor: pointer; padding: 7px 8px; border-radius: 6px; display: grid; gap: 2px; font-size: 13px; }
  .v:hover { background: var(--fill); }
  .v.active { background: var(--tint); color: var(--on-tint); font-weight: 600; }
  .v.active .pill { color: inherit; background: transparent; outline: 1px solid currentColor; }
  .v.active .subj { color: inherit; }
  .when { white-space: nowrap; }
  .subj { font-size: var(--fs-xs); color: var(--muted); font-weight: 400; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pv { border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 10px 14px; overflow-wrap: anywhere; background: var(--bg); }
  .plain { white-space: pre-wrap; font-size: var(--fs-sm); margin: 0; }
  .vtabs { margin-bottom: 10px; }
  .vtabs .segb[aria-selected='true'] { background: var(--accent); color: var(--accent-fg); }
  .state { display: grid; justify-items: center; text-align: center; gap: 8px; padding: 30px 10px; color: var(--muted); }
  .state h3 { margin: 0; color: var(--fg); font-size: 16px; }
  .state p { margin: 0; max-width: 460px; }
  .pad { display: grid; gap: 10px; padding: 10px 0; }
  .pad .skel { height: 13px; }
  .w70 { width: 70%; } .w50 { width: 50%; }
  .sm { font-size: var(--fs-xs); }
  .morerow { display: flex; justify-content: center; padding: 6px 0 2px; }
  @container (max-width: 640px) { .cols { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr) minmax(0, 1.4fr); } }
</style>
