<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, del, post, reconnect } from '../lib/api';
  import { app, go, saveSettings, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import ThemePicker from './ThemePicker.svelte';

  let { reload }: { reload: () => Promise<void> } = $props();

  const tabs = ['general', 'ai', 'git', 'modules', 'daemon', 'appLayer'] as const;
  const tabIcon: Record<string, string> = { general: 'settings', ai: 'ai', git: 'git', modules: 'modules', daemon: 'daemon', appLayer: 'palette' };
  let tab = $state<(typeof tabs)[number]>('general');
  let s = $state<any>(structuredClone($state.snapshot(app.settings)));
  let opts = $state<any>({ models: [], efforts: [], permissionModes: [] });
  let claude = $state<any>(app.status?.claude ?? {});
  let gitStatus = $state<any>({});
  let modules = $state<any[]>([]);
  let updates = $state<any[]>([]);
  let picked = $state<Record<string, boolean>>({});
  let ext = $state<any>(null);
  let apiToken = $state('');
  let remote = $state({ name: '', url: '', username: '', token: '' });
  let customizeText = $state('');

  onMount(async () => {
    api('/ai/claude/options').then((o) => (opts = o)).catch(() => {});
    api('/git').then((g) => (gitStatus = g)).catch(() => {});
    api('/modules').then((m) => (modules = m));
    api('/updates').then((u) => (updates = u));
    api('/extensions').then((e) => (ext = e)).catch(() => {});
  });

  async function save() {
    try { await saveSettings(s); s = structuredClone($state.snapshot(app.settings)); toast('Saved'); }
    catch (e: any) { toast(e.message); }
  }

  async function refreshClaude() { claude = (await api('/status')).claude; }

  async function saveToken() {
    await post('/ai/claude/token', { token: apiToken });
    apiToken = '';
    toast('Token stored in the system keychain');
    app.settings = await api('/settings'); s.ai.claude.hasApiToken = app.settings.ai.claude.hasApiToken;
  }

  async function addRemote() {
    try {
      await post('/git/remotes', remote);
      remote = { name: '', url: '', username: '', token: '' };
      gitStatus = await api('/git');
    } catch (e: any) { toast(e.message); }
  }

  async function removeRemote(n: string) {
    await del(`/git/remotes/${n}`);
    gitStatus = await api('/git');
  }

  async function enableModule(id: string) {
    const r = await post(`/modules/${id}/enable`);
    app.settings = await api('/settings');
    s.modules = app.settings.modules;
    if (r.chatId) go({ kind: 'chat', id: r.chatId });
  }

  async function customize() {
    const r = await post('/layer/customize', { text: customizeText });
    customizeText = '';
    go({ kind: 'chat', id: r.chatId });
  }

  async function applyUpdate(pack: string) {
    const items = Object.entries(picked).filter(([k, v]) => v && k.startsWith(pack + ':')).map(([k]) => k.split(':')[1]);
    if (!items.length) return toast('Pick at least one item');
    const r = await post('/updates/apply', { pack, items });
    go({ kind: 'chat', id: r.chatId });
  }

  async function reorganize() {
    const instructions = prompt('Anything specific? (optional) — runs on a branch; nothing changes until you approve the diff.') ?? null;
    if (instructions === null) return;
    try { const r = await post('/reorganize', { instructions }); go({ kind: 'chat', id: r.chatId }); }
    catch (e: any) { toast(e.message); }
  }

  async function installSearch() {
    try { await post('/extensions/search/install'); ext = await api('/extensions'); toast('Search extension installed'); }
    catch (e: any) { toast(e.message); }
  }
</script>

<div class="settings">
  <nav class="cats">
    <h1>{t('settings')}</h1>
    {#each tabs as k}<button class="cat" class:on={tab === k} onclick={() => (tab = k)}><span class="ci"><Icon name={tabIcon[k]} size={15} /></span>{t(k)}</button>{/each}
  </nav>
  <div class="content">
  <h2 class="title">{t(tab)}</h2>

  {#if tab === 'general'}
    <div class="card">
      <label>{t('language')}</label>
      <select bind:value={s.language}><option value="en">English</option><option value="ar">العربية</option></select>
      <label>Your name</label><input bind:value={s.userName} />
      <ThemePicker bind:theme={s.theme} bind:mode={s.mode} />
      <label class="check"><input type="checkbox" bind:checked={s.notifications} /> Notify me when a chat or build finishes</label>
    </div>
  {:else if tab === 'ai'}
    <div class="card">
      <label>Primary assistant</label>
      <select bind:value={s.ai.primary}><option value="claude">Claude Code</option><option value="codex" disabled>Codex (next milestone)</option></select>
      <label>Second assistant (used interchangeably)</label>
      <select bind:value={s.ai.secondary}><option value={null}>None</option><option value="codex" disabled>Codex (next milestone)</option></select>
      <h2>Claude Code</h2>
      <div class="row">
        {#if claude.installed}
          <span class="pill" class:accent={claude.loggedIn}>{claude.loggedIn ? 'signed in' : 'not signed in'}</span>
          <span class="muted sm">{claude.version} · {claude.path}</span>
          {#if !claude.loggedIn}<button class="btn" onclick={async () => { await post('/ai/claude/login'); toast('Finish signing in in your browser, then press Refresh.'); }}>Sign in</button>{/if}
        {:else}
          <span class="pill">not installed</span>
          <a href="https://claude.com/claude-code" target="_blank">Install Claude Code</a>
        {/if}
        <button class="btn ghost" onclick={refreshClaude}>Refresh</button>
      </div>
      <label>Custom path to the <code>claude</code> binary (optional)</label><input bind:value={s.ai.claude.binary} placeholder="auto-detect" />
      <div class="grid3">
        <div><label>Default model</label>
          <input list="models" bind:value={s.ai.claude.model} placeholder={t('default')} />
          <datalist id="models">{#each opts.models as m}<option value={m}></option>{/each}</datalist></div>
        <div><label>Default thinking</label>
          <select bind:value={s.ai.claude.effort}><option value={null}>{t('default')}</option>{#each opts.efforts as m}<option value={m}>{m}</option>{/each}</select></div>
        <div><label>Default permissions</label>
          <select bind:value={s.ai.claude.permissionMode}><option value={null}>{t('default')}</option>{#each opts.permissionModes as m}<option value={m}>{m}</option>{/each}</select></div>
      </div>
      <p class="muted sm">These lists come from your installed Claude Code, so new models and modes appear automatically. Note: in-app chats can't answer interactive permission prompts yet — <code>acceptEdits</code> or <code>auto</code> work best.</p>
      <div class="grid3">
        <div><label>Mini-wrap model (cheap)</label><input bind:value={s.ai.claude.miniWrapModel} placeholder="haiku" /></div>
        <div><label>Idle minutes before mini-wrap</label><input type="number" min="1" bind:value={s.idleMinutes} /></div>
        <div><label>Wrap reminder after N chats</label><input type="number" min="1" bind:value={s.wrapNagThreshold} /></div>
      </div>
      <h2>Any provider (recommended for API keys)</h2>
      <p class="muted sm">Claude Code works with any Anthropic-compatible endpoint (Anthropic API, OpenRouter, DeepSeek, Kimi, a local gateway…). Set the base URL and token; the token is kept in the system keychain.</p>
      <label>Base URL</label><input bind:value={s.ai.claude.baseUrl} placeholder="https://api.anthropic.com (leave empty to use your Claude login)" />
      <label>API token {#if s.ai.claude.hasApiToken}<span class="pill accent">stored</span>{/if}</label>
      <div class="row"><input type="password" class="grow" bind:value={apiToken} placeholder="sk-…" /><button class="btn" onclick={saveToken}>{apiToken ? 'Store' : 'Clear'}</button></div>
    </div>
  {:else if tab === 'git'}
    <div class="card">
      <label>Library folder</label><input bind:value={s.libraryPath} />
      <label>Sync</label>
      <select bind:value={s.sync.mode}>
        <option value="git-auto">Git — automatic (commit + push after every wrap)</option>
        <option value="git-manual">Git — manual (Sync button)</option>
        <option value="folder">Folder sync (iCloud / Dropbox / OneDrive) — no git</option>
        <option value="none">Local only</option>
      </select>
      <p class="muted sm">Git and a cloud-synced folder on the same library corrupt repositories, so the app won't combine them.</p>
      <label>Store git credentials in</label>
      <select bind:value={s.sync.credentialStore}>
        <option value="keychain">System keychain (recommended)</option>
        <option value="file">~/.thoughts/git-credentials file (outside the library, never committed)</option>
      </select>
    </div>
    {#if s.sync.mode.startsWith('git')}
      <div class="card">
        <h2>{t('remotes')}</h2>
        {#if gitStatus.repo}<p class="muted sm">Branch <b>{gitStatus.branch}</b> · {gitStatus.dirty} changed · ↑{gitStatus.ahead} ↓{gitStatus.behind} · {gitStatus.last}</p>{/if}
        {#each gitStatus.remotes || [] as r, i}
          <div class="row"><b>{r.name}</b>{#if i === 0}<span class="pill">pull from</span>{/if}<code class="grow sm">{r.url}</code><button class="btn ghost" onclick={() => removeRemote(r.name)}>Remove</button></div>
        {/each}
        <p class="muted sm">Sync pulls from the first remote and pushes to all of them.</p>
        <div class="grid2">
          <input bind:value={remote.name} placeholder="name (origin, backup…)" />
          <input bind:value={remote.url} placeholder="https://github.com/me/thoughts.git" />
          <input bind:value={remote.username} placeholder="username" />
          <input type="password" bind:value={remote.token} placeholder="token / app password (optional for SSH)" />
        </div>
        <button class="btn" onclick={addRemote} disabled={!remote.url}>Add remote</button>
      </div>
    {/if}
    <div class="card">
      <h2>Library tools</h2>
      <div class="row"><button class="btn" onclick={reorganize}>{t('reorganize')}…</button><span class="muted sm">AI restructures everything on a branch; you review the diff and merge or discard.</span></div>
      {#if ext}
        <h2>Search extension</h2>
        <p class="muted sm">Optional semantic search (vector index) for big libraries. Lives in its own repo, cloned only if you want it. Your library: {ext.search.files} markdown files (suggested from {ext.search.threshold}).</p>
        {#if ext.search.installed}<span class="pill accent">installed</span>{:else}<button class="btn" onclick={installSearch}>Install (git clone {ext.search.repo})</button>{/if}
      {/if}
    </div>
  {:else if tab === 'modules'}
    <div class="card">
      <p class="muted">Turn on anything you skipped — your AI fits it into the library as it is today.</p>
      {#each modules as m}
        <div class="row mod">
          <div class="grow"><b>{s.language === 'ar' ? m.nameAr : m.name}</b><div class="muted sm">{m.description}</div></div>
          {#if s.modules?.[m.id]}<span class="pill accent">{t('enabled')}</span>
          {:else}<button class="btn" onclick={() => enableModule(m.id)}>{t('enable')}</button>{/if}
        </div>
      {/each}
    </div>
  {:else if tab === 'daemon'}
    <div class="card">
      <p class="muted">The background service owns your AI sessions, so they survive app updates, installs and restarts.</p>
      <label class="check"><input type="checkbox" bind:checked={s.daemon.keepRunning} /> Keep running after I close the window (long chats and builds continue)</label>
      <label class="check"><input type="checkbox" bind:checked={s.daemon.runAtLogin} /> Start at login {#if app.status?.platform !== 'macos'}<span class="muted">(macOS only for now)</span>{/if}</label>
      <button class="btn" onclick={async () => { await reconnect(); toast('Background service restarted'); }}>Restart background service</button>
    </div>
  {:else if tab === 'appLayer'}
    <div class="card">
      <h2>{t('customize')}</h2>
      <p class="muted sm">Describe any change — a new view, a theme, a widget, a different layout. Your AI edits your app layer; every change is a commit you can roll back.</p>
      <div class="row"><input class="grow" bind:value={customizeText} placeholder="e.g. add a view that shows my todos on a weekly calendar" /><button class="btn primary" onclick={customize}>Customize</button></div>
      <div class="row" style="margin-top:8px"><button class="btn" onclick={() => (app.showRollback = true)}><Icon name="rollback" size={15} /> {t('rollback')}…</button><button class="btn" onclick={() => location.replace(location.pathname + '?safe=1')}>Reload in safe mode</button></div>
    </div>
    <div class="card">
      <h2>{t('updates')}</h2>
      <p class="muted sm">Official updates come as packs of files + prompts. Pick what you want; your AI merges it into your customised app without overwriting your changes.</p>
      {#each updates as u}
        <div class="upd"><b>{u.title}</b>
          {#each u.items as it}<label class="check"><input type="checkbox" bind:checked={picked[u.id + ':' + it.id]} /> <b>{it.title}</b> — {it.description}</label>{/each}
          <button class="btn" onclick={() => applyUpdate(u.id)}>Apply selected</button>
        </div>
      {/each}
    </div>
  {/if}

  {#if !['modules', 'appLayer'].includes(tab)}
    <div class="savebar"><button class="btn primary" onclick={save}>{t('save')}</button></div>
  {/if}
  </div>
</div>

<style>
  .settings { display: grid; grid-template-columns: 210px 1fr; min-height: calc(100vh - 26px); }
  .cats { padding: 6px 10px 20px 16px; border-inline-end: 1px solid var(--hairline); display: flex; flex-direction: column; gap: 2px; }
  .cats h1 { font-size: 20px; margin: 4px 6px 12px; }
  .cat { all: unset; cursor: default; display: flex; align-items: center; gap: 10px; padding: 5px 6px; border-radius: 7px; font-size: 13px; }
  .cat:hover { background: var(--fill); }
  .cat.on { background: var(--accent); color: var(--accent-fg); }
  .ci { width: 24px; height: 24px; border-radius: 6px; display: grid; place-items: center; background: var(--tint); color: var(--accent); }
  .cat.on .ci { background: rgba(255,255,255,.22); color: inherit; }
  .content { padding: 6px 28px 28px; max-width: 760px; }
  .title { font-size: 20px; margin: 4px 0 14px; }
  .savebar { position: sticky; bottom: 0; padding: 12px 0; display: flex; justify-content: flex-end; background: linear-gradient(transparent, var(--bg) 40%); }
  .card { margin-bottom: 12px; }
  .check { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin: 8px 0; }
  .grid3 { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }
  .mod { padding: 10px 0; border-bottom: 1px solid var(--hairline); }
  .mod:last-child { border-bottom: 0; }
  .upd { padding: 8px 0; }
  .sm { font-size: 12px; }
</style>
