<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, post, put } from '../lib/api';
  import { md, resolveLink } from '../lib/md';
  import { go, refreshChats, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';

  let { path }: { path?: string } = $props();

  let files = $state<any[]>([]);
  let current = $state<string>('');
  let doc = $state<any>(null);
  let editing = $state(false);
  let draft = $state('');
  let results = $state<any[] | null>(null);
  let filter = $state('');
  let open = $state<Record<string, boolean>>({});

  onMount(async () => {
    try { files = await api('/library/tree'); } catch (e: any) { toast(e.message); return; }
    if (path?.startsWith('?')) { results = await api(`/library/search?q=${encodeURIComponent(path.slice(1))}`); }
    else openFile(path || (files.some((f) => f.path === 'INDEX.md') ? 'INDEX.md' : files[0]?.path));
  });

  async function openFile(p?: string) {
    if (!p) return;
    results = null; editing = false; current = p;
    const top = p.split('/')[0];
    open[top] = true;
    if (!p.endsWith('.md')) { doc = { content: '', binary: true }; return; }
    try { doc = await api(`/library/file?path=${encodeURIComponent(p)}`); } catch (e: any) { toast(e.message); }
  }

  async function save() {
    await put('/library/file', { path: current, content: draft });
    editing = false;
    openFile(current);
  }

  function clickLink(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest('a');
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (/^[a-z]+:/i.test(href) || href.startsWith('#')) return;
    e.preventDefault();
    openFile(resolveLink(current, href));
  }

  async function discuss() {
    const c = await post('/chats', {});
    const slug = current.split('/').slice(0, 2).join('/');
    await post(`/chats/${c.id}/send`, { text: `continue ${slug.split('/')[1] || current}`, mentions: [current] });
    await refreshChats();
    go({ kind: 'chat', id: c.id });
  }

  const groups = $derived.by(() => {
    const g = new Map<string, any[]>();
    for (const f of files) {
      if (filter && !f.path.toLowerCase().includes(filter.toLowerCase())) continue;
      const parts = f.path.split('/');
      const k = parts.length === 1 ? '/' : parts[0];
      g.set(k, [...(g.get(k) || []), f]);
    }
    return [...g.entries()].sort(([a], [b]) => (a === '/' ? -1 : b === '/' ? 1 : a.localeCompare(b)));
  });
</script>

<div class="docs">
  <aside class="tree scroll">
    <input placeholder="Filter files…" bind:value={filter} />
    {#each groups as [k, list]}
      {#if k === '/'}
        {#each list as f}<button class="f" class:active={current === f.path} onclick={() => openFile(f.path)}>{f.path}</button>{/each}
      {:else}
        <button class="dir" onclick={() => (open[k] = !open[k])}><Icon name={open[k] || filter ? 'down' : 'right'} size={13} /> {k}</button>
        {#if open[k] || filter}
          {#each list as f}<button class="f in" class:active={current === f.path} onclick={() => openFile(f.path)} title={f.path}>{f.path.slice(k.length + 1)}</button>{/each}
        {/if}
      {/if}
    {/each}
  </aside>
  <section class="view scroll">
    {#if results}
      <h1>{t('search')}: {path?.slice(1)}</h1>
      {#if !results.length}<p class="muted">No matches.</p>{/if}
      {#each results as r}
        <button class="card hit" onclick={() => openFile(r.path)}>
          <b>{r.path}</b>
          {#each r.matches as m}<div class="muted sm">{m.line}: {m.text}</div>{/each}
        </button>
      {/each}
    {:else if doc}
      <div class="row bar">
        <code class="grow">{current}</code>
        {#if !doc.binary}
          {#if editing}
            <button class="btn" onclick={() => (editing = false)}>{t('cancel')}</button>
            <button class="btn primary" onclick={save}>{t('save')}</button>
          {:else}
            <button class="btn" onclick={discuss}><Icon name="chat" size={15} /> Discuss</button>
            <button class="btn" onclick={() => { draft = doc.content; editing = true; }}>{t('edit')}</button>
          {/if}
        {/if}
      </div>
      {#if doc.binary}
        <p class="muted">This file isn't markdown. It lives at <code>{current}</code> in your library.</p>
      {:else if editing}
        <textarea class="editor" bind:value={draft}></textarea>
      {:else}
        {#if doc.frontmatter && typeof doc.frontmatter === 'object'}
          <div class="row fm">
            {#each Object.entries(doc.frontmatter).filter(([k]) => ['status', 'area', 'updated', 'level'].includes(k)) as [k, v]}<span class="pill">{k}: {v}</span>{/each}
          </div>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <article class="md" dir="auto" onclick={clickLink}>{@html md(doc.content)}</article>
      {/if}
    {/if}
  </section>
</div>

<style>
  .docs { display: grid; grid-template-columns: 260px 1fr; height: calc(100vh - 26px); }
  .tree { border-inline-end: var(--border-w) solid var(--border); padding: 0 8px 12px; display: flex; flex-direction: column; gap: 1px; }
  .tree input { margin-bottom: 6px; padding: 4px 8px; font-size: 12px; }
  .f, .dir { all: unset; cursor: pointer; font-size: 12.5px; padding: 3px 6px; border-radius: 6px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dir { font-weight: 600; margin-top: 4px; }
  .f.in { padding-inline-start: 18px; color: var(--muted); }
  .f:hover, .dir:hover { background: var(--panel-2); }
  .f.active { background: var(--accent); color: var(--accent-fg); }
  .view { padding: 0 32px 40px; }
  .bar { position: sticky; top: 0; background: var(--bg); padding: 6px 0; z-index: 2; }
  .fm { flex-wrap: wrap; margin: 6px 0 10px; }
  .editor { height: calc(100vh - 110px); font-family: var(--mono); font-size: 13px; }
  .hit { all: unset; display: block; cursor: pointer; background: var(--panel); border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 10px; margin-bottom: 8px; }
  .hit:hover { border-color: var(--accent); }
  .sm { font-size: 12px; }
</style>
