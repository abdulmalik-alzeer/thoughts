<script lang="ts">
  import HelpTip from '../lib/HelpTip.svelte';
  // The Chats page: every chat with a filter box, pinning, archiving, selecting several to delete,
  // and the choice to delete old chats automatically.
  import { onMount } from 'svelte';
  import { api, del, patch, post, put } from '../lib/api';
  import { app, go, refreshChats, toast } from '../lib/store.svelte';
  import { tip } from '../lib/shortcuts';
  import { failed } from '../lib/errors';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { t, tn, ago, fmtDate, fmtNum } from '../lib/i18n';
  import { fold } from '../lib/CommandPalette.svelte';
  import Icon from '../lib/Icon.svelte';
  import PageExtras from './PageExtras.svelte';
  import Sections from './Sections.svelte';
  import { pageText, pageTitle, shows } from '../lib/pages.svelte';
  import { listedChats } from '../lib/mado/chats';

  let { projects, newChat }: { projects: any[]; newChat: (pid?: string | null) => void } = $props();

  type Filter = 'all' | 'mine' | 'system' | 'archived';
  let filter = $state<Filter>('all');
  let q = $state('');
  let selecting = $state(false);
  let picked = $state<Set<string>>(new Set());
  let showAllSystem = $state(false);
  let busy = $state(false);

  const SYSTEM_SHOWN = 8;
  const kindLabel = (k: string) => { const key = 'shell.kind.' + k; const s = t(key); return s === key ? k : s; };
  const sorted = (l: any[]) => [...l].sort((a, b) => Number(!!b.pinned) - Number(!!a.pinned));

  const needle = $derived(fold(q.trim()));
  const matches = (c: any) => !needle || fold(String(c.title || '')).includes(needle);
  const live = $derived(listedChats(app.chats).filter((c: any) => !c.archived));
  const mine = $derived(sorted(live.filter((c: any) => c.kind === 'chat' && matches(c))));
  const system = $derived(live.filter((c: any) => c.kind !== 'chat' && matches(c)));
  const archived = $derived(sorted(listedChats(app.chats).filter((c: any) => c.archived && matches(c))));
  // What the current filter lists, as one or two groups.
  const shown = $derived(filter === 'archived' ? archived : filter === 'mine' ? mine : filter === 'system' ? system : [...mine, ...system]);
  const systemRows = $derived(filter === 'system' || showAllSystem ? system : system.slice(0, SYSTEM_SHOWN));

  const pickedList = $derived(shown.filter((c: any) => picked.has(c.id)));
  const unsavedPicked = $derived(pickedList.filter((c: any) => c.unwrapped).length);

  function toggleSelect() { selecting = !selecting; picked = new Set(); }
  function toggle(id: string) { const s = new Set(picked); if (s.has(id)) s.delete(id); else s.add(id); picked = s; }
  const allPicked = $derived(shown.length > 0 && shown.every((c: any) => picked.has(c.id)));
  function pickAll() { picked = allPicked ? new Set() : new Set(shown.map((c: any) => c.id)); }
  // Only what is on screen counts as selected: a chat hidden by a filter is never acted on.

  async function setFlag(c: any, key: 'pinned' | 'archived') {
    try {
      await patch(`/chats/${c.id}`, { [key]: !c[key] });
      await refreshChats();
    } catch (e) { failed(t('chatb.failChange'), e); }
  }

  async function archivePicked(on: boolean) {
    if (busy || !pickedList.length) return;
    busy = true;
    try {
      const results = await Promise.allSettled(pickedList.map((c: any) => patch(`/chats/${c.id}`, { archived: on })));
      const bad = results.find((r) => r.status === 'rejected') as PromiseRejectedResult | undefined;
      await refreshChats();
      if (bad) failed(t('chatb.failChanges'), bad.reason); else { picked = new Set(); if (on) toast(t('chatb.archivedToast')); }
    } finally { busy = false; }
  }

  async function deletePicked() {
    const list = pickedList;
    if (busy || !list.length) return;
    const body = t('chatb.delBody') + (unsavedPicked ? '\n\n' + tn('chatb.delUnsaved', unsavedPicked) : '');
    if (!(await confirmDialog({ title: tn('chatb.delTitle', list.length), body, confirmLabel: t('delete'), danger: true }))) return;
    busy = true;
    try {
      const r = await post('/chats/bulk-delete', { ids: list.map((c: any) => c.id) });
      // Drafts and unsent files of a deleted chat go with it.
      for (const c of list) { try { localStorage.removeItem(`thoughts.chatdraft:${c.id}`); } catch { /* ignore */ } }
      await refreshChats();
      picked = new Set();
      toast(r.skipped ? tn('chatb.deletedSkipped', r.skipped, { done: tn('chatb.deleted', r.deleted) }) : tn('chatb.deleted', r.deleted), { kind: 'success' });
      if (!app.chats.length) selecting = false;
    } catch (e) { failed(t('chatb.failDelete'), e); }
    finally { busy = false; }
  }

  // ---------- delete old chats automatically ----------
  let keep = $state<{ days: number; choices: number[]; preview: any } | null>(null);
  let chosen = $state(0);
  let saving = $state(false);
  let loadSeq = 0;

  async function loadKeep(days?: number) {
    const mine = ++loadSeq;
    try {
      const r = await api(days === undefined ? '/chats/retention' : `/chats/retention?days=${days}`);
      if (mine !== loadSeq) return;
      keep = r;
      if (days === undefined) chosen = r.days;
    } catch (e) { if (mine === loadSeq) failed(t('chatb.keep.loadFail'), e); }
  }
  onMount(() => { loadKeep(); });
  function choose(v: number) { chosen = v; if (v > 0) loadKeep(v); }

  // One plain description of what a clean-up after the chosen time would do right now.
  const preview = $derived.by(() => {
    const p = keep?.preview;
    if (!p || chosen === 0 || p.days !== chosen) return [] as string[];
    const out = [p.total ? tn('chatb.keep.would', p.total) : t('chatb.keep.none')];
    if (p.unsaved) out.push(tn('chatb.keep.unsaved', p.unsaved));
    if (p.pinned) out.push(tn('chatb.keep.pinned', p.pinned));
    return out;
  });

  async function saveKeep() {
    if (saving || !keep || chosen === keep.days) return;
    if (chosen > 0 && !(await confirmDialog({ title: t('chatb.keep.confirmTitle'), body: [...preview, t('chatb.keep.confirmBody')].join(' '), confirmLabel: t('chatb.keep.confirm'), danger: true }))) return;
    saving = true;
    try {
      const r = await put('/chats/retention', { days: chosen });
      keep = r; chosen = r.days;
      if (r.removed) refreshChats().catch(() => {});
      toast(chosen === 0 ? t('chatb.keep.savedOff') : tn('chatb.keep.saved', r.removed ?? 0), { kind: 'success' });
    } catch (e) { failed(t('chatb.keep.fail'), e); loadKeep().catch(() => {}); }
    finally { saving = false; }
  }
</script>

    {#snippet card(c: any)}
      <div class="card item" class:sel={picked.has(c.id)}>
        {#if selecting}<input type="checkbox" class="pick" checked={picked.has(c.id)} onchange={() => toggle(c.id)} aria-label={t('chatb.selectChat', { title: c.title })} />{/if}
        <button class="open" onclick={() => (selecting ? toggle(c.id) : go({ kind: 'chat', id: c.id }))}>
          <div class="row">
            {#if c.pinned}<span class="pinmark" title={t('chatb.pinned')}><Icon name="pin" size={13} /><span class="sr-only">({t('chatb.pinned')})</span></span>{/if}
            <b class="grow ell" dir="auto">{c.title}</b>
            {#if c.running}<span class="pill accent">{t('shell.dot.running')}</span>{/if}
            {#if c.unwrapped}<span class="pill">{t('shell.dot.unwrapped')}</span>{/if}
            {#if c.kind !== 'chat'}<span class="pill">{kindLabel(c.kind)}</span>{/if}
          </div>
          <div class="muted sm"><span title={fmtDate(c.updated, { dateStyle: 'medium', timeStyle: 'short' })}>{ago(c.updated)}</span> · {c.messageCount ? tn('shell.msgs', c.messageCount) : t('chatb.noMessages')}</div>
        </button>
        {#if !selecting}
          <div class="acts">
            {#if c.kind === 'chat' && !c.archived}
              <button class="iconbtn" class:on={c.pinned} aria-pressed={!!c.pinned} title={c.pinned ? t('chatb.unpin') : t('chatb.pin')} aria-label={c.pinned ? t('chatb.unpinNamed', { title: c.title }) : t('chatb.pinNamed', { title: c.title })} onclick={() => setFlag(c, 'pinned')}><Icon name="pin" size={15} /></button>
            {/if}
            <button class="iconbtn" title={c.archived ? t('chatb.unarchive') : t('chatb.archive')} aria-label={c.archived ? t('chatb.unarchiveNamed', { title: c.title }) : t('chatb.archiveNamed', { title: c.title })} onclick={() => setFlag(c, 'archived')}><Icon name={c.archived ? 'unarchive' : 'archive'} size={15} /></button>
          </div>
        {/if}
      </div>
    {/snippet}

{#snippet actionsS()}
  {#if shows('chats', 'newChat') || (shows('chats', 'folderChats') && projects.length)}
  <div class="row" style="flex-wrap:wrap">
    {#if shows('chats', 'newChat')}<button class="btn primary" title={tip(pageText('chats', 'newChat', t('newChat')), 'chat.new')} onclick={() => newChat()}><Icon name="plus" size={15} />{pageText('chats', 'newChat', t('newChat'))}</button>{/if}
    {#each shows('chats', 'folderChats') ? projects : [] as p (p.id)}<button class="btn" title={t('shell.newChatIn', { name: p.name })} onclick={() => newChat(p.id)}><Icon name="folder" size={15} /><span dir="auto">{p.name}</span></button>{/each}
  </div>
  {/if}

{/snippet}

{#snippet toolsS()}
  {#if app.chats.length}
    {#if shows('chats', 'search') || shows('chats', 'select')}
    <div class="bar row">
      {#if shows('chats', 'search')}
      <div class="find grow">
        <Icon name="search" size={15} />
        <input type="search" bind:value={q} placeholder={pageText('chats', 'search', t('chatb.search'))} aria-label={pageText('chats', 'search', t('chatb.search'))} dir="auto" />
      </div>
      {:else}<span class="grow"></span>{/if}
      {#if shows('chats', 'select')}<button class="btn" aria-pressed={selecting} onclick={toggleSelect}>{selecting ? t('chatb.selectDone') : t('chatb.select')}</button>{/if}
    </div>
    {/if}
    {#if shows('chats', 'filters')}
    <div class="seg" role="group" aria-label={t('chatb.filters')}>
      {#each [['all', 'chatb.filter.all'], ['mine', 'chatb.filter.mine'], ['system', 'chatb.filter.system'], ['archived', 'chatb.filter.archived']] as [id, label]}
        <button class="segb" aria-pressed={filter === id} onclick={() => (filter = id as Filter)}>{t(label)}{#if id === 'archived' && archived.length && !needle} <span class="n">{fmtNum(archived.length)}</span>{/if}</button>
      {/each}
    </div>
    {/if}

    {#if selecting}
      <div class="selbar row" role="region" aria-label={t('chatb.select')}>
        <label class="all"><input type="checkbox" checked={allPicked} disabled={!shown.length} onchange={pickAll} />{allPicked ? t('chatb.selectNone') : t('chatb.selectAll')}</label>
        <span class="grow muted" aria-live="polite">{pickedList.length ? tn('chatb.selected', pickedList.length) : ''}</span>
        <button class="btn" disabled={!pickedList.length || busy} onclick={() => archivePicked(filter !== 'archived')}><Icon name="archive" size={15} />{filter === 'archived' ? t('chatb.unarchive') : t('chatb.archive')}</button>
        <button class="btn danger" disabled={!pickedList.length || busy} onclick={deletePicked}><Icon name="trash" size={15} />{t('delete')}</button>
      </div>
    {/if}

    {#if filter === 'archived' && !needle && archived.length}<p class="muted sm note">{t('chatb.archivedNote')}</p>{/if}


  {/if}
{/snippet}

{#snippet listS()}
  {#if !app.chats.length}
    <p class="muted">{pageText('chats', 'empty', t('noChats'))}</p>
  {:else}
    {#if !shown.length}
      <p class="muted">{needle ? t('chatb.noMatch', { q: q.trim() }) : filter === 'archived' ? t('chatb.noneArchived') : filter === 'system' ? t('chatb.noneSystem') : pageText('chats', 'empty', t('noChats'))}</p>
    {:else if filter === 'all'}
      <div class="list">{#each mine as c (c.id)}{@render card(c)}{/each}</div>
      {#if system.length && shows('chats', 'systemChats')}
        <h2 class="head">{t('chatb.systemHead')}</h2>
        <div class="list">{#each systemRows as c (c.id)}{@render card(c)}{/each}</div>
        {#if system.length > SYSTEM_SHOWN}
          <button class="more" onclick={() => (showAllSystem = !showAllSystem)}>{showAllSystem ? t('chatb.showFewer') : t('chatb.showAll', { n: fmtNum(system.length) })}</button>
        {/if}
      {/if}
    {:else}
      <div class="list">{#each shown as c (c.id)}{@render card(c)}{/each}</div>
    {/if}

  {/if}
{/snippet}

{#snippet keepS()}
  {#if app.chats.length}
    {#if shows('chats', 'keep')}
    <section class="keep card" aria-labelledby="chatb-keep">
      <h2 id="chatb-keep" class="head first">{t('chatb.keep.title')} <HelpTip text={t('chatb.keep.body')} /></h2>
      {#if keep}
        <p class="sm" role="status">{keep.days ? t('chatb.keep.statusOn', { n: fmtNum(keep.days) }) : t('chatb.keep.statusOff')}</p>
        <div class="row" style="flex-wrap:wrap">
          <label class="lbl" for="chatb-days">{t('chatb.keep.label')}</label>
          <select id="chatb-days" class="dayssel" value={String(chosen)} onchange={(e) => choose(Number((e.target as HTMLSelectElement).value))}>
            <option value="0">{t('chatb.keep.off')}</option>
            {#each keep.choices as d}<option value={String(d)}>{tn('chatb.keep.days', d)}</option>{/each}
            {#if keep.days && !keep.choices.includes(keep.days)}<option value={String(keep.days)}>{tn('chatb.keep.days', keep.days)}</option>{/if}
          </select>
          <button class="btn" disabled={saving || chosen === keep.days} aria-busy={saving} onclick={saveKeep}>{t('chatb.keep.save')}</button>
        </div>
        {#if preview.length}<p class="sm" aria-live="polite">{preview.join(' ')}</p>{/if}
      {/if}
    </section>
    {/if}
  {/if}
{/snippet}

<div class="page">
  <h1>{pageTitle('chats')}</h1>
  <PageExtras page="chats" />
  <Sections page="chats" parts={{ actions: actionsS, tools: toolsS, list: listS, keep: keepS }} />
  <PageExtras page="chats" place="bottom" />
</div>

<style>
  .bar { margin: 16px 0 10px; }
  .find { display: flex; align-items: center; gap: 8px; padding: 0 10px; border-radius: var(--radius); background: var(--fill); border: var(--border-w) solid var(--control-border); color: var(--muted); }
  .find input { border: 0; background: none; box-shadow: none; padding: 7px 0; color: var(--fg); }
  .find input:focus { background: none; box-shadow: none; }
  .find:focus-within { box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent); background: var(--bg); }
  .n { opacity: .75; font-variant-numeric: tabular-nums; }
  .selbar { margin-top: 12px; padding: 8px 12px; border-radius: var(--radius); background: var(--tint); color: var(--on-tint); flex-wrap: wrap; }
  .all { display: flex; align-items: center; gap: 8px; margin: 0; color: inherit; font-size: var(--fs-sm); }
  .note { margin: 12px 0 0; }
  .list { display: grid; gap: 10px; margin-top: 14px; }
  .head { font-size: var(--fs-sm); font-weight: 600; color: var(--muted); margin: 26px 0 0; }
  .head.first { margin-top: 0; color: var(--fg); }
  .item { display: flex; align-items: center; gap: 10px; padding: 10px 12px; background: var(--panel); border: 1px solid var(--hairline); border-radius: var(--radius-lg); box-shadow: var(--shadow); transition: transform .15s, box-shadow .15s; }
  .item:hover { transform: translateY(-1px); box-shadow: var(--shadow-lg); }
  .item.sel { background: var(--tint); }
  .open { font: inherit; color: inherit; background: none; border: 0; text-align: start; cursor: pointer; flex: 1; min-width: 0; display: grid; gap: 2px; padding: 4px 0; border-radius: 6px; }
  .pick { flex: none; }
  .pinmark { color: var(--accent-2); display: inline-flex; }
  .acts { display: flex; gap: 2px; flex: none; }
  .acts .iconbtn.on { color: var(--accent-2); }
  .sm { font-size: var(--fs-xs); }
  .more { margin-top: 10px; }
  .keep { margin-top: 32px; display: grid; gap: 8px; }
  .keep p { margin: 0; }
  .lbl { margin: 0; font-size: var(--fs-sm); color: var(--fg); }
  .dayssel { width: auto; min-width: 140px; }
</style>
