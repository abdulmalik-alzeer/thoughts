<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import ItemActions from '../lib/ItemActions.svelte';
  import { onMount } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { go, refreshChats } from '../lib/store.svelte';
  import { t, tn, fmtDate, fmtNum } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { term } from '../lib/terms';
  import { fold } from '../lib/CommandPalette.svelte';

  let ideas = $state<any[]>([]);
  let loading = $state(true);
  let loadError = $state('');
  let status = $state('');
  let q = $state('');
  let newIdea = $state('');
  let busy = $state(false);

  async function load() {
    try { ideas = await api('/library/ideas'); loadError = ''; }
    catch (e: any) { loadError = e?.message ?? String(e); }
    finally { loading = false; }
  }
  onMount(() => { load(); return on((e) => { if (e.type === 'library.changed' || e.type === 'resync') load(); }); });

  const ORDER = ['active', 'seed', 'parked', 'done', 'dropped'];
  const statuses = $derived([...new Set(ideas.map((i) => i.status).filter(Boolean))].sort((a, b) => (ORDER.indexOf(a) + 99) % 99 - (ORDER.indexOf(b) + 99) % 99));
  const shown = $derived(ideas.filter((i) => (!status || i.status === status) &&
    (!q.trim() || fold(`${i.title} ${i.summary} ${(i.tags || []).join(' ')}`).includes(fold(q.trim())))));
  const areas = $derived([...new Set(shown.map((i) => i.area))].sort());

  /** Known statuses are translated; anything custom is shown as written. */
  const statusLabel = (s: string) => { const k = `idea.status.${s}`; const x = t(k); return x === k ? s : x; };
  const when = (d: any) => (/^\d{4}-\d{2}-\d{2}/.test(String(d)) ? fmtDate(String(d).slice(0, 10) + 'T12:00:00') : String(d ?? ''));

  async function capture() {
    const text = newIdea.trim();
    if (!text || busy) return;
    busy = true;
    try {
      const c = await post('/chats', {});
      await post(`/chats/${c.id}/send`, { text: `new idea: ${text}` });
      newIdea = '';
      await refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
    } catch (e) { failed(t('ui.idea.captureFailed'), e); }
    finally { busy = false; }
  }
</script>

<div class="page">
  <h1>{term('title')}</h1>
  <form class="row" onsubmit={(e) => { e.preventDefault(); capture(); }}>
    <input class="grow" placeholder={term('newPlaceholder')} aria-label={term('newPlaceholder')} bind:value={newIdea} dir="auto" />
    <button class="btn primary" disabled={!newIdea.trim() || busy}><Icon name="ideas" size={14} /> {term('capture')}</button>
  </form>

  {#if loading}
    <div class="grid sk" aria-busy="true"><div class="skel"></div><div class="skel"></div><div class="skel"></div></div>
  {:else if loadError && !ideas.length}
    <div class="state" role="alert"><Icon name="warning" size={24} /><p>{t('idea.loadFailed')}</p><p class="muted sm" dir="auto">{loadError}</p>
      <button class="btn primary" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('idea.tryAgain')}</button></div>
  {:else if !ideas.length}
    <div class="state"><Icon name="ideas" size={28} /><h2>{term('emptyTitle')}</h2><p class="muted">{term('emptyBody')}</p><p class="muted sm">{t('ui.idea.emptyTip')}</p></div>
  {:else}
    <div class="row filters">
      <input type="search" placeholder={t('idea.searchPlaceholder')} aria-label={t('idea.searchPlaceholder')} bind:value={q} class="q" dir="auto" />
      <div class="row pills" role="group" aria-label={t('idea.statusFilter')}>
        <button class="pill" class:accent={!status} aria-pressed={!status} onclick={() => (status = '')}>{t('idea.all')}</button>
        {#each statuses as s}<button class="pill" class:accent={status === s} aria-pressed={status === s} onclick={() => (status = s)}>{statusLabel(s)}</button>{/each}
      </div>
      <span class="grow"></span>
      <span class="muted sm" role="status">{tn('idea.count', shown.length)}</span>
    </div>
    {#if !shown.length}
      <div class="state">
        <Icon name="search" size={24} />
        <p>{t('idea.noMatches')}</p>
        <button class="btn" onclick={() => { q = ''; status = ''; }}>{t('idea.clearFilters')}</button>
      </div>
    {/if}
    {#each areas as area}
      <h2 dir="auto">{area}</h2>
      <div class="grid">
        {#each shown.filter((i) => i.area === area) as i (i.path)}
          <div class="cell">
          <button class="idea surface lit" onclick={() => go({ kind: 'docs', path: i.path })}>
            <div class="row"><b class="grow" dir="auto">{i.title}</b>{#if i.status}<span class="pill st-{i.status}">{statusLabel(i.status)}</span>{/if}</div>
            <p class="muted" dir="auto">{i.summary}</p>
            <div class="row sm muted"><span class="grow">{when(i.updated)}</span>{#if i.level}<span class="pill"><bdi>{i.level}</bdi></span>{/if}
              {#if i.openTodos}<span class="row" style="gap:4px" title={tn('idea.openTodos', i.openTodos)}><Icon name="todos" size={13} /> <span aria-hidden="true">{fmtNum(i.openTodos)}</span><span class="sr">{tn('idea.openTodos', i.openTodos)}</span></span>{/if}</div>
          </button>
          <span class="ia"><ItemActions target="idea" name={i.title} item={{ path: i.path, area: i.area, slug: i.slug, title: i.title, status: i.status, summary: i.summary, tags: i.tags, updated: i.updated }} /></span>
          </div>
        {/each}
      </div>
    {/each}
  {/if}
</div>

<style>
  .filters { margin: 12px 0; flex-wrap: wrap; }
  .q { max-width: 260px; }
  .pills { flex-wrap: wrap; gap: 6px; }
  .filters .pill { font: inherit; font-size: var(--fs-xs); font-weight: 500; border: 0; cursor: pointer; padding: 3px 11px; }
  .pill { font-size: var(--fs-xs); }
  .pill.st-active { background: var(--tint); color: var(--on-tint); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; }
  .cell { position: relative; display: grid; }
  .ia { position: absolute; inset-block-end: 8px; inset-inline-end: 8px; }
  .idea { font: inherit; color: inherit; text-align: start; box-sizing: border-box; cursor: pointer; padding: 14px 16px; display: flex; flex-direction: column; gap: 4px; transition: transform .2s; }
  .idea > * { position: relative; }
  .idea:hover { transform: translateY(-2px); }
  .idea b { overflow-wrap: anywhere; }
  .idea p { margin: 0; font-size: 13px; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
  .state { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; padding: 40px 0; color: var(--muted); }
  .state h2 { margin: 4px 0 0; color: var(--fg); }
  .state p { margin: 0; max-width: 440px; }
  .sk { margin-top: 16px; }
  .skel { height: 110px; border-radius: var(--radius-lg); background: var(--fill); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .sm { font-size: var(--fs-xs); }
</style>
