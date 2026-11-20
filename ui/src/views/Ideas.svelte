<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import ItemActions from '../lib/ItemActions.svelte';
  import { onMount } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { go, refreshChats, toast } from '../lib/store.svelte';
  import { t, tn, fmtDate, fmtNum } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { areaKey, areaLabel, areaWord, newItemTrigger, term } from '../lib/terms';
  import { fold } from '../lib/CommandPalette.svelte';
  import { openItem } from '../lib/item';
  import PageExtras from './PageExtras.svelte';
  import Sections from './Sections.svelte';
  import { pageText, pageTitle, shows } from '../lib/pages.svelte';

  let ideas = $state<any[]>([]);
  let loading = $state(true);
  let loadError = $state('');
  let status = $state('');
  let q = $state('');
  // One tab per area of the library ('' = all of them).
  let area = $state('');
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
  // Areas in a fixed order: the user's work area first, then the rest alphabetically, ideas last.
  const work = $derived(areaWord());
  const rank = (a: string) => (areaKey(a) === work && work !== 'ideas' ? 0 : areaKey(a) === 'ideas' ? 2 : 1);
  const byArea = (a: string, b: string) => rank(a) - rank(b) || areaLabel(a).localeCompare(areaLabel(b));
  const allAreas = $derived([...new Set(ideas.map((i) => i.area))].sort(byArea));
  $effect(() => { if (area && !allAreas.includes(area)) area = ''; });
  // Sorting: most recently updated first (the default), by name, or by status. Remembered on this device.
  type Sort = 'updated' | 'name' | 'status';
  const SORT_KEY = 'thoughts.ideas.sort';
  let sort = $state<Sort>((() => { try { const v = localStorage.getItem(SORT_KEY); return v === 'name' || v === 'status' ? v : 'updated'; } catch { return 'updated'; } })());
  $effect(() => { const v = sort; try { localStorage.setItem(SORT_KEY, v); } catch { /* private mode */ } });
  const rankOf = (st: string) => { const i = ORDER.indexOf(st); return i < 0 ? 99 : i; };
  const byName = (a: any, b: any) => String(a.title ?? '').localeCompare(String(b.title ?? ''));
  const byUpdated = (a: any, b: any) => String(b.updated ?? '').localeCompare(String(a.updated ?? '')) || byName(a, b);
  const sorter = $derived(sort === 'name' ? byName : sort === 'status' ? (a: any, b: any) => rankOf(a.status) - rankOf(b.status) || byUpdated(a, b) : byUpdated);
  const shown = $derived(ideas.filter((i) => (!status || i.status === status) && (!area || i.area === area) &&
    (!q.trim() || fold(`${i.title} ${i.summary} ${[i.tags ?? []].flat().join(' ')}`).includes(fold(q.trim())))).sort(sorter));

  // Changing an item's status from its card. The card shows the new status straight away and goes
  // back if it could not be saved.
  let saving = $state<Record<string, boolean>>({});
  async function setStatus(i: any, next: string, sel: HTMLSelectElement) {
    if (!next || next === i.status || saving[i.path]) return;
    const prev = i.status;
    saving[i.path] = true;
    i.status = next;
    try {
      await post('/library/ideas/status', { path: i.path, status: next });
      toast(t('todob.idea.statusSaved', { title: i.title || '', status: statusLabel(next) }), { kind: 'success', key: 'idea-status:' + i.path,
        action: { label: t('undo'), run: () => { post('/library/ideas/status', { path: i.path, status: prev }).catch((e) => failed(t('todob.idea.statusFailed'), e)); } } });
    } catch (e) { i.status = prev; sel.value = prev ?? ''; failed(t('todob.idea.statusFailed'), e); }
    finally { saving[i.path] = false; }
  }
  const areas = $derived([...new Set(shown.map((i) => i.area))].sort(byArea));
  // With a work word the page holds more than ideas, so search and the counter speak of items.
  const isWork = $derived(work !== 'ideas');

  /** Known statuses are translated; anything custom is shown as written. */
  const statusLabel = (s: string) => { const k = `idea.status.${s}`; const x = t(k); return x === k ? s : x; };
  const when = (d: any) => (/^\d{4}-\d{2}-\d{2}/.test(String(d)) ? fmtDate(String(d).slice(0, 10) + 'T12:00:00') : String(d ?? ''));

  async function capture() {
    const text = newIdea.trim();
    if (!text || busy) return;
    busy = true;
    try {
      const c = await post('/chats', {});
      // The user's own word ("new client: …"), so the assistant files a client, not an idea.
      await post(`/chats/${c.id}/send`, { text: newItemTrigger('en') + text });
      newIdea = '';
      await refreshChats().catch(() => {});
      go({ kind: 'chat', id: c.id });
    } catch (e) { failed(t('ui.idea.captureFailed'), e); }
    finally { busy = false; }
  }
</script>

{#snippet captureS()}
  {#if shows('ideas', 'capture')}
  <form class="row" onsubmit={(e) => { e.preventDefault(); capture(); }}>
    <input class="grow" placeholder={pageText('ideas', 'capturePlaceholder', term('newPlaceholder'))} aria-label={pageText('ideas', 'capturePlaceholder', term('newPlaceholder'))} bind:value={newIdea} dir="auto" />
    <button class="btn primary" disabled={!newIdea.trim() || busy}><Icon name="ideas" size={14} /> {pageText('ideas', 'capture', term('capture'))}</button>
  </form>
  {/if}

{/snippet}

{#snippet toolsS()}
  {#if !loading && ideas.length}
    {#if allAreas.length > 1 && shows('ideas', 'areas')}
      <div class="row tabs" role="group" aria-label={t('todoa.work.areas')}>
        <button class="tab" aria-pressed={!area} onclick={() => (area = '')}>{t('idea.all')}</button>
        {#each allAreas as a (a)}<button class="tab" aria-pressed={area === a} onclick={() => (area = a)} dir="auto">{areaLabel(a)}</button>{/each}
      </div>
    {/if}
    <div class="row filters">
      {#if shows('ideas', 'search')}<input type="search" placeholder={pageText('ideas', 'search', t(isWork ? 'todoa.work.search' : 'idea.searchPlaceholder'))} aria-label={pageText('ideas', 'search', t(isWork ? 'todoa.work.search' : 'idea.searchPlaceholder'))} bind:value={q} class="q" dir="auto" />{/if}
      {#if shows('ideas', 'statusFilter')}
      <div class="row pills" role="group" aria-label={t('idea.statusFilter')}>
        <button class="pill" class:accent={!status} aria-pressed={!status} onclick={() => (status = '')}>{t('idea.all')}</button>
        {#each statuses as s}<button class="pill" class:accent={status === s} aria-pressed={status === s} onclick={() => (status = s)}>{statusLabel(s)}</button>{/each}
      </div>
      {/if}
      <span class="grow"></span>
      {#if shows('ideas', 'sort')}
      <label class="muted sm" for="idea-sort">{t('todob.idea.sort')}</label>
      <select id="idea-sort" class="sortsel" bind:value={sort}>
        <option value="updated">{t('todob.idea.sort.updated')}</option>
        <option value="name">{t('todob.idea.sort.name')}</option>
        <option value="status">{t('todob.idea.sort.status')}</option>
      </select>
      {/if}
      {#if shows('ideas', 'count')}<span class="muted sm" role="status">{tn(isWork ? 'todoa.work.count' : 'idea.count', shown.length)}</span>{/if}
    </div>
  {/if}
{/snippet}

{#snippet listS()}
  {#if loading}
    <div class="grid sk" aria-busy="true"><div class="skel"></div><div class="skel"></div><div class="skel"></div></div>
  {:else if loadError && !ideas.length}
    <div class="state" role="alert"><Icon name="warning" size={24} /><p>{t('idea.loadFailed')}</p><p class="muted sm" dir="auto">{loadError}</p>
      <button class="btn primary" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('idea.tryAgain')}</button></div>
  {:else if !ideas.length}
    <div class="state"><Icon name="ideas" size={28} /><h2>{term('emptyTitle')}</h2><p class="muted">{term('emptyBody')}</p><p class="muted sm">{t('ui.idea.emptyTip')}</p></div>
  {:else}
    {#if !shown.length}
      <div class="state">
        <Icon name="search" size={24} />
        <p>{t('idea.noMatches')}</p>
        <button class="btn" onclick={() => { q = ''; status = ''; area = ''; }}>{t('idea.clearFilters')}</button>
      </div>
    {/if}
    {#each areas as a (a)}
      {#if !area}<h2 dir="auto">{areaLabel(a)}</h2>{/if}
      <div class="grid">
        {#each shown.filter((i) => i.area === a) as i (i.path)}
          <div class="cell">
          <button class="idea surface lit" onclick={() => openItem(i.path)}>
            <div class="row"><b class="grow" dir="auto">{i.title}</b><span class="stspace" aria-hidden="true"></span></div>
            <p class="muted" dir="auto">{i.summary}</p>
            {#if i.detailsUnreadable}<p class="warn sm" role="note"><Icon name="warning" size={12} /> {t('todoa.work.unreadable')}</p>{/if}
            <div class="row sm muted"><span class="grow">{when(i.updated)}</span>{#if i.level}<span class="pill"><bdi>{i.level}</bdi></span>{/if}
              {#if i.openTodos}<span class="row" style="gap:4px" title={tn('idea.openTodos', i.openTodos)}><Icon name="todos" size={13} /> <span aria-hidden="true">{fmtNum(i.openTodos)}</span><span class="sr">{tn('idea.openTodos', i.openTodos)}</span></span>{/if}</div>
          </button>
          <span class="st">
            <select class="pill st-{i.status}" value={i.status ?? ''} disabled={saving[i.path]} aria-label={t('todob.idea.statusOf', { title: i.title || '' })} title={t('todob.idea.statusHint')}
              onchange={(e) => setStatus(i, e.currentTarget.value, e.currentTarget)}>
              {#if !i.status}<option value="" disabled>{t('todob.idea.noStatus')}</option>{/if}
              {#each ORDER as st (st)}<option value={st}>{statusLabel(st)}</option>{/each}
              {#if i.status && !ORDER.includes(i.status)}<option value={i.status}>{i.status}</option>{/if}
            </select>
          </span>
          <span class="ia"><ItemActions target="idea" name={i.title} item={{ path: i.path, area: i.area, slug: i.slug, title: i.title, status: i.status, summary: i.summary, tags: i.tags, updated: i.updated }} /></span>
          </div>
        {/each}
      </div>
    {/each}
  {/if}
{/snippet}

<div class="page">
  <h1>{pageTitle('ideas')}</h1>
  <PageExtras page="ideas" />
  <Sections page="ideas" parts={{ capture: captureS, tools: toolsS, list: listS }} />
  <PageExtras page="ideas" place="bottom" />
</div>

<style>
  .filters { margin: 12px 0; flex-wrap: wrap; }
  .tabs { margin-top: 14px; gap: 4px; flex-wrap: wrap; border-bottom: var(--border-w) solid var(--hairline); }
  .tab { font: inherit; font-size: 13.5px; font-weight: 500; color: var(--muted); background: none; border: 0; border-bottom: 2px solid transparent; margin-bottom: -1px; padding: 6px 10px; cursor: pointer; }
  .tab:hover { color: var(--fg); }
  .tab[aria-pressed='true'] { color: var(--fg); border-bottom-color: var(--accent); }
  .warn { display: inline-flex; align-items: center; gap: 4px; color: var(--muted); }
  .q { max-width: 260px; }
  .pills { flex-wrap: wrap; gap: 6px; }
  .filters .pill { font: inherit; font-size: var(--fs-xs); font-weight: 500; border: 0; cursor: pointer; padding: 3px 11px; }
  .pill { font-size: var(--fs-xs); }
  .pill.st-active { background: var(--tint); color: var(--on-tint); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; }
  .cell { position: relative; display: grid; }
  .st { position: absolute; inset-block-start: 12px; inset-inline-end: 12px; }
  .st select { font: inherit; font-size: var(--fs-xs); font-weight: 500; width: auto; padding: 2px 8px; border-radius: 99px; cursor: pointer; }
  .stspace { width: 92px; flex: none; }
  .sortsel { width: auto; font-size: 13px; padding: 4px 8px; }
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
