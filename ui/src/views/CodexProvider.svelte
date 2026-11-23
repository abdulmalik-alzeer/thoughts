<script lang="ts">
  // Settings › Assistant › Codex: another provider. Off unless the user sets it; then Codex's runs go
  // to that OpenAI-compatible service (its Responses API) with the key saved for it. The address and
  // the key are saved together, through their own route: the background service keeps the key in
  // the password store and binds it to this one address, exactly as for Claude Code's service.
  import Icon from '../lib/Icon.svelte';
  import ProviderPresets from './ProviderPresets.svelte';
  import HelpTip from '../lib/HelpTip.svelte';
  import { del, post, put } from '../lib/api';
  import { app, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { codexProviderKind, keylessAllowed, onHomeNetwork } from '../lib/providers';

  let { onsaved, readOnly = false }: { onsaved: () => Promise<void>; readOnly?: boolean } = $props();

  const saved = $derived<any>(app.settings?.ai?.codex ?? {});
  const savedUrl = $derived(String(saved.baseUrl ?? '').trim());
  const kind = $derived(codexProviderKind(saved));
  const fullOn = $derived(!!app.settings?.fullAccess?.on);
  const installed = $derived(!!app.status?.codex?.installed);

  let url = $state<string>(String(app.settings?.ai?.codex?.baseUrl ?? ''));
  let token = $state('');
  let noKey = $state<boolean>(!!app.settings?.ai?.codex?.keyless);
  let test = $state<{ ok: boolean; message?: string } | null>(null);
  let busy = $state<Record<string, boolean>>({});

  const noKeyOffered = $derived(!!url.trim() && keylessAllowed(url, fullOn));
  const sendNoKey = $derived(noKeyOffered && noKey && !token.trim());
  const same = $derived(!!url.trim() && url.trim() === savedUrl);
  const dirty = $derived(url.trim() !== savedUrl || !!token.trim() || (!!url.trim() && noKey) !== !!saved.keyless);
  const err = $derived.by(() => {
    const u = url.trim();
    if (u && !/^https?:\/\/\S+$/.test(u)) return t('set.errUrl');
    if (dirty && u && noKey && !token.trim() && !noKeyOffered) return t('prov5.noKeyOnlyLocal');
    if (dirty && u && !token.trim() && !sendNoKey && !(same && saved.hasApiToken)) return t('runa.prov.needKey');
    return '';
  });
  const canTest = $derived(!err && !!url.trim() && (!!token.trim() || sendNoKey || (same && (saved.hasApiToken || saved.keyless))));
  const body = () => ({ baseUrl: url.trim() || null, token: token.trim() || null, noKey: sendNoKey });

  async function act(key: string, fn: () => Promise<void>) {
    if (busy[key]) return;
    busy[key] = true;
    try { await fn(); } catch (e) { failed(t(key === 'models' ? 'prov5.models.failed' : 'prov5.codex.failed'), e); }
    finally { busy[key] = false; }
  }

  const save = () => act('save', async () => {
    if (err || !dirty) return;
    const r = await put<{ provider: string; keyRemoved: boolean }>('/ai/codex/provider', body());
    token = ''; test = null;
    await onsaved();
    url = String(app.settings.ai.codex?.baseUrl ?? ''); noKey = !!app.settings.ai.codex?.keyless;
    toast(t('prov5.codex.saved.' + (r.provider === 'service' ? 'service' : 'own')) + (r.keyRemoved ? ' ' + t('runa.prov.oldKeyRemoved') : ''), { kind: 'success' });
  });

  async function remove() {
    if (kind === 'own') return;
    if (!(await confirmDialog({ title: t('prov5.codex.removeTitle'), body: t('prov5.codex.removeBody'), confirmLabel: t('runa.prov.remove'), danger: true }))) return;
    await act('save', async () => {
      await del('/ai/codex/provider');
      token = ''; test = null; url = ''; noKey = false;
      await onsaved();
      toast(t('prov5.codex.removed'));
    });
  }

  const tryIt = () => act('test', async () => {
    test = null;
    test = await post('/ai/codex/provider/test', dirty ? body() : {});
  });

  const loadModels = () => act('models', async () => {
    const r = await post<{ models: string[] }>('/ai/codex/provider/models');
    toast(t('prov5.models.loaded', { n: r.models.length }), { kind: 'success' });
  });
</script>

{#if installed || kind !== 'own'}
  <h3>{t('prov5.codex.title')} <HelpTip text={t('prov5.codex.hint')} label={t('prov5.codex.title')} /></h3>
  {#if kind === 'service'}<p class="sm"><span class="pill accent">{t('prov5.codex.on')}</span></p>
  {:else if kind === 'incomplete'}<p class="err sm" role="status">{t('prov5.codex.incomplete')}</p>{/if}
  <ProviderPresets program="codex" {url} disabled={readOnly} onpick={(u, n) => { url = u; noKey = n; test = null; }} />
  <label for="st-cx-base">{t('prov5.codex.address')}</label>
  <input id="st-cx-base" dir="ltr" bind:value={url} placeholder="https://openrouter.ai/api/v1" aria-invalid={!!err} aria-describedby={err ? 'st-cx-err' : undefined} autocomplete="off" disabled={readOnly} />
  {#if onHomeNetwork(url, fullOn)}<p class="muted sm" role="note">{t('prov5.lan')}</p>{/if}
  {#if noKeyOffered}
    <label class="check"><input type="checkbox" bind:checked={noKey} disabled={readOnly} aria-describedby="st-cx-nokey" /> {t('prov5.noKey')}</label>
    <p class="muted sm" id="st-cx-nokey">{t('prov5.codex.noKeyHint')}</p>
  {/if}
  <label for="st-cx-key">{t('prov5.codex.key')} <HelpTip text={t('prov5.codex.keyHint')} label={t('prov5.codex.key')} /> {#if saved.hasApiToken}<span class="pill accent">{t('prov5.codex.keyOn')}</span>{:else if saved.keyless && kind === 'service'}<span class="pill">{t('prov5.keylessPill')}</span>{/if}</label>
  <input id="st-cx-key" dir="ltr" type="password" bind:value={token} placeholder="sk-…" aria-describedby={err ? 'st-cx-err' : undefined} autocomplete="off" disabled={readOnly || sendNoKey} />
  {#if err}<p class="err" id="st-cx-err" role="alert">{err}</p>{/if}
  <div class="row wrap gap-top">
    <button type="button" class="btn primary" disabled={!dirty || !!err || busy.save || readOnly} aria-busy={busy.save} onclick={save}>{t('runa.prov.save')}</button>
    <button type="button" class="btn" disabled={!canTest || busy.test} aria-busy={busy.test} onclick={tryIt}>{busy.test ? t('runa.prov.testing') : t('runa.prov.test')}</button>
    <HelpTip text={t('runc.prov.testHint')} label={t('runa.prov.test')} />
    <button type="button" class="btn danger" disabled={kind === 'own' || busy.save || readOnly} onclick={remove}>{t('runa.prov.remove')}</button>
  </div>
  {#if test}<p class="sm" class:err={!test.ok} role="status" dir="auto">{test.ok ? t('runa.prov.testOk') : test.message}</p>{/if}
  {#if kind === 'service'}
    <div class="row wrap gap-top">
      <button type="button" class="btn" disabled={dirty || busy.models} aria-busy={busy.models} aria-describedby="st-cx-models" onclick={loadModels}><Icon name="sync" size={14} /> {busy.models ? t('prov5.models.loading') : t('prov5.models.load')}</button>
    </div>
    <p class="muted sm" id="st-cx-models">{t('prov5.models.hint')}</p>
  {/if}
{/if}

<style>
  /* The same look as the rest of Settings › Assistant (Settings.svelte). */
  h3 { font-size: 14px; font-weight: 600; margin: 18px 0 6px; }
  .row { display: flex; gap: 8px; }
  .wrap { flex-wrap: wrap; }
  .gap-top { margin-top: 10px; }
  .check { display: flex; gap: 8px; align-items: center; color: var(--fg); font-size: 13px; }
  .err { color: var(--danger); font-size: var(--fs-sm); margin: 4px 0 0; }
  .sm { font-size: var(--fs-xs); }
</style>
