<script lang="ts">
  // History of the user's NOTES (the library's saved changes), plus the app-change history as a second tab.
  import { onMount } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import Rollback from './Rollback.svelte';
  import { api, on, post } from '../lib/api';
  import { go, toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { errCode, errDetail, failed } from '../lib/errors';
  import { t, tn, fmtDate } from '../lib/i18n';

  type Entry = { hash: string; short?: string; date: string; subject: string; kind: string; files: string[]; first?: boolean };

  let tab = $state<'notes' | 'app'>('notes');
  let entries = $state<Entry[]>([]);
  let loading = $state(true);
  let loadError = $state('');
  let noHistory = $state(false);
  let noGit = $state(false);
  let limit = $state(50);
  let busy = $state('');
  let open = $state<Record<string, boolean>>({});

  async function load() {
    loadError = '';
    try {
      const r = await api(`/library/history?limit=${limit}`);
      entries = Array.isArray(r) ? r : [];
      noHistory = false; noGit = false;
    } catch (e) {
      if (errCode(e) === 'NO_HISTORY') { noHistory = true; noGit = false; entries = []; }
      // History is on, but the tool that records it is not installed on this computer yet.
      else if (errCode(e) === 'git_missing') { noGit = true; noHistory = false; entries = []; }
      else loadError = errDetail(e) || t('ui.hist.loadFailed');
    } finally { loading = false; }
  }
  onMount(() => { load(); return on((e) => { if (e.type === 'library.changed' || e.type === 'resync') load(); }); });

  const KINDS = ['wrap', 'sync', 'edit', 'assistant'];
  const kindLabel = (k: string) => (k === 'start' ? t('hista.hist.kindStart') : t('ui.hist.kind.' + (KINDS.includes(k) ? k : 'other')));
  const kindIcon = (k: string) => ({ wrap: 'wrap', sync: 'sync', edit: 'edit', assistant: 'ai' } as Record<string, string>)[k] ?? 'clock';
  /** What an entry says it did. The entry a history starts with is described in the reader's language. */
  const subjectOf = (e: Entry) => (e.kind === 'start' ? t('hista.hist.startSubject') : e.subject.replace(/^(edit|wrap|sync|assistant|restore|undo|module|init|reorganize|start):\s*/i, ''));

  // Grouped by calendar day, newest first (the list already arrives newest first).
  const days = $derived.by(() => {
    const out: { day: string; items: Entry[] }[] = [];
    for (const e of entries) {
      const d = new Date(e.date);
      const day = isNaN(d.getTime()) ? '' : `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
      const last = out[out.length - 1];
      if (last && last.day === day) last.items.push(e); else out.push({ day, items: [e] });
    }
    return out;
  });
  const dayTitle = (e: Entry) => fmtDate(e.date, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
  const time = (e: Entry) => fmtDate(e.date, { timeStyle: 'short' });

  async function undo(e: Entry) {
    const ok = await confirmDialog({
      title: t('ui.hist.undoTitle'), body: t('ui.hist.undoBody'), list: e.files.slice(0, 12),
      note: t('ui.hist.undoNote'), confirmLabel: t('ui.hist.undo'),
    });
    if (!ok) return;
    busy = e.hash;
    try {
      await post('/library/undo', { hash: e.hash });
      toast(t('ui.hist.undone'), { kind: 'success' });
      await load();
    } catch (err) {
      if (errCode(err) === 'CONFLICT') {
        toast(t('ui.hist.undoConflict'), { kind: 'error', detail: errDetail(err).split(';').map((f) => f.trim()).filter(Boolean).join('\n') || undefined, duration: 0 });
      } else failed(t('ui.hist.undoFailed'), err);
    } finally { busy = ''; }
  }
</script>

<div class="page hist">
  <h1>{t('ui.hist.title')}</h1>
  <div class="seg" role="tablist" aria-label={t('ui.hist.title')}>
    <button class="segb" role="tab" id="hist-tab-notes" aria-selected={tab === 'notes'} aria-controls="hist-panel" onclick={() => (tab = 'notes')}><Icon name="docs" size={14} /> {t('ui.hist.tabNotes')}</button>
    <button class="segb" role="tab" id="hist-tab-app" aria-selected={tab === 'app'} aria-controls="hist-panel" onclick={() => (tab = 'app')}><Icon name="palette" size={14} /> {t('ui.hist.tabApp')}</button>
  </div>

  <div id="hist-panel" role="tabpanel" aria-labelledby={'hist-tab-' + tab}>
  {#if tab === 'app'}
    <p class="muted intro">{t('ui.hist.appIntro')}</p>
    <div class="card"><Rollback embedded /></div>
  {:else}
    <p class="muted intro">{t('ui.hist.intro')}</p>
    {#if loading}
      <div class="card" aria-busy="true"><span class="sr-only">{t('shell.loading')}</span><div class="skel"></div><div class="skel w70"></div><div class="skel w50"></div></div>
    {:else if noHistory}
      <div class="state">
        <Icon name="rollback" size={28} />
        <h2>{t('ui.hist.offTitle')}</h2>
        <p class="muted">{t('err.noHistory')}</p>
        <button class="btn primary" onclick={() => go({ kind: 'settings', id: 'git' })}><Icon name="settings" size={14} /> {t('hista.openSettings')}</button>
      </div>
    {:else if noGit}
      <div class="state">
        <Icon name="rollback" size={28} />
        <h2>{t('hista.hist.noGitTitle')}</h2>
        <p class="muted">{t('hista.hist.noGitBody')}</p>
        <button class="btn primary" onclick={() => go({ kind: 'settings', id: 'git' })}><Icon name="settings" size={14} /> {t('hista.openSettings')}</button>
      </div>
    {:else if loadError}
      <div class="state" role="alert">
        <Icon name="warning" size={24} /><p>{t('ui.hist.loadFailed')}</p><p class="muted sm" dir="auto">{loadError}</p>
        <button class="btn primary" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('ui.retry')}</button>
      </div>
    {:else if !entries.length}
      <div class="state"><Icon name="rollback" size={28} /><h2>{t('ui.hist.emptyTitle')}</h2><p class="muted">{t('ui.hist.emptyBody')}</p></div>
    {:else}
      {#each days as d (d.day + d.items[0].hash)}
        <section aria-label={dayTitle(d.items[0])}>
          <h2 class="day">{dayTitle(d.items[0])}</h2>
          <div class="card list">
            {#each d.items as e (e.hash)}
              <div class="entry">
                <span class="ki" aria-hidden="true"><Icon name={kindIcon(e.kind)} size={15} /></span>
                <div class="grow body">
                  <div class="row top">
                    <span class="pill">{kindLabel(e.kind)}</span>
                    <span class="muted sm">{time(e)}</span>
                  </div>
                  <div class="subj" dir="auto">{subjectOf(e)}</div>
                  {#if e.files?.length}
                    <div class="files">
                      {#each (open[e.hash] ? e.files : e.files.slice(0, 4)) as f (f)}
                        <button class="file" title={t('ui.hist.openFile')} onclick={() => go({ kind: 'docs', path: f })}><bdi>{f}</bdi></button>
                      {/each}
                      {#if e.files.length > 4 && !open[e.hash]}
                        <button class="file more" aria-expanded="false" onclick={() => (open[e.hash] = true)}>{tn('ui.hist.moreFiles', e.files.length - 4)}</button>
                      {/if}
                    </div>
                  {/if}
                </div>
                {#if e.first}
                  <!-- Nothing exists before the first entry, so there is nothing to go back to. -->
                  <span class="muted sm firstmark">{t('hista.hist.first')}</span>
                {:else}
                  <button class="btn" disabled={!!busy} aria-busy={busy === e.hash} onclick={() => undo(e)}><Icon name="undo" size={14} /> {t('ui.hist.undo')}</button>
                {/if}
              </div>
            {/each}
          </div>
        </section>
      {/each}
      {#if entries.length >= limit && limit < 500}
        <div class="row morerow"><button class="btn" onclick={() => { limit = Math.min(500, limit + 100); load(); }}>{t('ui.hist.older')}</button></div>
      {/if}
      <p class="muted sm tip">{t('ui.hist.versionsTip')}</p>
    {/if}
  {/if}
  </div>
</div>

<style>
  .hist { max-width: 860px; }
  .intro { margin: 12px 0 14px; max-width: 640px; }
  .day { font-size: 14px; font-weight: 600; margin: 20px 0 8px; }
  .list { padding: 4px 14px; }
  .entry { display: flex; gap: 12px; align-items: flex-start; padding: 12px 0; border-bottom: 1px solid var(--hairline); }
  .entry:last-child { border-bottom: 0; }
  .ki { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); flex: none; }
  .top { gap: 8px; }
  .subj { margin: 3px 0 4px; overflow-wrap: anywhere; }
  .files { display: flex; flex-wrap: wrap; gap: 4px 6px; }
  .file { font: inherit; font-size: var(--fs-xs); color: var(--muted); background: var(--fill); border: 0; border-radius: 6px; padding: 3px 8px; min-height: 24px; cursor: pointer; max-width: 100%; overflow-wrap: anywhere; text-align: start; }
  .file:hover { color: var(--fg); background: var(--fill-2); }
  .file.more { color: var(--on-tint); background: none; text-decoration: underline; text-underline-offset: 3px; }
  .entry > .btn, .firstmark { flex: none; }
  .firstmark { align-self: center; }
  .morerow { justify-content: center; margin-top: 14px; }
  .tip { margin-top: 18px; }
  .state { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; padding: 40px 0; color: var(--muted); }
  .state h2 { margin: 4px 0 0; color: var(--fg); }
  .state p { margin: 0; max-width: 460px; }
  .skel { height: 14px; margin: 10px 0; }
  .w70 { width: 70%; } .w50 { width: 50%; }
  .sm { font-size: var(--fs-xs); }
  @container main (max-width: 620px) { .entry { flex-wrap: wrap; } .entry > .btn { margin-inline-start: 40px; } }
</style>
