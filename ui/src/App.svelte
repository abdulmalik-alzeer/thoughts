<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { api, connect, daemonStale, del, on, post, reconnect, reloadApp } from './lib/api';
  import { a11y, app, applyTheme, cachedLook, command, go, goBack, goForward, refreshChats, refreshWrap, saveSettings, setSidebarCollapsed, toast, track, versionLabel, zoomStep, APP_VERSION } from './lib/store.svelte';
  import { recheckSignIn, signIn } from './lib/auth';
  import { loadLayer, retryLayer, checkPending, openReview, addonTag, safeCrashText, viewLabel, keyFromAddOn, keyMayComeFromAddOn } from './lib/layer';
  import { failed } from './lib/errors';
  import { backup, healthWarning, lastBackupText, refreshBackup, syncDone, syncFailed } from './lib/backup.svelte';
  import { navLabel } from './lib/terms';
  import { startReminders } from './lib/reminders';
  import { fold } from './lib/CommandPalette.svelte';
  import { checkAppUpdate, checkLayerUpdates } from './lib/updates';
  import { announceWhatsNew, markWhatsNewSeen } from './lib/whatsnew';
  import { startupChecks } from './lib/maintenance';
  import { t, tn, ago, fmtDate, fmtNum } from './lib/i18n';
  import { matchShortcut, keys, tip, isMac, isTyping } from './lib/shortcuts';
  import { confirmDialog, dialogs, dismissDialog } from './lib/dialog.svelte';
  import Onboarding from './views/Onboarding.svelte';
  import LibrarySwitcher from './lib/LibrarySwitcher.svelte';
  import LibraryAdd from './lib/LibraryAdd.svelte';
  import { libraryPaletteItems, startLibraries } from './lib/libraries.svelte';
  import Chat from './views/Chat.svelte';
  import ChatList from './views/ChatList.svelte';
  import Ideas from './views/Ideas.svelte';
  import Item from './views/Item.svelte';
  import { itemOf, openItem } from './lib/item';
  import Docs from './views/Docs.svelte';
  import Todos from './views/Todos.svelte';
  import Inbox from './views/Inbox.svelte';
  import { inbox, watchInbox } from './lib/inbox.svelte';
  import Projects from './views/Projects.svelte';
  import Settings from './views/Settings.svelte';
  import Rollback from './views/Rollback.svelte';
  import Review from './views/Review.svelte';
  import WrapNag from './views/WrapNag.svelte';
  import WrapWaiting from './views/WrapWaiting.svelte';
  import UsageBanner from './views/UsageBanner.svelte';
  import PluginView from './views/PluginView.svelte';
  import Reorganize from './views/Reorganize.svelte';
  import Tour from './views/Tour.svelte';
  import Home from './views/Home.svelte';
  import Help from './views/Help.svelte';
  import History from './views/History.svelte';
  import Customise from './views/Customise.svelte';
  import PreviewBanner from './views/PreviewBanner.svelte';
  import SetupWaiting from './views/SetupWaiting.svelte';
  import Icon from './lib/Icon.svelte';
  import Logo from './lib/Logo.svelte';
  import Dialogs from './lib/Dialogs.svelte';
  import SyncConflict from './lib/SyncConflict.svelte';
  import { settleClash } from './lib/conflict.svelte';
  import Toasts from './lib/Toasts.svelte';
  import VoiceBar from './lib/voice/VoiceBar.svelte';
  import RecentMessages from './lib/RecentMessages.svelte';
  import { crashText, reportError } from './lib/crash';
  import CommandPalette, { type PaletteItem } from './lib/CommandPalette.svelte';
  import { workflowNotice } from './lib/automatic';
  import Marked from './lib/Marked.svelte';
  import { findOnOpen, findWords, markTerms, openFind, searchAll } from './lib/search';
  import { prettyName } from './lib/content/util';
  import { watchDeepLinks } from './lib/deeplink';

  let projects = $state<any[]>([]);
  let showTour = $state(false);
  let syncing = $state(false);
  let libraryGone = $state(false);
  let dismissedRuns = $state<string[]>([]);
  let ideas = $state<any[]>([]);
  let stale = $state(daemonStale());
  let showRecent = $state(false);
  function openRecent() { showRecent = true; app.toastLogUnread = false; }

  // A view that throws while it renders is replaced by a card with a way out, and the error is logged.
  let crashCopied = $state(false);
  function viewFailed(err: unknown) { reportError('view:' + app.view.kind, err); }
  async function copyCrash(err: unknown) {
    try { await navigator.clipboard.writeText(`Thoughts ${versionLabel()} · ${app.view.kind}\n${crashText(err)}`); crashCopied = true; setTimeout(() => (crashCopied = false), 2000); }
    catch { crashCopied = false; }
  }

  // ---------- boot ----------
  async function boot() {
    try {
      await connect();
      app.status = await api('/status');
      app.settings = await api('/settings');
      applyTheme();
      if (app.settings.onboarded) await afterOnboard();
      app.ready = true;
      statusNotices();
      if (app.settings.onboarded) { checkAppUpdate(false); checkLayerUpdates(); announceWhatsNew(); startupChecks(); } else markWhatsNewSeen(); // a fresh install has nothing new to announce
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
    startLibraries();
    startReminders();
    // A change staged earlier (or by an import / app update) is still waiting for a decision.
    checkPending(true);
  }

  async function loadProjects() {
    try { projects = await api('/projects'); } catch { /* keeps the previous list; the connection banner explains */ }
  }

  // Is there a library, and is its folder still there?
  async function checkLibrary() {
    // During first-run setup there is no library yet, and nothing is missing.
    if (!app.settings?.onboarded) { libraryGone = false; return; }
    if (!app.status?.library || app.status?.libraryMissing) { libraryGone = true; return; }
    if (app.status.libraryMissing === false) { libraryGone = false; return; }
    // Older background service without `libraryMissing`: work it out from the error text.
    try { await api('/library/stats'); libraryGone = false; }
    catch (e: any) { if (/no library|no such file|not found|os error 2|does not exist/i.test(e.message || '')) libraryGone = true; }
  }

  let statusAt = Date.now();
  async function refreshStatus() {
    statusAt = Date.now();
    try { app.status = await api('/status'); checkLibrary(); } catch { /* offline */ }
  }

  // ---------- sign-in to Claude ----------
  // Known up front, not when a message fails: the status says so at start, the service says so the
  // moment a run is refused (`auth.changed`), and coming back to the window looks again (the user
  // may have signed in or out in a terminal meanwhile).
  // The program chosen in Settings › Assistant is the one that has to be signed in (Claude Code or Codex).
  const onCodex = $derived(app.settings?.ai?.primary === 'codex');
  const primaryStatus = $derived<any>(onCodex ? app.status?.codex : app.status?.claude);
  const signedOut = $derived(!!app.settings?.onboarded && primaryStatus?.installed === true && primaryStatus.loggedIn === false && (!onCodex || primaryStatus.known !== false));
  let authBusy = $state(false);
  async function authChanged() {
    const was = signedOut;
    await refreshStatus();
    if (was && !signedOut) toast(t(onCodex ? 'codex.auth.signedIn' : 'auth.signedIn'), { kind: 'success', key: 'auth' });
  }
  async function checkSignIn() {
    if (authBusy) return;
    authBusy = true;
    try { toast(t((await recheckSignIn(onCodex ? 'codex' : 'claude')) ? (onCodex ? 'codex.auth.signedIn' : 'auth.signedIn') : 'auth.stillOut'), { key: 'auth' }); }
    catch (e: any) { toast(t('set.failed'), { kind: 'error', key: 'auth', detail: e?.message }); }
    finally { authBusy = false; }
  }
  function onFocus() { if (app.ready && app.online && Date.now() - statusAt > 60_000) refreshStatus(); }

  // At a high zoom (or in a small window) the full sidebar would leave the view little room: below
  // 720 CSS pixels the shell shows the icon rail, until the user opens the sidebar there themselves.
  let narrow = $state(false);
  let openedNarrow = $state(false);
  function measure() {
    const z = parseFloat((document.documentElement.style as any).zoom) || 1; // CSS zoom (browser preview) does not shrink innerWidth
    const n = window.innerWidth / z < 720;
    if (n !== narrow) { narrow = n; openedNarrow = false; }
  }
  $effect(() => { a11y().zoom; const h = setTimeout(measure, 150); return () => clearTimeout(h); });
  const railed = $derived(app.sidebarCollapsed || (narrow && !openedNarrow));
  function toggleSidebar() {
    if (!narrow) { setSidebarCollapsed(!app.sidebarCollapsed); return; }
    if (railed) { openedNarrow = true; if (app.sidebarCollapsed) setSidebarCollapsed(false); } else openedNarrow = false;
  }

  async function zoom(dir: -1 | 0 | 1) {
    try { toast(t('a11y.zoomNow', { n: await zoomStep(dir) }), { key: 'zoom', duration: 1500 }); }
    catch (e: any) { toast(t('a11y.saveFailed'), { kind: 'error', key: 'zoom', detail: e?.message }); }
  }

  // Things the background service had to repair on start: say so once, in plain words.
  let noticed = false;
  function statusNotices() {
    if (noticed || !app.status) return;
    noticed = true;
    if (app.status.settingsRecovered) { toast(t('layer.settingsRecovered'), { kind: 'error', key: 'recovered', duration: 0 }); post('/status/ack-recovered').catch(() => {}); }
    if (app.status.settingsNewer) toast(t('ui.settingsNewer'), { kind: 'error', key: 'newer-settings', duration: 0 });
    const n = Number(app.status.unreadableChats) || 0;
    if (n > 0) toast(tn('layer.unreadableChats', n), { kind: 'error', key: 'unreadable', duration: 0 });
  }

  const layerError = (err: any) => toast(t('shell.layerFailed'), { kind: 'error', detail: err.message });
  // Links in rendered markdown (chats, docs, plugins) must never navigate the app window — image-map
  // areas and middle clicks included. Safe external links open in the default browser; everything
  // else is dropped.
  const linkOf = (e: Event) => (e.target as Element | null)?.closest?.('a, area') as HTMLAnchorElement | HTMLAreaElement | null;
  const external = (href: string) => /^(https?:|mailto:)/i.test(href);
  function guardLinks(e: MouseEvent) {
    linkTip = ''; // a click can remove the link it was over, and its mouseout never comes
    if (e.defaultPrevented) return;
    if (e.type === 'auxclick' && e.button !== 1) return;
    const a = linkOf(e);
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (href.startsWith('#') && e.type === 'click') return;
    e.preventDefault();
    if (external(href)) openLink(a, href);
  }

  // A link in a chat or a note can say one thing and go somewhere else, and its address can carry
  // text out of the library. Before opening such a link, show where it really goes.
  const bare = (u: string) => u.trim().replace(/^mailto:/i, '').replace(/^https?:\/\//i, '').replace(/^www\./i, '').replace(/\/+$/, '').toLowerCase();
  function longQuery(href: string) {
    try { const u = new URL(href); return u.search.length + u.hash.length > 80 || href.length > 300; } catch { return true; }
  }
  async function openLink(a: Element, href: string) {
    const inContent = !!a.closest('.md');
    const long = longQuery(href);
    if (inContent && (bare(a.textContent || '') !== bare(href) || long)) {
      let host = href.replace(/^mailto:/i, '');
      try { if (/^https?:/i.test(href)) host = new URL(href).host; } catch { /* keep the address */ }
      const yes = await confirmDialog({
        title: t('shellui.link.title'), body: t('shellui.link.body', { host }), list: [href],
        note: long ? t('shellui.link.long') : undefined, confirmLabel: t('shellui.link.open'),
      });
      if (!yes) return;
    }
    import('@tauri-apps/plugin-opener').then((o) => o.openUrl(href)).catch(() => window.open(href, '_blank', 'noopener'));
  }

  // The window has no status bar: hovering or focusing a link shows its full address at the bottom.
  let linkTip = $state('');
  function tipFor(e: Event) {
    const a = linkOf(e);
    const href = a?.getAttribute('href') || '';
    linkTip = a && external(href) ? href : '';
  }

  onMount(() => {
    boot();
    watchInbox();
    const off = on(async (e) => {
      try {
        if (e.type === 'connection') {
          const was = app.online;
          app.online = e.online;
          if (e.online && !was && app.ready) toast(t('shell.backOnline'), { kind: 'success', key: 'conn' });
          return;
        }
        if (e.type === 'daemon.stale') { stale = !!e.stale; return; }
        if (!app.ready) return;
        if (e.type === 'chat.updated' || e.type === 'chat.done' || e.type === 'chat.removed') {
          await refreshChats();
          await refreshWrap();
        }
        // The month's figure and the limits follow what the assistant reports, without a reload.
        if (e.type === 'usage.changed' && app.status && e.usage) app.status = { ...app.status, usage: e.usage };
        if (e.type === 'limit.changed' && app.status) app.status = { ...app.status, planLimit: e.planLimit ?? null };
        if (e.type === 'chat.done' && app.settings?.notifications && !document.hasFocus()) notify('Thoughts', t('shell.chatFinished', { title: e.title || t('shell.aChat') }));
        if (e.type === 'workflow.result') workflowNotice(e, notify);
        // The assistant waits for the user's OK in a chat they are not looking at (or the window is in the background).
        if (e.type === 'approval.asked' && e.chatId) {
          if (!(app.view.kind === 'chat' && app.view.id === e.chatId)) toast(t('appr.notifyTitle'), { key: 'approval:' + e.chatId, action: { label: t('appr.open'), run: () => go({ kind: 'chat', id: e.chatId }) } });
          if (app.settings?.notifications && !document.hasFocus()) notify(t('appr.notifyTitle'), t('appr.notifyBody', { title: e.title || t('shell.aChat') }));
        }
        if (e.type === 'notify') { toast(`${e.title}: ${e.body}`); if (app.settings?.notifications && !document.hasFocus()) notify(e.title, e.body); }
        if (e.type === 'layer.changed') loadLayer().catch(layerError);
        if (e.type === 'layer.pending' || e.type === 'chat.done') checkPending(true);
        if (e.type === 'chat.persistError') toast(t('layer.chat.persistError'), { kind: 'error', key: 'persist:' + e.chatId, detail: e.error ? String(e.error) : undefined });
        if (e.type === 'wrap.failed') { toast(t(e.rolledBack === false ? 'layer.wrap.failedKept' : 'layer.wrap.failedRolledBack'), { kind: 'error', key: 'wrap', action: e.chatId ? { label: t('layer.details'), run: () => go({ kind: 'chat', id: e.chatId }) } : undefined }); refreshWrap().catch(() => {}); }
        if (e.type === 'wrap.done') { refreshWrap().catch(() => {}); if (syncOn) refreshBackup(); }
        // First-run setup ended while the user was looking at something else.
        if (e.type === 'setup.finished' && e.chatId && !(app.view.kind === 'chat' && app.view.id === e.chatId)) toast(t(e.ok ? 'sa.toast.ready' : 'sa.toast.failed'), { kind: e.ok ? 'success' : 'error', key: 'setup', action: { label: t(e.ok ? 'sa.toast.open' : 'sa.toast.see'), run: () => go({ kind: 'chat', id: e.chatId }) } });
        if (e.type === 'reorganize.ready' && e.chatId && app.view.id !== e.chatId) toast(t(e.ok === false ? 'layer.reorg.failed' : 'layer.reorg.ready'), { kind: e.ok === false ? 'error' : 'info', action: { label: t('layer.details'), run: () => go(e.ok === false ? { kind: 'chat', id: e.chatId } : { kind: 'reorganize', id: e.chatId }) } });
        if (e.type === 'settings.changed') { app.settings = await api('/settings'); applyTheme(); loadProjects(); refreshStatus(); refreshChats().catch(() => {}); refreshWrap().catch(() => {}); }
        if (e.type === 'auth.changed') authChanged();
        if (e.type === 'library.changed') ideas = [];
        if (e.type === 'resync') { refreshChats().catch(() => {}); refreshWrap().catch(() => {}); refreshStatus(); }
      } catch { /* a refresh failed; the connection banner covers the offline case */ }
    });
    // Recovery menu (native, outside the app layer)
    (async () => {
      if ((window as any).__TAURI_INTERNALS__) {
        const { listen } = await import('@tauri-apps/api/event');
        const root = document.documentElement;
        root.classList.add('native');
        // Older shells don't tag the platform; the translucent window styles are macOS-only.
        if (!/\bos-/.test(root.className)) root.classList.add(isMac ? 'os-macos' : /Win/i.test(navigator.userAgent) ? 'os-windows' : 'os-linux');
        listen('recovery', () => (app.showRollback = true));
        listen('open-settings', () => command('settings'));
        listen('open-help', () => command('help'));
        listen('check-updates', () => checkAppUpdate(true));
        watchDeepLinks();
        // Everything else in the Recovery menu (undo, going back, safe mode, reload, restart) is done
        // by the desktop app itself, with its own dialogs: it must work when this page does not.
      }
    })();
    window.addEventListener('thoughts:command', onCommand);
    window.addEventListener('mouseup', onMouseNav);
    measure();
    return () => { off(); window.removeEventListener('thoughts:command', onCommand); window.removeEventListener('mouseup', onMouseNav); };
  });

  async function forceRestart() {
    try { await reconnect(true); toast(t('set.restarted'), { kind: 'success', key: 'restart' }); }
    catch (e: any) { toast(t('ui.restartFailed'), { kind: 'error', detail: e?.message ?? String(e) }); }
  }
  async function restartStale() {
    if (!(await confirmDialog({ title: t('ui.stale.confirmTitle'), body: t('ui.stale.confirmBody'), confirmLabel: t('ui.stale.restart'), danger: true }))) return;
    forceRestart();
  }

  // Mouse "back" / "forward" buttons.
  function onMouseNav(e: MouseEvent) {
    if (e.button !== 3 && e.button !== 4) return;
    if (!app.ready || !app.settings?.onboarded || document.querySelector('[aria-modal="true"]')) return;
    e.preventDefault();
    command(e.button === 3 ? 'back' : 'forward');
  }

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
      const c = await post('/chats', { projectId, byUser: true });
      await refreshChats();
      go({ kind: 'chat', id: c.id });
    } catch (e: any) { toast(t('shell.newChatFailed'), { kind: 'error', detail: e.message }); }
  }

  // A chat nobody wrote in is not kept once the person leaves it (New chat pressed, then something else opened).
  let lastChatView: string | null = null;
  $effect(() => {
    const now = app.view.kind === 'chat' ? (app.view.id ?? null) : null;
    const left = lastChatView;
    lastChatView = now;
    if (left && left !== now) setTimeout(() => dropIfEmpty(left), 600);
  });
  async function dropIfEmpty(id: string) {
    try {
      if (app.view.id === id) return;
      const c = app.chats.find((x: any) => x.id === id);
      if (!c || c.kind !== 'chat' || c.messageCount > 0 || c.pinned || c.running) return;
      // Text typed (or files added) but not sent keeps the chat.
      try { if (localStorage.getItem(`thoughts.chatdraft:${id}`)) return; } catch { /* no storage */ }
      const full = await api(`/chats/${id}`);
      if (full.messages?.length || full.running) return;
      await del(`/chats/${id}`);
      await refreshChats();
    } catch { /* it stays; the daemon removes it later */ }
  }

  // A wrap holds the library while it runs: Wrap and Sync wait for it.
  const wrapRunning = $derived(!!app.status?.busy || app.chats.some((c) => c.kind === 'wrap' && c.running));

  async function startWrap() {
    if (!app.wrap.count) { app.showWrapNag = false; toast(t('shell.nothingToWrap')); return; }
    if (wrapRunning) { toast(t('ui.wrapRunning'), { key: 'wrap' }); return; }
    try {
      // With several chats waiting, the Wrap's page first asks which ones to save.
      const r = await post('/wrap', { choose: true });
      app.showWrapNag = false;
      await refreshChats();
      go({ kind: 'chat', id: r.chatId });
    } catch (e: any) { failed(t('shell.wrapFailed'), e, { key: 'wrap' }); }
  }

  const syncOn = $derived(!!app.settings?.sync?.mode?.startsWith('git'));
  // The Sync button says when the library last really reached an online location.
  $effect(() => { if (syncOn && app.settings?.libraryPath) refreshBackup(); });
  // …and, before that, when the backup needs attention (a location failed, changes waited too long).
  const syncWarn = $derived(healthWarning());
  const syncTitle = $derived([t('shell.syncTip'), syncWarn || lastBackupText() || t('hista.sync.never')].join(' · '));

  async function sync() {
    if (syncing) return;
    if (wrapRunning) { toast(t('ui.syncWaitsForWrap'), { key: 'sync' }); return; }
    if (!syncOn) { toast(t('shell.syncOff'), { action: { label: t('settings'), run: () => go({ kind: 'settings' }) } }); return; }
    syncing = true;
    try {
      // An answer means every online location has the library; anything else is an error that names the location.
      syncDone(await post('/git/sync'), t('shell.synced'));
    } catch (e: any) {
      const m = /CONFLICT::(.*)/s.exec(e.message || '');
      if (m) await resolveConflict(m[1].split(';').map((f) => f.trim()).filter(Boolean));
      else syncFailed(e, t('shell.syncFailed'));
    } finally { syncing = false; }
  }

  // The same documents changed here and on another device: the user picks for each one, in plain words.
  async function resolveConflict(files: string[]) { await settleClash(files); }

  async function setLook(patch: { theme?: string; mode?: string }) {
    try { await saveSettings(patch); }
    catch (e: any) { toast(t('shell.themeNotSaved'), { kind: 'error', detail: e.message }); }
  }

  async function onboarded(setupChatId: string | null) {
    if (setupChatId) go({ kind: 'chat', id: setupChatId });
    // The status in hand is from before setup, when there was no library: fetch the new one before
    // anything asks whether the library is there, or "can't find your library" flashes and is read out.
    await refreshStatus();
    if (!app.status?.library && app.settings?.libraryPath) app.status = { ...(app.status ?? {}), library: app.settings.libraryPath, libraryMissing: false };
    afterOnboard();
    // The short tour, once. A new library takes the assistant a couple of minutes to set up anyway.
    showTour = true;
  }
  const setupAtWork = $derived(app.chats.some((c) => c.kind === 'setup' && c.running));

  // ---------- commands & keyboard ----------
  const actions: Record<string, (d: any) => void> = {
    'chat.new': () => newChat(),
    'chat.open': (d) => d.chatId && go({ kind: 'chat', id: d.chatId }),
    'doc.open': (d) => d.path && go({ kind: 'docs', path: d.path }),
    'wrap.start': () => startWrap(),
    'sync': () => sync(),
    'rollback': () => (app.showRollback = true),
    'history': () => go({ kind: 'history' }),
    'back': () => goBack(),
    'forward': () => goForward(),
    'customise': () => go({ kind: 'customise' }),
    'review': () => openReview(),
    'updates.check': () => checkAppUpdate(true),
    'settings': () => go({ kind: 'settings' }),
    'help': () => go({ kind: 'help' }),
    'tour': () => (showTour = true),
    // Docs prints the open document itself (it listens for this command); every other view prints as shown.
    'print': () => { if (app.view.kind !== 'docs') window.print(); },
    'palette': () => openPalette(!app.showPalette),
    'sidebar.toggle': () => toggleSidebar(),
    'search.focus': () => focusSearch(),
    'find': () => { if (!openFind()) focusSearch(); },
    'zoom.in': () => zoom(1),
    'zoom.out': () => zoom(-1),
    'zoom.reset': () => zoom(0),
    'settings.accessibility': () => go({ kind: 'settings', id: 'accessibility' }),
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
    if (showRecent) { showRecent = false; return true; }
    if (app.showWrapNag) { app.showWrapNag = false; return true; }
    if (app.showReview) { app.showReview = false; return true; }
    if (app.showRollback) { app.showRollback = false; return true; }
    if (searchOpen) { searchOpen = false; return true; }
    return false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.defaultPrevented) return;
    let id = matchShortcut(e);
    if (!id) return;
    // A key pressed inside an add-on's page runs only moving around and the palette (lib/layer.ts).
    if (keyFromAddOn(e) && !keyMayComeFromAddOn(id)) return;
    if (id === 'escape') { if (closeTop()) e.preventDefault(); return; }
    if (!app.ready || !app.settings?.onboarded) return;
    const modal = !!dialogs.current || app.showRollback || app.showReview || app.showWrapNag || showTour || showRecent;
    if (modal || (app.showPalette && id !== 'palette')) return;
    if (id.startsWith('nav.')) {
      const it = navItems[+id.slice(4) - 1];
      if (!it) return;
      e.preventDefault();
      it.run();
      return;
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
  /** Chats whose text matched (from the service); until it answers, chat titles are matched here. */
  let chatResults = $state<any[] | null>(null);
  let searchTerms = $state<string[]>([]);
  let sel = $state(-1);
  let searchSeq = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  const chatHits = $derived.by(() => {
    if (chatResults) return chatResults.slice(0, 4);
    const q = fold(searchQ.trim());
    return q ? app.chats.filter((c) => c.kind === 'chat' && fold(String(c.title || '')).includes(q)).slice(0, 4) : [];
  });
  type Hit = { key: string; icon: string; title: string; marks?: [number, number][]; sub?: string; subMarks?: [number, number][]; path?: string; open: () => void };
  /** Open a result with its words highlighted there. */
  const reveal = (where: string, v: { kind: string; id?: string; path?: string }, msg?: number) => { findOnOpen(where, findWords(searchQ), msg); go(v); };
  const hits = $derived<Hit[]>([
    ...chatHits.map((c) => ({ key: 'c:' + c.id, icon: 'chat', title: c.title, marks: markTerms(c.title, searchTerms),
      sub: c.matches?.[0]?.text, subMarks: c.matches?.[0]?.marks, open: () => reveal('chat:' + c.id, { kind: 'chat', id: c.id }, c.matches?.[0]?.msg) })),
    ...fileHits.map((f) => ({ key: 'f:' + f.path, icon: 'docs', title: f.title || prettyName(f.path), marks: markTerms(f.title || prettyName(f.path), searchTerms), path: f.path,
      sub: f.matches?.[0]?.text, subMarks: f.matches?.[0]?.marks,
      // An item's front page opens as the item's page (its overview shows the words found).
      open: () => reveal('doc:' + f.path, itemOf(f.path) ? { kind: 'item', path: itemOf(f.path)!, id: 'overview' } : { kind: 'docs', path: f.path }) })),
  ]);

  async function focusSearch() {
    if (railed) { toggleSidebar(); await tick(); }
    searchEl?.focus();
    searchEl?.select();
  }

  function onSearchInput() {
    clearTimeout(searchTimer);
    const q = searchQ.trim();
    sel = -1;
    searchFailed = false;
    const mine = ++searchSeq;
    chatResults = null;
    if (!q) { fileHits = []; searchTerms = []; searching = false; searchOpen = false; return; }
    searchOpen = true;
    searching = true;
    searchTimer = setTimeout(async () => {
      track('search');
      try {
        const r = await searchAll(q, 12);
        if (mine === searchSeq) { fileHits = r.notes.slice(0, 6); chatResults = r.chats; searchTerms = r.terms; }
      } catch {
        if (mine === searchSeq) { fileHits = []; searchFailed = true; }
      } finally { if (mine === searchSeq) searching = false; }
    }, 200);
  }

  function closeSearch() { searchOpen = false; sel = -1; }

  function pick(h: Hit) { closeSearch(); h.open(); searchQ = ''; fileHits = []; chatResults = null; }

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
      if (searchOpen) closeSearch(); else if (searchQ) { searchQ = ''; fileHits = []; chatResults = null; } else searchEl?.blur();
    }
  }

  // ---------- derived lists ----------
  const system = $derived(app.chats.filter((c) => c.kind !== 'chat').slice(0, 6));
  const groups = $derived.by(() => {
    // Running or open system chats are listed under System already: never twice.
    const inSystem = new Set(system.map((c) => c.id));
    const visible = app.chats.filter((c) => (c.kind === 'chat' && (!c.archived || c.running || app.view.id === c.id)) || ((c.running || app.view.id === c.id) && !inSystem.has(c.id)))
      .sort((a, b) => Number(!!b.pinned) - Number(!!a.pinned));
    const byProj = new Map<string | null, any[]>();
    for (const c of visible) {
      const k = c.projectId ?? null;
      byProj.set(k, [...(byProj.get(k) || []), c]);
    }
    return byProj;
  });

  const meOpen = $derived(app.view.kind === 'docs' && (app.view.path ?? '').startsWith('me/'));
  const isActive = (k: string) => (app.view.kind === k || (k === 'ideas' && app.view.kind === 'item')) && !(k === 'docs' && meOpen);

  // Sidebar items: built-ins, "Me" and plugin views — then `nav.json` (hidden / order / labels) from the
  // customisation layer. Home can never be hidden; Settings and Help live in the footer, outside this list.
  // `custom`: a page an add-on put there (`from` is the add-on's file, `alias` the short id it chose).
  type NavItem = { id: string; label: string; icon: string; active: boolean; custom?: boolean; alias?: string; from?: string; run: () => void };
  const APP_PAGES = ['home', 'chats', 'ideas', 'todos', 'docs', 'projects', 'inbox', 'me'];
  const navItems = $derived.by((): NavItem[] => {
    const m = app.settings?.modules ?? {};
    const items: NavItem[] = [];
    const add = (id: string, on = true) => { if (on) items.push({ id, label: navLabel(id), icon: id, active: isActive(id), run: () => { go({ kind: id }); } }); };
    add('home'); add('chats'); add('ideas'); add('todos', m.todos !== false); add('docs', m.docs !== false); add('projects', m.builds !== false);
    if (inbox.count > 0 || app.view.kind === 'inbox') items.push({ id: 'inbox', label: navLabel('inbox', t('inbox.nav')), icon: 'inbox', active: app.view.kind === 'inbox', run: () => { go({ kind: 'inbox' }); } });
    if (m.me) items.push({ id: 'me', label: navLabel('me'), icon: 'me', active: meOpen, run: () => { go({ kind: 'docs', path: 'me/README.md' }); } });
    // An add-on's page is keyed by an id that carries the add-on's name, so two add-ons (or an add-on and
    // the app) can use the same short id; `nav.json` may name it by either.
    for (const v of app.pluginViews) items.push({ id: v.id, alias: v.short && !APP_PAGES.includes(v.short) ? v.short : undefined, from: v.plugin, label: viewLabel(v), icon: v.icon || 'plugin', custom: true, active: app.view.kind === 'plugin' && app.view.id === v.id, run: () => { go({ kind: 'plugin', id: v.id }); } });
    // Customise is where customisations are switched off and undone, so no customisation hides it.
    items.push({ id: 'customise', label: navLabel('customise', t('layer.customise')), icon: 'palette', active: isActive('customise'), run: () => { go({ kind: 'customise' }); } });
    const hidden = new Set((app.nav?.hidden ?? []).filter((id) => id !== 'home' && id !== 'customise'));
    const list = items.filter((i) => !hidden.has(i.id) && !(i.alias && hidden.has(i.alias)));
    const first: NavItem[] = [];
    for (const id of app.nav?.order ?? []) {
      const it = list.find((i) => i.id === id) ?? list.find((i) => i.alias === id && !first.includes(i));
      if (it && !first.includes(it)) first.push(it);
    }
    return [...first, ...list.filter((i) => !first.includes(i))];
  });

  // ---------- focus and announcement on navigation ----------
  // The new view's heading takes focus (so keyboard and screen-reader users land in the content) and the
  // change is announced politely — unless the user is typing somewhere, which is never interrupted.
  let announce = $state('');
  let viewEl = $state<HTMLElement | null>(null);
  const viewKey = $derived(`${app.view.kind}:${app.view.id ?? ''}:${app.view.path ?? ''}`);
  let firstView = true;
  $effect(() => {
    viewKey;
    linkTip = '';
    // The "can't find your library" card replaces the view: when it comes or goes, say what is shown now.
    libraryGone;
    if (!app.ready || !app.settings?.onboarded) return;
    if (firstView) { firstView = false; return; }
    const kind = app.view.kind;
    tick().then(() => setTimeout(() => {
      const h = viewEl?.querySelector<HTMLElement>('h1') ?? null;
      const name = h?.textContent?.trim() || navLabel(kind, t(kind === 'help' ? 'shell.help' : kind === 'history' ? 'ui.hist.title' : kind));
      announce = t('ui.nowShowing', { name });
      const a = document.activeElement as HTMLElement | null;
      if (a && a.isConnected && a !== document.body && isTyping(a)) return;
      if (document.querySelector('[aria-modal="true"]')) return;
      const target = h ?? viewEl;
      if (target) { target.tabIndex = -1; target.focus({ preventScroll: true }); }
    }, 80));
  });

  const interrupted = $derived(((app.status?.interrupted ?? []) as { chatId: string; title: string }[]).filter((r) => !dismissedRuns.includes(r.chatId)).slice(0, 3));

  function kindLabel(k: string) { const key = 'shell.kind.' + k; const s = t(key); return s === key ? k : s; }
  function dotLabel(c: any) { return c.running ? t('shell.dot.running') : c.unwrapped ? t('shell.dot.unwrapped') : ''; }

  const paletteItems = $derived.by((): PaletteItem[] => {
    const g = { go: t('shell.pal.goto'), act: t('shell.pal.actions'), look: t('shell.pal.look'), chat: t('shell.pal.chat'), idea: t('shell.pal.idea'), plug: t('shell.pal.plugin') };
    const items: PaletteItem[] = [];
    const add = (id: string, title: string, group: string, icon: string, extra: Partial<PaletteItem> = {}) =>
      items.push({ id, title, group, icon, keys: keys(id) || undefined, run: () => command(id), ...extra });
    add('chat.new', t('newChat'), g.act, 'plus');
    // What an add-on added says so, with the add-on's name, wherever it is listed.
    // (Customise is listed once, below, with the words people search it by.)
    navItems.forEach((it, i) => { if (it.id !== 'customise') items.push({ id: (it.custom ? 'view:' : 'nav.') + it.id, title: it.label, group: it.custom ? addonTag(it.from) : g.go, icon: it.icon, keys: keys('nav.' + (i + 1)) || undefined, run: it.run }); });
    add('settings', t('settings'), g.go, 'settings');
    add('help', t('shell.help'), g.go, 'help', { keywords: 'help shortcuts privacy cost مساعدة اختصارات خصوصية' });
    add('wrap.start', t('shell.pal.wrap'), g.act, 'wrap', { keywords: 'wrap save حفظ تلخيص' });
    if (syncOn) add('sync', t('shell.pal.sync'), g.act, 'sync', { keywords: 'sync backup مزامنة' });
    add('customise', t('layer.customise'), g.act, 'palette', { keywords: 'customise customize personalise change app add-on plugin theme تخصيص تعديل إضافة' });
    if (app.pending?.exists) add('review', t('layer.reviewTitle'), g.act, 'eye', { keywords: 'review pending change مراجعة' });
    add('history', t('ui.hist.title'), g.go, 'rollback', { keywords: 'history versions undo restore notes السجل استعادة تراجع نسخة' });
    add('rollback', t('ui.pal.appHistory'), g.act, 'rollback', { keywords: 'history rollback undo restore app السجل استعادة تراجع التطبيق' });
    add('nav.todos.calendar', t('ui.cal.title'), g.go, 'calendar', { keywords: 'calendar due dates month تقويم مواعيد', run: () => { go({ kind: 'todos', id: 'calendar' }); } });
    if (app.canBack) add('back', t('ui.back'), g.go, 'left');
    if (app.canForward) add('forward', t('ui.forward'), g.go, 'right');
    add('updates.check', t('layer.upd.check'), g.act, 'download', { keywords: 'update version تحديث' });
    add('search.focus', t('shell.pal.search'), g.act, 'search');
    add('sidebar.toggle', railed ? t('shell.sidebarShow') : t('shell.sidebarHide'), g.act, 'sidebar');
    add('tour', t('shell.pal.tour'), g.act, 'book');
    add('print', t('shell.pal.print'), g.act, 'print');
    add('zoom.in', t('a11y.zoomIn'), g.look, 'zoomIn', { keywords: 'zoom bigger larger text size تكبير' });
    add('zoom.out', t('a11y.zoomOut'), g.look, 'zoomOut', { keywords: 'zoom smaller text size تصغير' });
    add('zoom.reset', t('a11y.zoomReset'), g.look, 'zoomOut', { keywords: 'zoom reset 100 actual size الحجم' });
    add('settings.accessibility', t('a11y.open'), g.look, 'accessibility', { keywords: 'accessibility a11y motion contrast focus تسهيلات' });
    for (const [id, name] of [['saudi', t('saudi')], ['contrast', t('contrast')], ['tech', t('tech')], ...app.customThemes.map((c) => [c.id, c.name])])
      add('theme.' + id, t('shell.pal.theme', { name }), g.look, 'palette', { keywords: 'theme سمة' });
    for (const [m, icon] of [['light', 'sun'], ['dark', 'moon'], ['system', 'system']])
      add('mode.' + m, t('shell.pal.mode', { name: t(m) }), g.look, icon, { keywords: 'appearance mode day night مظهر' });
    items.push(...libraryPaletteItems(g.act));
    for (const c of app.commands) items.push({ id: 'plugin:' + c.id, title: c.title, group: c.plugin ? addonTag(c.plugin) : g.plug, icon: c.icon || 'plugin', run: () => { command('plugin:' + c.id); c.run(); } });
    for (const c of app.chats.slice(0, 40)) items.push({ id: 'chat:' + c.id, title: c.title, group: g.chat, icon: 'chat', run: () => command('chat.open', { chatId: c.id }) });
    for (const i of ideas) items.push({ id: 'idea:' + i.path, title: i.title, group: g.idea, icon: 'ideas', keywords: i.summary, run: () => { openItem(i.path); } });
    return items;
  });
</script>

<svelte:window onclick={guardLinks} onauxclick={guardLinks} onkeydown={onKey} onfocus={onFocus} onresize={measure}
  onmouseover={tipFor} onfocusin={tipFor} onfocusout={() => (linkTip = '')} />
<Dialogs />
<SyncConflict />

{#if app.error}
  <div class="center">
    <div class="card down" role="alert">
      <Logo size={44} />
      <h1>{t('shell.down.title')}</h1>
      <p>{t('shell.down.body')}</p>
      <p class="muted sm"><bdi>{app.error}</bdi></p>
      <button class="btn primary" onclick={() => reloadApp()}><Icon name="retry" size={15} /> {t('shell.retry')}</button>
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
  <div class="shell" class:collapsed={railed}>
    {#if !railed}
    <aside class="sidebar no-print" aria-label={t('shell.sidebar')}>
      <div class="drag" data-tauri-drag-region></div>
      <div class="brand row"><Logo size={24} /><span class="grow">Thoughts</span>
        {#if app.safeMode}<span class="pill accent" title={t('shell.safe.pillTip')}>{t('shell.safe.pill')}</span>{/if}
        <button class="iconbtn" aria-label={t('shell.sidebarHide')} title={tip(t('shell.sidebarHide'), 'sidebar.toggle')} onclick={() => command('sidebar.toggle')}><Icon name="sidebar" class="flip-rtl" /></button>
      </div>
      <LibrarySwitcher />
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
                <span class="grow" title={h.path}>
                  <span class="ell blk" dir="auto"><Marked text={h.title} marks={h.marks} /></span>
                  {#if h.sub}<span class="ell blk muted sm" dir="auto"><Marked text={h.sub} marks={h.subMarks} /></span>{/if}
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
            {#if !searching && searchQ.trim()}
              <button type="submit" class="all" onmousedown={(e) => e.preventDefault()} onclick={() => (sel = -1)}>{t(hits.length ? 'srch.sideAll' : 'srch.sideMore')} <Icon name="arrow" size={13} class="flip-rtl" /></button>
            {/if}
          </div>
        {/if}
      </form>
      <nav aria-label={t('shell.mainNav')}>
        {#each navItems as it, i (it.id)}
          <button class="nav" class:active={it.active} aria-current={it.active ? 'page' : undefined} title={tip(it.custom ? `${it.label} — ${addonTag(it.from)}` : it.label, 'nav.' + (i + 1))} onclick={it.run}>
            <Icon name={it.icon} /><span class="grow ell" dir="auto">{it.label}</span>
            {#if it.custom}<span class="addon" aria-hidden="true"><Icon name="plugin" size={12} /></span><span class="sr-only">({addonTag(it.from)})</span>{/if}
            {#if it.id === 'inbox' && inbox.count > 0}<span class="badge" title={tn('inbox.badge', inbox.count)}><span aria-hidden="true">{fmtNum(inbox.count)}</span><span class="sr-only">{tn('inbox.badge', inbox.count)}</span></span>{/if}
            {#if it.id === 'todos' && app.dueToday > 0}<span class="badge" title={tn('ui.dueNow', app.dueToday)}><span aria-hidden="true">{fmtNum(app.dueToday)}</span><span class="sr-only">{tn('ui.dueNow', app.dueToday)}</span></span>{/if}
          </button>
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
                <button class="iconbtn sm" aria-label={t('shell.dismissNamed', { name: r.title || t('shell.aChat') })} title={t('shell.dismiss')} onclick={() => { dismissedRuns = [...dismissedRuns, r.chatId]; post(`/chats/${r.chatId}/ack-interrupted`).catch(() => {}); }}><Icon name="close" size={13} /></button>
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
              <span class="dot" class:run={c.running} class:unw={!c.running && c.unwrapped} title={dotLabel(c) || undefined}></span>
              <span class="grow ell"><bdi>{c.title}</bdi></span>
              {#if c.pinned}<span class="cut" title={t('chatb.pinned')}><Icon name="pin" size={12} /><span class="sr-only">({t('chatb.pinned')})</span></span>{/if}
              {#if c.interrupted}<span class="cut" title={t('layer.chat.interrupted')}><Icon name="warning" size={12} /><span class="sr-only">({t('layer.chat.interruptedShort')})</span></span>{/if}
              {#if c.bypass}<span class="cut" style="color: var(--danger)" title={t('perm.bypass.on')}><Icon name="shield" size={12} /><span class="sr-only">({t('perm.bypass.on')})</span></span>{/if}
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
        {#if (app.wrap as any).miniWrapsPaused}<div class="notice" role="status"><Icon name="offline" size={15} /><span class="grow">{t('layer.wrap.paused')}</span></div>{/if}
        <button class="btn primary wide" onclick={() => command('wrap.start')} disabled={!app.wrap.count || wrapRunning} aria-busy={wrapRunning}
          title={wrapRunning ? t('ui.wrapRunning') : app.wrap.count ? tip(tn('shell.chatsToWrap', app.wrap.count), 'wrap.start') : t('shell.nothingToWrap')}>
          <Icon name="wrap" size={15} /> {wrapRunning ? t('ui.wrapRunningShort') : t('wrap')}
          {#if app.wrap.count}<span class="pill" aria-hidden="true">{fmtNum(app.wrap.count)}</span><span class="sr-only">— {tn('shell.chatsToWrap', app.wrap.count)}</span>{/if}
        </button>
        <div class="row tools">
          {#if syncOn}<button class="iconbtn" class:busy={syncing} class:warned={!!syncWarn} disabled={syncing || wrapRunning} aria-busy={syncing} onclick={() => command('sync')} aria-label={syncing ? t('shell.syncing') : wrapRunning ? t('ui.syncWaitsForWrap') : syncTitle} title={syncing ? t('shell.syncing') : wrapRunning ? t('ui.syncWaitsForWrap') : syncTitle}><Icon name="sync" />{#if syncWarn && !syncing}<span class="rdot" class:bad={backup.health?.state === 'failed'} aria-hidden="true"></span>{/if}</button>{/if}
          <button class="iconbtn" class:on={app.view.kind === 'history'} onclick={() => command('history')} aria-label={t('ui.hist.title')} title={t('shell.rollbackTip')} aria-current={app.view.kind === 'history' ? 'page' : undefined}><Icon name="rollback" /></button>
          <button class="iconbtn" class:on={app.view.kind === 'help'} onclick={() => command('help')} aria-label={t('shell.help')} title={t('shell.help')} aria-current={app.view.kind === 'help' ? 'page' : undefined}><Icon name="help" /></button>
          <button class="iconbtn" onclick={() => command('palette')} aria-label={t('shell.palette')} title={tip(t('shell.palette'), 'palette')}><Icon name="command" /></button>
          <button class="iconbtn bell" class:on={showRecent} onclick={openRecent} aria-haspopup="dialog" aria-label={app.toastLogUnread ? t('shellui.recent.openUnread') : t('shellui.recent.title')} title={t('shellui.recent.title')}><Icon name="bell" />{#if app.toastLogUnread}<span class="rdot" aria-hidden="true"></span>{/if}</button>
          <span class="grow"></span>
          <button class="iconbtn" class:on={app.view.kind === 'settings'} onclick={() => command('settings')} aria-label={t('settings')} title={tip(t('settings'), 'settings')} aria-current={app.view.kind === 'settings' ? 'page' : undefined}><Icon name="settings" /></button>
        </div>
        <button class="ver" onclick={() => command('help')} title={app.status?.version && app.status.version !== APP_VERSION ? t('ver.mismatch', { app: APP_VERSION, service: app.status.version }) : t('shell.help.version', { v: APP_VERSION })}>
          <bdi>Thoughts v{versionLabel()}</bdi>
        </button>
      </div>
    </aside>
    {:else}
    <!-- Collapsed: a narrow rail keeps every destination one click away. -->
    <aside class="rail no-print" aria-label={t('shell.sidebar')}>
      <div class="drag" data-tauri-drag-region></div>
      <button class="iconbtn" aria-label={t('shell.sidebarShow')} title={tip(t('shell.sidebarShow'), 'sidebar.toggle')} onclick={() => command('sidebar.toggle')}><Icon name="sidebar" class="flip-rtl" /></button>
      <nav aria-label={t('shell.mainNav')}>
        {#each navItems as it, i (it.id)}
          <button class="iconbtn rb" class:on={it.active} aria-current={it.active ? 'page' : undefined} aria-label={it.custom ? `${it.label} — ${addonTag(it.from)}` : it.label} title={tip(it.custom ? `${it.label} — ${addonTag(it.from)}` : it.label, 'nav.' + (i + 1))} onclick={it.run}>
            <Icon name={it.icon} />
            {#if it.custom}<span class="rmark" aria-hidden="true"></span>{/if}
            {#if it.id === 'inbox' && inbox.count > 0}<span class="rdot" aria-hidden="true"></span><span class="sr-only">— {tn('inbox.badge', inbox.count)}</span>{/if}
            {#if it.id === 'todos' && app.dueToday > 0}<span class="rdot" aria-hidden="true"></span><span class="sr-only">— {tn('ui.dueNow', app.dueToday)}</span>{/if}
          </button>
        {/each}
        <button class="iconbtn rb" aria-label={t('newChat')} title={tip(t('newChat'), 'chat.new')} onclick={() => command('chat.new')}><Icon name="plus" /></button>
        <button class="iconbtn rb" aria-label={t('shell.searchLabel')} title={tip(t('shell.searchLabel'), 'search.focus')} onclick={() => command('search.focus')}><Icon name="search" /></button>
      </nav>
      <span class="grow"></span>
      <button class="iconbtn rb wrapb" disabled={!app.wrap.count || wrapRunning} aria-busy={wrapRunning} onclick={() => command('wrap.start')}
        aria-label={wrapRunning ? t('ui.wrapRunning') : app.wrap.count ? `${t('wrap')} — ${tn('shell.chatsToWrap', app.wrap.count)}` : t('shell.nothingToWrap')}
        title={wrapRunning ? t('ui.wrapRunning') : app.wrap.count ? tip(tn('shell.chatsToWrap', app.wrap.count), 'wrap.start') : t('shell.nothingToWrap')}>
        <Icon name="wrap" />{#if app.wrap.count}<span class="rdot" aria-hidden="true"></span>{/if}
      </button>
      <button class="iconbtn rb" class:on={app.view.kind === 'history'} onclick={() => command('history')} aria-label={t('ui.hist.title')} title={t('shell.rollbackTip')}><Icon name="rollback" /></button>
      <button class="iconbtn rb" class:on={app.view.kind === 'help'} onclick={() => command('help')} aria-label={t('shell.help')} title={t('shell.help')}><Icon name="help" /></button>
      <button class="iconbtn rb" class:on={showRecent} onclick={openRecent} aria-haspopup="dialog" aria-label={app.toastLogUnread ? t('shellui.recent.openUnread') : t('shellui.recent.title')} title={t('shellui.recent.title')}><Icon name="bell" />{#if app.toastLogUnread}<span class="rdot" aria-hidden="true"></span>{/if}</button>
      <button class="iconbtn rb" class:on={app.view.kind === 'settings'} onclick={() => command('settings')} aria-label={t('settings')} title={tip(t('settings'), 'settings')} aria-current={app.view.kind === 'settings' ? 'page' : undefined}><Icon name="settings" /></button>
    </aside>
    {/if}

    <main class="main">
      <div class="drag top no-print" data-tauri-drag-region></div>
      <div class="sr-only" role="status" aria-live="polite">{announce}</div>
      {#if stale}
        <div class="safe no-print" role="status">
          <Icon name="download" size={16} />
          <span class="grow">{t('ui.stale.body')}</span>
          <button class="btn" onclick={restartStale}><Icon name="retry" size={14} /> {t('ui.stale.restart')}</button>
        </div>
      {/if}
      {#if !app.online}
        <div class="banner no-print" role="status"><Icon name="offline" size={15} /> {t('shell.reconnecting')}</div>
      {/if}
      {#if signedOut && app.online}
        <div class="safe no-print" role="alert">
          <Icon name="warning" size={16} />
          <span class="grow">{t(onCodex ? (primaryStatus.authFailed ? 'codex.auth.bannerExpired' : 'codex.auth.banner') : (app.status.claude.authFailed ? 'auth.bannerExpired' : 'auth.banner'))}</span>
          <button class="btn primary" onclick={() => signIn(onCodex ? 'codex' : 'claude')}><Icon name="external" size={14} /> {t('auth.signIn')}</button>
          <button class="btn" disabled={authBusy} aria-busy={authBusy} onclick={checkSignIn}><Icon name="retry" size={14} /> {t('auth.check')}</button>
        </div>
      {/if}
      <UsageBanner />
      <WrapWaiting />
      <SetupWaiting />
      <VoiceBar />
      {#if app.safeMode}
        <div class="safe no-print" role={app.safeReason === 'crash' ? 'alert' : 'status'}>
          <Icon name="shield" size={16} />
          <span class="grow">{app.safeReason === 'crash' ? safeCrashText() : t('shell.safe.manual')}</span>
          {#if app.safeReason === 'crash'}<button class="btn" onclick={() => command('rollback')}><Icon name="rollback" size={14} /> {t('shell.openRollback')}</button>{/if}
          <button class="btn" onclick={retryLayer}><Icon name="retry" size={14} /> {app.safeReason === 'crash' ? t('shell.safe.retry') : t('shell.safe.leave')}</button>
        </div>
      {/if}
      {#if app.preview}<PreviewBanner />{/if}
      {#key viewKey}<div class="view" bind:this={viewEl}>
      <svelte:boundary onerror={viewFailed}>
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
      {:else if app.view.kind === 'chats'}<ChatList {projects} {newChat} />
      {:else if app.view.kind === 'ideas'}<Ideas />
      {:else if app.view.kind === 'item'}<Item path={app.view.path} tab={app.view.id} />
      {:else if app.view.kind === 'docs'}<Docs path={app.view.path} />
      {:else if app.view.kind === 'todos'}<Todos mode={app.view.id === 'calendar' ? 'calendar' : 'list'} />
      {:else if app.view.kind === 'inbox'}<Inbox />
      {:else if app.view.kind === 'history'}<History />
      {:else if app.view.kind === 'customise'}<Customise />
      {:else if app.view.kind === 'projects'}<Projects {projects} reload={loadProjects} {newChat} />
      {:else if app.view.kind === 'settings'}<Settings reload={loadProjects} />
      {:else if app.view.kind === 'help'}<Help />
      {:else if app.view.kind === 'reorganize'}<Reorganize id={app.view.id} />
      {:else if app.view.kind === 'plugin'}{#key app.view.id}<PluginView id={app.view.id!} />{/key}
      {/if}
      {#snippet failed(error, reset)}
        <div class="page missing">
          <div class="card" role="alert">
            <Icon name="warning" size={28} />
            <h1>{t('shellui.crash.title')}</h1>
            <p>{t('shellui.crash.body')}</p>
            <div class="row wrap">
              <button class="btn primary" onclick={reset}><Icon name="retry" size={15} /> {t('shell.retry')}</button>
              <button class="btn" onclick={() => reloadApp()}>{t('shellui.crash.reload')}</button>
              <button class="btn" onclick={() => (app.showRollback = true)}><Icon name="rollback" size={15} /> {t('rollback')}</button>
              <button class="btn" onclick={() => copyCrash(error)}><Icon name={crashCopied ? 'check' : 'copy'} size={15} /> {crashCopied ? t('shell.copied') : t('shellui.crash.copy')}</button>
            </div>
          </div>
        </div>
      {/snippet}
      </svelte:boundary>
      </div>{/key}
    </main>
  </div>
{/if}

{#if app.showRollback}<Rollback />{/if}
{#if app.showReview && app.pending}<Review />{/if}
{#if app.showWrapNag}<WrapNag {startWrap} />{/if}
{#if showTour}<Tour close={() => (showTour = false)} note={setupAtWork ? t('sa.tour.meanwhile') : ''} />{/if}
{#if showRecent}<RecentMessages close={() => (showRecent = false)} />{/if}
{#if app.showPalette}<CommandPalette items={paletteItems} close={() => (app.showPalette = false)} />{/if}
{#if app.ready && app.settings?.onboarded}<LibraryAdd />{/if}

<Toasts />
{#if linkTip}<div class="linktip no-print" aria-hidden="true" dir="ltr">{linkTip}</div>{/if}

<style>
  .center { height: 100vh; display: grid; place-items: center; padding: 20px; }
  .down { max-width: 520px; display: grid; gap: 10px; justify-items: start; }
  .down h1, .down p { margin: 0; }
  .boot { display: grid; gap: 12px; justify-items: center; }
  .pulse { animation: breathe 1.6s ease-in-out infinite alternate; margin-bottom: 6px; }
  @keyframes breathe { to { opacity: .55; transform: scale(.96); } }
  .shell { display: grid; grid-template-columns: var(--sidebar-w) 1fr; height: 100vh; }
  .shell.collapsed { grid-template-columns: 50px 1fr; }
  .rail { display: flex; flex-direction: column; align-items: center; gap: 2px; padding: 0 0 10px; min-height: 0; background: var(--sidebar-glass); border-inline-end: 1px solid var(--hairline); }
  :global(html.native.os-macos) .rail { background: color-mix(in srgb, var(--sidebar) 40%, transparent); }
  :global(html.native:not(.os-macos)) .rail { background: var(--sidebar); }
  .rail nav { margin: 8px 0; align-items: center; gap: 2px; overflow: auto; min-height: 0; }
  .rail .rb { position: relative; width: 34px; height: 32px; }
  .rail .wrapb:not(:disabled) { color: var(--accent); }
  .bell { position: relative; }
  .linktip { position: fixed; bottom: 6px; inset-inline-start: 6px; z-index: 110; max-width: min(640px, 70vw); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; pointer-events: none;
    font-size: var(--fs-xs); padding: 3px 9px; border-radius: 6px; background: var(--panel); color: var(--fg); border: var(--border-w) solid var(--hairline); box-shadow: var(--shadow); }
  .wrap { flex-wrap: wrap; }
  .iconbtn.warned { position: relative; }
  .rdot.bad { background: var(--danger); }
  .rdot { position: absolute; top: 4px; inset-inline-end: 4px; width: 7px; height: 7px; border-radius: 50%; background: var(--accent-2); }
  /* A page an add-on added: a small hollow square in the corner (rail), a small puzzle piece (sidebar). */
  .rmark { position: absolute; bottom: 4px; inset-inline-end: 4px; width: 6px; height: 6px; border: 1.5px solid currentColor; border-radius: 1px; opacity: .8; }
  .nav .addon { flex: none; display: inline-flex; opacity: .75; }
  .badge { flex: none; font-size: var(--fs-xxs); font-weight: 600; min-width: 18px; text-align: center; padding: 0 5px; border-radius: 99px; background: var(--accent); color: var(--accent-fg); }
  :global([data-theme="contrast"]) .nav.active .badge { background: var(--accent-fg); color: var(--accent); }
  .sidebar {
    background: var(--sidebar-glass); -webkit-backdrop-filter: blur(30px) saturate(180%); backdrop-filter: blur(30px) saturate(180%);
    border-inline-end: 1px solid var(--hairline); display: flex; flex-direction: column; min-height: 0; padding: 0 10px 10px;
  }
  .drag { height: 30px; flex: none; }
  /* `main` is a size container so views can adapt to the room they really have (sidebar open or not). */
  .main { position: relative; overflow: auto; min-width: 0; background: var(--aura), var(--bg); background-repeat: no-repeat; container: main / inline-size; }
  .view:focus { outline: none; }
  :global(html.native.os-macos) .sidebar { background: color-mix(in srgb, var(--sidebar) 40%, transparent); -webkit-backdrop-filter: none; backdrop-filter: none; }
  :global(html.native:not(.os-macos)) .sidebar { background: var(--sidebar); -webkit-backdrop-filter: none; backdrop-filter: none; }
  .main .drag.top { position: sticky; top: 0; height: 26px; z-index: 5; }
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
  .hit[aria-selected="true"] { background: var(--tint); color: var(--on-tint); }
  :global([data-theme="contrast"]) .hit[aria-selected="true"] :global(*) { color: inherit; }
  .blk { display: block; }
  .note { margin: 0; padding: 8px; font-size: 13px; }
  .all { font: inherit; font-size: 13px; color: var(--accent); background: none; border: 0; width: 100%; text-align: start; padding: 7px 8px; border-radius: 6px; cursor: pointer; display: flex; align-items: center; gap: 4px; border-top: 1px solid var(--hairline); margin-top: 4px; }
  .all:hover { background: var(--fill); }

  .iconbtn { font: inherit; background: none; border: 0; padding: 0; cursor: pointer; width: 30px; height: 28px; display: grid; place-items: center; border-radius: 7px; color: var(--muted); flex: none; }
  .iconbtn:hover { background: var(--fill); color: var(--fg); }
  .iconbtn.on { background: var(--tint); color: var(--on-tint); }
  .iconbtn:disabled { opacity: .6; cursor: default; }
  .iconbtn.busy :global(.icon) { animation: spin 1s linear infinite; }
  .iconbtn.sm { width: 24px; height: 24px; }
  .tools { gap: 2px; }
  nav { display: flex; flex-direction: column; gap: 1px; margin: 10px 0; }
  /* A short window (or a large zoom): the list scrolls instead of pushing the chats out of sight. */
  .sidebar nav { flex: 0 1 auto; min-height: 64px; overflow-y: auto; }
  .nav, .chat {
    font: inherit; background: none; border: 0; text-align: start; width: 100%;
    display: flex; align-items: center; gap: 9px; padding: 5px 8px; border-radius: 6px;
    cursor: pointer; color: var(--fg); font-size: 13px;
  }
  .nav :global(.icon) { flex: none; color: var(--muted); }
  .nav:hover, .chat:hover { background: var(--fill); }
  .nav.active, .chat.active { background: var(--tint); color: var(--on-tint); font-weight: 600; }
  .nav.active :global(.icon) { color: inherit; }
  .nav:focus-visible, .chat:focus-visible, .iconbtn:focus-visible { outline-offset: -2px; }

  .notice { display: flex; gap: 8px; align-items: flex-start; font-size: var(--fs-xs); padding: 8px; margin-bottom: 8px; border-radius: var(--radius); background: var(--fill); border: var(--border-w) solid var(--hairline); }
  .notice > :global(.icon) { flex: none; margin-top: 2px; color: var(--accent-2); }
  .run { gap: 2px; }
  .cut { flex: none; color: var(--accent-2); display: inline-flex; }
  .foot .notice { margin-bottom: 0; }
  .link { font: inherit; font-weight: 600; color: var(--accent); background: none; border: 0; padding: 2px 0; text-align: start; cursor: pointer; text-decoration: underline; text-underline-offset: 3px; }

  .chats { flex: 1 1 0; min-height: 72px; border-top: 1px solid var(--hairline); padding-top: 6px; }
  .sect { padding: 0 6px; color: var(--muted); }
  .sect h2 { font-size: var(--fs-xs); font-weight: 600; margin: 0; letter-spacing: 0; }
  .group { font-size: var(--fs-xs); color: var(--muted); padding: 8px 8px 2px; }
  .empty { font-size: var(--fs-xs); padding: 6px 8px; margin: 0; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--border); flex: none; }
  /* State is carried by shape as well as colour: plain = small round, not saved yet = diamond, working = ring. */
  .dot.unw { background: var(--accent-2); border-radius: 1px; transform: rotate(45deg); }
  .dot.run { width: 9px; height: 9px; background: transparent; border: 2px solid var(--accent); animation: pulse 1s infinite alternate; }
  :global([data-theme="contrast"]) .chat.active .dot { background: currentColor; }
  :global([data-theme="contrast"]) .chat.active .dot.run { background: transparent; border-color: currentColor; }
  @keyframes pulse { to { opacity: .3; } }
  .ver { font: inherit; font-size: 11px; color: var(--muted); background: none; border: 0; padding: 0 4px; text-align: start; cursor: pointer; font-variant-numeric: tabular-nums; }
  .ver:hover { color: var(--fg); }
  .foot { display: flex; flex-direction: column; gap: 6px; padding-top: 10px; border-top: 1px solid var(--hairline); }
  .wide { justify-content: center; }
  .sm { font-size: var(--fs-xs); }

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
</style>
