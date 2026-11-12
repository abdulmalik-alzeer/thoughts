<script lang="ts">
  import { failed } from '../lib/errors';
  import Icon from '../lib/Icon.svelte';
  import { del, post } from '../lib/api';
  import { app, go, toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { t } from '../lib/i18n';

  let { projects, reload, newChat }: { projects: any[]; reload: () => Promise<void>; newChat: (pid: string) => void } = $props();

  let name = $state('');
  let path = $state('');
  let inside = $state(false);
  let adding = $state(false);
  let removing = $state<Record<string, boolean>>({});

  const canAdd = $derived(inside ? !!name.trim() : !!path.trim());

  async function browse() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true });
      if (typeof p === 'string') { path = p; if (!name) name = p.split(/[\\/]/).filter(Boolean).pop() || ''; }
    } catch { toast(t('proj.browseFail'), { kind: 'error' }); }
  }

  async function add() {
    if (!canAdd || adding) return;
    adding = true;
    try {
      await post('/projects', { name: name.trim(), path: path.trim(), inside_library: inside });
      name = ''; path = ''; inside = false;
      await reload();
      toast(t('proj.linked'), { kind: 'success' });
    } catch (e) { failed(t('proj.failed'), e); }
    finally { adding = false; }
  }

  async function remove(p: any) {
    const ok = await confirmDialog({ title: t('proj.unlinkTitle', { name: p.name }), body: t('proj.unlinkBody'), confirmLabel: t('proj.unlink') });
    if (!ok) return;
    removing[p.id] = true;
    try { await del(`/projects/${p.id}`); await reload(); }
    catch (e) { failed(t('proj.failed'), e); }
    finally { removing[p.id] = false; }
  }
</script>

<div class="page">
  <h1>{t('projects')}</h1>
  <p class="muted">{t('proj.intro')}</p>
  <p class="note"><Icon name="warning" size={15} /> <span>{t('proj.access')}</span></p>

  <form class="card" onsubmit={(e) => { e.preventDefault(); add(); }}>
    <label for="pj-name">{t('proj.name')}</label>
    <input id="pj-name" dir="auto" bind:value={name} placeholder={t('proj.namePh')} autocomplete="off" />
    <label class="check"><input type="checkbox" bind:checked={inside} /> <span>{t('proj.inside')}</span></label>
    {#if !inside}
      <label for="pj-path">{t('proj.folder')}</label>
      <div class="row"><input id="pj-path" class="grow" dir="ltr" bind:value={path} placeholder="~/Documents/Clients/Annual report" autocomplete="off" /><button type="button" class="btn" onclick={browse}><Icon name="folder" size={14} /> {t('proj.browse')}</button></div>
    {/if}
    <div class="row actions">
      <button type="submit" class="btn primary" disabled={!canAdd || adding} aria-busy={adding}>{adding ? t('proj.linking') : t('proj.link')}</button>
      {#if !canAdd}<span class="muted sm">{t(inside ? 'proj.needName' : 'proj.needFolder')}</span>{/if}
    </div>
  </form>

  <div class="list">
    {#each projects as p (p.id)}
      {@const chats = app.chats.filter((c) => c.projectId === p.id)}
      <div class="card">
        <div class="row"><b class="grow" dir="auto">{p.name}</b>
          <button type="button" class="btn primary" onclick={() => newChat(p.id)}><Icon name="plus" size={15} /> {t('newChat')}</button>
          <button type="button" class="btn ghost" disabled={removing[p.id]} onclick={() => remove(p)}>{t('proj.unlink')}</button></div>
        <code class="muted sm path"><bdi>{p.path}</bdi></code>
        {#each chats.slice(0, 5) as c (c.id)}<button type="button" class="chat" onclick={() => go({ kind: 'chat', id: c.id })}><Icon name="chat" size={13} /> <span dir="auto">{c.title}</span></button>{/each}
      </div>
    {:else}
      <div class="empty"><Icon name="projects" size={22} /><b>{t('proj.emptyTitle')}</b><span class="muted">{t('proj.emptyBody')}</span></div>
    {/each}
  </div>
</div>

<style>
  .list { display: grid; gap: 10px; margin-top: 16px; }
  .chat { font: inherit; color: inherit; background: none; border: 0; text-align: start; display: flex; gap: 6px; align-items: center; cursor: pointer; font-size: 13px; padding: 3px 0; width: 100%; }
  .chat:hover { color: var(--accent); }
  .sm { font-size: var(--fs-xs); }
  .path { display: inline-block; overflow-wrap: anywhere; }
  .check { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; }
  .actions { margin-top: 12px; }
  .note { display: flex; gap: 8px; align-items: flex-start; font-size: 13px; margin: 0 0 14px; }
  .note :global(.icon) { flex: none; margin-top: 3px; color: var(--accent); }
  .empty { display: grid; justify-items: center; gap: 4px; text-align: center; padding: 28px 16px; border: 1px dashed var(--border); border-radius: var(--radius-lg); }
</style>
