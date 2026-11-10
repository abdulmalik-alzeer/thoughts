<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { api, connect, on, post } from './lib/api';
  import { app, applyTheme, cachedLook, command, go, refreshChats, refreshWrap, saveSettings, setSidebarCollapsed, toast } from './lib/store.svelte';
  import { loadLayer, layerBooted, retryLayer } from './lib/layer';
  import { t, tn, ago, fmtDate, fmtNum } from './lib/i18n';
  import { matchShortcut, keys, tip } from './lib/shortcuts';
  import { choiceDialog, dialogs, dismissDialog } from './lib/dialog.svelte';
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
  import Help from './views/Help.svelte';
  import Icon from './lib/Icon.svelte';
  import Logo from './lib/Logo.svelte';
  import Dialogs from './lib/Dialogs.svelte';
  import Toasts from './lib/Toasts.svelte';
  import CommandPalette, { type PaletteItem } from './lib/CommandPalette.svelte';

  let projects = $state<any[]>([]);
  let showTour = $state(false);
  let syncing = $state(false);
  let libraryGone = $state(false);
  let dismissedRuns = $state<string[]>([]);
  let ideas = $state<any[]>([]);

  // ---------- boot ----------
  async function boot() {
    try {
      await connect();
      app.status = await api('/status');
      app.settings = await api('/settings');
      applyTheme();
      if (app.settings.onboarded) await afterOnboard();
      app.ready = true;
      // The app is up: a later start no longer needs to suspect the customisations.
      setTimeout(layerBooted, 2500);
    } catch (e: any) {
      // Settings never loaded, so fall back to the language and theme used last time.
      if (!app.settings) { const look = cachedLook(); if (look?.language) app.settings = { ...look, onboarded: false }; }
      app.error = e?.message || String(e);
    }
  }

  async function afterOnboard() {
    await Promise.all([
      refreshChats().catch(() => {}), refreshWrap().catch(() => {}), loadProjects(),
      loadLayer().catch((e) => toast(t('shell.layerFailed'), { kind: 'error', detail: e.message })),
    ]);
    checkLibrary();
    try {
      const ext = await api('/extensions');
      if (ext.search.suggest) toast(t('shell.searchExtHint'), { action: { label: t('settings'), run: () => go({ kind: 'settings' }) } });
    } catch { /* optional */ }
  }

  async function loadProjects() {
    try { projects = await api('/projects'); } catch { /* keeps the previous list; the connection banner explains */ }
  }

  // Is there a library, and is its folder still there?
  async function checkLibrary() {
    if (!app.status?.library || app.status?.libraryMissing) { libraryGone = true; return; }
    try { await api('/library/stats'); libraryGone = false; }
    catch (e: any) { if (/no library|no such file|not found|os error 2|does not exist/i.test(e.message || '')) libraryGone = true; }
  }

  async function refreshStatus() {
    try { app.status = await api('/status'); checkLibrary(); } catch { /* offline */ }
  }

  // Links in rendered markdown (chats, docs, plugins) must never navigate the app window.
  // Safe external links open in the default browser; everything else is dropped.
  function guardLinks(e: MouseEvent) {
    if (e.defaultPrevented) return;
    const a = (e.target as HTMLElement)?.closest?.('a');
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (href.startsWith('#')) return;
    e.preventDefault();
    if (/^(https?:|mailto:)/i.test(href)) {
      import('@tauri-apps/plugin-opener').then((o) => o.openUrl(href)).catch(() => window.open(href, '_blank', 'noopener'));
    }
  }

  onMount(() => {
    boot();
    const off = on(async (e) => {
      try {
        if (e.type === 'connection') {
          const was = app.online;
          app.online = e.online;
          if (e.online && !was && app.ready) toast(t('shell.backOnline'), { kind: 'success', key: 'conn' });
          return;
        }
        if (!app.ready) return;
        if (e.type === 'chat.updated' || e.type === 'chat.done') {
          await refreshChats();
          await refreshWrap();
        }
        if (e.type === 'chat.done' && app.settings?.notifications && !document.hasFocus()) notify('Thoughts', t('shell.chatFinished', { title: e.title || t('shell.aChat') }));
        if (e.type === 'notify') { toast(`${e.title}: ${e.body}`); if (app.settings?.notifications && !document.hasFocus()) notify(e.title, e.body); }
        if (e.type === 'layer.changed') loadLayer().catch((err) => toast(t('shell.layerFailed'), { kind: 'error', detail: err.message }));
        if (e.type === 'settings.changed') { app.settings = await api('/settings'); applyTheme(); loadProjects(); refreshStatus(); }
        if (e.type === 'library.changed') ideas = [];
        if (e.type === 'resync') { refreshChats().catch(() => {}); refreshWrap().catch(() => {}); refreshStatus(); }
      } catch { /* a refresh failed; the connection banner covers the offline case */ }
    });
    // Recovery menu (native, outside the app layer)
    (async () => {
      if ((window as any).__TAURI_INTERNALS__) {
        const { listen } = await import('@tauri-apps/api/event');
        document.documentElement.classList.add('native');
        listen('recovery', () => (app.showRollback = true));
        listen('open-settings', () => command('settings'));
      }
    })();
    window.addEventListener('thoughts:command', onCommand);
    return () => { off(); window.removeEventListener('thoughts:command', onCommand); };
  });

  async function notify(title: string, body: string) {
    try {
      const n = await import('@tauri-apps/plugin-notification');
      if (!(await n.isPermissionGranted())) await n.requestPermission();
      n.sendNotification({ title, body });
    } catch { /* notifications unavailable */ }
  }

  // ---------- actions ----------
  async function newChat(projectId: string | null = null) {
    try {
      const c = await post('/chats', { projectId });
      await refreshChats();
      go({ kind: 'chat', id: c.id });
    } catch (e: any) { toast(t('shell.newChatFailed'), { kind: 'error', detail: e.message }); }
  }

  async function startWrap() {
    if (!app.wrap.count) { app.showWrapNag = false; toast(t('shell.nothingToWrap')); return; }
    try {
      const r = await post('/wrap');
      app.showWrapNag = false;
      await refreshChats();
      go({ kind: 'chat', id: r.chatId });
    } catch (e: any) { toast(t('shell.wrapFailed'), { kind: 'error', detail: e.message }); }
  }

  const syncOn = $derived(!!app.settings?.sync?.mode?.startsWith('git'));

  async function sync() {
    if (syncing) return;
    if (!syncOn) { toast(t('shell.syncOff'), { action: { label: t('settings'), run: () => go({ kind: 'settings' }) } }); return; }
    syncing = true;
    try {
      const r = await post('/git/sync');
      const log: string[] = Array.isArray(r?.log) ? r.log : [];
      toast(t('shell.synced'), { kind: 'success', key: 'sync', detail: log.length ? log.join('\n') : undefined });
    } catch (e: any) {
      const m = /CONFLICT::(.*)/s.exec(e.message || '');
      if (m) await resolveConflict(m[1].split(';').map((f) => f.trim()).filter(Boolean));
      else toast(t('shell.syncFailed'), { kind: 'error', key: 'sync', detail: e.message });
    } finally { syncing = false; }
  }

  // The same file changed here and in the synced copy: let the user pick, in plain words.
  async function resolveConflict(files: string[]) {
    const strategy = await choiceDialog({
      title: t('shell.conflict.title'),
      body: tn('shell.conflict.body', files.length),
      list: files,
      note: t('shell.conflict.backup'),
      choices: [
        { id: 'theirs', label: t('shell.conflict.theirs') },
        { id: 'mine', label: t('shell.conflict.mine'), primary: true },
      ],
    });
    if (!strategy) { toast(t('shell.conflict.cancelled')); return; }
    try {
      const r = await post('/git/resolve', { strategy });
      const log: string[] = Array.isArray(r?.log) ? r.log : [];
      toast(strategy === 'mine' ? t('shell.conflict.keptMine') : t('shell.conflict.usedTheirs'), { kind: 'success', key: 'sync', detail: log.length ? log.join('\n') : undefined });
    } catch (e: any) { toast(t('shell.conflict.failed'), { kind: 'error', key: 'sync', detail: e.message }); }
  }

  async function setLook(patch: { theme?: string; mode?: string }) {
    try { await saveSettings(patch); }
    catch (e: any) { toast(t('shell.themeNotSaved'), { kind: 'error', detail: e.message }); }
  }

  function onboarded(setupChatId: string | null) {
    afterOnboard();
    refreshStatus();
    showTour = true;
    if (setupChatId) go({ kind: 'chat', id: setupChatId });
  }

  // ---------- commands & keyboard ----------
  const actions: Record<string, (d: any) => void> = {
    'chat.new': () => newChat(),
    'chat.open': (d) => d.chatId && go({ kind: 'chat', id: d.chatId }),
    'doc.open': (d) => d.path && go({ kind: 'docs', path: d.path }),
    'wrap.start': () => startWrap(),
    'sync': () => sync(),
    'rollback': () => (app.showRollback = true),
    'settings': () => go({ kind: 'settings' }),
    'help': () => go({ kind: 'help' }),
    'tour': () => (showTour = true),
    'print': () => window.print(),
    'palette': () => openPalette(!app.showPalette),
    'sidebar.toggle': () => setSidebarCollapsed(!app.sidebarCollapsed),
    'search.focus': () => focusSearch(),
  };

  // Every shell action goes through the `thoughts:command` window event, so views and plugins can
  // both trigger one (`command('wrap.start')`) and react to one (`addEventListener('thoughts:command', …)`).
  function onCommand(e: Event) {
    const d = (e as CustomEvent).detail || {};
    const id = String(d.id || '');
    if (!app.ready || !app.settings?.onboarded) return;
    if (id.startsWith('nav.')) {
      const k = id.slice(4);
      if (k === 'me') go({ kind: 'docs', path: 'me/README.md' }); else go({ kind: k });
    } else if (id.startsWith('theme.')) setLook({ theme: id.slice(6) });
    else if (id.startsWith('mode.')) setLook({ mode: id.slice(5) });
    else actions[id]?.(d);
  }

  /** Close whatever is on top. Returns true if something was closed. */
  function closeTop(): boolean {
    if (dismissDialog()) return true;
    if (app.showPalette) { app.showPalette = false; return true; }
    if (showTour) { showTour = false; return true; }
    if (app.showWrapNag) { app.showWrapNag = false; return true; }
    if (app.showRollback) { app.showRollback = false; return true; }
    if (searchOpen) { searchOpen = false; return true; }
    return false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.defaultPrevented) return;
    let id = matchShortcut(e);
    if (!id) return;
    if (id === 'escape') { if (closeTop()) e.preventDefault(); return; }
    if (!app.ready || !app.settings?.onboarded) return;
    const modal = !!dialogs.current || app.showRollback || app.showWrapNag || showTour;
    if (modal || (app.showPalette && id !== 'palette')) return;
    if (id.startsWith('nav.')) {
      const k = nav[+id.slice(4) - 1];
      if (!k) return;
      id = 'nav.' + k;
    }
    e.preventDefault();
    command(id);
  }

  function openPalette(v: boolean) {
    app.showPalette = v;
    if (v && !ideas.length) api('/library/ideas').then((r) => (ideas = Array.isArray(r) ? r : [])).catch(() => {});
  }

  // ---------- sidebar search ----------
  let searchQ = $state('');
  let searchEl = $state<HTMLInputElement | null>(null);
  let searchOpen = $state(false);
  let searching = $state(false);
  let searchFailed = $state(false);
  let fileHits = $state<any[]>([]);
  let sel = $state(-1);
  let searchSeq = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  const chatHits = $derived.by(() => {
    const q = searchQ.trim().toLowerCase();
    return q ? app.chats.filter((c) => String(c.title || '').toLowerCase().includes(q)).slice(0, 6) : [];
  });
  type Hit = { key: string; icon: string; title: string; sub?: string; path?: boolean; open: () => void };
  const hits = $derived<Hit[]>([
    ...chatHits.map((c) => ({ key: 'c:' + c.id, icon: 'chat', title: c.title, open: () => go({ kind: 'chat', id: c.id }) })),
    ...fileHits.map((f) => ({ key: 'f:' + f.path, icon: 'docs', title: f.path, path: true, sub: f.matches?.[0]?.text, open: () => go({ kind: 'docs', path: f.path }) })),
  ]);

  async function focusSearch() {
    if (app.sidebarCollapsed) { setSidebarCollapsed(false); await tick(); }
    searchEl?.focus();
    searchEl?.select();
  }

  function onSearchInput() {
    clearTimeout(searchTimer);
    const q = searchQ.trim();
    sel = -1;
    searchFailed = false;
    const mine = ++searchSeq;
    if (!q) { fileHits = []; searching = false; searchOpen = false; return; }
    searchOpen = true;
    searching = true;
    searchTimer = setTimeout(async () => {
      try {
        const r = await api('/library/search?q=' + encodeURIComponent(q));
        if (mine === searchSeq) fileHits = (Array.isArray(r) ? r : []).slice(0, 8);
      } catch {
        if (mine === searchSeq) { fileHits = []; searchFailed = true; }
      } finally { if (mine === searchSeq) searching = false; }
    }, 250);
  }

  function closeSearch() { searchOpen = false; sel = -1; }

  function pick(h: Hit) { closeSearch(); searchQ = ''; fileHits = []; h.open(); }

  function submitSearch(e: Event) {
    e.preventDefault();
    const q = searchQ.trim();
    if (!q) return; // never run an empty search
    if (sel >= 0 && hits[sel]) { pick(hits[sel]); return; }
    closeSearch();
    go({ kind: 'docs', path: '?' + q });
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.isComposing) return;
    if (e.key === 'ArrowDown' && hits.length) { e.preventDefault(); searchOpen = true; sel = (sel + 1) % hits.length; }
    else if (e.key === 'ArrowUp' && hits.length) { e.preventDefault(); sel = sel <= 0 ? hits.length - 1 : sel - 1; }
    else if (e.key === 'Escape') {
      e.stopPropagation();
      if (searchOpen) closeSearch(); else if (searchQ) { searchQ = ''; fileHits = []; } else searchEl?.blur();
    }
  }

  // ---------- derived lists ----------
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
  ]);
  const meOpen = $derived(app.view.kind === 'docs' && (app.view.path ?? '').startsWith('me/'));
  const isActive = (k: string) => app.view.kind === k && !(k === 'docs' && meOpen);

  const interrupted = $derived(((app.status?.interrupted ?? []) as { chatId: string; title: string }[]).filter((r) => !dismissedRuns.includes(r.chatId)).slice(0, 3));

  function kindLabel(k: string) { const key = 'shell.kind.' + k; const s = t(key); return s === key ? k : s; }
  function dotLabel(c: any) { return c.running ? t('shell.dot.running') : c.unwrapped ? t('shell.dot.unwrapped') : ''; }

  const paletteItems = $derived.by((): PaletteItem[] => {
    const g = { go: t('shell.pal.goto'), act: t('shell.pal.actions'), look: t('shell.pal.look'), chat: t('shell.pal.chat'), idea: t('shell.pal.idea'), plug: t('shell.pal.plugin') };
    const items: PaletteItem[] = [];
    const add = (id: string, title: string, group: string, icon: string, extra: Partial<PaletteItem> = {}) =>
      items.push({ id, title, group, icon, keys: keys(id) || undefined, run: () => command(id), ...extra });
    add('chat.new', t('newChat'), g.act, 'plus');
    nav.forEach((k, i) => add('nav.' + k, t(k), g.go, k, { keys: keys('nav.' + (i + 1)) || undefined }));
    if (app.settings?.modules?.me) add('nav.me', t('me'), g.go, 'me');
    for (const v of app.pluginViews) items.push({ id: 'view:' + v.id, title: v.title, group: g.go, icon: v.icon || 'plugin', run: () => go({ kind: 'plugin', id: v.id }) });
    add('settings', t('settings'), g.go, 'settings');
    add('help', t('shell.help'), g.go, 'help', { keywords: 'help shortcuts privacy cost مساعدة اختصارات خصوصية' });
    add('wrap.start', t('shell.pal.wrap'), g.act, 'wrap', { keywords: 'wrap save حفظ تلخيص' });
    if (syncOn) add('sync', t('shell.pal.sync'), g.act, 'sync', { keywords: 'sync backup مزامنة' });
    add('rollback', t('shell.rb.title'), g.act, 'rollback', { keywords: 'rollback undo restore استعادة' });
    add('search.focus', t('shell.pal.search'), g.act, 'search');
    add('sidebar.toggle', app.sidebarCollapsed ? t('shell.sidebarShow') : t('shell.sidebarHide'), g.act, 'sidebar');
    add('tour', t('shell.pal.tour'), g.act, 'book');
    add('print', t('shell.pal.print'), g.act, 'print');
    for (const [id, name] of [['saudi', t('saudi')], ['contrast', t('contrast')], ['tech', t('tech')], ...app.customThemes.map((c) => [c.id, c.name])])
      add('theme.' + id, t('shell.pal.theme', { name }), g.look, 'palette', { keywords: 'theme سمة' });
    for (const [m, icon] of [['light', 'sun'], ['dark', 'moon'], ['system', 'system']])
      add('mode.' + m, t('shell.pal.mode', { name: t(m) }), g.look, icon, { keywords: 'appearance mode day night مظهر' });
    for (const c of app.commands) items.push({ id: 'plugin:' + c.id, title: c.title, group: g.plug, icon: c.icon || 'plugin', run: () => { command('plugin:' + c.id); c.run(); } });
    for (const c of app.chats.slice(0, 40)) items.push({ id: 'chat:' + c.id, title: c.title, group: g.chat, icon: 'chat', run: () => command('chat.open', { chatId: c.id }) });
    for (const i of ideas) items.push({ id: 'idea:' + i.path, title: i.title, group: g.idea, icon: 'ideas', keywords: i.summary, run: () => command('doc.open', { path: i.path }) });
    return items;
  });
</script>

<svelte:window onclick={guardLinks} onkeydown={onKey} />
<Dialogs />

{#if app.error}
  <div class="center">
    <div class="card down" role="alert">
      <Logo size={44} />
      <h1>{t('shell.down.title')}</h1>
      <p>{t('shell.down.body')}</p>
      <p class="muted sm"><bdi>{app.error}</bdi></p>
      <button class="btn primary" onclick={() => location.reload()}><Icon name="retry" size={15} /> {t('shell.retry')}</button>
    </div>
  </div>
{:else if !app.ready}
  <div class="center" role="status" aria-busy="true">
    <div class="boot">
      <div class="pulse"><Logo size={56} /></div>
      <span class="sr-only">{t('shell.loading')}</span>
      <div class="skel" style="width:180px;height:12px"></div>
      <div class="skel" style="width:120px;height:12px"></div>
    </div>
  </div>
{:else if !app.settings.onboarded}
  <Onboarding done={onboarded} />
{:else}
  <div class="shell" class:collapsed={app.sidebarCollapsed}>
    {#if !app.sidebarCollapsed}
    <aside class="sidebar no-print" aria-label={t('shell.sidebar')}>
      <div class="drag" data-tauri-drag-region></div>
      <div class="brand row"><Logo size={24} /><span class="grow">Thoughts</span>
        {#if app.safeMode}<span class="pill accent" title={t('shell.safe.pillTip')}>{t('shell.safe.pill')}</span>{/if}
        <button class="iconbtn" aria-label={t('shell.sidebarHide')} title={tip(t('shell.sidebarHide'), 'sidebar.toggle')} onclick={() => command('sidebar.toggle')}><Icon name="sidebar" class="flip-rtl" /></button>
      </div>
      <form class="search" role="search" onsubmit={submitSearch}
        onfocusout={(e) => { if (!(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node)) closeSearch(); }}>
        <Icon name="search" size={14} />
        <input bind:this={searchEl} bind:value={searchQ} oninput={onSearchInput} onkeydown={onSearchKey}
          onfocus={() => { if (searchQ.trim()) searchOpen = true; }}
          placeholder={t('search')} aria-label={t('shell.searchLabel')} title={tip(t('shell.searchLabel'), 'search.focus')} dir="auto"
          role="combobox" aria-expanded={searchOpen} aria-controls="side-hits" aria-autocomplete="list"
          aria-activedescendant={sel >= 0 ? 'side-hit-' + sel : undefined} autocomplete="off" />
        {#if searching}<span class="spin" role="status" aria-label={t('shell.searching')}></span>{/if}
        {#if searchOpen}
          <div class="hits" id="side-hits" role="listbox" aria-label={t('shell.searchResults')}>
            {#each hits as h, i (h.key)}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div class="hit" id={'side-hit-' + i} role="option" tabindex="-1" aria-selected={i === sel}
                onmousedown={(e) => e.preventDefault()} onclick={() => pick(h)} onmousemove={() => (sel = i)}>
                <Icon name={h.icon} size={14} />
                <span class="grow">
                  {#if h.path}<bdi class="ell blk">{h.title}</bdi>{:else}<span class="ell blk" dir="auto">{h.title}</span>{/if}
                  {#if h.sub}<span class="ell blk muted sm" dir="auto">{h.sub}</span>{/if}
                </span>
              </div>
            {/each}
            {#if searching && !hits.length}
              <p class="muted note">{t('shell.searching')}</p>
            {:else if searchFailed}
              <p class="muted note" role="alert">{t('shell.searchFailed')}</p>
            {:else if !hits.length}
              <p class="muted note">{t('shell.searchNone')}</p>
            {/if}
            {#if !searching && fileHits.length}
              <button type="submit" class="all" onmousedown={(e) => e.preventDefault()} onclick={() => (sel = -1)}>{t('shell.searchAll')} <Icon name="arrow" size={13} class="flip-rtl" /></button>
            {/if}
          </div>
        {/if}
      </form>
      <nav aria-label={t('shell.mainNav')}>
        {#each nav as k, i}
          <button class="nav" class:active={isActive(k)} aria-current={isActive(k) ? 'page' : undefined} title={tip(t(k), 'nav.' + (i + 1))} onclick={() => command('nav.' + k)}><Icon name={k} />{t(k)}</button>
        {/each}
        {#if app.settings.modules?.me}
          <button class="nav" class:active={meOpen} aria-current={meOpen ? 'page' : undefined} onclick={() => command('nav.me')}><Icon name="me" />{t('me')}</button>
        {/if}
        {#each app.pluginViews as v (v.id)}
          {@const cur = app.view.kind === 'plugin' && app.view.id === v.id}
          <button class="nav" class:active={cur} aria-current={cur ? 'page' : undefined} onclick={() => go({ kind: 'plugin', id: v.id })}><Icon name={v.icon || 'plugin'} /><span dir="auto">{v.title}</span></button>
        {/each}
      </nav>

      {#if interrupted.length}
        <div class="notice" role="status">
          <Icon name="warning" size={15} />
          <div class="grow">
            <div>{tn('shell.interrupted', interrupted.length)}</div>
            {#each interrupted as r (r.chatId)}
              <div class="row run">
                <button class="link grow ell" onclick={() => go({ kind: 'chat', id: r.chatId })}><bdi>{r.title || t('shell.aChat')}</bdi></button>
                <button class="iconbtn sm" aria-label={t('shell.dismissNamed', { name: r.title || t('shell.aChat') })} title={t('shell.dismiss')} onclick={() => (dismissedRuns = [...dismissedRuns, r.chatId])}><Icon name="close" size={13} /></button>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <div class="chats scroll">
        <div class="row sect"><h2 class="grow">{t('chats')}</h2><button class="iconbtn" aria-label={t('newChat')} title={tip(t('newChat'), 'chat.new')} onclick={() => command('chat.new')}><Icon name="plus" size={15} /></button></div>
        {#if !app.chats.length}<p class="muted empty">{t('noChats')}</p>{/if}
        {#each [...groups.entries()] as [pid, list]}
          {@const proj = projects.find((p) => p.id === pid)}
          <div class="group" dir="auto">{pid ? (proj?.name ?? t('shell.project')) : t('library')}</div>
          {#each list as c (c.id)}
            <button class="chat" class:active={app.view.id === c.id} aria-current={app.view.id === c.id ? 'page' : undefined} title={c.title} onclick={() => go({ kind: 'chat', id: c.id })}>
              <span class="dot" class:run={c.running} class:unw={c.unwrapped} title={dotLabel(c) || undefined}></span>
              <span class="grow ell"><bdi>{c.title}</bdi></span>
              {#if dotLabel(c)}<span class="sr-only">({dotLabel(c)})</span>{/if}
            </button>
          {/each}
        {/each}
        {#if system.length}
          <div class="group">{t('shell.system')}</div>
          {#each system as c (c.id)}
            <button class="chat" class:active={app.view.id === c.id} aria-current={app.view.id === c.id ? 'page' : undefined} title={c.title} onclick={() => go({ kind: 'chat', id: c.id })}>
              <span class="dot" class:run={c.running} title={c.running ? t('shell.dot.running') : undefined}></span><span class="grow ell muted"><bdi>{c.title}</bdi></span>
              {#if c.running}<span class="sr-only">({t('shell.dot.running')})</span>{/if}
            </button>
          {/each}
        {/if}
      </div>

      <div class="foot">
        <button class="btn primary wide" onclick={() => command('wrap.start')} disabled={!app.wrap.count}
          title={app.wrap.count ? tip(tn('shell.chatsToWrap', app.wrap.count), 'wrap.start') : t('shell.nothingToWrap')}>
          <Icon name="wrap" size={15} /> {t('wrap')}
          {#if app.wrap.count}<span class="pill" aria-hidden="true">{fmtNum(app.wrap.count)}</span><span class="sr-only">— {tn('shell.chatsToWrap', app.wrap.count)}</span>{/if}
        </button>
        <div class="row tools">
          {#if syncOn}<button class="iconbtn" class:busy={syncing} disabled={syncing} onclick={() => command('sync')} aria-label={syncing ? t('shell.syncing') : t('shell.syncTip')} title={syncing ? t('shell.syncing') : t('shell.syncTip')}><Icon name="sync" /></button>{/if}
          <button class="iconbtn" onclick={() => command('rollback')} aria-label={t('shell.rb.title')} title={t('shell.rollbackTip')}><Icon name="rollback" /></button>
          <button class="iconbtn" class:on={app.view.kind === 'help'} onclick={() => command('help')} aria-label={t('shell.help')} title={t('shell.help')} aria-current={app.view.kind === 'help' ? 'page' : undefined}><Icon name="help" /></button>
          <button class="iconbtn" onclick={() => command('palette')} aria-label={t('shell.palette')} title={tip(t('shell.palette'), 'palette')}><Icon name="command" /></button>
          <span class="grow"></span>
          <button class="iconbtn" class:on={app.view.kind === 'settings'} onclick={() => command('settings')} aria-label={t('settings')} title={tip(t('settings'), 'settings')} aria-current={app.view.kind === 'settings' ? 'page' : undefined}><Icon name="settings" /></button>
        </div>
      </div>
    </aside>
    {/if}

    <main class="main">
      <div class="drag top no-print" data-tauri-drag-region>
        {#if app.sidebarCollapsed}
          <button class="iconbtn expand" aria-label={t('shell.sidebarShow')} title={tip(t('shell.sidebarShow'), 'sidebar.toggle')} onclick={() => command('sidebar.toggle')}><Icon name="sidebar" class="flip-rtl" /></button>
        {/if}
      </div>
      {#if !app.online}
        <div class="banner no-print" role="status"><Icon name="offline" size={15} /> {t('shell.reconnecting')}</div>
      {/if}
      {#if app.safeMode}
        <div class="safe no-print" role={app.safeReason === 'crash' ? 'alert' : 'status'}>
          <Icon name="shield" size={16} />
          <span class="grow">{app.safeReason === 'crash' ? t('shell.safe.crash') : t('shell.safe.manual')}</span>
          {#if app.safeReason === 'crash'}<button class="btn" onclick={() => command('rollback')}><Icon name="rollback" size={14} /> {t('shell.openRollback')}</button>{/if}
          <button class="btn" onclick={retryLayer}><Icon name="retry" size={14} /> {app.safeReason === 'crash' ? t('shell.safe.retry') : t('shell.safe.leave')}</button>
        </div>
      {/if}
      {#key `${app.view.kind}:${app.view.id ?? ''}:${app.view.path ?? ''}`}<div class="view">
      {#if libraryGone && app.view.kind !== 'settings' && app.view.kind !== 'help'}
        <div class="page missing">
          <div class="card" role="alert">
            <Icon name="folder" size={28} />
            <h1>{t('shell.lib.title')}</h1>
            <p>{t('shell.lib.body')}</p>
            {#if app.status?.library}<p class="muted sm">{t('shell.lib.where')} <bdi>{app.status.library}</bdi></p>{/if}
            <div class="row">
              <button class="btn primary" onclick={() => command('settings')}><Icon name="folder" size={15} /> {t('shell.lib.choose')}</button>
              <button class="btn" onclick={refreshStatus}><Icon name="retry" size={15} /> {t('shell.lib.recheck')}</button>
            </div>
          </div>
        </div>
      {:else if app.view.kind === 'home'}<Home {projects} {newChat} {startWrap} />
      {:else if app.view.kind === 'chat' && app.view.id}
        {#key app.view.id}<Chat id={app.view.id} {projects} />{/key}
      {:else if app.view.kind === 'chats'}
        <div class="page">
          <h1>{t('chats')}</h1>
          <div class="row" style="flex-wrap:wrap">
            <button class="btn primary" title={tip(t('newChat'), 'chat.new')} onclick={() => newChat()}><Icon name="plus" size={15} />{t('newChat')}</button>
            {#each projects as p (p.id)}<button class="btn" title={t('shell.newChatIn', { name: p.name })} onclick={() => newChat(p.id)}><Icon name="folder" size={15} /><span dir="auto">{p.name}</span></button>{/each}
          </div>
          {#if !app.chats.length}<p class="muted">{t('noChats')}</p>{/if}
          <div class="list">
            {#each app.chats as c (c.id)}
              <button class="card item" onclick={() => go({ kind: 'chat', id: c.id })}>
                <div class="row"><b class="grow ell" dir="auto">{c.title}</b>{#if c.running}<span class="pill accent">{t('shell.dot.running')}</span>{/if}{#if c.unwrapped}<span class="pill">{t('shell.dot.unwrapped')}</span>{/if}{#if c.kind !== 'chat'}<span class="pill">{kindLabel(c.kind)}</span>{/if}</div>
                <div class="muted sm"><span title={fmtDate(c.updated, { dateStyle: 'medium', timeStyle: 'short' })}>{ago(c.updated)}</span> · {tn('shell.msgs', c.messageCount ?? 0)}</div>
              </button>
            {/each}
          </div>
        </div>
      {:else if app.view.kind === 'ideas'}<Ideas />
      {:else if app.view.kind === 'docs'}{#key app.view.path}<Docs path={app.view.path} />{/key}
      {:else if app.view.kind === 'todos'}<Todos />
      {:else if app.view.kind === 'projects'}<Projects {projects} reload={loadProjects} {newChat} />
      {:else if app.view.kind === 'settings'}<Settings reload={loadProjects} />
      {:else if app.view.kind === 'help'}<Help />
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
{#if app.showPalette}<CommandPalette items={paletteItems} close={() => (app.showPalette = false)} />{/if}

<Toasts />

<style>
  .center { height: 100vh; display: grid; place-items: center; padding: 20px; }
  .down { max-width: 520px; display: grid; gap: 10px; justify-items: start; }
  .down h1, .down p { margin: 0; }
  .boot { display: grid; gap: 12px; justify-items: center; }
  .pulse { animation: breathe 1.6s ease-in-out infinite alternate; margin-bottom: 6px; }
  @keyframes breathe { to { opacity: .55; transform: scale(.96); } }
  .shell { display: grid; grid-template-columns: var(--sidebar-w) 1fr; height: 100vh; }
  .shell.collapsed { grid-template-columns: 1fr; }
  .sidebar {
    background: var(--sidebar-glass); -webkit-backdrop-filter: blur(30px) saturate(180%); backdrop-filter: blur(30px) saturate(180%);
    border-inline-end: 1px solid var(--hairline); display: flex; flex-direction: column; min-height: 0; padding: 0 10px 10px;
  }
  .drag { height: 30px; flex: none; }
  .main { position: relative; overflow: auto; min-width: 0; background: var(--aura), var(--bg); background-repeat: no-repeat; }
  :global(html.native) .sidebar { background: color-mix(in srgb, var(--sidebar) 40%, transparent); -webkit-backdrop-filter: none; backdrop-filter: none; }
  .main .drag.top { position: sticky; top: 0; height: 26px; z-index: 5; }
  .expand { position: absolute; top: 2px; inset-inline-start: 10px; background: var(--glass); }
  :global(html.native[dir="ltr"]) .expand { inset-inline-start: 78px; }
  .brand { font-weight: 700; font-size: 15px; padding: 2px 6px 12px; letter-spacing: -.01em; gap: 8px; }
  .brand .iconbtn { margin-inline-end: -4px; }

  .search { position: relative; display: flex; align-items: center; color: var(--muted); }
  .search > :global(.icon) { position: absolute; inset-inline-start: 9px; pointer-events: none; }
  .search input { padding-inline-start: 30px; padding-block: 5px; font-size: 13px; }
  .spin { position: absolute; inset-inline-end: 9px; width: 12px; height: 12px; border-radius: 50%; border: 2px solid var(--fill-2); border-top-color: var(--accent); animation: spin .7s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .hits {
    position: absolute; top: calc(100% + 6px); inset-inline: 0; z-index: 20; padding: 5px; color: var(--fg);
    background: var(--panel); border: var(--border-w) solid var(--hairline); border-radius: var(--radius-lg); box-shadow: var(--shadow-lg);
    max-height: 60vh; overflow: auto;
  }
  .hit { display: flex; gap: 8px; align-items: flex-start; padding: 6px 8px; border-radius: 6px; font-size: 13px; cursor: pointer; }
  .hit :global(.icon) { flex: none; margin-top: 3px; color: var(--muted); }
  .hit[aria-selected="true"] { background: var(--tint); }
  :global([data-theme="contrast"]) .hit[aria-selected="true"] { background: var(--accent); color: var(--accent-fg); }
  :global([data-theme="contrast"]) .hit[aria-selected="true"] :global(*) { color: inherit; }
  .blk { display: block; }
  .note { margin: 0; padding: 8px; font-size: 13px; }
  .all { font: inherit; font-size: 13px; color: var(--accent); background: none; border: 0; width: 100%; text-align: start; padding: 7px 8px; border-radius: 6px; cursor: pointer; display: flex; align-items: center; gap: 4px; border-top: 1px solid var(--hairline); margin-top: 4px; }
  .all:hover { background: var(--fill); }

  .iconbtn { font: inherit; background: none; border: 0; padding: 0; cursor: pointer; width: 30px; height: 28px; display: grid; place-items: center; border-radius: 7px; color: var(--muted); flex: none; }
  .iconbtn:hover { background: var(--fill); color: var(--fg); }
  .iconbtn.on { background: var(--tint); color: var(--accent); }
  :global([data-theme="contrast"]) .iconbtn.on { color: var(--accent-fg); }
  .iconbtn:disabled { opacity: .6; cursor: default; }
  .iconbtn.busy :global(.icon) { animation: spin 1s linear infinite; }
  .iconbtn.sm { width: 22px; height: 22px; }
  .tools { gap: 2px; }
  nav { display: flex; flex-direction: column; gap: 1px; margin: 10px 0; }
  .nav, .chat {
    font: inherit; background: none; border: 0; text-align: start; width: 100%;
    display: flex; align-items: center; gap: 9px; padding: 5px 8px; border-radius: 6px;
    cursor: pointer; color: var(--fg); font-size: 13px;
  }
  .nav :global(.icon) { flex: none; color: var(--muted); }
  .nav.active :global(.icon) { color: var(--accent); }
  .nav:hover, .chat:hover { background: var(--fill); }
  .nav.active, .chat.active { background: var(--tint); color: var(--accent); font-weight: 600; }
  :global([data-theme="contrast"]) .nav.active, :global([data-theme="contrast"]) .chat.active { background: var(--accent); }
  :global([data-theme="contrast"]) .nav.active :global(.icon) { color: var(--accent-fg); }
  .nav:focus-visible, .chat:focus-visible, .iconbtn:focus-visible { outline-offset: -2px; }

  .notice { display: flex; gap: 8px; align-items: flex-start; font-size: 12px; padding: 8px; margin-bottom: 8px; border-radius: var(--radius); background: var(--fill); border: var(--border-w) solid var(--hairline); }
  .notice > :global(.icon) { flex: none; margin-top: 2px; color: var(--accent-2); }
  .run { gap: 2px; }
  .link { font: inherit; font-weight: 600; color: var(--accent); background: none; border: 0; padding: 2px 0; text-align: start; cursor: pointer; text-decoration: underline; text-underline-offset: 3px; }

  .chats { flex: 1; min-height: 0; border-top: 1px solid var(--hairline); padding-top: 6px; }
  .sect { padding: 0 6px; color: var(--muted); }
  .sect h2 { font-size: 12px; font-weight: 600; margin: 0; letter-spacing: 0; }
  .group { font-size: 12px; color: var(--muted); padding: 8px 8px 2px; }
  .empty { font-size: 12px; padding: 6px 8px; margin: 0; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--border); flex: none; }
  .dot.unw { background: var(--accent-2); }
  .dot.run { background: var(--accent); animation: pulse 1s infinite alternate; }
  :global([data-theme="contrast"]) .chat.active .dot { background: var(--accent-fg); }
  @keyframes pulse { to { opacity: .3; } }
  .foot { display: flex; flex-direction: column; gap: 6px; padding-top: 10px; border-top: 1px solid var(--hairline); }
  .wide { justify-content: center; }
  .sm { font-size: 12px; }

  .banner {
    position: sticky; top: 30px; z-index: 6; width: fit-content; margin: 0 auto; display: flex; align-items: center; gap: 8px;
    font-size: 13px; padding: 5px 14px; border-radius: 99px; background: var(--panel); color: var(--fg);
    border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow);
  }
  .banner :global(.icon) { color: var(--accent-2); }
  .safe { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin: 4px 28px 0; padding: 10px 14px; border-radius: var(--radius-lg); background: var(--panel); border: var(--border-w) solid var(--accent-2); font-size: 13px; }
  .safe > :global(.icon) { flex: none; color: var(--accent-2); }
  .safe .grow { min-width: 220px; }
  .missing { display: grid; place-items: center; min-height: 70vh; max-width: none; }
  .missing .card { max-width: 520px; display: grid; gap: 10px; justify-items: start; padding: 24px; }
  .missing h1, .missing p { margin: 0; }
  .list { display: grid; gap: 8px; margin-top: 16px; }
  .item { font: inherit; color: inherit; text-align: start; width: 100%; cursor: pointer; display: block; background: var(--panel); border: 1px solid var(--hairline); border-radius: var(--radius-lg); padding: 12px 14px; box-shadow: var(--shadow); transition: transform .15s, box-shadow .15s; }
  .item:hover { transform: translateY(-1px); box-shadow: var(--shadow-lg); }
</style>
