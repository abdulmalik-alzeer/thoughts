<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, post } from '../lib/api';
  import { go, refreshChats, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';

  let ideas = $state<any[]>([]);
  let status = $state('');
  let q = $state('');
  let newIdea = $state('');

  onMount(async () => { try { ideas = await api('/library/ideas'); } catch (e: any) { toast(e.message); } });

  const statuses = $derived([...new Set(ideas.map((i) => i.status).filter(Boolean))]);
  const shown = $derived(ideas.filter((i) => (!status || i.status === status) &&
    (!q || `${i.title} ${i.summary} ${(i.tags || []).join(' ')}`.toLowerCase().includes(q.toLowerCase()))));
  const areas = $derived([...new Set(shown.map((i) => i.area))].sort());

  async function capture() {
    if (!newIdea.trim()) return;
    const c = await post('/chats', {});
    await post(`/chats/${c.id}/send`, { text: `new idea: ${newIdea}` });
    newIdea = '';
    await refreshChats();
    go({ kind: 'chat', id: c.id });
  }
</script>

<div class="page">
  <h1>{t('ideas')}</h1>
  <form class="row" onsubmit={(e) => { e.preventDefault(); capture(); }}>
    <input class="grow" placeholder="Describe a new idea…" bind:value={newIdea} />
    <button class="btn primary">Capture</button>
  </form>
  <div class="row filters">
    <input placeholder={t('search')} bind:value={q} style="max-width:260px" />
    <button class="pill" class:accent={!status} onclick={() => (status = '')}>all</button>
    {#each statuses as s}<button class="pill" class:accent={status === s} onclick={() => (status = s)}>{s}</button>{/each}
  </div>
  {#if !ideas.length}<p class="muted">No ideas yet. Capture one above, or say “new idea: …” in any chat.</p>{/if}
  {#each areas as area}
    <h2>{area}</h2>
    <div class="grid">
      {#each shown.filter((i) => i.area === area) as i (i.path)}
        <button class="idea surface lit" onclick={() => go({ kind: 'docs', path: i.path })}>
          <div class="row"><b class="grow">{i.title}</b><span class="pill">{i.status}</span></div>
          <p class="muted">{i.summary}</p>
          <div class="row sm muted"><span class="grow">{i.updated}</span>{#if i.level}<span class="pill">{i.level}</span>{/if}{#if i.openTodos}<span class="row" style="gap:4px"><Icon name="todos" size={13} /> {i.openTodos}</span>{/if}</div>
        </button>
      {/each}
    </div>
  {/each}
</div>

<style>
  .filters { margin: 12px 0; flex-wrap: wrap; }
  .filters .pill { cursor: pointer; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; }
  .idea { font: inherit; color: inherit; text-align: start; box-sizing: border-box; cursor: pointer; padding: 14px 16px; display: flex; flex-direction: column; gap: 4px; transition: transform .2s; }
  .idea > * { position: relative; }
  .idea:hover { transform: translateY(-2px); }
  .idea p { margin: 0; font-size: 13px; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
  .sm { font-size: 12px; }
</style>
