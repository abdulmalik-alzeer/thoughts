<script lang="ts">
  import { onMount } from 'svelte';
  import { api, on, post } from '../lib/api';
  import { go, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';

  let lists = $state<any[]>([]);
  let showDone = $state(false);
  let drafts = $state<Record<string, string>>({});
  let editing = $state<string | null>(null);
  let editText = $state('');

  async function load() {
    try { lists = await api('/todos'); } catch (e: any) { toast(e.message); }
  }
  onMount(() => { load(); return on((e) => { if (e.type === 'todos.changed' || e.type === 'library.changed') load(); }); });

  async function op(name: string, body: any) {
    try { await post(`/todos/${name}`, body); } catch (e: any) { toast(e.message); }
    load();
  }

  async function add(file: string) {
    const text = drafts[file]?.trim();
    if (!text) return;
    drafts[file] = '';
    await op('add', { file, text });
  }

  async function createRoot() {
    await op('add', { file: 'TODO.md', text: 'My first todo' });
  }

  const open = $derived(lists.reduce((n, l) => n + l.items.filter((i: any) => !i.done).length, 0));
</script>

<div class="page">
  <div class="row"><h1 class="grow">{t('todos')} <span class="pill">{open}</span></h1>
    <label class="row"><input type="checkbox" bind:checked={showDone} /> show done</label></div>
  {#if !lists.length}
    <p class="muted">No todo lists yet.</p>
    <button class="btn primary" onclick={createRoot}>Create TODO.md</button>
  {/if}
  {#each lists as l (l.file)}
    <section class="card list">
      <div class="row head">
        <b class="grow">{l.idea ?? 'General'}</b>
        <button class="btn ghost sm" onclick={() => go({ kind: 'docs', path: l.idea ? `${l.idea}/README.md` : l.file })}>{l.file}</button>
      </div>
      {#each l.items.filter((i: any) => showDone || !i.done) as it, idx (l.file + it.line)}
        {#if idx === 0 || it.group !== l.items.filter((i: any) => showDone || !i.done)[idx - 1]?.group}
          {#if it.group}<div class="group">{it.group}</div>{/if}
        {/if}
        <div class="item row" class:done={it.done}>
          <input type="checkbox" checked={it.done} onchange={() => op('toggle', { file: l.file, line: it.line, text: it.text })} />
          {#if editing === l.file + it.line}
            <form class="grow row" onsubmit={(e) => { e.preventDefault(); editing = null; op('edit', { file: l.file, line: it.line, text: it.text, new_text: editText }); }}>
              <input class="grow" bind:value={editText} />
              <button class="btn sm">{t('save')}</button>
            </form>
          {:else}
            <button class="txt grow" ondblclick={() => { editing = l.file + it.line; editText = it.text; }}>{it.text}</button>
            <button class="btn ghost sm del" title={t('delete')} onclick={() => op('delete', { file: l.file, line: it.line, text: it.text })}>×</button>
          {/if}
        </div>
      {/each}
      <form class="row add" onsubmit={(e) => { e.preventDefault(); add(l.file); }}>
        <input class="grow" placeholder={t('addTodo')} bind:value={drafts[l.file]} />
      </form>
    </section>
  {/each}
</div>

<style>
  .list { margin-top: 12px; }
  .head { margin-bottom: 6px; }
  .group { font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); margin: 8px 0 2px; }
  .item { padding: 3px 0; }
  .item.done .txt { text-decoration: line-through; color: var(--muted); }
  .txt { all: unset; cursor: text; }
  .del { opacity: 0; }
  .item:hover .del { opacity: 1; }
  .add input { border-style: dashed; margin-top: 6px; }
  .sm { font-size: 12px; padding: 2px 8px; }
</style>
