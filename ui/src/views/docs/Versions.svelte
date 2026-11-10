<script lang="ts">
  // Earlier versions of one library file: list → preview → restore.
  import { onMount } from 'svelte';
  import Icon from '../../lib/Icon.svelte';
  import { api, post } from '../../lib/api';
  import { errCode, errDetail, failed } from '../../lib/errors';
  import { trapFocus } from '../../lib/focus';
  import { md } from '../../lib/md';
  import { go } from '../../lib/store.svelte';
  import { t, fmtDate } from '../../lib/i18n';

  let { path, close, restored }: { path: string; close: () => void; restored: () => void } = $props();

  type Ver = { hash: string; short?: string; date: string; subject?: string; kind?: string };
  let list = $state<Ver[] | null>(null);
  let error = $state('');
  let noHistory = $state(false);
  let sel = $state<Ver | null>(null);
  let content = $state<string | null>(null);
  let verError = $state('');
  let verLoading = $state(false);
  let restoring = $state(false);
  let seq = 0;

  const kindLabel = (k?: string) => t('docs3.kind.' + (['wrap', 'sync', 'edit', 'assistant'].includes(k ?? '') ? k : 'other'));
  const when = (d: string) => { try { return fmtDate(d, { dateStyle: 'medium', timeStyle: 'short' }); } catch { return d; } };

  async function load() {
    error = ''; noHistory = false; list = null;
    try {
      const r = await api(`/library/history?path=${encodeURIComponent(path)}&limit=50`);
      list = Array.isArray(r) ? r : [];
    } catch (e) {
      if (errCode(e) === 'NO_HISTORY') noHistory = true; else error = errDetail(e) || t('docs3.versionsFailed');
      list = [];
    }
  }
  onMount(load);

  async function show(v: Ver) {
    const mine = ++seq;
    sel = v; content = null; verError = ''; verLoading = true;
    try {
      const r = await api(`/library/version?path=${encodeURIComponent(path)}&hash=${encodeURIComponent(v.hash)}`);
      if (mine === seq) content = String(r?.content ?? '');
    } catch (e) {
      if (mine === seq) verError = errDetail(e) || t('docs3.versionFailed');
    } finally { if (mine === seq) verLoading = false; }
  }

  async function restore() {
    if (!sel || restoring) return;
    restoring = true;
    try {
      await post('/library/restore', { path, hash: sel.hash });
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
        <button class="btn primary" data-autofocus onclick={() => { close(); go({ kind: 'settings', id: 'git' }); }}><Icon name="settings" size={14} /> {t('docs3.openBackup')}</button>
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
                {#if v.subject}<span class="subj" dir="auto">{v.subject.replace(/^(edit|wrap|sync|assistant|restore|undo|module|init|reorganize):\s*/i, '')}</span>{/if}
              </button>
            </li>
          {/each}
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
            {#if /\.md$/i.test(path)}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
              <article class="md" dir="auto" onclick={(e) => { if ((e.target as HTMLElement).closest('a')) e.preventDefault(); }}>{@html md(content)}</article>
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
  .ver { width: min(980px, 94vw); display: flex; flex-direction: column; gap: 12px; overflow: hidden; }
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
  .state { display: grid; justify-items: center; text-align: center; gap: 8px; padding: 30px 10px; color: var(--muted); }
  .state h3 { margin: 0; color: var(--fg); font-size: 16px; }
  .state p { margin: 0; max-width: 460px; }
  .pad { display: grid; gap: 10px; padding: 10px 0; }
  .pad .skel { height: 13px; }
  .w70 { width: 70%; } .w50 { width: 50%; }
  .sm { font-size: var(--fs-xs); }
  @media (max-width: 760px) { .cols { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr) minmax(0, 1.4fr); } }
</style>
