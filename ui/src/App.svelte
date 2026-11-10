<script lang="ts">
  import { onMount } from 'svelte';
  import { api, connect, on, post } from './lib/api';
  import { app, applyTheme, go, refreshChats, refreshWrap, toast } from './lib/store.svelte';
  import { loadLayer } from './lib/layer';
  import { t } from './lib/i18n';
  import Onboarding from './views/Onboarding.svelte';
  import Chat from './views/Chat.svelte';
  import Ideas from './views/Ideas.svelte';
  import Docs from './views/Docs.svelte';
  import Todos from './views/Todos.svelte';
  import Projects from './views/Projects.svelte';
  import Settings from './views/Settings.svelte';
  import Rollback from './views/Rollback.svelte';
  import WrapNag from './views/WrapNag.svelte';
  import PluginView from './views/PluginView.svelte';
  import Reorganize from './views/Reorganize.svelte';
  import Tour from './views/Tour.svelte';
  import Home from './views/Home.svelte';
  import Icon from './lib/Icon.svelte';
  import Logo from './lib/Logo.svelte';

  let projects = $state<any[]>([]);
  let showTour = $state(false);
  let searchQ = $state('');

  async function boot() {
    try {
      await connect();
      app.status = await api('/status');
      app.settings = await api('/settings');
      applyTheme();
      if (app.settings.onboarded) await afterOnboard();
      app.ready = true;
    } catch (e: any) {
      app.error = e.message;
    }
  }

  async function afterOnboard() {
    await Promise.all([refreshChats(), refreshWrap(), loadProjects(), loadLayer().catch(() => {})]);
    try {
      const ext = await api('/extensions');
      if (ext.search.suggest) toast('Your library is large — the optional Search extension (Settings → Library) adds semantic search.');
    } catch {}
  }

  async function loadProjects() { projects = await api('/projects'); }

  onMount(() => {
    boot();
    const off = on(async (e) => {
      if (e.type === 'chat.updated' || e.type === 'chat.done') {
        await refreshChats();
        await refreshWrap();
      }
      if (e.type === 'chat.done' && app.settings?.notifications && !document.hasFocus()) notify('Thoughts', `${e.title || 'Chat'} finished`);
      if (e.type === 'notify') { toast(`${e.title}: ${e.body}`); if (app.settings?.notifications && !document.hasFocus()) notify(e.title, e.body); }
      if (e.type === 'layer.changed') loadLayer().catch(() => {});
      if (e.type === 'settings.changed') { app.settings = await api('/settings'); applyTheme(); loadProjects(); }
      if (e.type === 'resync') { refreshChats(); refreshWrap(); }
    });
    // Recovery menu (native, outside the app layer)
    (async () => {
      if ((window as any).__TAURI_INTERNALS__) {
        const { listen } = await import('@tauri-apps/api/event');
        document.documentElement.classList.add('native');
        listen('recovery', () => (app.showRollback = true));
        listen('open-settings', () => go({ kind: 'settings' }));
      }
    })();
    return off;
  });

  async function notify(title: string, body: string) {
    try {
      const n = await import('@tauri-apps/plugin-notification');
      if (!(await n.isPermissionGranted())) await n.requestPermission();
      n.sendNotification({ title, body });
    } catch {}
  }

  async function newChat(projectId: string | null = null) {
    const c = await post('/chats', { projectId });
    await refreshChats();
    go({ kind: 'chat', id: c.id });
  }

  async function startWrap() {
    try {
      const r = await post('/wrap');
      app.showWrapNag = false;
      await refreshChats();
      go({ kind: 'chat', id: r.chatId });
    } catch (e: any) { toast(e.message); }
  }

  async function sync() {
    try {
      const r = await post('/git/sync');
      toast(r.log?.join(' · ') || 'Synced');
    } catch (e: any) { toast(e.message); }
  }

  function onboarded(setupChatId: string | null) {
    afterOnboard();
    showTour = true;
    if (setupChatId) go({ kind: 'chat', id: setupChatId });
  }

  const groups = $derived.by(() => {
    const visible = app.chats.filter((c) => c.kind === 'chat' || c.running || app.view.id === c.id);
    const byProj = new Map<string | null, any[]>();
    for (const c of visible) {
      const k = c.projectId ?? null;
      byProj.set(k, [...(byProj.get(k) || []), c]);
    }
    return byProj;
  });
  const system = $derived(app.chats.filter((c) => c.kind !== 'chat').slice(0, 6));

  const nav = $derived([
    'home', 'chats', 'ideas',
    ...(app.settings?.modules?.todos !== false ? ['todos'] : []),
    ...(app.settings?.modules?.docs !== false ? ['docs'] : []),
    ...(app.settings?.modules?.builds !== false ? ['projects'] : []),
  ] as const);
</script>

{#if app.error}
  <div class="center">
    <div class="card" style="max-width:520px">
      <h1>Thoughts can't reach its background service</h1>
      <p class="muted">{app.error}</p>
      <button class="btn primary" onclick={() => location.reload()}>Retry</button>
    </div>
  </div>
{:else if !app.ready}
  <div class="center muted">…</div>
{:else if !app.settings.onboarded}
  <Onboarding done={onboarded} />
{:else}
  <div class="shell">
    <aside class="sidebar">
      <div class="drag" data-tauri-drag-region></div>
      <div class="brand row"><Logo size={24} /><span class="grow">Thoughts</span>
        {#if app.safeMode}<span class="pill accent" title="Customisations disabled">safe</span>{/if}
      </div>
      <form class="search" onsubmit={(e) => { e.preventDefault(); go({ kind: 'docs', path: '?' + searchQ }); }}>
        <Icon name="search" size={14} />
        <input placeholder={t('search')} bind:value={searchQ} />
      </form>
      <nav>
        {#each nav as k}
          <button class="nav" class:active={app.view.kind === k} onclick={() => go({ kind: k })}><Icon name={k} />{t(k)}</button>
        {/each}
        {#if app.settings.modules?.me}
          <button class="nav" class:active={app.view.kind === 'me'} onclick={() => go({ kind: 'docs', path: 'me/README.md' })}><Icon name="me" />{t('me')}</button>
        {/if}
        {#each app.pluginViews as v}
          <button class="nav" class:active={app.view.kind === 'plugin' && app.view.id === v.id} onclick={() => go({ kind: 'plugin', id: v.id })}><Icon name={v.icon || 'plugin'} />{v.title}</button>
        {/each}
      </nav>

      <div class="chats scroll">
        <div class="row sect"><span class="grow">{t('chats')}</span><button class="iconbtn" title={t('newChat')} onclick={() => newChat()}><Icon name="plus" size={15} /></button></div>
        {#each [...groups.entries()] as [pid, list]}
          {@const proj = projects.find((p) => p.id === pid)}
          <div class="group">{pid ? (proj?.name ?? 'Project') : t('library')}</div>
          {#each list as c (c.id)}
            <button class="chat" class:active={app.view.id === c.id} onclick={() => go({ kind: 'chat', id: c.id })}>
              <span class="dot" class:run={c.running} class:unw={c.unwrapped}></span>
              <span class="grow ell">{c.title}</span>
            </button>
          {/each}
        {/each}
        {#if system.length}
          <div class="group">System</div>
          {#each system as c (c.id)}
            <button class="chat" class:active={app.view.id === c.id} onclick={() => go({ kind: 'chat', id: c.id })}>
              <span class="dot" class:run={c.running}></span><span class="grow ell muted">{c.title}</span>
            </button>
          {/each}
        {/if}
      </div>

      <div class="foot">
        <button class="btn primary wide" onclick={startWrap} disabled={!app.wrap.count}>
          <Icon name="wrap" size={15} /> {t('wrap')} {#if app.wrap.count}<span class="pill">{app.wrap.count} {t('unwrapped')}</span>{/if}
        </button>
        <div class="row tools">
          {#if app.settings.sync?.mode?.startsWith('git')}<button class="iconbtn" onclick={sync} title={t('sync')}><Icon name="sync" /></button>{/if}
          <button class="iconbtn" onclick={() => (app.showRollback = true)} title="{t('rollback')} — restore the app to any earlier version"><Icon name="rollback" /></button>
          <span class="grow"></span>
          <button class="iconbtn" class:on={app.view.kind === 'settings'} onclick={() => go({ kind: 'settings' })} title="{t('settings')} (⌘,)"><Icon name="settings" /></button>
        </div>
      </div>
    </aside>

    <main class="main">
      <div class="drag top" data-tauri-drag-region></div>
      {#key `${app.view.kind}:${app.view.id ?? ''}:${app.view.path ?? ''}`}<div class="view">
      {#if app.view.kind === 'home'}<Home {projects} {newChat} {startWrap} />
      {:else if app.view.kind === 'chat' && app.view.id}
        {#key app.view.id}<Chat id={app.view.id} {projects} />{/key}
      {:else if app.view.kind === 'chats'}
        <div class="page">
          <h1>{t('chats')}</h1>
          <div class="row" style="flex-wrap:wrap">
            <button class="btn primary" onclick={() => newChat()}><Icon name="plus" size={15} />{t('newChat')}</button>
            {#each projects as p}<button class="btn" onclick={() => newChat(p.id)}><Icon name="folder" size={15} />{p.name}</button>{/each}
          </div>
          {#if !app.chats.length}<p class="muted">{t('noChats')}</p>{/if}
          <div class="list">
            {#each app.chats as c (c.id)}
              <button class="card item" onclick={() => go({ kind: 'chat', id: c.id })}>
                <div class="row"><b class="grow ell">{c.title}</b>{#if c.unwrapped}<span class="pill">{t('unwrapped')}</span>{/if}<span class="pill">{c.kind}</span></div>
                <div class="muted sm">{new Date(c.updated).toLocaleString()} · {c.messageCount} msgs</div>
              </button>
            {/each}
          </div>
        </div>
      {:else if app.view.kind === 'ideas'}<Ideas />
      {:else if app.view.kind === 'docs'}{#key app.view.path}<Docs path={app.view.path} />{/key}
      {:else if app.view.kind === 'todos'}<Todos />
      {:else if app.view.kind === 'projects'}<Projects {projects} reload={loadProjects} {newChat} />
      {:else if app.view.kind === 'settings'}<Settings reload={loadProjects} />
      {:else if app.view.kind === 'reorganize'}<Reorganize id={app.view.id} />
      {:else if app.view.kind === 'plugin'}{#key app.view.id}<PluginView id={app.view.id!} />{/key}
      {/if}
      </div>{/key}
    </main>
  </div>
{/if}

{#if app.showRollback}<Rollback />{/if}
{#if app.showWrapNag}<WrapNag {startWrap} />{/if}
{#if showTour}<Tour close={() => (showTour = false)} />{/if}

<div class="toast-wrap">
  {#each app.toasts as x (x.id)}<div class="toast">{x.text}</div>{/each}
</div>

<style>
  .center { height: 100vh; display: grid; place-items: center; padding: 20px; }
  .shell { display: grid; grid-template-columns: var(--sidebar-w) 1fr; height: 100vh; }
  .sidebar {
    background: var(--sidebar-glass); -webkit-backdrop-filter: blur(30px) saturate(180%); backdrop-filter: blur(30px) saturate(180%);
    border-inline-end: 1px solid var(--hairline); display: flex; flex-direction: column; min-height: 0; padding: 0 10px 10px;
  }
  .drag { height: 30px; flex: none; }
  .main { position: relative; overflow: auto; min-width: 0; background: var(--aura), var(--bg); background-repeat: no-repeat; }
  :global(html.native) .sidebar { background: color-mix(in srgb, var(--sidebar) 40%, transparent); -webkit-backdrop-filter: none; backdrop-filter: none; }
  .main .drag.top { position: sticky; top: 0; height: 26px; z-index: 5; }
  .brand { font-weight: 700; font-size: 15px; padding: 2px 6px 12px; letter-spacing: -.01em; gap: 8px; }
  .logo { width: 24px; height: 24px; border-radius: 7px; display: grid; place-items: center; background: var(--accent); color: var(--accent-fg); box-shadow: 0 1px 3px rgba(0,0,0,.2); }
  .search { position: relative; display: flex; align-items: center; color: var(--muted); }
  .search :global(.icon) { position: absolute; inset-inline-start: 9px; pointer-events: none; }
  .search input { padding-inline-start: 30px; }
  .iconbtn { all: unset; cursor: default; width: 30px; height: 28px; display: grid; place-items: center; border-radius: 7px; color: var(--muted); }
  .iconbtn:hover { background: var(--fill); color: var(--fg); }
  .iconbtn.on { background: var(--tint); color: var(--accent); }
  .tools { gap: 2px; }
  .search input { padding-top: 5px; padding-bottom: 5px; font-size: 13px; }
  nav { display: flex; flex-direction: column; gap: 1px; margin: 10px 0; }
  .nav, .chat {
    all: unset; display: flex; align-items: center; gap: 9px; padding: 5px 8px; border-radius: 6px;
    cursor: default; color: var(--fg); font-size: 13px;
  }
  .nav :global(.icon) { flex: none; color: var(--muted); }
  .nav.active :global(.icon) { color: var(--accent); }
  .nav:hover, .chat:hover { background: var(--fill); }
  .nav.active, .chat.active { background: var(--tint); color: var(--accent); font-weight: 600; }
  :global([data-theme="contrast"]) .nav.active, :global([data-theme="contrast"]) .chat.active { background: var(--accent); }
  .chats { flex: 1; min-height: 0; border-top: 1px solid var(--hairline); padding-top: 6px; }
  .sect { font-size: 11px; font-weight: 600; color: var(--muted); padding: 0 6px; }
  .group { font-size: 11px; color: var(--muted); padding: 8px 8px 2px; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--border); flex: none; }
  .dot.unw { background: var(--accent-2); }
  .dot.run { background: var(--accent); animation: pulse 1s infinite alternate; }
  @keyframes pulse { to { opacity: .3; } }
  .foot { display: flex; flex-direction: column; gap: 6px; padding-top: 10px; border-top: 1px solid var(--hairline); }
  .wide { justify-content: center; }
  .sm { font-size: 12px; }
  .btn.sm { padding: 0 6px; }
  .list { display: grid; gap: 8px; margin-top: 16px; }
  .item { all: unset; cursor: pointer; display: block; background: var(--panel); border: 1px solid var(--hairline); border-radius: var(--radius-lg); padding: 12px 14px; box-shadow: var(--shadow); transition: transform .15s, box-shadow .15s; }
  .item:hover { transform: translateY(-1px); box-shadow: var(--shadow-lg); }
</style>
