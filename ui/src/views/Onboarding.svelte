<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { api, post } from '../lib/api';
  import { app, applyTheme, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { cloudFor } from '../lib/cloud.svelte';
  import ThemePicker from './ThemePicker.svelte';
  import Logo from '../lib/Logo.svelte';

  let { done }: { done: (setupChatId: string | null) => void } = $props();

  // Four steps: Welcome · Assistant · Your notes (folder + backup) · Review (privacy + look).
  const steps = ['welcome', 'ai', 'library', 'ready'] as const;
  let step = $state(0);
  let busy = $state(false);
  let checking = $state(false);
  let working = $state<Record<string, boolean>>({});
  let claude = $state<any>(app.status?.claude ?? {});
  let git = $state<any>(app.status?.git ?? { installed: true });
  let modules = $state<any[]>([]);
  let modulesLoading = $state(true);
  let modulesErr = $state('');
  let s = $state<any>(structuredClone($state.snapshot(app.settings)));
  let libraryMode = $state<'new' | 'existing'>('new');
  let seeds = $state('');
  let remoteUrl = $state('');
  let remoteUser = $state('');
  let remoteToken = $state('');
  let remoteAck = $state(false);
  let useApi = $state(false);
  let apiToken = $state('');
  let profession = $state('');
  let professionOther = $state('');
  // What is at the chosen folder, as the background service sees it (asked again whenever the path changes).
  let folder = $state<{ path: string; state: string } | null>(null);
  let probe = $state<'idle' | 'busy' | 'failed'>('idle');
  const professions = ['finance', 'legal', 'consulting', 'teaching', 'research', 'founder', 'personal', 'other'];

  // Start with the safest choice: nothing leaves this computer until the user asks for a backup.
  // The history of changes is a separate switch, and it starts on.
  s.sync.mode = 'none';
  s.keepHistory = true;
  s.libraryPath = app.status?.home ? app.status.home + '/Thoughts' : '~/Thoughts';
  // First screen in the system's language: Arabic systems start in Arabic (an explicit choice wins on a re-run).
  const LANG_KEY = 'thoughts.ob.lang';
  const lsGet = (k: string) => { try { return localStorage.getItem(k); } catch { return null; } };
  const picked = lsGet(LANG_KEY);
  if (picked === 'ar' || picked === 'en') s.language = picked;
  else if (/^ar\b/i.test(navigator.language || '')) s.language = 'ar';
  function setLang(l: 'ar' | 'en') { s.language = l; try { localStorage.setItem(LANG_KEY, l); } catch { /* private mode */ } }
  // Only two ways of working exist: the assistant edits notes, or it only reads.
  if (s.ai?.claude && s.ai.claude.permissionMode === 'default') s.ai.claude.permissionMode = 'plan';

  // Focus follows the step: each screen's heading is announced when it appears.
  let bodyEl = $state<HTMLElement | null>(null);
  let firstStep = true;
  $effect(() => {
    step;
    if (firstStep) { firstStep = false; return; }
    tick().then(() => bodyEl?.querySelector<HTMLElement>('h1')?.focus());
  });

  const ar = $derived(s.language === 'ar');
  const isGit = $derived(String(s.sync.mode).startsWith('git'));
  // Whether a cloud drive syncs the chosen folder is the service's call (real sync folders, shortcuts resolved).
  const cloud = cloudFor(() => String(s.libraryPath ?? ''));
  const inCloud = $derived(cloud.current.synced);
  const cloudName = $derived(cloud.current.service || t('hista.cloud.someDrive'));
  const fullPath = (p: string) => /^(~\/|\/|[A-Za-z]:[\\/])/.test(p);
  const typedPath = $derived(String(s.libraryPath ?? '').trim());
  /** missing · empty · starter (a setup that stopped half-way) · library · files · notFolder — '' while not known. */
  const here = $derived(folder && folder.path === typedPath ? folder.state : '');
  const whatIsThere = (path: string) => post('/onboarding/folder', { path }).then((r) => String(r?.state ?? ''));
  let probeSeq = 0;
  $effect(() => {
    const p = typedPath;
    if (step !== 2) return;
    const seq = ++probeSeq;
    if (!p || !fullPath(p)) { probe = 'idle'; return; }
    probe = 'busy';
    // A short pause, so a path being typed is not looked up letter by letter.
    const timer = setTimeout(async () => {
      try { const state = await whatIsThere(p); if (seq === probeSeq) { folder = { path: p, state }; probe = 'idle'; } }
      catch { if (seq === probeSeq) probe = 'failed'; /* Finish checks the folder again */ }
    }, 300);
    return () => clearTimeout(timer);
  });
  const validUrl = (u: string) => /^https?:\/\/\S+$/.test((u ?? '').trim());
  const validRemote = (u: string) => /^(https?:\/\/|ssh:\/\/|git@)\S+$/.test(u.trim());

  onMount(loadModules);

  async function loadModules() {
    modulesLoading = true; modulesErr = '';
    try {
      modules = await api('/modules');
      s.modules = Object.fromEntries(modules.map((m) => [m.id, s.modules?.[m.id] ?? m.defaultOn]));
    } catch (e: any) { modulesErr = t('ob3.modulesFailed'); }
    finally { modulesLoading = false; }
  }

  // Picking a language on the first step switches the whole flow (text and direction) immediately.
  $effect(() => { app.settings.language = s.language; applyTheme(); });

  async function act(key: string, fn: () => Promise<void>, failKey = 'ob.failed') {
    if (working[key]) return;
    working[key] = true;
    try { await fn(); } catch (e) { failed(t(failKey), e); }
    finally { working[key] = false; }
  }

  async function checkAgain() {
    if (checking) return;
    checking = true;
    try {
      const st = await api('/status');
      claude = st.claude ?? {};
      git = st.git ?? git;
      const ready = step === 1 ? claude.installed && (claude.loggedIn || useApi) : git.installed;
      toast(t(ready ? 'ob.checkOk' : 'ob.checkStill'));
    } catch (e) { failed(t('ob3.checkFailed'), e); }
    finally { checking = false; }
  }

  const installGit = () => act('git', async () => {
    const r = await post('/git/install');
    // Windows: the service hands back the download page; Linux: the command for this system.
    if (r?.url) { try { const { openUrl } = await import('@tauri-apps/plugin-opener'); await openUrl(r.url); } catch { window.open(r.url, '_blank', 'noopener'); } }
    if (r?.hint) toast(t('ob.gitInstallToast'), { detail: String(r.hint), duration: 0 }); else toast(t('ob.gitInstallToast'));
  }, 'ob3.gitInstallFailed');
  const login = () => act('login', async () => { await post('/ai/claude/login'); toast(t('ob.loginToast')); }, 'ob3.loginFailed');

  async function browse() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true });
      if (typeof p !== 'string') return;
      if (libraryMode !== 'new') { s.libraryPath = p; return; }
      // A new library goes straight into an empty folder, and into a "Thoughts" folder inside one that
      // holds other things. A folder that already holds a library is opened, not started over.
      const picked = p.replace(/[\\/]+$/, '') || p;
      const state = await whatIsThere(picked).catch(() => '');
      if (state === 'library') { libraryMode = 'existing'; s.libraryPath = picked; toast(t('sa.folder.pickedLibrary')); }
      else s.libraryPath = state === 'empty' || state === 'starter' ? picked : picked + '/Thoughts';
    } catch { toast(t('ob.browseFail'), { kind: 'error' }); }
  }

  function setUseApi(on: boolean) {
    useApi = on;
    if (on && !s.ai.claude.baseUrl) s.ai.claude.baseUrl = 'https://api.anthropic.com';
  }

  /** Backup is one of: no backup, a cloud folder, an online location (sent when Sync is pressed, or after every Wrap). */
  function setBackup(kind: 'none' | 'folder' | 'online') {
    s.sync.mode = kind === 'online' ? (isGit ? s.sync.mode : 'git-manual') : kind;
    if (kind === 'online') s.keepHistory = true;
  }
  // An online location is sent the history itself, so there the history is always kept.
  const historyOn = $derived(isGit || s.keepHistory !== false);

  /** Why the Next button is disabled on the current step ('' = good to go). */
  const blocker = $derived.by(() => {
    if (step === 1) {
      if (!claude.installed) return t('ob.needClaude');
      if (useApi) {
        if (!validUrl(s.ai.claude.baseUrl)) return t('ob.needApiUrl');
        if (!apiToken.trim()) return t('ob.needApiKey');
      } else if (!claude.loggedIn) return t('ob.needSignIn');
    }
    if (step === 2) {
      const p = String(s.libraryPath ?? '').trim();
      if (!p) return t('ob.needFolder');
      if (!fullPath(p)) return t('ob.needFullPath');
      if (probe === 'busy') return t('sa.folder.checking');
      if (libraryMode === 'new') {
        if (here === 'library') return t('sa.folder.needOther');
        if (here === 'files') return t('sa.folder.files');
        if (here === 'notFolder') return t('sa.folder.notFolder');
      } else {
        if (here === 'missing') return t('sa.folder.missing');
        if (here === 'starter') return t('sa.folder.needSetUp');
        if (here === 'files' || here === 'empty' || here === 'notFolder') return t('sa.folder.notLibrary');
      }
    }
    if (step === 2) {
      // Git is only a must for an online location; without it the app still works, minus the history.
      if (isGit && !git.installed) return t('hista.online.needGit');
      if (isGit && inCloud && !s.sync.allowCloud) return t('hista.online.cloudClash');
      if (isGit && remoteUrl.trim()) {
        if (!validRemote(remoteUrl)) return t('ob.needRemoteUrl');
        if (!remoteAck) return t('ob.needAck');
      }
    }
    return '';
  });

  const modName = (m: any) => (ar && m.nameAr) || m.name;
  const modDesc = (m: any) => (ar && m.descriptionAr) || m.description;
  const syncLabel = $derived(t(isGit ? 'hista.backup.online' : s.sync.mode === 'folder' ? 'hista.backup.folder' : 'hista.backup.none'));

  async function finish() {
    if (busy) return;
    busy = true;
    try {
      const settings = structuredClone($state.snapshot(s));
      if (!useApi) settings.ai.claude.baseUrl = null;
      if (settings.ai?.claude && !['acceptEdits', 'plan'].includes(settings.ai.claude.permissionMode ?? 'acceptEdits')) settings.ai.claude.permissionMode = 'plan';
      // The app keeps the choice itself (its id, or the user's own few words): every screen reads its
      // wording from that. The name of the choice in the user's language is for the assistant only.
      const own = profession === 'other' ? professionOther.trim() : '';
      settings.profession = own || profession || null;
      const r = await post('/onboarding', {
        settings, libraryMode, seeds: seeds.split('\n').map((x) => x.trim()).filter(Boolean),
        remoteUrl: isGit ? remoteUrl.trim() : '', remoteUsername: isGit ? remoteUser : '', remoteToken: isGit ? remoteToken : '',
        apiToken: useApi ? apiToken.trim() : '',
        profession: own || (profession ? t('layer.prof.' + profession) : ''),
      });
      try { app.settings = await api('/settings'); applyTheme(); } catch { /* the app reloads settings on its own */ }
      done(r.setupChatId);
    } catch (e) {
      failed(t('ob3.finishFailed'), e);
    } finally { busy = false; }
  }

  // Enter in a text box moves on, like the Next button (never from a checkbox, a list or the notes box).
  function onKey(e: KeyboardEvent) {
    if (e.key !== 'Enter' || e.isComposing || e.shiftKey || e.metaKey || e.ctrlKey || e.altKey || e.defaultPrevented) return;
    const el = e.target;
    if (!(el instanceof HTMLInputElement) || !['text', 'password', 'url', 'email', 'search', ''].includes(el.type)) return;
    if (step >= steps.length - 1 || blocker || busy) return;
    e.preventDefault();
    step++;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="ob" onkeydown={onKey}>
  <div class="drag" data-tauri-drag-region></div>
  <p class="prog" role="status">{t('ob3.stepOf', { n: step + 1, total: steps.length })} · <b>{t('ob.step.' + steps[step])}</b></p>
  <ol class="steps" aria-label={t('ob.stepsLabel')}>
    {#each steps as name, i}
      <li class="st" class:on={i === step} class:past={i < step} aria-current={i === step ? 'step' : undefined}>{t('ob.step.' + name)}</li>
    {/each}
  </ol>

  <div class="card body" bind:this={bodyEl}>
    {#if step === 0}
      <div class="hero-logo"><Logo size={72} /></div>
      <h1 class="brand" tabindex="-1">Thoughts</h1>
      <p>{t('ob.welcomeBody')}</p>
      <div class="lbl" id="ob-lang">{t('language')}</div>
      <div class="row" role="group" aria-labelledby="ob-lang">
        <button type="button" class="btn" class:primary={s.language === 'ar'} aria-pressed={s.language === 'ar'} lang="ar" onclick={() => setLang('ar')}>العربية</button>
        <button type="button" class="btn" class:primary={s.language !== 'ar'} aria-pressed={s.language !== 'ar'} lang="en" onclick={() => setLang('en')}>English</button>
      </div>
      <label for="ob-name">{t('ob.nameLabel')}</label>
      <input id="ob-name" dir="auto" bind:value={s.userName} placeholder={t('ob.namePh')} autocomplete="off" />
      <label for="ob-prof">{t('layer.prof.label')}</label>
      <select id="ob-prof" bind:value={profession}>
        <option value="">{t('layer.prof.choose')}</option>
        {#each professions as p}<option value={p}>{t('layer.prof.' + p)}</option>{/each}
      </select>
      {#if profession === 'other'}
        <label for="ob-prof-other">{t('sa.prof.otherLabel')}</label>
        <input id="ob-prof-other" dir="auto" bind:value={professionOther} placeholder={t('sa.prof.otherPh')} maxlength="160" autocomplete="off" />
      {/if}
      <p class="muted sm">{t('layer.prof.hint')}</p>
    {:else if step === 1}
      <h1 tabindex="-1">{t('ob.aiTitle')}</h1>
      <p>{t('ob.aiBody')}</p>
      <div class="card status">
        <p class="row"><Icon name="info" size={16} /> <b>{t('ob3.planTitle')}</b></p>
        <ul class="plain">
          <li>{t('ob3.planPaid')}</li>
          <li>{t('ob3.planNoBill')}</li>
          <li>{t('ob3.planData')}</li>
        </ul>
      </div>

      <div class="card status">
        <p><b>{t('ob3.toolTitle')}</b></p>
        <div class="tool">
          <span class="mark" class:ok={claude.installed}><Icon name={claude.installed ? 'check' : 'warning'} size={15} /></span>
          <div class="grow"><b><bdi>Claude Code</bdi></b><div class="muted sm">{t('ob.claudeWhy')}</div></div>
          {#if claude.installed}<span class="pill accent">{t('ob.installed')}</span>
          {:else}<a class="btn primary" href="https://claude.com/claude-code" target="_blank" rel="noreferrer">{t('ob.installClaude')} <Icon name="external" size={13} /></a>{/if}
        </div>
        {#if !claude.installed}<p class="muted sm">{t('ob.toolsAfter')}</p>{/if}
        {#if claude.installed}
          <div class="tool">
            <span class="mark" class:ok={claude.loggedIn || useApi}><Icon name={claude.loggedIn || useApi ? 'check' : 'warning'} size={15} /></span>
            <div class="grow"><b>{t(claude.loggedIn ? 'ob.signedIn' : 'ob.notSignedIn')}</b><div class="muted sm">{t(claude.loggedIn ? 'ob.signedInHint' : 'ob.signInHint')}</div></div>
            {#if !claude.loggedIn && !useApi}<button type="button" class="btn primary" disabled={working.login} onclick={login}>{working.login ? t('ob.opening') : t('ob.signIn')}</button>{/if}
          </div>
        {/if}
      </div>

      <div class="row gap-top">
        <button type="button" class="btn" disabled={checking} aria-busy={checking} onclick={checkAgain}><Icon name="sync" size={14} class={checking ? 'spin' : ''} /> {checking ? t('ob.checking') : t('ob.checkAgain')}</button>
      </div>

      <details class="adv" open={useApi}>
        <summary>{t('ob.advApi')}</summary>
        <label class="check"><input type="checkbox" checked={useApi} onchange={(e) => setUseApi(e.currentTarget.checked)} /> {t('ob.useApi')}</label>
        {#if useApi}
          <label for="ob-base">{t('ob.apiUrl')}</label>
          <input id="ob-base" dir="ltr" bind:value={s.ai.claude.baseUrl} placeholder="https://api.anthropic.com" autocomplete="off" />
          <label for="ob-key">{t('ob.apiKey')}</label>
          <input id="ob-key" dir="ltr" type="password" bind:value={apiToken} autocomplete="off" />
          <p class="muted sm">{t('ob3.apiKeyHint')}</p>
        {/if}
      </details>
    {:else if step === 2}
      <h1 tabindex="-1">{t('ob.libTitle')}</h1>
      <p>{t('ob.libBody')}</p>
      <div class="opts">
        <button type="button" class="opt" class:sel={libraryMode === 'new'} aria-pressed={libraryMode === 'new'} onclick={() => (libraryMode = 'new')}><b>{t('ob.libNew')}</b><span class="muted">{t('ob.libNewHint')}</span></button>
        <button type="button" class="opt" class:sel={libraryMode === 'existing'} aria-pressed={libraryMode === 'existing'} onclick={() => (libraryMode = 'existing')}><b>{t('ob.libExisting')}</b><span class="muted">{t('ob.libExistingHint')}</span></button>
      </div>
      <label for="ob-path">{t('ob.folder')}</label>
      <div class="row"><input id="ob-path" class="grow" dir="ltr" bind:value={s.libraryPath} autocomplete="off" aria-describedby="ob-path-says" /><button type="button" class="btn" onclick={browse}><Icon name="folder" size={14} /> {t('ob.browse')}</button></div>
      <div id="ob-path-says" role="status">
        {#if libraryMode === 'new'}
          {#if here === 'missing'}<p class="muted sm">{t('sa.folder.new')}</p>
          {:else if here === 'empty'}<p class="muted sm">{t('sa.folder.empty')}</p>
          {:else if here === 'starter'}<p class="okline"><Icon name="check" size={15} /> <span>{t('sa.folder.starter')}</span></p>
          {:else if here === 'library'}
            <div class="card status">
              <p class="row"><Icon name="info" size={16} /> <b>{t('sa.folder.library')}</b></p>
              <p class="muted">{t('sa.folder.libraryHint')}</p>
              <div class="row gap-top"><button type="button" class="btn primary" onclick={() => (libraryMode = 'existing')}>{t('sa.folder.open')}</button></div>
            </div>
          {:else if here === 'files'}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sa.folder.files')}</span></p>
          {:else if here === 'notFolder'}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sa.folder.notFolder')}</span></p>
          {/if}
        {:else if here === 'library'}<p class="okline"><Icon name="check" size={15} /> <span>{t('sa.folder.found')}</span></p>
        {:else if here === 'starter'}
          <div class="card status">
            <p class="row"><Icon name="info" size={16} /> <b>{t('sa.folder.unfinished')}</b></p>
            <p class="muted">{t('sa.folder.unfinishedHint')}</p>
            <div class="row gap-top"><button type="button" class="btn primary" onclick={() => (libraryMode = 'new')}>{t('sa.folder.setUp')}</button></div>
          </div>
        {:else if here === 'missing'}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sa.folder.missing')}</span></p>
        {:else if here}<p class="warn"><Icon name="warning" size={15} /> <span>{t('sa.folder.notLibrary')}</span></p>
        {/if}
      </div>
      {#if libraryMode === 'new'}
        <label for="ob-seeds">{t('ob.seeds')}</label>
        <textarea id="ob-seeds" rows="2" dir="auto" bind:value={seeds} placeholder={t('ob.seedsPh')}></textarea>
      {/if}

      <h2 id="ob-history">{t('hista.historyTitle')}</h2>
      <label class="check"><input type="checkbox" checked={historyOn} disabled={isGit} onchange={(e) => (s.keepHistory = e.currentTarget.checked)} /> {t('hista.keep')}</label>
      <p class="muted sm">{t(isGit ? 'hista.keepForced' : historyOn ? 'hista.keepHint' : 'hista.keepOffHint')}</p>
      {#if historyOn && !git.installed}
        <div class="card status" role="note">
          <p class="row"><Icon name="info" size={16} /> <b>{t('hista.needsGitTitle')}</b></p>
          <p class="muted">{t(isGit ? 'hista.needsGitOnline' : 'hista.needsGitBody')}</p>
          <div class="row gap-top">
            <button type="button" class="btn" class:primary={isGit} disabled={working.git} onclick={installGit}>{working.git ? t('ob.opening') : t('ob3.installGit')}</button>
            <button type="button" class="btn" disabled={checking} aria-busy={checking} onclick={checkAgain}><Icon name="sync" size={14} class={checking ? 'spin' : ''} /> {checking ? t('ob.checking') : t('ob.checkAgain')}</button>
          </div>
        </div>
      {/if}

      <h2 id="ob-backup">{t('ob3.backupTitle')}</h2>
      <p class="muted">{t('hista.backupBody')}</p>
      <div class="opts three" role="group" aria-labelledby="ob-backup">
        <button type="button" class="opt" class:sel={s.sync.mode === 'none'} aria-pressed={s.sync.mode === 'none'} onclick={() => setBackup('none')}><b>{t('hista.backup.none')}</b><span class="muted">{t('hista.backup.noneHint')}</span></button>
        <button type="button" class="opt" class:sel={s.sync.mode === 'folder'} aria-pressed={s.sync.mode === 'folder'} onclick={() => setBackup('folder')}><b>{t('hista.backup.folder')}</b><span class="muted">{t('hista.backup.folderHint')}</span></button>
        <button type="button" class="opt" class:sel={isGit} aria-pressed={isGit} onclick={() => setBackup('online')}><b>{t('hista.backup.online')}</b><span class="muted">{t('hista.backup.onlineHint')}</span></button>
      </div>
      {#if s.sync.mode === 'none'}
        <p class="warn"><Icon name="warning" size={15} /> <span>{t('hista.backup.noneWarn')}</span></p>
      {:else if s.sync.mode === 'folder'}
        <div class="card status">
          <p>{t('ob.folderPick')}</p>
          <div class="row gap-top"><code class="grow path"><bdi>{s.libraryPath}</bdi></code><button type="button" class="btn" onclick={browse}><Icon name="folder" size={14} /> {t('ob.chooseFolder')}</button></div>
          {#if inCloud}<p class="okline"><Icon name="check" size={15} /> <span>{t('hista.cloud.inside', { service: cloudName })}</span></p>
          {:else}<p class="warn"><Icon name="warning" size={15} /> <span>{t('ob.folderNotCloud')}</span></p>{/if}
          <p class="muted sm">{t('ob.cloudPrivacy')}</p>
        </div>
      {:else}
        <div class="card status">
          <p class="muted sm">{t('hista.online.body')}</p>
          {#if inCloud}
            <div class="warn box" role="note">
              <Icon name="warning" size={15} />
              <div>
                <div>{t('hista.cloud.clash', { service: cloudName })}</div>
                <label class="check"><input type="checkbox" bind:checked={s.sync.allowCloud} /> {t('hista.cloud.anyway')}</label>
              </div>
            </div>
          {/if}
          <label for="ob-gitmode">{t('hista.online.when')}</label>
          <select id="ob-gitmode" bind:value={s.sync.mode}>
            <option value="git-manual">{t('hista.online.manual')}</option>
            <option value="git-auto">{t('hista.online.auto')}</option>
          </select>
          <p class="muted sm">{t('ob.wrapDef')}</p>
          <label for="ob-remote">{t('hista.online.address')}</label>
          <input id="ob-remote" dir="ltr" bind:value={remoteUrl} placeholder="https://github.com/you/thoughts.git" autocomplete="off" />
          {#if remoteUrl.trim()}
            <div class="warn box" role="note">
              <Icon name="warning" size={15} />
              <div>
                <b>{t('ob.uploadTitle')}</b>
                <div>{t('ob.uploadBody')}</div>
                <label class="check"><input type="checkbox" bind:checked={remoteAck} /> {t('ob.uploadAck')}</label>
              </div>
            </div>
            <div class="row top">
              <div class="grow"><label for="ob-ruser">{t('ob.remoteUser')}</label><input id="ob-ruser" dir="ltr" bind:value={remoteUser} autocomplete="off" /></div>
              <div class="grow"><label for="ob-rtoken">{t('ob.remoteToken')}</label><input id="ob-rtoken" dir="ltr" type="password" bind:value={remoteToken} autocomplete="off" /></div>
            </div>
            <label for="ob-cred">{t('ob.credStore')}</label>
            <select id="ob-cred" bind:value={s.sync.credentialStore}><option value="keychain">{t('ob.credKeychain')}</option><option value="file">{t('ob.credFile')}</option></select>
            <p class="muted sm">{t(s.sync.credentialStore === 'file' ? 'ob3.credFileNote' : 'ob3.credKeychainNote')}</p>
          {/if}
        </div>
      {/if}

      <details class="adv">
        <summary>{t('ob3.featuresAdv')}</summary>
        <p class="muted sm">{t('ob.modBody')}</p>
        {#if modulesLoading}
          <p class="muted" role="status">{t('ob.loading')}</p>
        {:else if modulesErr}
          <p class="warn" role="alert"><Icon name="warning" size={15} /> <span>{modulesErr}</span></p>
          <button type="button" class="btn" onclick={loadModules}>{t('ob3.retry')}</button>
        {:else}
          {#each modules as m (m.id)}
            <label class="mod" class:locked={m.id === 'ideas'}>
              <input type="checkbox" bind:checked={s.modules[m.id]} disabled={m.id === 'ideas'} />
              <span class="grow"><b>{modName(m)}</b><span class="muted sm blk">{modDesc(m)}</span></span>
              {#if m.id === 'ideas'}<span class="pill accent">{t('ob.alwaysOn')}</span>
              {:else if !s.modules[m.id]}<span class="pill">{t('addLater')}</span>{/if}
            </label>
          {/each}
        {/if}
      </details>
    {:else}
      <h1 tabindex="-1">{t('ob.readyTitle')}</h1>
      <div class="card status">
        <p class="row"><Icon name="shield" size={16} /> <b>{t('ob.privTitle')}</b></p>
        <ul class="plain">
          <li>{t('ob3.privSent')}</li>
          <li>{t('ob3.privStays')}</li>
          <li>{t('ob3.privFolders')}</li>
        </ul>
      </div>
      <dl class="sum">
        <dt>{t('ob.sumNotes')}</dt><dd><code><bdi>{s.libraryPath}</bdi></code> · {t(libraryMode === 'new' ? 'ob.sumNew' : 'ob.sumExisting')}</dd>
        <dt>{t('ob.sumAi')}</dt><dd><bdi>Claude</bdi>{#if useApi} · <bdi>{s.ai.claude.baseUrl}</bdi>{/if}</dd>
        <dt>{t('ob.sumBackup')}</dt><dd>{syncLabel}{#if isGit && remoteUrl.trim()} · <bdi>{remoteUrl.trim()}</bdi>{/if}{#if s.sync.mode === 'folder' && !inCloud} · {t('sa.sum.notCloud')}{/if}</dd>
        <dt>{t('hista.historyTitle')}</dt><dd>{t(historyOn ? 'hista.on' : 'hista.off')}{#if historyOn && !git.installed} · {t('hista.sumNeedsGit')}{/if}</dd>
      </dl>
      <h2>{t('ob.lookTitle')}</h2>
      <ThemePicker bind:theme={s.theme} bind:mode={s.mode} />
      <details class="adv">
        <summary>{t('ob3.moreOptions')}</summary>
        <label class="check"><input type="checkbox" bind:checked={s.notifications} /> {t('ob.notify')}</label>
        <label class="check"><input type="checkbox" bind:checked={s.daemon.keepRunning} /> {t('ob.keepRunning')}</label>
        <label class="check"><input type="checkbox" bind:checked={s.daemon.runAtLogin} /> {t('ob.runAtLogin')}</label>
      </details>
      <p class="muted">{t('ob.wrapDef')}</p>
      {#if libraryMode === 'new'}<p class="muted">{t('ob.readyNew')}</p>{/if}
    {/if}
  </div>

  <div class="row nav">
    {#if step > 0}<button type="button" class="btn" disabled={busy} onclick={() => step--}>{t('back')}</button>{/if}
    <span class="grow why" id="ob-why" role="status">{#if blocker && step < steps.length - 1}{blocker}{/if}</span>
    {#if step < steps.length - 1}
      <button type="button" class="btn primary" disabled={!!blocker} aria-describedby="ob-why" onclick={() => step++}>{t('next')}</button>
    {:else}
      <button type="button" class="btn primary" disabled={busy} aria-busy={busy} onclick={finish}>{busy ? t('ob.finishing') : t('finish')}</button>
    {/if}
  </div>
</div>

<style>
  .ob { max-width: 760px; margin: 0 auto; padding: 0 24px 24px; min-height: 100vh; display: flex; flex-direction: column; }
  .drag { height: 34px; }
  .prog { font-size: 13px; color: var(--muted); margin: 0 0 4px; }
  .prog b { color: var(--fg); }
  .body h1:focus { outline: none; }
  .body h1:focus-visible { outline: 2px solid var(--accent); outline-offset: 4px; border-radius: 4px; }
  .check { margin: 6px 0; }
  .steps { list-style: none; counter-reset: st; display: flex; flex-wrap: wrap; gap: 4px 14px; font-size: var(--fs-xs); color: var(--muted); margin: 0 0 14px; padding: 0; }
  .st { counter-increment: st; }
  .st::before { content: counter(st) '. '; }
  .st.on { color: var(--accent); font-weight: 700; }
  .st.past { color: var(--fg); }
  .body { flex: 1; padding: 24px; }
  .brand { font-size: 36px; }
  .hero-logo { margin-bottom: 14px; filter: drop-shadow(0 10px 24px rgba(0,0,0,.25)); }
  .lbl { font-size: var(--fs-xs); font-weight: 500; color: var(--muted); margin: 10px 0 4px; }
  .opts { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-bottom: 12px; }
  .opts.three { grid-template-columns: repeat(3, 1fr); }
  @media (max-width: 640px) { .opts.three { grid-template-columns: 1fr; } }
  .opt { font: inherit; color: inherit; text-align: start; cursor: pointer; border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 12px; display: flex; flex-direction: column; gap: 4px; background: var(--bg); }
  .opt.sel { border-color: var(--accent); outline: 2px solid var(--accent); box-shadow: var(--glow); }
  .opt:focus-visible { outline: 3px solid color-mix(in srgb, var(--accent) 60%, transparent); outline-offset: 2px; }
  .status { background: var(--bg); margin-top: 10px; }
  .status p { margin: 0 0 6px; }
  .tool { display: flex; gap: 10px; align-items: center; padding: 8px 0; border-top: 1px solid var(--hairline); }
  .mark { width: 24px; height: 24px; border-radius: 50%; display: grid; place-items: center; background: var(--fill); color: var(--danger); flex: none; }
  .mark.ok { color: var(--ok); }
  a.btn { text-decoration: none; }
  .gap-top { margin-top: 10px; }
  .row.top { align-items: flex-start; }
  .check { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; }
  .mod { display: flex; gap: 10px; align-items: center; color: var(--fg); font-size: 14px; padding: 8px 0; border-bottom: var(--border-w) solid var(--border); margin: 0; cursor: pointer; }
  .mod.locked { opacity: .8; }
  .blk { display: block; font-weight: 400; }
  .adv { margin-top: 14px; border-top: 1px solid var(--hairline); padding-top: 4px; }
  .adv > summary { cursor: pointer; font-size: 13px; font-weight: 500; color: var(--muted); padding: 6px 0; }
  .warn, .okline { display: flex; gap: 8px; align-items: flex-start; font-size: 13px; margin: 8px 0; }
  .warn :global(.icon), .okline :global(.icon) { flex: none; margin-top: 3px; }
  .warn :global(.icon) { color: var(--danger); }
  .okline :global(.icon) { color: var(--ok); }
  .warn.box { border: var(--border-w) solid var(--danger); border-radius: var(--radius); padding: 10px 12px; background: color-mix(in srgb, var(--danger) 7%, transparent); }
  .path { overflow-wrap: anywhere; }
  .plain { margin: 6px 0 0; padding-inline-start: 20px; }
  .plain li { margin-bottom: 4px; }
  .sum { display: grid; grid-template-columns: max-content 1fr; gap: 6px 14px; margin: 16px 0; }
  .sum dt { color: var(--muted); font-size: 13px; }
  .sum dd { margin: 0; overflow-wrap: anywhere; }
  .nav { padding-top: 14px; }
  .why { font-size: var(--fs-sm); color: var(--muted); text-align: end; }
  .sm { font-size: var(--fs-xs); }
  :global(.icon.spin) { animation: ob-spin 1s linear infinite; }
  @keyframes ob-spin { to { transform: rotate(360deg); } }
</style>
