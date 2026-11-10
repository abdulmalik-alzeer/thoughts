<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { del, post } from '../lib/api';
  import { app, go, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';

  let { projects, reload, newChat }: { projects: any[]; reload: () => Promise<void>; newChat: (pid: string) => void } = $props();

  let name = $state('');
  let path = $state('');
  let inside = $state(false);

  async function browse() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true });
      if (typeof p === 'string') { path = p; if (!name) name = p.split('/').pop() || ''; }
    } catch { toast('Type the folder path instead.'); }
  }

  async function add() {
    try {
      await post('/projects', { name, path, inside_library: inside });
      name = ''; path = ''; inside = false;
      await reload();
    } catch (e: any) { toast(e.message); }
  }

  async function remove(id: string) {
    if (!confirm('Unlink this project? Its folder and files are not touched.')) return;
    await del(`/projects/${id}`);
    await reload();
  }
</script>

<div class="page">
  <h1>{t('projects')}</h1>
  <p class="muted">Link any folder you work in — client files, a report, a code repo, a research folder. Chats in a project run inside that folder (using its own instructions, if it has any) and can still read your library.</p>

  <div class="card">
    <div class="row">
      <div class="grow"><label>Name</label><input bind:value={name} placeholder="Client — Annual report" /></div>
    </div>
    <label class="row"><input type="checkbox" bind:checked={inside} /> Create it inside the library (<code>projects/…</code>)</label>
    {#if !inside}
      <label>Folder</label>
      <div class="row"><input class="grow" bind:value={path} placeholder="~/Documents/Clients/Annual report" /><button class="btn" onclick={browse}>Browse…</button></div>
    {/if}
    <div style="margin-top:10px"><button class="btn primary" onclick={add} disabled={!name && !path}>Link project</button></div>
  </div>

  <div class="list">
    {#each projects as p (p.id)}
      {@const chats = app.chats.filter((c) => c.projectId === p.id)}
      <div class="card">
        <div class="row"><b class="grow">{p.name}</b>
          <button class="btn primary" onclick={() => newChat(p.id)}><Icon name="plus" size={15} /> {t('newChat')}</button>
          <button class="btn ghost" onclick={() => remove(p.id)}>Unlink</button></div>
        <code class="muted sm">{p.path}</code>
        {#each chats.slice(0, 5) as c}<button class="chat" onclick={() => go({ kind: 'chat', id: c.id })}><Icon name="chat" size={13} /> {c.title}</button>{/each}
      </div>
    {/each}
  </div>
</div>

<style>
  .list { display: grid; gap: 10px; margin-top: 16px; }
  .chat { all: unset; display: flex; gap: 6px; align-items: center; cursor: pointer; font-size: 13px; padding: 3px 0; }
  .chat:hover { color: var(--accent); }
  .sm { font-size: 12px; }
  label.row { color: var(--fg); font-size: 13px; }
</style>
