<script lang="ts">
  // Settings › Maintenance: "Updates for testers". While the app's releases are not public, a
  // tester gets updates through their own `gh` sign-in (decisions.md, 2026-11-15). This card only
  // presents that: it says what is in place, shows each terminal line with a Copy button, and
  // checks again. It never installs anything, never signs in, and never turns the switch on: only
  // the terminal line does (`ghfeed::updates_gh_state` in the shell reads, it does not write).
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { inTauri } from '../lib/api';
  import { t } from '../lib/i18n';
  import { copyText } from '../lib/content/util';
  import { toast } from '../lib/store.svelte';

  type State = { on: boolean; gh: boolean; state: string; account?: string | null; detail?: string; command?: { on?: string | null; off?: string | null } };

  const DOWNLOAD = 'https://cli.github.com/';
  const SIGN_IN = 'gh auth login';
  const os = (window as any).__THOUGHTS_PLATFORM__ as string | undefined;
  const mac = os === 'macos';
  const install = mac ? 'brew install gh' : os === 'windows' ? 'winget install --id GitHub.cli' : '';

  let st = $state<State | null>(null);
  let failedToAsk = $state(false);
  let checking = $state(false);
  let copied = $state('');

  async function check() {
    if (checking) return;
    checking = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      st = await invoke<State>('updates_gh_state');
      failedToAsk = false;
    } catch { failedToAsk = true; }
    finally { checking = false; }
  }
  onMount(() => { if (inTauri()) check(); });

  async function copy(line: string) {
    if (await copyText(line)) { copied = line; setTimeout(() => { if (copied === line) copied = ''; }, 2000); }
    else toast(t('ux-settings.gh.copyFailed'), { kind: 'error' });
  }

  async function openDownload() {
    try { const { openUrl } = await import('@tauri-apps/plugin-opener'); await openUrl(DOWNLOAD); }
    catch { window.open(DOWNLOAD, '_blank', 'noopener'); }
  }

  const ready = $derived(st?.on && st.state === 'ready');
  /** The sentence for a switch that is on but not usable yet. */
  const onProblem = $derived(st?.on && st.state !== 'ready' ? ({
    'gh-missing': 'ux-settings.gh.onMissing', 'gh-signed-out': 'ux-settings.gh.onSignedOut', 'gh-refused': 'ux-settings.gh.onRefused',
    'gh-no-access': 'ux-settings.gh.noAccess', offline: 'ux-settings.gh.offline',
  } as Record<string, string>)[st.state] ?? 'ux-settings.gh.failed' : '');
  /** Show the sign-in line again (step 2) for a switch that is on. */
  const needsSignIn = $derived(st?.on && ['gh-signed-out', 'gh-refused', 'gh-no-access'].includes(st.state));
</script>

{#snippet line(text: string)}
  <div class="line">
    <code dir="ltr">{text}</code>
    <button type="button" class="btn sm" onclick={() => copy(text)} aria-label={t('ux-settings.gh.copyAria', { line: text })}>
      <Icon name={copied === text ? 'check' : 'copy'} size={13} /> {copied === text ? t('ux-settings.gh.copied') : t('ux-settings.gh.copy')}
    </button>
  </div>
{/snippet}

{#snippet getGh()}
  <p class="muted sm">{t('ux-settings.gh.step1Missing')}</p>
  <button type="button" class="btn sm" onclick={openDownload}><Icon name="external" size={13} /> {t('ux-settings.gh.download')}</button>
  {#if install}
    <p class="muted sm">{t(mac ? 'ux-settings.gh.brew' : 'ux-settings.gh.winget')}</p>
    {@render line(install)}
  {/if}
{/snippet}

{#if inTauri()}
  <details class="card" open={!!st?.on}>
    <summary><h3>{t('ux-settings.gh.title')}</h3></summary>
    <div aria-live="polite">
      {#if !st && !failedToAsk}
        <p class="muted sm">{t('ux-settings.gh.checking')}</p>
      {:else if failedToAsk || !st}
        <p class="err" role="alert">{t('ux-settings.gh.unknown')}</p>
      {:else if ready}
        <p class="ok"><Icon name="check" size={15} /> <span>{st.account ? t('ux-settings.gh.readyAs', { name: st.account }) : t('ux-settings.gh.ready')}</span></p>
        <p class="muted sm">{t('ux-settings.gh.readyBody')}</p>
      {:else if st.on}
        <p class="warn"><Icon name="warning" size={15} /> <span>{t(onProblem)}</span></p>
        {#if st.state === 'failed' && st.detail}<p class="muted sm detail" dir="auto">{st.detail}</p>{/if}
        {#if st.state === 'gh-missing'}{@render getGh()}{/if}
        {#if needsSignIn}
          {#if st.state === 'gh-signed-out'}<p class="muted sm">{t('ux-settings.gh.step2Body')}</p>{/if}
          {@render line(SIGN_IN)}
        {/if}
      {:else}
        <p class="muted sm">{t('ux-settings.gh.intro')}</p>
        <p class="muted sm">{t(mac ? 'ux-settings.gh.terminal' : 'ux-settings.gh.terminalOther')}</p>
        <ol class="steps">
          <li>
            <b>{t('ux-settings.gh.step1')}</b>
            {#if st.gh}
              <p class="ok"><Icon name="check" size={14} /> <span>{t('ux-settings.gh.step1Found')}</span></p>
            {:else}
              {@render getGh()}
            {/if}
          </li>
          <li>
            <b>{t('ux-settings.gh.step2')}</b>
            <p class="muted sm">{t('ux-settings.gh.step2Body')}</p>
            {@render line(SIGN_IN)}
            <p class="muted sm">{t('ux-settings.gh.step2Later')}</p>
          </li>
          <li>
            <b>{t('ux-settings.gh.step3')}</b>
            <p class="muted sm">{t('ux-settings.gh.step3Body')}</p>
            {#if st.command?.on}{@render line(st.command.on)}{:else}<p class="muted sm">{t('ux-settings.gh.noCommand')}</p>{/if}
          </li>
        </ol>
      {/if}
    </div>
    {#if st?.on && st.command?.off}
      <details class="off">
        <summary class="sm">{t('ux-settings.gh.turnOff')}</summary>
        <p class="muted sm">{t('ux-settings.gh.turnOffBody')}</p>
        {@render line(st.command.off)}
      </details>
    {/if}
    <button type="button" class="btn recheck" disabled={checking} onclick={check}><Icon name="sync" size={14} /> {checking ? t('ux-settings.gh.checking') : t('ux-settings.gh.recheck')}</button>
  </details>
{/if}

<style>
  summary { cursor: pointer; }
  summary h3 { display: inline; margin: 0; }
  .sm { font-size: var(--fs-xs); }
  .steps { margin: 8px 0 0; padding-inline-start: 20px; }
  .steps li { margin: 10px 0; }
  .steps p { margin: 4px 0; }
  .line { display: flex; gap: 8px; align-items: center; margin: 6px 0; flex-wrap: wrap; }
  .line code { flex: 1; min-width: 0; overflow-wrap: anywhere; padding: 6px 8px; border-radius: var(--radius); background: var(--fill); font-size: 12px; text-align: start; user-select: all; }
  .line .btn { flex: none; }
  .ok, .warn { display: flex; gap: 6px; align-items: flex-start; font-size: 13px; margin: 8px 0 4px; }
  .ok { color: var(--ok); }
  .warn :global(.icon) { flex: none; margin-top: 2px; color: var(--danger); }
  .ok :global(.icon) { flex: none; margin-top: 2px; }
  .err { color: var(--danger); font-size: var(--fs-sm); margin: 6px 0 0; }
  .detail { overflow-wrap: anywhere; }
  .off { margin-top: 10px; }
  .recheck { margin-top: 10px; }
</style>
