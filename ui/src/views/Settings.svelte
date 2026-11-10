<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onDestroy, onMount } from 'svelte';
  import { api, del, inTauri, post, reconnect } from '../lib/api';
  import { app, applyTheme, go, saveSettings, toast } from '../lib/store.svelte';
  import { confirmDialog, promptDialog } from '../lib/dialog.svelte';
  import { fmtNum, t } from '../lib/i18n';
  import ThemePicker from './ThemePicker.svelte';

  let { reload }: { reload: () => Promise<void> } = $props();

  const cats = ['general', 'ai', 'git', 'privacy', 'modules', 'daemon', 'appLayer'] as const;
  type Cat = (typeof cats)[number];
  const catIcon: Record<Cat, string> = { general: 'settings', ai: 'ai', git: 'sync', privacy: 'shield', modules: 'modules', daemon: 'daemon', appLayer: 'palette' };
  // Row labels per category (i18n keys) — used by the search box. `adv` rows live behind the Advanced disclosure.
  const basic: Record<Cat, string[]> = {
    general: ['language', 'set.name', 'theme', 'mode', 'set.notify'],
    ai: ['set.account', 'set.perm', 'set.wrapNag'],
    git: ['set.libraryPath', 'set.backup', 'reorganize'],
    privacy: ['set.privTitle', 'set.cost'],
    modules: ['set.modulesIntro'],
    daemon: ['set.keepRunning', 'set.runAtLogin'],
    appLayer: ['customize', 'rollback', 'updates'],
  };
  const adv: Record<Cat, string[]> = {
    general: [],
    ai: ['set.model', 'set.effort', 'set.binary', 'set.miniModel', 'set.idle', 'set.provider', 'set.baseUrl', 'set.apiKey'],
    git: ['set.gitWhen', 'set.credStore', 'set.remotes', 'set.searchExt'],
    privacy: [],
    modules: [],
    daemon: ['set.restart'],
    appLayer: ['set.safeMode'],
  };
  const perms = ['default', 'acceptEdits', 'plan'] as const;
  const cloudWords = ['Mobile Documents', 'Library/CloudStorage', 'Dropbox', 'OneDrive', 'Google Drive', 'iCloud'];

  const fresh = () => { const x = structuredClone($state.snapshot(app.settings)); x.privatePaths ??= []; return x; };
  const norm = (x: any) => JSON.stringify(x, (_k, v) => (v === '' || v === undefined ? null : v));

  let tab = $state<Cat>('general');
  let query = $state('');
  let s = $state<any>(fresh());
  let base = $state<any>(fresh()); // last saved state; `s` is the draft
  let opts = $state<any>({ models: [], efforts: [], permissionModes: [] });
  let claude = $state<any>(app.status?.claude ?? {});
  let gitStatus = $state<any>({});
  let modules = $state<any[]>([]);
  let updates = $state<any[]>([]);
  let loadErr = $state<Record<string, string>>({});
  let loading = $state<Record<string, boolean>>({ modules: true, updates: true });
  let busy = $state<Record<string, boolean>>({});
  let picked = $state<Record<string, boolean>>({});
  let ext = $state<any>(null);
  let apiToken = $state('');
  let remote = $state({ name: '', url: '', username: '', token: '' });
  let customizeText = $state('');
  let privErr = $state('');
  let advOpen = $state<Record<string, boolean>>({});

  const isDirty = () => norm(s) !== norm(base);
  const dirty = $derived(isDirty());
  const isGit = $derived(String(s.sync.mode).startsWith('git'));
  const syncKind = $derived(isGit ? 'git' : s.sync.mode === 'folder' ? 'folder' : 'none');
  const pm = $derived(s.ai.claude.permissionMode);
  const permValue = $derived(pm == null || pm === '' ? 'default' : (perms as readonly string[]).includes(pm) ? pm : '');
  const cost = $derived(app.status?.usage?.monthCostUsd);
  const validRemote = $derived(/^(https?:\/\/|ssh:\/\/|git@)\S+$/.test(remote.url.trim()));

  const errors = $derived.by(() => {
    const e: Record<string, string> = {};
    const whole = (v: any) => Number.isInteger(v) && v >= 1;
    if (!String(s.libraryPath ?? '').trim()) e.libraryPath = t('set.errLibrary');
    if (!whole(s.idleMinutes)) e.idleMinutes = t('set.errWhole');
    if (!whole(s.wrapNagThreshold)) e.wrapNagThreshold = t('set.errWhole');
    const u = String(s.ai.claude.baseUrl ?? '').trim();
    if (u && !/^https?:\/\/\S+$/.test(u)) e.baseUrl = t('set.errUrl');
    if (permValue === '') e.permission = t('set.errPerm');
    if (isGit && cloudWords.some((k) => String(s.libraryPath ?? '').includes(k))) e.sync = t('set.errCloudGit');
    return e;
  });
  const invalid = $derived(Object.keys(errors).length > 0);

  // ----- search -----
  const q = $derived(query.trim().toLowerCase());
  const hit = (k: string) => t(k).toLowerCase().includes(q);
  const catAll = (c: Cat) => !q || t('set.cat.' + c).toLowerCase().includes(q);
  const vis = (c: Cat, ...keys: string[]) => catAll(c) || keys.some(hit);
  const catShown = (c: Cat) => vis(c, ...basic[c], ...adv[c]);
  const shown = $derived(q ? cats.filter(catShown) : [tab]);
  const tx = (k: string, fallback: string) => { const v = t(k); return v === k ? fallback : v; };

  async function load(key: string, path: string, set: (v: any) => void) {
    loading[key] = true; loadErr[key] = '';
    try { set(await api(path)); }
    catch (e: any) { loadErr[key] = e?.message || t('set.failed'); }
    finally { loading[key] = false; }
  }
  const loadModules = () => load('modules', '/modules', (m) => (modules = m));
  const loadUpdates = () => load('updates', '/updates', (u) => (updates = u));
  const loadGit = () => load('git', '/git', (g) => (gitStatus = g));

  onMount(() => {
    api('/ai/claude/options').then((o) => (opts = o)).catch(() => {});
    api('/extensions').then((e) => (ext = e)).catch(() => {});
    loadGit(); loadModules(); loadUpdates();
  });

  /** Runs one action with a loading flag; any failure is shown to the user. */
  async function run(key: string, fn: () => Promise<unknown>) {
    if (busy[key]) return;
    busy[key] = true;
    try { await fn(); } catch (e: any) { toast(e?.message || t('set.failed')); }
    finally { busy[key] = false; }
  }

  function payload() {
    const x = structuredClone($state.snapshot(s));
    const c = x.ai.claude;
    for (const k of ['binary', 'model', 'effort', 'permissionMode', 'miniWrapModel', 'baseUrl']) c[k] = typeof c[k] === 'string' && c[k].trim() ? c[k].trim() : null;
    x.libraryPath = String(x.libraryPath ?? '').trim();
    return x;
  }

  async function save() {
    if (invalid) return toast(t('set.fixFirst'));
    await run('save', async () => {
      await saveSettings(payload());
      s = fresh(); base = fresh();
      toast(t('set.saved'));
    });
  }

  function restorePreview(prev: any) {
    if (!app.settings) return;
    app.settings.theme = prev.theme; app.settings.mode = prev.mode; applyTheme();
  }

  function discard() { s = structuredClone($state.snapshot(base)); restorePreview(base); }

  // Leaving the view with unsaved edits: this component cannot veto navigation, so it asks right after instead.
  onDestroy(() => {
    if (!isDirty()) return;
    const draft = payload(), prev = $state.snapshot(base), bad = invalid;
    confirmDialog({ title: t('set.leaveTitle'), body: t(bad ? 'set.leaveInvalid' : 'set.leaveBody'), confirmLabel: t('save'), cancelLabel: t('set.discard') }).then(async (ok) => {
      if (!ok) return restorePreview(prev);
      try { await saveSettings(draft); toast(t('set.saved')); }
      catch (e: any) { restorePreview(prev); toast(t('set.leaveFailed', { error: e?.message ?? '' })); }
    });
  });

  function onBeforeUnload(e: BeforeUnloadEvent) { if (isDirty()) { e.preventDefault(); e.returnValue = ''; } }

  const resets: Partial<Record<Cat, { list: string[]; apply: () => void }>> = {
    general: { list: ['theme', 'mode', 'set.notify'], apply: () => { s.theme = 'saudi'; s.mode = 'system'; s.notifications = true; restorePreview(s); } },
    ai: { list: ['set.perm', 'set.wrapNag', 'set.model', 'set.effort', 'set.binary', 'set.miniModel', 'set.idle'], apply: () => {
      const c = s.ai.claude; c.permissionMode = 'acceptEdits'; c.model = null; c.effort = null; c.binary = null; c.miniWrapModel = null;
      s.idleMinutes = 15; s.wrapNagThreshold = 5;
    } },
    daemon: { list: ['set.keepRunning', 'set.runAtLogin'], apply: () => { s.daemon.keepRunning = true; s.daemon.runAtLogin = false; } },
  };

  async function resetCat(c: Cat) {
    const r = resets[c];
    if (!r) return;
    const ok = await confirmDialog({ title: t('set.resetTitle', { name: t('set.cat.' + c) }), body: t('set.resetBody'), list: r.list.map((k) => t(k)), confirmLabel: t('set.reset'), danger: true });
    if (ok) r.apply();
  }

  async function syncServerFields() {
    app.settings = await api('/settings');
    s.ai.claude.hasApiToken = base.ai.claude.hasApiToken = app.settings.ai.claude.hasApiToken;
    s.modules = structuredClone($state.snapshot(app.settings.modules));
    base.modules = structuredClone($state.snapshot(app.settings.modules));
  }

  const refreshClaude = () => run('claude', async () => { claude = (await api('/status')).claude ?? {}; });
  const signIn = () => run('login', async () => { await post('/ai/claude/login'); toast(t('set.loginToast')); });

  const saveToken = () => run('token', async () => {
    if (!apiToken.trim()) return;
    await post('/ai/claude/token', { token: apiToken.trim() });
    apiToken = '';
    await syncServerFields();
    toast(t('set.keyStored'));
  });

  async function clearToken() {
    if (!s.ai.claude.hasApiToken) return;
    if (!(await confirmDialog({ title: t('set.clearKeyTitle'), body: t('set.clearKeyBody'), confirmLabel: t('set.clearKey'), danger: true }))) return;
    await run('token', async () => { await post('/ai/claude/token', { token: '' }); await syncServerFields(); toast(t('set.keyCleared')); });
  }

  async function browseLibrary() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true, defaultPath: s.libraryPath || undefined });
      if (typeof p === 'string') s.libraryPath = p;
    } catch { toast(t('set.browseFail')); }
  }

  function setSyncKind(k: string) { s.sync.mode = k === 'git' ? (isGit ? s.sync.mode : 'git-manual') : k; }

  async function addRemote() {
    if (!validRemote) return;
    const ok = await confirmDialog({ title: t('set.uploadTitle'), body: t('set.uploadBody'), list: [remote.url.trim()], confirmLabel: t('set.addRemote') });
    if (!ok) return;
    await run('remote', async () => {
      await post('/git/remotes', { ...remote, name: remote.name.trim() || ((gitStatus.remotes || []).length ? 'backup' : 'origin'), url: remote.url.trim() });
      remote = { name: '', url: '', username: '', token: '' };
      gitStatus = await api('/git');
    });
  }

  async function removeRemote(r: any) {
    if (!(await confirmDialog({ title: t('set.removeRemoteTitle'), body: t('set.removeRemoteBody'), list: [r.url], confirmLabel: t('set.remove'), danger: true }))) return;
    await run('remote', async () => { await del(`/git/remotes/${encodeURIComponent(r.name)}`); gitStatus = await api('/git'); });
  }

  const enableModule = (id: string) => run('mod:' + id, async () => {
    const r = await post(`/modules/${id}/enable`);
    await syncServerFields();
    if (r?.chatId) go({ kind: 'chat', id: r.chatId });
  });

  const customize = () => run('customize', async () => {
    if (!customizeText.trim()) return;
    const r = await post('/layer/customize', { text: customizeText.trim() });
    customizeText = '';
    go({ kind: 'chat', id: r.chatId });
  });

  const pickedOf = (pack: string) => Object.entries(picked).filter(([k, v]) => v && k.startsWith(pack + ':')).map(([k]) => k.slice(pack.length + 1));
  const applyUpdate = (pack: string) => run('upd:' + pack, async () => {
    const items = pickedOf(pack);
    if (!items.length) return toast(t('set.pickOne'));
    const r = await post('/updates/apply', { pack, items });
    go({ kind: 'chat', id: r.chatId });
  });

  async function reorganize() {
    const instructions = await promptDialog({ title: t('set.reorgTitle'), body: t('set.reorgBody'), placeholder: t('set.reorgPh'), confirmLabel: t('set.reorgStart') });
    if (instructions === null) return;
    await run('reorg', async () => { const r = await post('/reorganize', { instructions }); go({ kind: 'chat', id: r.chatId }); });
  }

  async function installSearch() {
    if (!(await confirmDialog({ title: t('set.searchExt'), body: t('set.searchConfirm'), list: [ext.search.repo], confirmLabel: t('set.install') }))) return;
    await run('search', async () => { await post('/extensions/search/install'); ext = await api('/extensions'); toast(t('set.searchInstalled')); });
  }

  const restart = () => run('restart', async () => { await reconnect(); toast(t('set.restarted')); });

  async function addPrivate() {
    privErr = '';
    const lib = String(base.libraryPath ?? '').replace(/\\/g, '/').replace(/\/+$/, '');
    let rel: string | null = null;
    if (inTauri()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const p = await open({ directory: true, defaultPath: lib || undefined });
        if (typeof p !== 'string') return;
        const abs = p.replace(/\\/g, '/').replace(/\/+$/, '');
        if (!lib || abs === lib || !abs.startsWith(lib + '/')) { privErr = t('set.privOutside'); return; }
        rel = abs.slice(lib.length + 1);
      } catch { rel = null; }
    }
    if (rel === null) {
      const typed = await promptDialog({ title: t('set.privAdd'), body: t('set.privTypeBody'), placeholder: 'clients/confidential' });
      if (typed === null) return;
      rel = typed.trim().replace(/\\/g, '/').replace(/^\.?\/+|\/+$/g, '');
      if (!rel || rel.split('/').includes('..') || /^([A-Za-z]:|~)/.test(rel)) { privErr = t('set.privOutside'); return; }
    }
    if (s.privatePaths.includes(rel)) { privErr = t('set.privDup'); return; }
    s.privatePaths = [...s.privatePaths, rel];
  }

  async function removePrivate(p: string) {
    if (!(await confirmDialog({ title: t('set.privRemoveTitle'), body: t('set.privRemoveBody'), list: [p], confirmLabel: t('set.remove'), danger: true }))) return;
    s.privatePaths = s.privatePaths.filter((x: string) => x !== p);
  }
</script>

<svelte:window onbeforeunload={onBeforeUnload} />

{#snippet err(key: string)}
  {#if errors[key]}<p class="err" id={'st-err-' + key} role="alert">{errors[key]}</p>{/if}
{/snippet}

{#snippet resetRow(c: Cat)}
  {#if resets[c] && !q}
    <div class="reset"><button type="button" class="btn ghost" onclick={() => resetCat(c)}><Icon name="rollback" size={14} /> {t('set.reset')}</button></div>
  {/if}
{/snippet}

{#snippet section(c: Cat)}
  {#if c === 'general'}
    <div class="card">
      {#if vis(c, 'language')}
        <label for="st-lang">{t('language')}</label>
        <select id="st-lang" bind:value={s.language}><option value="ar" lang="ar">العربية</option><option value="en" lang="en">English</option></select>
      {/if}
      {#if vis(c, 'set.name')}
        <label for="st-name">{t('set.name')}</label><input id="st-name" dir="auto" bind:value={s.userName} autocomplete="off" />
      {/if}
      {#if vis(c, 'theme', 'mode')}<ThemePicker bind:theme={s.theme} bind:mode={s.mode} />{/if}
      {#if vis(c, 'set.notify')}
        <label class="check"><input type="checkbox" bind:checked={s.notifications} /> {t('set.notify')}</label>
      {/if}
    </div>
  {:else if c === 'ai'}
    <div class="card">
      {#if vis(c, 'set.account')}
        <h3>{t('set.account')}</h3>
        <div class="row wrap">
          {#if claude.installed}
            <span class="pill" class:accent={claude.loggedIn}>{t(claude.loggedIn ? 'set.signedIn' : 'set.notSignedIn')}</span>
            {#if !claude.loggedIn}<button type="button" class="btn primary" disabled={busy.login} onclick={signIn}>{t('set.signIn')}</button>{/if}
          {:else}
            <span class="pill">{t('set.notInstalled')}</span>
            <a href="https://claude.com/claude-code" target="_blank" rel="noreferrer">{t('set.installClaude')} <Icon name="external" size={13} /></a>
          {/if}
          <button type="button" class="btn ghost" disabled={busy.claude} aria-busy={busy.claude} onclick={refreshClaude}><Icon name="sync" size={14} /> {busy.claude ? t('set.checking') : t('set.checkAgain')}</button>
        </div>
        <p class="muted sm">{t('set.accountHint')}</p>
      {/if}
      {#if vis(c, 'set.perm')}
        <fieldset class="perm" aria-describedby={errors.permission ? 'st-err-permission' : undefined}>
          <legend>{t('set.perm')}</legend>
          {#each perms as p}
            <label class="radio">
              <input type="radio" name="st-perm" value={p} checked={permValue === p} onchange={() => (s.ai.claude.permissionMode = p)} />
              <span><b>{t('set.perm.' + p)}</b><span class="muted sm blk">{t('set.perm.' + p + '.hint')}</span></span>
            </label>
          {/each}
        </fieldset>
        {@render err('permission')}
      {/if}
      {#if vis(c, 'set.wrapNag')}
        <label for="st-nag">{t('set.wrapNag')}</label>
        <input id="st-nag" class="num" type="number" min="1" step="1" bind:value={s.wrapNagThreshold} aria-invalid={!!errors.wrapNagThreshold} aria-describedby="st-nag-hint" />
        {@render err('wrapNagThreshold')}
        <p class="muted sm" id="st-nag-hint">{t('set.wrapDef')}</p>
      {/if}
    </div>
    {#if vis(c, ...adv.ai)}
      <details class="card adv" open={!!q || advOpen.ai} ontoggle={(e) => { if (!q) advOpen.ai = e.currentTarget.open; }}>
        <summary>{t('set.advanced')}</summary>
        {#if vis(c, 'set.model')}
          <label for="st-model">{t('set.model')}</label>
          <input id="st-model" dir="ltr" list="st-models" bind:value={s.ai.claude.model} placeholder={t('set.auto')} autocomplete="off" />
          <datalist id="st-models">{#each opts.models as m}<option value={m}></option>{/each}</datalist>
          <p class="muted sm">{t('set.modelHint')}</p>
        {/if}
        {#if vis(c, 'set.effort')}
          <label for="st-effort">{t('set.effort')}</label>
          <select id="st-effort" bind:value={s.ai.claude.effort}><option value={null}>{t('set.auto')}</option>{#each opts.efforts as m}<option value={m}>{tx('set.effort.' + m, m)}</option>{/each}</select>
        {/if}
        {#if vis(c, 'set.binary')}
          <label for="st-bin">{t('set.binary')}</label>
          <input id="st-bin" dir="ltr" bind:value={s.ai.claude.binary} placeholder={claude.path || t('set.auto')} autocomplete="off" />
          <p class="muted sm">{t('set.binaryHint')}{#if claude.version} <bdi>({claude.version})</bdi>{/if}</p>
        {/if}
        {#if vis(c, 'set.miniModel')}
          <label for="st-mini">{t('set.miniModel')}</label>
          <input id="st-mini" dir="ltr" bind:value={s.ai.claude.miniWrapModel} placeholder="haiku" autocomplete="off" />
          <p class="muted sm">{t('set.miniHint')}</p>
        {/if}
        {#if vis(c, 'set.idle')}
          <label for="st-idle">{t('set.idle')}</label>
          <input id="st-idle" class="num" type="number" min="1" step="1" bind:value={s.idleMinutes} aria-invalid={!!errors.idleMinutes} />
          {@render err('idleMinutes')}
        {/if}
        {#if vis(c, 'set.provider', 'set.baseUrl', 'set.apiKey')}
          <h3>{t('set.provider')}</h3>
          <p class="muted sm">{t('set.providerHint')}</p>
          <label for="st-base">{t('set.baseUrl')}</label>
          <input id="st-base" dir="ltr" bind:value={s.ai.claude.baseUrl} placeholder="https://api.anthropic.com" aria-invalid={!!errors.baseUrl} autocomplete="off" />
          {@render err('baseUrl')}
          <label for="st-key">{t('set.apiKey')} {#if s.ai.claude.hasApiToken}<span class="pill accent">{t('set.keyOn')}</span>{/if}</label>
          <div class="row wrap">
            <input id="st-key" dir="ltr" type="password" class="grow" bind:value={apiToken} placeholder="sk-…" autocomplete="off" />
            <button type="button" class="btn" disabled={!apiToken.trim() || busy.token} onclick={saveToken}>{t('set.storeKey')}</button>
            <button type="button" class="btn danger" disabled={!s.ai.claude.hasApiToken || busy.token} onclick={clearToken}>{t('set.clearKey')}</button>
          </div>
          <p class="muted sm">{t('set.apiKeyHint')}</p>
        {/if}
      </details>
    {/if}
  {:else if c === 'git'}
    <div class="card">
      {#if vis(c, 'set.libraryPath')}
        <label for="st-lib">{t('set.libraryPath')}</label>
        <div class="row"><input id="st-lib" class="grow" dir="ltr" bind:value={s.libraryPath} aria-invalid={!!errors.libraryPath} autocomplete="off" /><button type="button" class="btn" onclick={browseLibrary}><Icon name="folder" size={14} /> {t('set.browse')}</button></div>
        {@render err('libraryPath')}
        <p class="muted sm">{t('set.libraryHint')}</p>
      {/if}
      {#if vis(c, 'set.backup')}
        <label for="st-sync">{t('set.backup')}</label>
        <select id="st-sync" value={syncKind} onchange={(e) => setSyncKind(e.currentTarget.value)} aria-invalid={!!errors.sync}>
          <option value="none">{t('set.backupNone')}</option>
          <option value="folder">{t('set.backupFolder')}</option>
          <option value="git">{t('set.backupGit')}</option>
        </select>
        {@render err('sync')}
        <p class="muted sm">{t(syncKind === 'none' ? 'set.backupNoneHint' : syncKind === 'folder' ? 'set.backupFolderHint' : 'set.backupGitHint')}</p>
      {/if}
      {#if vis(c, 'reorganize')}
        <h3>{t('reorganize')}</h3>
        <div class="row wrap"><button type="button" class="btn" disabled={busy.reorg} onclick={reorganize}>{t('reorganize')}…</button><span class="muted sm grow">{t('set.reorgHint')}</span></div>
      {/if}
    </div>
    {#if vis(c, ...adv.git)}
      <details class="card adv" open={!!q || advOpen.git} ontoggle={(e) => { if (!q) advOpen.git = e.currentTarget.open; }}>
        <summary>{t('set.advancedGit')}</summary>
        {#if !isGit}<p class="muted sm">{t('set.gitOff')}</p>{/if}
        {#if isGit && vis(c, 'set.gitWhen')}
          <label for="st-gitmode">{t('set.gitWhen')}</label>
          <select id="st-gitmode" bind:value={s.sync.mode}>
            <option value="git-manual">{t('set.gitManual')}</option>
            <option value="git-auto">{t('set.gitAuto')}</option>
          </select>
        {/if}
        {#if isGit && vis(c, 'set.credStore')}
          <label for="st-cred">{t('set.credStore')}</label>
          <select id="st-cred" bind:value={s.sync.credentialStore}>
            <option value="keychain">{t('set.credKeychain')}</option>
            <option value="file">{t('set.credFile')}</option>
          </select>
        {/if}
        {#if isGit && vis(c, 'set.remotes')}
          <h3>{t('set.remotes')}</h3>
          {#if loadErr.git}<p class="err" role="alert">{loadErr.git} <button type="button" class="btn ghost" onclick={loadGit}>{t('set.retry')}</button></p>{/if}
          {#if gitStatus.repo}<p class="muted sm">{t('set.gitState', { branch: gitStatus.branch, changed: fmtNum(gitStatus.dirty ?? 0), ahead: fmtNum(gitStatus.ahead ?? 0), behind: fmtNum(gitStatus.behind ?? 0) })}</p>{/if}
          {#each gitStatus.remotes || [] as r, i (r.name)}
            <div class="row remote"><b dir="auto">{r.name}</b>{#if i === 0}<span class="pill">{t('set.remoteMain')}</span>{/if}<code class="grow sm path"><bdi>{r.url}</bdi></code><button type="button" class="btn ghost danger" disabled={busy.remote} onclick={() => removeRemote(r)}>{t('set.remove')}</button></div>
          {:else}
            {#if !loading.git && !loadErr.git}<p class="muted sm">{t('set.noRemotes')}</p>{/if}
          {/each}
          <p class="muted sm">{t('set.remotesHint')}</p>
          <div class="grid2">
            <div><label for="st-rurl">{t('set.remoteUrl')}</label><input id="st-rurl" dir="ltr" bind:value={remote.url} placeholder="https://github.com/me/thoughts.git" autocomplete="off" /></div>
            <div><label for="st-rname">{t('set.remoteName')}</label><input id="st-rname" dir="ltr" bind:value={remote.name} placeholder="origin" autocomplete="off" /></div>
            <div><label for="st-ruser">{t('set.remoteUser')}</label><input id="st-ruser" dir="ltr" bind:value={remote.username} autocomplete="off" /></div>
            <div><label for="st-rtoken">{t('set.remoteToken')}</label><input id="st-rtoken" dir="ltr" type="password" bind:value={remote.token} autocomplete="off" /></div>
          </div>
          {#if remote.url.trim() && !validRemote}<p class="err" role="alert">{t('set.errRemote')}</p>{/if}
          <p class="note"><Icon name="warning" size={15} /> <span>{t('set.uploadBody')}</span></p>
          <button type="button" class="btn" onclick={addRemote} disabled={!validRemote || busy.remote}>{t('set.addRemote')}</button>
        {/if}
        {#if ext && vis(c, 'set.searchExt')}
          <h3>{t('set.searchExt')}</h3>
          <p class="muted sm">{t('set.searchHint', { files: fmtNum(ext.search.files ?? 0), threshold: fmtNum(ext.search.threshold ?? 0) })}</p>
          {#if ext.search.installed}<span class="pill accent">{t('set.installed')}</span>{:else}<button type="button" class="btn" disabled={busy.search} onclick={installSearch}>{busy.search ? t('set.installing') : t('set.install')}</button>{/if}
        {/if}
      </details>
    {/if}
  {:else if c === 'privacy'}
    {#if vis(c, 'set.privTitle')}
      <div class="card">
        <h3>{t('set.privSentTitle')}</h3>
        <p class="muted sm">{t('set.privSent')}</p>
        <h3>{t('set.privTitle')}</h3>
        <p class="muted sm">{t('set.privBody')}</p>
        {#each s.privatePaths as p (p)}
          <div class="row remote"><Icon name="shield" size={15} /><code class="grow path"><bdi>{p}</bdi></code><button type="button" class="btn ghost danger" onclick={() => removePrivate(p)}>{t('set.remove')}</button></div>
        {:else}
          <p class="muted sm">{t('set.privEmpty')}</p>
        {/each}
        {#if privErr}<p class="err" role="alert">{privErr}</p>{/if}
        <div class="row gap-top"><button type="button" class="btn" onclick={addPrivate}><Icon name="plus" size={14} /> {t('set.privAdd')}</button></div>
      </div>
    {/if}
    {#if vis(c, 'set.cost')}
      <div class="card">
        <h3>{t('set.cost')}</h3>
        {#if typeof cost === 'number'}
          <p class="cost"><bdi>${fmtNum(Math.round(cost * 100) / 100)}</bdi></p>
        {:else}
          <p class="muted sm">{t('set.costNone')}</p>
        {/if}
        <p class="muted sm">{t('set.costHint')}</p>
      </div>
    {/if}
  {:else if c === 'modules'}
    <div class="card">
      <p class="muted">{t('set.modulesIntro')}</p>
      {#if loading.modules}<p class="muted sm" role="status">{t('set.loading')}</p>
      {:else if loadErr.modules}<p class="err" role="alert">{loadErr.modules} <button type="button" class="btn ghost" onclick={loadModules}>{t('set.retry')}</button></p>
      {:else}
        {#each modules as m (m.id)}
          <div class="row mod">
            <div class="grow"><b>{(s.language === 'ar' && m.nameAr) || m.name}</b><div class="muted sm">{(s.language === 'ar' && m.descriptionAr) || m.description}</div></div>
            {#if s.modules?.[m.id]}<span class="pill accent">{t('enabled')}</span>
            {:else}<button type="button" class="btn" disabled={busy['mod:' + m.id]} onclick={() => enableModule(m.id)}>{busy['mod:' + m.id] ? t('set.enabling') : t('enable')}</button>{/if}
          </div>
        {:else}
          <p class="muted sm">{t('set.noModules')}</p>
        {/each}
      {/if}
    </div>
  {:else if c === 'daemon'}
    <div class="card">
      <p class="muted">{t('set.daemonIntro')}</p>
      {#if vis(c, 'set.keepRunning')}<label class="check"><input type="checkbox" bind:checked={s.daemon.keepRunning} /> {t('set.keepRunning')}</label>{/if}
      {#if vis(c, 'set.runAtLogin')}<label class="check"><input type="checkbox" bind:checked={s.daemon.runAtLogin} /> <span>{t('set.runAtLogin')} {#if app.status?.platform !== 'macos'}<span class="muted">{t('set.macOnly')}</span>{/if}</span></label>{/if}
    </div>
    {#if vis(c, ...adv.daemon)}
      <details class="card adv" open={!!q || advOpen.daemon} ontoggle={(e) => { if (!q) advOpen.daemon = e.currentTarget.open; }}>
        <summary>{t('set.advanced')}</summary>
        <p class="muted sm">{t('set.restartHint')}</p>
        <button type="button" class="btn" disabled={busy.restart} onclick={restart}>{busy.restart ? t('set.restarting') : t('set.restart')}</button>
      </details>
    {/if}
  {:else if c === 'appLayer'}
    {#if vis(c, 'customize', 'rollback')}
      <div class="card">
        <h3>{t('customize')}</h3>
        <p class="muted sm">{t('set.customizeHint')}</p>
        <form class="row wrap" onsubmit={(e) => { e.preventDefault(); customize(); }}>
          <input class="grow" dir="auto" bind:value={customizeText} aria-label={t('customize')} placeholder={t('set.customizePh')} />
          <button type="submit" class="btn primary" disabled={!customizeText.trim() || busy.customize}>{busy.customize ? t('set.starting') : t('set.customizeGo')}</button>
        </form>
        <div class="row gap-top"><button type="button" class="btn" onclick={() => (app.showRollback = true)}><Icon name="rollback" size={15} /> {t('rollback')}…</button></div>
      </div>
    {/if}
    {#if vis(c, 'updates')}
      <div class="card">
        <h3>{t('updates')}</h3>
        <p class="muted sm">{t('set.updatesHint')}</p>
        {#if loading.updates}<p class="muted sm" role="status">{t('set.loading')}</p>
        {:else if loadErr.updates}<p class="err" role="alert">{loadErr.updates} <button type="button" class="btn ghost" onclick={loadUpdates}>{t('set.retry')}</button></p>
        {:else}
          {#each updates as u (u.id)}
            <div class="upd"><b dir="auto">{u.title}</b>
              {#each u.items as it (it.id)}<label class="check"><input type="checkbox" bind:checked={picked[u.id + ':' + it.id]} /> <span dir="auto"><b>{it.title}</b> — {it.description}</span></label>{/each}
              <button type="button" class="btn" disabled={!pickedOf(u.id).length || busy['upd:' + u.id]} onclick={() => applyUpdate(u.id)}>{busy['upd:' + u.id] ? t('set.starting') : t('set.applyUpdate')}</button>
            </div>
          {:else}
            <p class="muted sm">{t('set.noUpdates')}</p>
          {/each}
        {/if}
      </div>
    {/if}
    {#if vis(c, ...adv.appLayer)}
      <details class="card adv" open={!!q || advOpen.appLayer} ontoggle={(e) => { if (!q) advOpen.appLayer = e.currentTarget.open; }}>
        <summary>{t('set.advanced')}</summary>
        <p class="muted sm">{t('set.safeHint')}</p>
        <button type="button" class="btn" onclick={() => location.replace(location.pathname + '?safe=1')}>{t('set.safeMode')}</button>
      </details>
    {/if}
  {/if}
  {@render resetRow(c)}
{/snippet}

<div class="settings">
  <nav class="cats" aria-label={t('settings')}>
    <h1>{t('settings')}</h1>
    <div class="find">
      <Icon name="search" size={14} />
      <input type="search" bind:value={query} placeholder={t('set.searchPh')} aria-label={t('set.searchPh')} autocomplete="off" />
    </div>
    {#each cats as k}
      {#if !q || catShown(k)}
        <button type="button" class="cat" class:on={!q && tab === k} aria-current={!q && tab === k ? 'page' : undefined} onclick={() => { tab = k; query = ''; }}><span class="ci"><Icon name={catIcon[k]} size={15} /></span>{t('set.cat.' + k)}</button>
      {/if}
    {/each}
  </nav>
  <div class="content">
    {#each shown as c (c)}
      <section aria-labelledby={'st-h-' + c}>
        <h2 class="title" id={'st-h-' + c}>{t('set.cat.' + c)}</h2>
        {@render section(c)}
      </section>
    {:else}
      <p class="muted none" role="status">{t('set.noMatch', { q: query.trim() })}</p>
    {/each}

    {#if dirty}
      <div class="savebar" role="region" aria-label={t('set.unsaved')}>
        <span class="dot" aria-hidden="true"></span>
        <span class="grow" role="status">{t('set.unsaved')}{#if invalid} — {t('set.fixFirst')}{/if}</span>
        <button type="button" class="btn" disabled={busy.save} onclick={discard}>{t('set.discard')}</button>
        <button type="button" class="btn primary" disabled={busy.save || invalid} aria-busy={busy.save} onclick={save}>{busy.save ? t('set.saving') : t('save')}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .settings { display: grid; grid-template-columns: 220px 1fr; min-height: calc(100vh - 26px); }
  .cats { padding-block: 6px 20px; padding-inline: 16px 10px; border-inline-end: 1px solid var(--hairline); display: flex; flex-direction: column; gap: 2px; }
  .cats h1 { font-size: 20px; margin: 4px 6px 12px; }
  .find { position: relative; margin-bottom: 8px; color: var(--muted); }
  .find :global(.icon) { position: absolute; inset-inline-start: 9px; inset-block-start: 50%; transform: translateY(-50%); pointer-events: none; }
  .find input { padding-inline-start: 30px; font-size: 13px; }
  .cat { font: inherit; color: inherit; background: none; border: 0; text-align: start; cursor: pointer; display: flex; align-items: center; gap: 10px; padding: 5px 6px; border-radius: 7px; font-size: 13px; width: 100%; }
  .cat:hover { background: var(--fill); }
  .cat.on { background: var(--accent); color: var(--accent-fg); }
  .ci { width: 24px; height: 24px; border-radius: 6px; display: grid; place-items: center; background: var(--tint); color: var(--accent); flex: none; }
  .cat.on .ci { background: rgba(255,255,255,.22); color: inherit; }
  .content { padding: 6px 28px 28px; max-width: 760px; min-width: 0; }
  .title { font-size: 20px; margin: 4px 0 14px; }
  section + section { margin-top: 26px; }
  h3 { font-size: 14px; font-weight: 600; margin: 18px 0 6px; }
  .card > h3:first-child { margin-top: 0; }
  .savebar { position: sticky; inset-block-end: 12px; z-index: 5; margin-top: 16px; padding: 10px 14px; display: flex; gap: 8px; align-items: center; background: var(--glass, var(--panel)); -webkit-backdrop-filter: blur(24px); backdrop-filter: blur(24px); border: 1px solid var(--hairline); border-radius: var(--radius-lg); box-shadow: var(--shadow-lg); font-size: 13px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); flex: none; }
  .card { margin-bottom: 12px; }
  .check, .radio { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; }
  .radio { align-items: flex-start; margin: 8px 0; cursor: pointer; }
  .radio input { margin-top: 4px; }
  .blk { display: block; font-weight: 400; }
  .perm { border: 0; padding: 0; margin: 14px 0 0; min-width: 0; }
  .perm legend { font-size: 12px; font-weight: 500; color: var(--muted); padding: 0; }
  .num { max-width: 120px; }
  .adv > summary { cursor: pointer; font-size: 13px; font-weight: 600; }
  .adv[open] > summary { margin-bottom: 6px; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 0 10px; margin: 4px 0 8px; }
  .wrap { flex-wrap: wrap; }
  .gap-top { margin-top: 10px; }
  .mod, .remote { padding: 10px 0; border-bottom: 1px solid var(--hairline); }
  .mod:last-child { border-bottom: 0; }
  .path { overflow-wrap: anywhere; }
  .upd { padding: 8px 0; }
  .err { color: var(--danger); font-size: 12.5px; margin: 4px 0 0; }
  .note { display: flex; gap: 8px; align-items: flex-start; font-size: 12.5px; margin: 8px 0; }
  .note :global(.icon) { flex: none; margin-top: 3px; color: var(--danger); }
  .cost { font-size: 26px; font-weight: 500; margin: 0 0 4px; }
  .reset { display: flex; justify-content: flex-end; }
  .none { padding: 24px 0; }
  .sm { font-size: 12px; }
  @media (max-width: 720px) { .settings { grid-template-columns: 1fr; } .cats { border-inline-end: 0; } .grid2 { grid-template-columns: 1fr; } }
</style>
