<script lang="ts">
  import { onMount } from 'svelte';
  import { api, post } from '../lib/api';
  import { app, applyTheme, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import ThemePicker from './ThemePicker.svelte';
  import Logo from '../lib/Logo.svelte';

  let { done }: { done: (setupChatId: string | null) => void } = $props();

  const steps = ['Welcome', 'AI', 'Library', 'Modules', 'Sync', 'Look & feel', 'Ready'];
  let step = $state(0);
  let busy = $state(false);
  let claude = $state<any>(app.status?.claude ?? {});
  let git = $state<any>(app.status?.git ?? { installed: true });
  let modules = $state<any[]>([]);
  let s = $state<any>(structuredClone($state.snapshot(app.settings)));
  let libraryMode = $state<'new' | 'existing'>('new');
  let seeds = $state('');
  let remoteUrl = $state('');
  let remoteUser = $state('');
  let remoteToken = $state('');
  let useApi = $state(false);
  let apiToken = $state('');
  let wantSearch = $state(false);

  onMount(async () => {
    modules = await api('/modules');
    s.modules = Object.fromEntries(modules.map((m) => [m.id, m.defaultOn]));
    s.libraryPath = (app.status?.home ? app.status.home + '/Thoughts' : '~/Thoughts');
  });

  $effect(() => { app.settings.language = s.language; applyTheme(); });

  async function checkClaude() { const st = await api('/status'); claude = st.claude; git = st.git; }
  async function installGit() {
    await post('/git/install');
    toast('Follow the installer that opened, then press “Check again”.');
  }

  async function login() {
    await post('/ai/claude/login');
    toast('A browser window opened — finish signing in, then press “Check again”.');
  }

  async function browse() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const p = await open({ directory: true });
      if (typeof p === 'string') s.libraryPath = libraryMode === 'new' ? p + '/Thoughts' : p;
    } catch {}
  }

  const canNext = $derived(
    step === 1 ? (git.installed && claude.installed && (claude.loggedIn || (useApi && s.ai.claude.baseUrl && apiToken))) :
    step === 2 ? !!s.libraryPath : true
  );

  async function finish(trusted = false) {
    busy = true;
    try {
      const r = await post('/onboarding', {
        settings: s, libraryMode, seeds: seeds.split('\n').map((x) => x.trim()).filter(Boolean),
        remoteUrl, remoteUsername: remoteUser, remoteToken, apiToken: useApi ? apiToken : '', trusted,
      });
      if (wantSearch) post('/extensions/search/install').catch((e) => toast('Search extension: ' + e.message));
      app.settings = await api('/settings');
      applyTheme();
      done(r.setupChatId);
    } catch (e: any) {
      const m = String(e.message || '');
      if (m.includes('UNTRUSTED:')) {
        const what = m.split('UNTRUSTED:')[1];
        busy = false;
        if (confirm(`This library contains settings that can run programs on your computer:\n\n• ${what.split('; ').join('\n• ')}\n\nOnly continue if you made this library or trust whoever did.`)) return finish(true);
        return;
      }
      toast(m);
    } finally { busy = false; }
  }
</script>

<div class="ob">
  <div class="drag" data-tauri-drag-region></div>
  <div class="steps row">{#each steps as name, i}<span class="st" class:on={i === step} class:past={i < step}>{i + 1}. {name}</span>{/each}</div>

  <div class="card body">
    {#if step === 0}
      <div class="hero-logo"><Logo size={72} /></div>
      <h1 class="brand">Thoughts</h1>
      <p>Your second brain and workstation: ideas, todos, docs and projects in plain markdown you own — with an AI that thinks with you, saves every discussion, and reshapes the app around you over time.</p>
      <label>{t('language')}</label>
      <div class="row">
        <button class="btn" class:primary={s.language === 'en'} onclick={() => (s.language = 'en')}>English</button>
        <button class="btn" class:primary={s.language === 'ar'} onclick={() => (s.language = 'ar')}>العربية</button>
      </div>
      <label>What should we call you?</label><input bind:value={s.userName} placeholder="Name" />
    {:else if step === 1}
      <h1>Choose your AI</h1>
      <div class="opts">
        <button class="opt" class:sel={s.ai.primary === 'claude'} onclick={() => (s.ai.primary = 'claude')}><b>Claude Code</b><span class="muted">Anthropic's agent. Works with your Claude login or any Anthropic-compatible API.</span></button>
        <button class="opt" disabled><b>Codex</b><span class="muted">Coming in the next milestone. You'll be able to run both side by side.</span></button>
      </div>
      {#if !git.installed}
        <div class="card status">
          <p><b>Git is needed</b> to keep your library's history, so every change can be undone and synced. It's a free tool from Apple.</p>
          <div class="row" style="margin-top:8px"><button class="btn primary" onclick={installGit}>Install git</button><button class="btn" onclick={checkClaude}>Check again</button></div>
        </div>
      {/if}
      <div class="card status">
        {#if !claude.installed}
          <p>Claude Code isn't installed on this computer.</p>
          <a href="https://claude.com/claude-code" target="_blank">Install Claude Code</a> then press <button class="btn" onclick={checkClaude}>Check again</button>
        {:else}
          <div class="row"><span class="pill" class:accent={claude.loggedIn}>{claude.loggedIn ? 'Signed in' : 'Not signed in'}</span><span class="muted sm">{claude.version}</span></div>
          {#if !claude.loggedIn}
            <div class="row" style="margin-top:8px"><button class="btn primary" onclick={login}>Sign in to Claude</button><button class="btn" onclick={checkClaude}>Check again</button></div>
            <label class="check"><input type="checkbox" bind:checked={useApi} /> Use an API key / other provider instead (recommended for API users)</label>
            {#if useApi}
              <label>Anthropic-compatible base URL</label><input bind:value={s.ai.claude.baseUrl} placeholder="https://api.anthropic.com" />
              <label>API token (stored in your system keychain)</label><input type="password" bind:value={apiToken} />
            {/if}
          {/if}
        {/if}
      </div>
    {:else if step === 2}
      <h1>Your library</h1>
      <div class="opts">
        <button class="opt" class:sel={libraryMode === 'new'} onclick={() => (libraryMode = 'new')}><b>Start a new library</b><span class="muted">From the Thoughts starter kit, shaped around your answers.</span></button>
        <button class="opt" class:sel={libraryMode === 'existing'} onclick={() => (libraryMode = 'existing')}><b>Open an existing library</b><span class="muted">A folder with CLAUDE.md + INDEX.md.</span></button>
      </div>
      <label>Folder</label>
      <div class="row"><input class="grow" bind:value={s.libraryPath} /><button class="btn" onclick={browse}>Browse…</button></div>
      {#if libraryMode === 'new'}
        <label>A few things on your mind right now? (one per line, optional — they become your first ideas)</label>
        <textarea rows="3" bind:value={seeds}></textarea>
      {/if}
    {:else if step === 3}
      <h1>{t('modules')}</h1>
      <p class="muted">Pick what you want now. Anything you leave off stays one click away in Settings → Modules — your AI adds it where it fits, even months later.</p>
      {#each modules as m}
        <label class="mod" class:locked={m.id === 'ideas'}>
          <input type="checkbox" bind:checked={s.modules[m.id]} disabled={m.id === 'ideas'} />
          <div class="grow"><b>{s.language === 'ar' ? m.nameAr : m.name}</b><div class="muted sm">{m.description}</div></div>
          {#if !s.modules[m.id]}<span class="pill">{t('addLater')}</span>{/if}
        </label>
      {/each}
    {:else if step === 4}
      <h1>Sync</h1>
      <select bind:value={s.sync.mode}>
        <option value="git-auto">Git — automatic (commit + push after every wrap)</option>
        <option value="git-manual">Git — manual (a Sync button)</option>
        <option value="folder">Folder sync — the library lives in iCloud / Dropbox / OneDrive</option>
        <option value="none">Local only</option>
      </select>
      {#if s.sync.mode.startsWith('git')}
        <label>Remote URL (optional — add more remotes later)</label><input bind:value={remoteUrl} placeholder="https://github.com/you/thoughts.git" />
        <div class="row">
          <div class="grow"><label>Username</label><input bind:value={remoteUser} /></div>
          <div class="grow"><label>Token / app password</label><input type="password" bind:value={remoteToken} /></div>
        </div>
        <label>Store credentials in</label>
        <select bind:value={s.sync.credentialStore}><option value="keychain">System keychain (recommended)</option><option value="file">A git-credentials file in ~/.thoughts</option></select>
      {:else if s.sync.mode === 'folder'}
        <p class="muted sm">Choose a library folder inside your cloud drive on the previous step. Git stays off — mixing git with cloud folders corrupts repositories.</p>
      {/if}
    {:else if step === 5}
      <h1>Look & feel</h1>
      <ThemePicker bind:theme={s.theme} bind:mode={s.mode} />
      <h2>Behaviour</h2>
      <label class="check"><input type="checkbox" bind:checked={s.notifications} /> Notify me when chats and builds finish</label>
      <label class="check"><input type="checkbox" bind:checked={s.daemon.keepRunning} /> Keep the background service running after I close the window</label>
      <label class="check"><input type="checkbox" bind:checked={s.daemon.runAtLogin} /> Start the background service at login</label>
      <label class="check"><input type="checkbox" bind:checked={wantSearch} /> Install the Search extension (semantic search for big libraries — requires a git clone of its separate repo)</label>
    {:else}
      <h1>Ready</h1>
      <ul>
        <li>Library: <code>{s.libraryPath}</code> ({libraryMode})</li>
        <li>AI: Claude Code{useApi ? ' via ' + s.ai.claude.baseUrl : ''}</li>
        <li>Modules: {Object.entries(s.modules).filter(([, v]) => v).map(([k]) => k).join(', ')}</li>
        <li>Sync: {s.sync.mode}{remoteUrl ? ' → ' + remoteUrl : ''}</li>
        <li>Theme: {s.theme} · {s.mode}</li>
      </ul>
      {#if libraryMode === 'new'}<p class="muted">Your AI will now shape the new library around these answers — you'll watch it happen in your first chat.</p>{/if}
    {/if}
  </div>

  <div class="row nav">
    {#if step > 0}<button class="btn" onclick={() => step--}>{t('back')}</button>{/if}
    <span class="grow"></span>
    {#if step < steps.length - 1}
      <button class="btn primary" disabled={!canNext} onclick={() => step++}>{t('next')}</button>
    {:else}
      <button class="btn primary" disabled={busy} onclick={() => finish()}>{busy ? '…' : t('finish')}</button>
    {/if}
  </div>
</div>

<style>
  .ob { max-width: 760px; margin: 0 auto; padding: 0 24px 24px; min-height: 100vh; display: flex; flex-direction: column; }
  .drag { height: 34px; }
  .steps { flex-wrap: wrap; gap: 4px 14px; font-size: 12px; color: var(--muted); margin-bottom: 14px; }
  .st.on { color: var(--accent); font-weight: 700; }
  .st.past { color: var(--fg); }
  .body { flex: 1; padding: 24px; }
  .brand { font-size: 36px; }
  .hero-logo { margin-bottom: 14px; filter: drop-shadow(0 10px 24px rgba(0,0,0,.25)); }
  .opts { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-bottom: 12px; }
  .opt { all: unset; cursor: pointer; border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 12px; display: flex; flex-direction: column; gap: 4px; background: var(--bg); }
  .opt.sel { border-color: var(--accent); outline: 2px solid var(--accent); box-shadow: var(--glow); }
  .opt:disabled { opacity: .5; cursor: default; }
  .status { background: var(--bg); }
  .check { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; }
  .mod { display: flex; gap: 10px; align-items: center; color: var(--fg); font-size: 14px; padding: 8px 0; border-bottom: var(--border-w) solid var(--border); margin: 0; cursor: pointer; }
  .mod.locked { opacity: .8; }
  .nav { padding-top: 14px; }
  .sm { font-size: 12px; }
</style>
