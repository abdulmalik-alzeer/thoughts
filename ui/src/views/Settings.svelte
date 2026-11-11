<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onDestroy, onMount, untrack } from 'svelte';
  import { api, del, inTauri, post, reloadApp } from '../lib/api';
  import { a11y, app, applyTheme, go, saveSettings, setA11y, setShowCost, toast, zoomStep, A11Y_DEFAULTS, ZOOM_STEPS, type A11y } from '../lib/store.svelte';
  import { recheckSignIn } from '../lib/auth';
  import { isolatedKeys, tip } from '../lib/shortcuts';
  import { choiceDialog, confirmDialog, promptDialog } from '../lib/dialog.svelte';
  import { fmtMoney, fmtNum, t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { restartService } from '../lib/shell';
  import ThemePicker from './ThemePicker.svelte';
  import Customise from './Customise.svelte';

  let { reload }: { reload: () => Promise<void> } = $props();

  const cats = ['general', 'accessibility', 'ai', 'git', 'privacy', 'modules', 'daemon', 'appLayer'] as const;
  type Cat = (typeof cats)[number];
  const catIcon: Record<Cat, string> = { general: 'settings', accessibility: 'accessibility', ai: 'ai', git: 'sync', privacy: 'shield', modules: 'modules', daemon: 'daemon', appLayer: 'palette' };
  // Row labels per category (i18n keys) — used by the search box. `adv` rows live behind the Advanced disclosure.
  const basic: Record<Cat, string[]> = {
    general: ['language', 'set.name', 'ui.set.profession', 'theme', 'mode', 'set.notify'],
    accessibility: ['a11y.zoom', 'a11y.motion', 'a11y.links', 'a11y.focus', 'a11y.spacing'],
    ai: ['set.account', 'set.perm', 'set.wrapNag'],
    git: ['set.libraryPath', 'set.backup', 'reorganize'],
    privacy: ['set.privTitle', 'layer.signalsTitle', 'set.cost', 'ui.set.showCost'],
    modules: ['set.modulesIntro'],
    daemon: ['set.keepRunning', 'set.runAtLogin'],
    appLayer: ['layer.customise', 'layer.history', 'layer.share', 'layer.workflows', 'layer.appUpdates'],
  };
  const adv: Record<Cat, string[]> = {
    general: [],
    accessibility: [],
    ai: ['set.model', 'set.effort', 'set.binary', 'set.miniModel', 'set.idle', 'set.provider', 'set.baseUrl', 'set.apiKey'],
    git: ['set.gitWhen', 'set.credStore', 'set.remotes', 'set.searchExt'],
    privacy: [],
    modules: [],
    daemon: ['set.restart'],
    appLayer: ['set.safeMode'],
  };
  // Claude Code's own mode names. "Ask before changes" (`default`) was removed — the app has no way
  // to answer the questions — so a stored `default` means plan mode. Bypass is confirmed in a dialog.
  const BYPASS = 'bypassPermissions';
  const perms = ['acceptEdits', 'plan', BYPASS] as const;
  const professions = ['finance', 'legal', 'consulting', 'teaching', 'research', 'founder', 'personal', 'other'];
  const cloudWords = ['Mobile Documents', 'Library/CloudStorage', 'Dropbox', 'OneDrive', 'Google Drive', 'iCloud'];

  const fresh = () => {
    const x = structuredClone($state.snapshot(app.settings));
    x.privatePaths ??= []; x.usageSignals ??= true; x.profession ??= null;
    if (x.ai?.claude && (x.ai.claude.permissionMode == null || x.ai.claude.permissionMode === '' || x.ai.claude.permissionMode === 'default')) x.ai.claude.permissionMode = 'plan';
    return x;
  };
  const norm = (x: any) => JSON.stringify(x, (_k, v) => (v === '' || v === undefined ? null : v));

  let tab = $state<Cat>((cats as readonly string[]).includes(app.view.id ?? '') ? (app.view.id as Cat) : 'general');
  let query = $state('');
  let s = $state<any>(fresh());
  let base = $state<any>(fresh()); // last saved state; `s` is the draft
  let opts = $state<any>({ models: [], efforts: [], permissionModes: [] });
  let claude = $state<any>(app.status?.claude ?? {});
  let gitStatus = $state<any>({});
  let modules = $state<any[]>([]);
  let loadErr = $state<Record<string, string>>({});
  let loading = $state<Record<string, boolean>>({ modules: true });
  let busy = $state<Record<string, boolean>>({});
  let ext = $state<any>(null);
  let apiToken = $state('');
  let remote = $state({ name: '', url: '', username: '', token: '' });
  let privErr = $state('');
  let advOpen = $state<Record<string, boolean>>({});

  const isDirty = () => norm(s) !== norm(base);
  const dirty = $derived(isDirty());
  const isGit = $derived(String(s.sync.mode).startsWith('git'));
  const syncKind = $derived(isGit ? 'git' : s.sync.mode === 'folder' ? 'folder' : 'none');
  const pm = $derived(s.ai.claude.permissionMode);
  const permValue = $derived(pm == null || pm === '' || pm === 'default' ? 'plan' : (perms as readonly string[]).includes(pm) ? pm : '');
  /** A settings file written by a newer version is never rewritten: everything here is read-only. */
  const readOnly = $derived(!!app.status?.settingsNewer);
  // The look is saved the moment it is chosen (ThemePicker), so it is never part of the unsaved draft.
  // The same goes for the accessibility preferences (zoom changes from the keyboard too).
  $effect(() => {
    const th = app.settings?.theme, m = app.settings?.mode, ax = JSON.stringify(app.settings?.accessibility ?? null);
    untrack(() => { s.theme = th; s.mode = m; base.theme = th; base.mode = m; s.accessibility = JSON.parse(ax); base.accessibility = JSON.parse(ax); });
  });
  const ax = $derived(a11y());
  const zoomChoices = $derived([...new Set([...ZOOM_STEPS, ax.zoom])].sort((a, b) => a - b));
  const setAx = (patch: Partial<A11y>) => { setA11y(patch).catch((e) => failed(t('a11y.saveFailed'), e)); };
  const stepZoom = (d: -1 | 1) => { zoomStep(d).catch((e) => failed(t('a11y.saveFailed'), e)); };
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
  const loadGit = () => load('git', '/git', (g) => (gitStatus = g));

  onMount(() => {
    api('/ai/claude/options').then((o) => (opts = o)).catch(() => {});
    api('/extensions').then((e) => (ext = e)).catch(() => {});
    loadGit(); loadModules();
    app.leaveGuard = leaveGuard;
  });

  // Plain headline per action; whatever the background service said goes in the detail.
  const FAIL: Record<string, string> = { save: 'ui.set.saveFailed', mod: 'ui.set.modFailed', remote: 'ui.set.remoteFailed', token: 'ui.set.keyFailed', login: 'ui.set.loginFailed', reorg: 'ui.set.reorgFailed', search: 'ui.set.searchFailed' };
  /** Runs one action with a loading flag; any failure is shown to the user. */
  async function run(key: string, fn: () => Promise<unknown>) {
    if (busy[key]) return;
    busy[key] = true;
    try { await fn(); } catch (e) { failed(t(FAIL[key.split(':')[0]] ?? 'set.failed'), e); }
    finally { busy[key] = false; }
  }

  function payload() {
    const x = structuredClone($state.snapshot(s));
    const c = x.ai.claude;
    for (const k of ['binary', 'model', 'effort', 'permissionMode', 'miniWrapModel', 'baseUrl']) c[k] = typeof c[k] === 'string' && c[k].trim() ? c[k].trim() : null;
    x.libraryPath = String(x.libraryPath ?? '').trim();
    return x;
  }

  // ----- bypass permissions: never switched on without the warning -----
  /** True once the user agreed to the warning for the draft that is being edited. */
  let bypassOk = false;
  const turnsBypassOn = () => s.ai.claude.permissionMode === BYPASS && app.settings?.ai?.claude?.permissionMode !== BYPASS;
  const askBypass = () => confirmDialog({
    title: t('perm.bypass.title'), body: `${t('perm.bypass.body')}\n\n${t('perm.bypass.bodyDefault')}`,
    note: t('perm.bypass.note'), confirmLabel: t('perm.bypass.confirm'), danger: true,
  });
  /** A click on a permission radio. Choosing bypass waits for the dialog; the radio only moves on a yes. */
  async function pickPerm(p: string, e: MouseEvent) {
    if (p !== BYPASS || permValue === BYPASS) { s.ai.claude.permissionMode = p; return; }
    e.preventDefault();
    if (!(await askBypass())) return;
    bypassOk = true;
    s.ai.claude.permissionMode = BYPASS;
  }
  /** Write the draft. The service refuses to switch bypass on without the confirmation sent here. */
  async function persist() {
    const confirm = turnsBypassOn();
    await saveSettings(payload(), confirm ? `?confirm=${BYPASS}` : '');
    bypassOk = false;
    s = fresh(); base = fresh();
    toast(t('set.saved'), { kind: 'success' });
  }

  async function save() {
    if (readOnly) return;
    if (invalid) return toast(t('set.fixFirst'), { kind: 'error' });
    if (turnsBypassOn() && !bypassOk) { if (!(await askBypass())) return; bypassOk = true; }
    await run('save', persist);
  }

  function discard() { s = structuredClone($state.snapshot(base)); bypassOk = false; applyTheme(); }

  // Leaving with unsaved edits: navigation waits for this (`app.leaveGuard`, awaited inside `go()`).
  async function leaveGuard(): Promise<boolean> {
    if (!isDirty() || readOnly) return true;
    const bad = invalid;
    const choice = await choiceDialog({
      title: t('set.leaveTitle'), body: t(bad ? 'layer.leaveInvalid' : 'set.leaveBody'), cancelLabel: t('layer.keepEditing'),
      choices: [{ id: 'discard', label: t('set.discard'), danger: true }, ...(bad ? [] : [{ id: 'save', label: t('save'), primary: true }])],
    });
    if (choice === 'discard') { discard(); return true; }
    if (choice !== 'save') return false;
    if (turnsBypassOn() && !bypassOk) { if (!(await askBypass())) return false; bypassOk = true; }
    try { await persist(); return true; }
    catch (e) { failed(t('ui.set.saveFailed'), e); return false; }
  }
  onDestroy(() => { if (app.leaveGuard === leaveGuard) app.leaveGuard = null; });

  function onBeforeUnload(e: BeforeUnloadEvent) { if (isDirty()) { e.preventDefault(); e.returnValue = ''; } }

  const resets: Partial<Record<Cat, { list: string[]; apply: () => void }>> = {
    general: { list: ['theme', 'mode', 'set.notify'], apply: () => {
      s.notifications = true;
      // The look applies (and saves) at once, like choosing it in the picker.
      saveSettings({ theme: 'saudi', mode: 'system' }).catch((e) => failed(t('shell.themeNotSaved'), e));
    } },
    accessibility: { list: basic.accessibility, apply: () => setAx({ ...A11Y_DEFAULTS }) },
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

  // "Check again" also forgets what a failed run said about the sign-in, and asks the CLI afresh.
  const refreshClaude = () => run('claude', async () => { await recheckSignIn(); claude = app.status?.claude ?? {}; });
  $effect(() => { const c = app.status?.claude; if (c) untrack(() => (claude = c)); });
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
    } catch { toast(t('set.browseFail'), { kind: 'error' }); }
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

  /** Turning a feature off hides it; nothing the user wrote with it is deleted. */
  async function disableModule(m: any) {
    const name = (s.language === 'ar' && m.nameAr) || m.name;
    if (!(await confirmDialog({ title: t('ui.set.modOffTitle', { name }), body: t('ui.set.modOffBody'), confirmLabel: t('ui.set.modOff') }))) return;
    await run('mod:' + m.id, async () => {
      await post(`/modules/${m.id}/disable`);
      await syncServerFields();
      await loadModules();
      toast(t('ui.set.modOffDone', { name }), { kind: 'success' });
    });
  }

  async function reorganize() {
    const instructions = await promptDialog({ title: t('set.reorgTitle'), body: t('set.reorgBody'), placeholder: t('set.reorgPh'), confirmLabel: t('set.reorgStart') });
    if (instructions === null) return;
    await run('reorg', async () => { const r = await post('/reorganize', { instructions }); go({ kind: 'chat', id: r.chatId }); });
  }

  async function installSearch() {
    if (!(await confirmDialog({ title: t('set.searchExt'), body: t('set.searchConfirm'), list: [ext.search.repo], confirmLabel: t('set.install') }))) return;
    await run('search', async () => { await post('/extensions/search/install'); ext = await api('/extensions'); toast(t('set.searchInstalled')); });
  }

  async function restart() {
    if (busy.restart) return;
    busy.restart = true;
    try { await restartService(); } finally { busy.restart = false; }
  }

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
      {#if vis(c, 'ui.set.profession')}
        <label for="st-prof">{t('ui.set.profession')}</label>
        <select id="st-prof" bind:value={s.profession} aria-describedby="st-prof-hint">
          <option value={null}>{t('ui.set.professionNone')}</option>
          {#each professions as p}<option value={p}>{t('layer.prof.' + p)}</option>{/each}
          {#if s.profession && !professions.includes(s.profession)}<option value={s.profession}>{s.profession}</option>{/if}
        </select>
        <p class="muted sm" id="st-prof-hint">{t('ui.set.professionHint')}</p>
      {/if}
      {#if vis(c, 'theme', 'mode')}<ThemePicker bind:theme={s.theme} bind:mode={s.mode} />{/if}
      {#if vis(c, 'set.notify')}
        <label class="check"><input type="checkbox" bind:checked={s.notifications} /> {t('set.notify')}</label>
      {/if}
    </div>
  {:else if c === 'accessibility'}
    <div class="card">
      {#if vis(c, 'a11y.zoom')}
        <label for="st-zoom">{t('a11y.zoom')}</label>
        <div class="row wrap">
          <button type="button" class="btn" disabled={ax.zoom <= ZOOM_STEPS[0]} onclick={() => stepZoom(-1)} aria-label={t('a11y.zoomOut')} title={tip(t('a11y.zoomOut'), 'zoom.out')}><Icon name="zoomOut" size={15} /></button>
          <select id="st-zoom" class="num" value={ax.zoom} onchange={(e) => setAx({ zoom: +e.currentTarget.value })} aria-describedby="st-zoom-hint">
            {#each zoomChoices as z}<option value={z}>{fmtNum(z)}%</option>{/each}
          </select>
          <button type="button" class="btn" disabled={ax.zoom >= ZOOM_STEPS[ZOOM_STEPS.length - 1]} onclick={() => stepZoom(1)} aria-label={t('a11y.zoomIn')} title={tip(t('a11y.zoomIn'), 'zoom.in')}><Icon name="zoomIn" size={15} /></button>
          <button type="button" class="btn ghost" disabled={ax.zoom === 100} onclick={() => setAx({ zoom: 100 })}>{t('a11y.zoomReset')}</button>
        </div>
        <p class="muted sm" id="st-zoom-hint">{t('a11y.zoomHint', { in: isolatedKeys('zoom.in'), out: isolatedKeys('zoom.out'), reset: isolatedKeys('zoom.reset') })}</p>
      {/if}
      {#each [['a11y.motion', 'reduceMotion'], ['a11y.links', 'underlineLinks'], ['a11y.focus', 'strongFocus'], ['a11y.spacing', 'textSpacing']] as [key, field]}
        {#if vis(c, key)}
          <label class="check ax"><input type="checkbox" checked={!!ax[field as keyof A11y]} onchange={(e) => setAx({ [field]: e.currentTarget.checked } as Partial<A11y>)} aria-describedby={'st-' + field + '-hint'} /> {t(key)}</label>
          <p class="muted sm axhint" id={'st-' + field + '-hint'}>{t(key + 'Hint')}</p>
        {/if}
      {/each}
      {#if !q}
        <p class="muted sm row wrap gap-top"><span>{t('a11y.contrast')}</span> <button type="button" class="btn ghost" onclick={() => (tab = 'general')}>{t('a11y.contrastOpen')}</button></p>
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
              <input type="radio" name="st-perm" value={p} checked={permValue === p} onclick={(e) => pickPerm(p, e)} />
              <span><b class:danger-text={p === BYPASS}>{t('set.perm.' + p)}</b><span class="muted sm blk">{t('set.perm.' + p + '.hint')}</span></span>
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
          <p class="muted sm">{t(s.sync.credentialStore === 'file' ? 'ui.set.credFileHint' : 'ui.set.credKeychainHint')}</p>
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
    {#if vis(c, 'layer.signalsTitle')}
      <div class="card">
        <h3>{t('layer.signalsTitle')}</h3>
        <p class="muted sm" id="st-signals-hint">{t('layer.signalsBody')}</p>
        <label class="check"><input type="checkbox" bind:checked={s.usageSignals} aria-describedby="st-signals-hint" /> {t('layer.signalsToggle')}</label>
      </div>
    {/if}
    {#if vis(c, 'set.cost')}
      <div class="card">
        <h3>{t('set.cost')}</h3>
        {#if typeof cost === 'number'}
          <p class="cost"><bdi>{fmtMoney(cost)}</bdi></p>
        {:else}
          <p class="muted sm">{t('set.costNone')}</p>
        {/if}
        <p class="muted sm">{t('set.costHint')}</p>
        {#if vis(c, 'ui.set.showCost')}
          <label class="check"><input type="checkbox" checked={app.showCost} onchange={(e) => setShowCost(e.currentTarget.checked)} aria-describedby="st-cost-hint" /> {t('ui.set.showCost')}</label>
          <p class="muted sm" id="st-cost-hint">{t('ui.set.showCostHint')}</p>
        {/if}
      </div>
    {/if}
  {:else if c === 'modules'}
    <div class="card">
      <p class="muted">{t('set.modulesIntro')} {t('ui.set.modKeeps')}</p>
      {#if loading.modules}<p class="muted sm" role="status">{t('set.loading')}</p>
      {:else if loadErr.modules}<p class="err" role="alert">{loadErr.modules} <button type="button" class="btn ghost" onclick={loadModules}>{t('set.retry')}</button></p>
      {:else}
        {#each modules as m (m.id)}
          <div class="row mod">
            <div class="grow"><b>{(s.language === 'ar' && m.nameAr) || m.name}</b><div class="muted sm">{(s.language === 'ar' && m.descriptionAr) || m.description}</div></div>
            {#if s.modules?.[m.id]}<span class="pill accent">{t('enabled')}</span>
              {#if m.canDisable !== false}<button type="button" class="btn ghost" disabled={busy['mod:' + m.id]} onclick={() => disableModule(m)}>{t('ui.set.modOff')}</button>{/if}
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
      {#if vis(c, 'set.runAtLogin')}<label class="check"><input type="checkbox" bind:checked={s.daemon.runAtLogin} /> <span>{t('set.runAtLogin')} {#if app.status?.autostart?.supported === false || app.status?.autostartSupported === false}<span class="muted">{t('set.macOnly')}</span>{/if}</span></label>{/if}
    </div>
    {#if vis(c, ...adv.daemon)}
      <details class="card adv" open={!!q || advOpen.daemon} ontoggle={(e) => { if (!q) advOpen.daemon = e.currentTarget.open; }}>
        <summary>{t('set.advanced')}</summary>
        <p class="muted sm">{t('set.restartHint')}</p>
        <button type="button" class="btn" disabled={busy.restart} onclick={restart}>{busy.restart ? t('set.restarting') : t('set.restart')}</button>
      </details>
    {/if}
  {:else if c === 'appLayer'}
    <Customise show={(...keys) => vis(c, ...keys)} />
    {#if vis(c, ...adv.appLayer)}
      <details class="card adv" open={!!q || advOpen.appLayer} ontoggle={(e) => { if (!q) advOpen.appLayer = e.currentTarget.open; }}>
        <summary>{t('set.advanced')}</summary>
        <p class="muted sm">{t('set.safeHint')}</p>
        <button type="button" class="btn" onclick={() => reloadApp('safe=1')}>{t('set.safeMode')}</button>
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
    {#if readOnly}
      <div class="ro card" role="status"><Icon name="lock" size={16} /><span>{t('ui.settingsNewer')}</span></div>
    {/if}
    {#each shown as c (c)}
      <section aria-labelledby={'st-h-' + c}>
        <h2 class="title" id={'st-h-' + c}>{t('set.cat.' + c)}</h2>
        <fieldset class="lockable" disabled={readOnly}>{@render section(c)}</fieldset>
      </section>
    {:else}
      <p class="muted none" role="status">{t('set.noMatch', { q: query.trim() })}</p>
    {/each}

    {#if dirty && !readOnly}
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
  .lockable { border: 0; padding: 0; margin: 0; min-width: 0; }
  .ro { display: flex; gap: 10px; align-items: flex-start; margin-bottom: 14px; font-size: 13px; border-color: var(--accent-2); }
  .ro :global(.icon) { flex: none; margin-top: 2px; color: var(--accent-2); }
  .cats { padding-block: 6px 20px; padding-inline: 16px 10px; border-inline-end: 1px solid var(--hairline); display: flex; flex-direction: column; gap: 2px; }
  .cats h1 { font-size: 20px; margin: 4px 6px 12px; }
  .find { position: relative; margin-bottom: 8px; color: var(--muted); }
  .find :global(.icon) { position: absolute; inset-inline-start: 9px; inset-block-start: 50%; transform: translateY(-50%); pointer-events: none; }
  .find input { padding-inline-start: 30px; font-size: 13px; }
  .cat { font: inherit; color: inherit; background: none; border: 0; text-align: start; cursor: pointer; display: flex; align-items: center; gap: 10px; padding: 5px 6px; border-radius: 7px; font-size: 13px; width: 100%; }
  .cat:hover { background: var(--fill); }
  /* A navigation list: the current row is tinted, like the sidebar (see "selected" in theme.css). */
  .cat.on { background: var(--tint); color: var(--on-tint); font-weight: 600; }
  .ci { width: 24px; height: 24px; border-radius: 6px; display: grid; place-items: center; background: var(--tint); color: var(--on-tint); flex: none; }
  .cat.on .ci { background: var(--accent); color: var(--accent-fg); }
  :global([data-theme="contrast"]) .cat.on .ci { background: var(--accent-fg); color: var(--accent); }
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
  .danger-text { color: var(--danger); }
  .ax { margin-top: 14px; }
  .axhint { margin: 2px 0 0; padding-inline-start: 24px; }
  .perm { border: 0; padding: 0; margin: 14px 0 0; min-width: 0; }
  .perm legend { font-size: var(--fs-xs); font-weight: 500; color: var(--muted); padding: 0; }
  .num { max-width: 120px; }
  .adv > summary { cursor: pointer; font-size: 13px; font-weight: 600; }
  .adv[open] > summary { margin-bottom: 6px; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 0 10px; margin: 4px 0 8px; }
  .wrap { flex-wrap: wrap; }
  .gap-top { margin-top: 10px; }
  .mod, .remote { padding: 10px 0; border-bottom: 1px solid var(--hairline); }
  .mod:last-child { border-bottom: 0; }
  .path { overflow-wrap: anywhere; }
  .err { color: var(--danger); font-size: var(--fs-sm); margin: 4px 0 0; }
  .note { display: flex; gap: 8px; align-items: flex-start; font-size: var(--fs-sm); margin: 8px 0; }
  .note :global(.icon) { flex: none; margin-top: 3px; color: var(--danger); }
  .cost { font-size: 26px; font-weight: 500; margin: 0 0 4px; }
  .reset { display: flex; justify-content: flex-end; }
  .none { padding: 24px 0; }
  .sm { font-size: var(--fs-xs); }
  /* The window can't be narrower than 920px, so a viewport query never fired: follow the room the view really has. */
  @container main (max-width: 700px) {
    .settings { grid-template-columns: 1fr; min-height: 0; }
    .cats { border-inline-end: 0; border-bottom: 1px solid var(--hairline); flex-direction: row; flex-wrap: wrap; align-items: center; gap: 4px; padding-block-end: 10px; }
    .cats h1, .find { flex-basis: 100%; }
    .cat { width: auto; padding-inline-end: 10px; }
    .content { padding-inline: 16px; }
    .grid2 { grid-template-columns: 1fr; }
  }
</style>
