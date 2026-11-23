<script lang="ts">
  // Guided install and sign-in for Claude Code, in plain steps for this computer, with a live check
  // that turns each step green by itself. The app never downloads or runs an installer: it shows
  // Anthropic's own install line to copy, opens the official pages, and starts the usual sign-in.
  import { onMount } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { api, on, post } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { copyText } from '../lib/content/util';

  /** `claude`: the assistant's status (`{installed, loggedIn, auth…}`), kept current here.
   *  `keyInstead`: the user pays with an API key, so signing in is not needed. */
  let { claude = $bindable({}), keyInstead = false, signInLocked = false }: { claude?: any; keyInstead?: boolean; signInLocked?: boolean } = $props();

  // Anthropic's recommended installer (code.claude.com/docs/en/setup), one line per kind of system.
  const UNIX = 'curl -fsSL https://claude.ai/install.sh | bash';
  const WINDOWS = 'irm https://claude.ai/install.ps1 | iex';
  const SETUP_URL = 'https://code.claude.com/docs/en/setup';
  const TERMINAL_URL = 'https://code.claude.com/docs/en/terminal-guide';
  const PLANS_URL = 'https://claude.com/pricing';

  const os = $derived(app.status?.platform === 'windows' ? 'win' : app.status?.platform === 'linux' ? 'linux' : 'mac');
  const command = $derived(os === 'win' ? WINDOWS : UNIX);
  const installed = $derived(!!claude?.installed);
  const signedIn = $derived(installed && !!claude?.loggedIn);
  const ready = $derived(installed && (signedIn || keyInstead));
  const auth = $derived(claude?.auth ?? {});
  const PLAN_NAMES: Record<string, string> = { pro: 'Claude Pro', max: 'Claude Max', team: 'Claude Team', enterprise: 'Claude Enterprise' };
  const plan = $derived(typeof auth.subscriptionType === 'string' && auth.subscriptionType ? PLAN_NAMES[auth.subscriptionType] ?? auth.subscriptionType : '');
  // Signed in with a Claude account whose plan the program does not report: say what to check.
  const planUnknown = $derived(signedIn && !keyInstead && auth.authMethod === 'claude.ai' && !plan);
  const who = $derived(typeof auth.email === 'string' ? auth.email : '');

  let copied = $state(false);
  let copiedAt = $state(0);
  let signingIn = $state(false);
  let opened = $state(false);
  let checking = $state(false);
  let checkFailed = $state(false);
  let now = $state(Date.now());
  // Said aloud (and kept on screen) when a step turns green by itself.
  let said = $state('');

  function apply(c: any) {
    if (!c || typeof c !== 'object') return;
    const was = { installed, signedIn };
    claude = c;
    if (app.status) app.status = { ...app.status, claude: c };
    if (!was.installed && c.installed) said = t(c.loggedIn || keyInstead ? 'sb.guide.ready' : 'sb.guide.nowInstalled');
    else if (was.installed && !was.signedIn && c.installed && c.loggedIn) { said = t('sb.guide.nowSignedIn'); opened = false; }
  }

  let asking = false;
  async function look(byHand = false) {
    if (asking) return;
    asking = true;
    if (byHand) checking = true;
    try { apply(await api('/ai/claude/status')); checkFailed = false; }
    catch { if (byHand) checkFailed = true; /* the service is restarting: the next look tries again */ }
    finally { asking = false; checking = false; }
  }

  // The live check: while something is still to do, look every few seconds (only while the window
  // is in front, so a hidden window costs nothing).
  $effect(() => {
    if (ready) return;
    const timer = setInterval(() => { now = Date.now(); if (document.visibilityState === 'visible') look(); }, 3000);
    return () => clearInterval(timer);
  });
  onMount(() => {
    look();
    // The service says when a sign-in attempt is over (finished, abandoned or timed out).
    return on((e) => { if (e.type === 'auth.changed') look(); });
  });

  async function copy() {
    if (await copyText(command)) { copied = true; copiedAt = Date.now(); setTimeout(() => (copied = false), 2500); }
    else toast(t('sb.guide.copyFailed'), { kind: 'error' });
  }

  async function signIn() {
    if (signingIn) return;
    signingIn = true;
    try { await post('/ai/claude/login'); opened = true; toast(t('sb.guide.signInOpened'), { key: 'auth' }); }
    catch (e: any) { toast(t('sb.guide.signInFailed'), { kind: 'error', key: 'auth', detail: e?.message }); }
    finally { signingIn = false; }
  }

  // A minute and a half after the line was copied and still nothing: the likely reasons.
  const slow = $derived(!installed && copiedAt > 0 && now - copiedAt > 90_000);
</script>

<div class="guide" aria-labelledby="sbg-title">
  <p class="sr-only" id="sbg-title">{t('sb.guide.title')}</p>
  <div class="sr-only" role="status" aria-live="polite">{said}</div>

  <section class="step" class:ok={installed} aria-labelledby="sbg-s1">
    <div class="head">
      <span class="mark" class:ok={installed}><Icon name={installed ? 'check' : 'download'} size={15} /></span>
      <h3 id="sbg-s1">{t('sb.guide.step1')}</h3>
    </div>
    {#if installed}
      <p class="okline">{t('sb.guide.installed')}{#if claude?.version} <span class="muted"><bdi>{claude.version}</bdi></span>{/if}</p>
    {:else}
      <p class="muted">{t('sb.guide.step1Body')}</p>
      <ol class="how">
        <li>{t('sb.guide.' + os + '1')}</li>
        <li>
          {t('sb.guide.' + os + '2')}
          <div class="cmd" dir="ltr">
            <code aria-label={t('sb.guide.cmdLabel')}>{command}</code>
            <button type="button" class="btn" onclick={copy}><Icon name={copied ? 'check' : 'copy'} size={14} /> {copied ? t('sb.guide.copied') : t('sb.guide.copy')}</button>
          </div>
          <span class="muted sm">{t('sb.guide.source')}</span>
        </li>
        <li>{t('sb.guide.step3')}</li>
      </ol>
      <p class="links">
        <a href={SETUP_URL} target="_blank" rel="noreferrer">{t('sb.guide.official')} <Icon name="external" size={13} /></a>
        <a href={TERMINAL_URL} target="_blank" rel="noreferrer">{t('sb.guide.terminalHelp')} <Icon name="external" size={13} /></a>
      </p>
      <p class="live" role="status"><span class="dot" aria-hidden="true"></span> {t('sb.guide.watching')}</p>
      {#if slow}<p class="muted sm">{t('sb.guide.notFound')}</p>{/if}
    {/if}
  </section>

  <section class="step" class:ok={signedIn || (installed && keyInstead)} class:off={!installed} aria-labelledby="sbg-s2">
    <div class="head">
      <span class="mark" class:ok={signedIn || (installed && keyInstead)}><Icon name={signedIn || (installed && keyInstead) ? 'check' : 'lock'} size={15} /></span>
      <h3 id="sbg-s2">{t('sb.guide.step2')}</h3>
    </div>
    {#if keyInstead}
      <p class="muted">{t('sb.guide.keyInstead')}</p>
    {:else if !installed}
      <p class="muted">{t('sb.guide.step2After')}</p>
    {:else if signedIn}
      <p class="okline">{who ? t('sb.guide.signedInAs', { who }) : t('sb.guide.signedIn')}{#if plan} {t('sb.guide.plan', { plan })}{/if}</p>
      {#if planUnknown}
        <p class="note"><Icon name="info" size={15} /> <span>{t('sb.guide.planUnknown')} <a href={PLANS_URL} target="_blank" rel="noreferrer">{t('sb.guide.plans')} <Icon name="external" size={12} /></a></span></p>
      {/if}
    {:else}
      <p class="muted">{t('sb.guide.step2Body')} <a href={PLANS_URL} target="_blank" rel="noreferrer">{t('sb.guide.plans')} <Icon name="external" size={12} /></a></p>
      <div class="row">
        <button type="button" class="btn primary" disabled={signingIn || signInLocked} aria-busy={signingIn} onclick={signIn}>{signingIn ? t('sb.guide.opening') : opened ? t('sb.guide.signInAgain') : t('sb.guide.signIn')}</button>
      </div>
      {#if opened}<p class="live" role="status"><span class="dot" aria-hidden="true"></span> {t('sb.guide.waitingSignIn')}</p>{/if}
    {/if}
  </section>

  {#if !ready}
    <div class="row foot">
      <button type="button" class="btn ghost" disabled={checking} aria-busy={checking} onclick={() => look(true)}><Icon name="sync" size={14} class={checking ? 'spin' : ''} /> {checking ? t('sb.guide.checking') : t('sb.guide.check')}</button>
      {#if checkFailed}<span class="muted sm" role="alert">{t('sb.guide.checkFailed')}</span>{/if}
    </div>
  {:else if said}
    <p class="okline" aria-hidden="true"><Icon name="check" size={15} /> {said}</p>
  {/if}
</div>

<style>
  .guide { margin-top: 10px; }
  .step { border: var(--border-w) solid var(--border); border-radius: var(--radius); padding: 12px 14px; margin-bottom: 10px; background: var(--bg); }
  .step.ok { border-color: color-mix(in srgb, var(--ok) 55%, var(--border)); }
  .step.off { opacity: .75; }
  .head { display: flex; gap: 10px; align-items: center; }
  .head h3 { margin: 0; font-size: var(--fs-md, 15px); }
  .mark { width: 24px; height: 24px; border-radius: 50%; display: grid; place-items: center; background: var(--fill); color: var(--muted); flex: none; }
  .mark.ok { color: var(--ok); }
  .step p { margin: 8px 0 0; }
  .how { margin: 8px 0 0; padding-inline-start: 20px; }
  .how li { margin-bottom: 8px; }
  .cmd { display: flex; gap: 8px; align-items: center; margin: 6px 0 2px; }
  .cmd code { flex: 1; min-width: 0; overflow-wrap: anywhere; padding: 8px 10px; border-radius: 8px; background: var(--fill); font-size: 13px; user-select: all; }
  .links { display: flex; flex-wrap: wrap; gap: 6px 16px; font-size: var(--fs-sm); }
  .links a, .step a { text-decoration: none; }
  .live { display: flex; gap: 8px; align-items: center; font-size: var(--fs-sm); color: var(--muted); }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); flex: none; animation: sbg-pulse 1.6s ease-in-out infinite; }
  .okline { color: var(--fg); display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .okline :global(.icon) { color: var(--ok); }
  .note { display: flex; gap: 8px; align-items: flex-start; font-size: var(--fs-sm); }
  .note :global(.icon) { flex: none; margin-top: 2px; }
  .foot { gap: 10px; align-items: center; }
  .sm { font-size: var(--fs-xs); }
  @keyframes sbg-pulse { 50% { opacity: .3; } }
  @media (prefers-reduced-motion: reduce) { .dot { animation: none; } }
  :global(.icon.spin) { animation: sbg-spin 1s linear infinite; }
  @keyframes sbg-spin { to { transform: rotate(360deg); } }
</style>
