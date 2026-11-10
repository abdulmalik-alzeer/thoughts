<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { api, post } from '../lib/api';
  import { app, applyTheme, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import ThemePicker from './ThemePicker.svelte';
  import Logo from '../lib/Logo.svelte';

  let { done }: { done: (setupChatId: string | null) => void } = $props();

  const steps = ['welcome', 'ai', 'library', 'modules', 'sync', 'look', 'ready'] as const;
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
  let wantSearch = $state(false);

  // Start with the safest choice: nothing leaves this computer until the user asks for a backup.
  s.sync.mode = 'none';
  s.libraryPath = app.status?.home ? app.status.home + '/Thoughts' : '~/Thoughts';

  const ar = $derived(s.language === 'ar');
  const isGit = $derived(String(s.sync.mode).startsWith('git'));
  const cloudWords = ['Mobile Documents', 'Library/CloudStorage', 'Dropbox', 'OneDrive', 'Google Drive', 'iCloud'];
  const inCloud = $derived(cloudWords.some((k) => String(s.libraryPath ?? '').includes(k)));
  const validUrl = (u: string) => /^https?:\/\/\S+$/.test((u ?? '').trim());
  const validRemote = (u: string) => /^(https?:\/\/|ssh:\/\/|git@)\S+$/.test(u.trim());

  onMount(loadModules);

  async function loadModules() {
    modulesLoading = true; modulesErr = '';
    try {
      modules = await api('/modules');
      s.modules = Object.fromEntries(modules.map((m) => [m.id, s.modules?.[m.id] ?? m.defaultOn]));
    } catch (e: any) { modulesErr = e?.message || t('ob.failed'); }
    finally { modulesLoading = false; }
  }

  // Picking a language on the first step switches the whole flow (text and direction) immediately.
  $effect(() => { app.settings.language = s.language; applyTheme(); });

  async function act(key: string, fn: () => Promise<void>) {
    if (working[key]) return;
    working[key] = true;
    try { await fn(); } catch (e: any) { toast(e?.message || t('ob.failed')); }
    finally { working[key] = false; }
  }

  async function checkAgain() {
    if (checking) return;
    checking = true;
    try {
      const st = await api('/status');
      claude = st.claude ?? {};
      git = st.git ?? git;
      toast(t(git.installed && claude.installed && (claude.loggedIn || useApi) ? 'ob.checkOk' : 'ob.checkStill'));
    } catch (e: any) { toast(e?.message || t('ob.failed')); }
    finally { checking = false; }
  }

  const installGit = () => act('git', async () => { await post('/git/install'); toast(t('ob.gitInstallToast')); });
  const login = () => act('login', async () => { await post('/ai/claude/login'); toast(t('ob.loginToast')); });

  async function browse() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true });
      if (typeof p === 'string') s.libraryPath = libraryMode === 'new' ? p.replace(/\/+$/, '') + '/Thoughts' : p;
    } catch { toast(t('ob.browseFail')); }
  }

  function setUseApi(on: boolean) {
    useApi = on;
    if (on && !s.ai.claude.baseUrl) s.ai.claude.baseUrl = 'https://api.anthropic.com';
  }

  function setGit(on: boolean) { s.sync.mode = on ? 'git-manual' : 'none'; }

  /** Why the Next button is disabled on the current step ('' = good to go). */
  const blocker = $derived.by(() => {
    if (step === 1) {
      if (!git.installed) return t('ob.needGit');
      if (!claude.installed) return t('ob.needClaude');
      if (useApi) {
        if (!validUrl(s.ai.claude.baseUrl)) return t('ob.needApiUrl');
        if (!apiToken.trim()) return t('ob.needApiKey');
      } else if (!claude.loggedIn) return t('ob.needSignIn');
    }
    if (step === 2) {
      const p = String(s.libraryPath ?? '').trim();
      if (!p) return t('ob.needFolder');
      if (!/^(~\/|\/|[A-Za-z]:[\\/])/.test(p)) return t('ob.needFullPath');
    }
    if (step === 3 && modulesLoading) return t('ob.loading');
    if (step === 4) {
      if (isGit && inCloud) return t('ob.gitCloudClash');
      if (isGit && remoteUrl.trim()) {
        if (!validRemote(remoteUrl)) return t('ob.needRemoteUrl');
        if (!remoteAck) return t('ob.needAck');
      }
    }
    return '';
  });

  const modName = (m: any) => (ar && m.nameAr) || m.name;
  const modDesc = (m: any) => (ar && m.descriptionAr) || m.description;
  const syncLabel = $derived(t(isGit ? 'ob.sumGit' : s.sync.mode === 'folder' ? 'ob.sumFolder' : 'ob.sumLocal'));

  async function finish() {
    if (busy) return;
    busy = true;
    try {
      const settings = structuredClone($state.snapshot(s));
      if (!useApi) settings.ai.claude.baseUrl = null;
      const r = await post('/onboarding', {
        settings, libraryMode, seeds: seeds.split('\n').map((x) => x.trim()).filter(Boolean),
        remoteUrl: isGit ? remoteUrl.trim() : '', remoteUsername: isGit ? remoteUser : '', remoteToken: isGit ? remoteToken : '',
        apiToken: useApi ? apiToken.trim() : '',
      });
      if (wantSearch) post('/extensions/search/install').catch((e) => toast(t('ob.searchFail', { error: e?.message ?? '' })));
      try { app.settings = await api('/settings'); applyTheme(); } catch { /* the app reloads settings on its own */ }
      done(r.setupChatId);
    } catch (e: any) {
      toast(e?.message || t('ob.failed'));
    } finally { busy = false; }
  }
</script>

<div class="ob">
  <div class="drag" data-tauri-drag-region></div>
  <ol class="steps" aria-label={t('ob.stepsLabel')}>
    {#each steps as name, i}
      <li class="st" class:on={i === step} class:past={i < step} aria-current={i === step ? 'step' : undefined}>{t('ob.step.' + name)}</li>
    {/each}
  </ol>

  <div class="card body">
    {#if step === 0}
      <div class="hero-logo"><Logo size={72} /></div>
      <h1 class="brand">Thoughts</h1>
      <p>{t('ob.welcomeBody')}</p>
      <div class="lbl" id="ob-lang">{t('language')}</div>
      <div class="row" role="group" aria-labelledby="ob-lang">
        <button type="button" class="btn" class:primary={s.language === 'ar'} aria-pressed={s.language === 'ar'} lang="ar" onclick={() => (s.language = 'ar')}>العربية</button>
        <button type="button" class="btn" class:primary={s.language !== 'ar'} aria-pressed={s.language !== 'ar'} lang="en" onclick={() => (s.language = 'en')}>English</button>
      </div>
      <label for="ob-name">{t('ob.nameLabel')}</label>
      <input id="ob-name" dir="auto" bind:value={s.userName} placeholder={t('ob.namePh')} autocomplete="off" />
    {:else if step === 1}
      <h1>{t('ob.aiTitle')}</h1>
      <p>{t('ob.aiBody')}</p>
      <p class="muted">{t('ob.aiCost')}</p>

      <div class="card status">
        <p><b>{t('ob.toolsTitle')}</b></p>
        <div class="tool">
          <span class="mark" class:ok={git.installed}><Icon name={git.installed ? 'check' : 'warning'} size={15} /></span>
          <div class="grow"><b><bdi>Git</bdi></b><div class="muted sm">{t('ob.gitWhy')}</div></div>
          {#if git.installed}<span class="pill accent">{t('ob.installed')}</span>
          {:else}<button type="button" class="btn primary" disabled={working.git} onclick={installGit}>{working.git ? t('ob.opening') : t('ob.install')}</button>{/if}
        </div>
        <div class="tool">
          <span class="mark" class:ok={claude.installed}><Icon name={claude.installed ? 'check' : 'warning'} size={15} /></span>
          <div class="grow"><b><bdi>Claude Code</bdi></b><div class="muted sm">{t('ob.claudeWhy')}</div></div>
          {#if claude.installed}<span class="pill accent">{t('ob.installed')}</span>
          {:else}<a class="btn primary" href="https://claude.com/claude-code" target="_blank" rel="noreferrer">{t('ob.installClaude')} <Icon name="external" size={13} /></a>{/if}
        </div>
        {#if !git.installed || !claude.installed}<p class="muted sm">{t('ob.toolsAfter')}</p>{/if}
      </div>

      {#if claude.installed}
        <div class="card status">
          <div class="row">
            <span class="pill" class:accent={claude.loggedIn}>{t(claude.loggedIn ? 'ob.signedIn' : 'ob.notSignedIn')}</span>
            <span class="muted sm grow">{t(claude.loggedIn ? 'ob.signedInHint' : 'ob.signInHint')}</span>
          </div>
          {#if !claude.loggedIn && !useApi}
            <div class="row gap-top"><button type="button" class="btn primary" disabled={working.login} onclick={login}>{working.login ? t('ob.opening') : t('ob.signIn')}</button></div>
          {/if}
        </div>
      {/if}

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
          <p class="muted sm">{t('ob.apiKeyHint')}</p>
        {/if}
      </details>
    {:else if step === 2}
      <h1>{t('ob.libTitle')}</h1>
      <p>{t('ob.libBody')}</p>
      <div class="opts">
        <button type="button" class="opt" class:sel={libraryMode === 'new'} aria-pressed={libraryMode === 'new'} onclick={() => (libraryMode = 'new')}><b>{t('ob.libNew')}</b><span class="muted">{t('ob.libNewHint')}</span></button>
        <button type="button" class="opt" class:sel={libraryMode === 'existing'} aria-pressed={libraryMode === 'existing'} onclick={() => (libraryMode = 'existing')}><b>{t('ob.libExisting')}</b><span class="muted">{t('ob.libExistingHint')}</span></button>
      </div>
      <label for="ob-path">{t('ob.folder')}</label>
      <div class="row"><input id="ob-path" class="grow" dir="ltr" bind:value={s.libraryPath} autocomplete="off" /><button type="button" class="btn" onclick={browse}><Icon name="folder" size={14} /> {t('ob.browse')}</button></div>
      {#if libraryMode === 'new'}
        <label for="ob-seeds">{t('ob.seeds')}</label>
        <textarea id="ob-seeds" rows="3" dir="auto" bind:value={seeds} placeholder={t('ob.seedsPh')}></textarea>
      {/if}
    {:else if step === 3}
      <h1>{t('ob.modTitle')}</h1>
      <p class="muted">{t('ob.modBody')}</p>
      {#if modulesLoading}
        <p class="muted" role="status">{t('ob.loading')}</p>
      {:else if modulesErr}
        <p class="warn" role="alert"><Icon name="warning" size={15} /> <span>{t('ob.modError')} <span dir="auto">{modulesErr}</span></span></p>
        <button type="button" class="btn" onclick={loadModules}>{t('ob.retry')}</button>
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
    {:else if step === 4}
      <h1>{t('ob.syncTitle')}</h1>
      <p>{t('ob.syncBody')}</p>
      <div class="opts">
        <button type="button" class="opt" class:sel={s.sync.mode === 'none'} aria-pressed={s.sync.mode === 'none'} onclick={() => (s.sync.mode = 'none')}><b>{t('ob.syncLocal')}</b><span class="muted">{t('ob.syncLocalHint')}</span></button>
        <button type="button" class="opt" class:sel={s.sync.mode === 'folder'} aria-pressed={s.sync.mode === 'folder'} onclick={() => (s.sync.mode = 'folder')}><b>{t('ob.syncFolder')}</b><span class="muted">{t('ob.syncFolderHint')}</span></button>
      </div>
      {#if s.sync.mode === 'none'}
        <p class="warn"><Icon name="warning" size={15} /> <span>{t('ob.syncLocalWarn')}</span></p>
      {:else if s.sync.mode === 'folder'}
        <div class="card status">
          <p>{t('ob.folderPick')}</p>
          <div class="row gap-top"><code class="grow path"><bdi>{s.libraryPath}</bdi></code><button type="button" class="btn" onclick={browse}><Icon name="folder" size={14} /> {t('ob.chooseFolder')}</button></div>
          {#if inCloud}<p class="okline"><Icon name="check" size={15} /> <span>{t('ob.folderOk')}</span></p>
          {:else}<p class="warn"><Icon name="warning" size={15} /> <span>{t('ob.folderNotCloud')}</span></p>{/if}
          <p class="muted sm">{t('ob.cloudPrivacy')}</p>
        </div>
      {/if}

      <details class="adv" open={isGit}>
        <summary>{t('ob.advGit')}</summary>
        <p class="muted sm">{t('ob.gitBody')}</p>
        <label class="check"><input type="checkbox" checked={isGit} onchange={(e) => setGit(e.currentTarget.checked)} /> {t('ob.useGit')}</label>
        {#if isGit}
          <label for="ob-gitmode">{t('ob.gitWhen')}</label>
          <select id="ob-gitmode" bind:value={s.sync.mode}>
            <option value="git-manual">{t('ob.gitManual')}</option>
            <option value="git-auto">{t('ob.gitAuto')}</option>
          </select>
          <p class="muted sm">{t('ob.wrapDef')}</p>
          <label for="ob-remote">{t('ob.remoteUrl')}</label>
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
          {/if}
        {/if}
      </details>
    {:else if step === 5}
      <h1>{t('ob.lookTitle')}</h1>
      <ThemePicker bind:theme={s.theme} bind:mode={s.mode} />
      <h2>{t('ob.behaviour')}</h2>
      <label class="check"><input type="checkbox" bind:checked={s.notifications} /> {t('ob.notify')}</label>
      <label class="check"><input type="checkbox" bind:checked={s.daemon.keepRunning} /> {t('ob.keepRunning')}</label>
      <label class="check"><input type="checkbox" bind:checked={s.daemon.runAtLogin} /> {t('ob.runAtLogin')}</label>
      <details class="adv">
        <summary>{t('ob.advanced')}</summary>
        <label class="check"><input type="checkbox" bind:checked={wantSearch} /> {t('ob.wantSearch')}</label>
        <p class="muted sm">{t('ob.wantSearchHint')}</p>
      </details>
    {:else}
      <h1>{t('ob.readyTitle')}</h1>
      <div class="card status">
        <p class="row"><Icon name="shield" size={16} /> <b>{t('ob.privTitle')}</b></p>
        <ul class="plain">
          <li>{t('ob.privSent')}</li>
          <li>{t('ob.privStays')}</li>
          <li>{t('ob.privFolders')}</li>
        </ul>
      </div>
      <dl class="sum">
        <dt>{t('ob.sumNotes')}</dt><dd><code><bdi>{s.libraryPath}</bdi></code> · {t(libraryMode === 'new' ? 'ob.sumNew' : 'ob.sumExisting')}</dd>
        <dt>{t('ob.sumAi')}</dt><dd><bdi>Claude</bdi>{#if useApi} · <bdi>{s.ai.claude.baseUrl}</bdi>{/if}</dd>
        <dt>{t('ob.sumModules')}</dt><dd>{modules.filter((m) => s.modules[m.id]).map(modName).join(ar ? '، ' : ', ') || '—'}</dd>
        <dt>{t('ob.sumBackup')}</dt><dd>{syncLabel}{#if isGit && remoteUrl.trim()} · <bdi>{remoteUrl.trim()}</bdi>{/if}</dd>
        <dt>{t('ob.sumLook')}</dt><dd>{t(s.theme)} · {t(s.mode)}</dd>
      </dl>
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
  .steps { list-style: none; counter-reset: st; display: flex; flex-wrap: wrap; gap: 4px 14px; font-size: 12px; color: var(--muted); margin: 0 0 14px; padding: 0; }
  .st { counter-increment: st; }
  .st::before { content: counter(st) '. '; }
  .st.on { color: var(--accent); font-weight: 700; }
  .st.past { color: var(--fg); }
  .body { flex: 1; padding: 24px; }
  .brand { font-size: 36px; }
  .hero-logo { margin-bottom: 14px; filter: drop-shadow(0 10px 24px rgba(0,0,0,.25)); }
  .lbl { font-size: 12px; font-weight: 500; color: var(--muted); margin: 10px 0 4px; }
  .opts { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-bottom: 12px; }
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
  .why { font-size: 12.5px; color: var(--muted); text-align: end; }
  .sm { font-size: 12px; }
  :global(.icon.spin) { animation: ob-spin 1s linear infinite; }
  @keyframes ob-spin { to { transform: rotate(360deg); } }
</style>
