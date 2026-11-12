<script lang="ts">
  // Settings › Assistant, the Codex parts: which assistant program answers (Claude Code or Codex),
  // Codex's sign-in and install steps, what it can and cannot enforce here, and its own model and
  // effort. Every list comes from the installed Codex (`/ai/codex/options`). Changes bubble up to
  // the Settings page, which saves them like any other setting.
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, post } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { copyText } from '../lib/content/util';

  let { ai = $bindable(), part, vis, readOnly = false }: {
    ai: any; part: 'which' | 'codex' | 'advanced';
    /** Settings search: is a row with one of these labels to be shown? */
    vis: (...keys: string[]) => boolean;
    readOnly?: boolean;
  } = $props();

  // A settings file from before Codex existed has no Codex part yet.
  $effect.pre(() => { if (ai && !ai.codex) ai.codex = { binary: null, model: null, effort: null, miniWrapModel: null }; });

  const codex = $derived<any>(app.status?.codex ?? {});
  const claude = $derived<any>(app.status?.claude ?? {});
  let opts = $state<any>({ models: [], efforts: [] });
  let busy = $state<Record<string, boolean>>({});
  let copied = $state('');

  function loadOpts() { api('/ai/codex/options').then((o) => (opts = { models: [], efforts: [], ...o })).catch(() => {}); }
  onMount(() => { if (codex.installed && part !== 'which') loadOpts(); });

  async function check() {
    if (busy.check) return;
    busy.check = true;
    try {
      const s = await post('/ai/codex/recheck');
      app.status = { ...app.status, codex: s };
      if (s?.installed) loadOpts();
    } catch (e) { failed(t('codex.set.check'), e); }
    finally { busy.check = false; }
  }

  async function signIn() {
    if (busy.login) return;
    busy.login = true;
    try { await post('/ai/codex/login'); toast(t('codex.set.signInToast'), { key: 'auth' }); }
    catch (e) { failed(t('codex.set.signInFailed'), e); }
    finally { busy.login = false; }
  }

  /** What a program's status says, in a few words. */
  function said(s: any, program: 'claude' | 'codex'): string {
    if (!s?.installed) return t('codex.set.notInstalled');
    if (program === 'codex') {
      if (s.loggedIn) return t(s.method ? `codex.set.signedIn.${s.method}` : 'codex.set.signedIn');
      return t(s.known === false ? 'codex.set.unknown' : 'codex.set.notSignedIn');
    }
    // Claude Code paid with a key of its own does not need the account.
    if (s.provider === 'anthropicKey' || s.provider === 'service') return t('runa.acct.' + s.provider);
    return t(s.loggedIn ? 'codex.set.signedIn' : 'codex.set.notSignedIn');
  }

  const programs = $derived([
    { id: 'claude', name: 'Claude Code', status: claude },
    { id: 'codex', name: 'Codex', status: codex },
  ]);

  const INSTALL = ['brew install codex', 'npm install -g @openai/codex'];
  async function copy(cmd: string) {
    if (await copyText(cmd)) { copied = cmd; setTimeout(() => { if (copied === cmd) copied = ''; }, 2000); }
  }
  const effortLabel = (e: string) => { const k = 'codex.effort.' + e; const v = t(k); return v === k ? e : v; };
</script>

{#if part === 'which' && vis('codex.set.which')}
  <fieldset class="which" aria-describedby="st-which-hint">
    <legend>{t('codex.set.which')}</legend>
    {#each programs as p (p.id)}
      <label class="radio">
        <input type="radio" name="st-primary" value={p.id} bind:group={ai.primary} disabled={readOnly || (!p.status?.installed && ai.primary !== p.id)} />
        <span><b><bdi>{p.name}</bdi></b> <span class="pill sm-pill" class:accent={p.status?.installed && (p.status?.loggedIn || p.status?.provider === 'anthropicKey' || p.status?.provider === 'service')}>{said(p.status, p.id as 'claude' | 'codex')}</span></span>
      </label>
    {/each}
    <p class="muted sm" id="st-which-hint">{t('codex.set.whichHint')}</p>
    {#if !codex.installed && ai.primary !== 'codex'}<p class="muted sm">{t('codex.set.needsInstall')}</p>{/if}
  </fieldset>
{:else if part === 'codex' && vis('codex.set.title', 'codex.set.limitsTitle')}
  <h3><bdi>Codex</bdi></h3>
  <p class="muted sm">{t('codex.set.intro')}</p>
  <div class="row wrap">
    <span class="pill" class:accent={codex.installed && codex.loggedIn}>{said(codex, 'codex')}</span>
    {#if codex.installed && !codex.loggedIn}
      <button type="button" class="btn primary" disabled={busy.login || readOnly} aria-busy={busy.login} onclick={signIn}>{t('codex.set.signIn')}</button>
    {/if}
    {#if codex.version}<span class="muted sm" dir="ltr"><bdi>{codex.version}</bdi></span>{/if}
    <button type="button" class="btn ghost" disabled={busy.check} aria-busy={busy.check} onclick={check}><Icon name="sync" size={14} /> {busy.check ? t('codex.set.checking') : t('codex.set.check')}</button>
  </div>
  {#if !codex.installed}
    <div class="install" role="group" aria-label={t('codex.set.installTitle')}>
      <p class="sm"><b>{t('codex.set.installTitle')}</b></p>
      <ol class="sm">
        <li>{t('codex.set.install1')} <a href="https://github.com/openai/codex" target="_blank" rel="noreferrer">{t('codex.set.installOpen')} <Icon name="external" size={12} /></a></li>
        <li>{t('codex.set.install2')}
          <ul class="cmds">
            {#each INSTALL as cmd (cmd)}
              <li><code dir="ltr">{cmd}</code> <button type="button" class="btn ghost tiny" onclick={() => copy(cmd)}>{copied === cmd ? t('codex.set.copied') : t('codex.set.copy')}</button></li>
            {/each}
          </ul>
        </li>
        <li>{t('codex.set.install3')}</li>
      </ol>
    </div>
  {:else}
    <p class="sm limits-title"><b>{t('codex.set.limitsTitle')}</b></p>
    <ul class="limits sm">
      <li>{t('codex.lim.sandbox')}</li>
      <li class="warn"><Icon name="warning" size={13} /> <span>{t('codex.lim.private')}</span></li>
      <li>{t('codex.lim.claude')}</li>
      <li>{t('codex.lim.cost')}</li>
      <li>{t('codex.lim.stream')}</li>
    </ul>
  {/if}
{:else if part === 'advanced' && ai.codex && vis('codex.set.model', 'codex.set.effort', 'codex.set.mini', 'codex.set.binary')}
  <h3><bdi>Codex</bdi></h3>
  {#if vis('codex.set.model')}
    <label for="st-cx-model">{t('codex.set.model')}</label>
    <input id="st-cx-model" dir="ltr" list="st-cx-models" bind:value={ai.codex.model} placeholder={t('codex.set.auto')} autocomplete="off" />
    <datalist id="st-cx-models">{#each opts.models as m (m)}<option value={m}></option>{/each}</datalist>
    <p class="muted sm">{t('codex.set.modelHint')}</p>
  {/if}
  {#if vis('codex.set.effort') && opts.efforts.length}
    <label for="st-cx-effort">{t('codex.set.effort')}</label>
    <select id="st-cx-effort" bind:value={ai.codex.effort}>
      <option value={null}>{t('codex.set.auto')}</option>
      {#each opts.efforts as e (e)}<option value={e}>{effortLabel(e)}</option>{/each}
    </select>
  {/if}
  {#if vis('codex.set.mini')}
    <label for="st-cx-mini">{t('codex.set.mini')}</label>
    <input id="st-cx-mini" dir="ltr" list="st-cx-models" bind:value={ai.codex.miniWrapModel} placeholder={t('codex.set.auto')} autocomplete="off" />
    <p class="muted sm">{t('codex.set.miniHint')}</p>
  {/if}
  {#if vis('codex.set.binary')}
    <label for="st-cx-bin">{t('codex.set.binary')}</label>
    <input id="st-cx-bin" dir="ltr" bind:value={ai.codex.binary} placeholder={codex.path || t('codex.set.auto')} autocomplete="off" />
    <p class="muted sm">{t('codex.set.binaryHint')}</p>
  {/if}
{/if}

<style>
  .which { border: 0; padding: 0; margin: 0 0 14px; min-width: 0; }
  .which legend { font-weight: 600; font-size: 14px; padding: 0; margin-bottom: 2px; }
  .radio { display: flex; gap: 8px; align-items: flex-start; margin: 8px 0; cursor: pointer; font-size: 13px; color: var(--fg); }
  .radio input { margin-top: 4px; }
  .sm-pill { font-size: var(--fs-xs); margin-inline-start: 6px; }
  .sm { font-size: var(--fs-xs); }
  h3 { font-size: 14px; font-weight: 600; margin: 18px 0 6px; }
  .install { margin-top: 10px; padding: 10px 12px; border-radius: var(--radius); background: var(--fill); }
  .install p { margin: 0 0 6px; }
  .install ol { margin: 0; padding-inline-start: 20px; display: grid; gap: 6px; }
  .cmds { list-style: none; margin: 6px 0 0; padding: 0; display: grid; gap: 4px; }
  .cmds code { font-family: var(--mono); padding: 2px 6px; border-radius: 6px; background: var(--panel); }
  .tiny { font-size: var(--fs-xs); padding: 2px 8px; }
  .limits-title { margin: 12px 0 4px; }
  .limits { margin: 0; padding-inline-start: 20px; display: grid; gap: 4px; }
  .limits .warn { list-style: none; margin-inline-start: -20px; display: flex; gap: 6px; align-items: flex-start; }
  .limits .warn :global(.icon) { flex: none; margin-top: 2px; color: var(--danger); }
</style>
