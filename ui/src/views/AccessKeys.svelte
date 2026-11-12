<script lang="ts">
  // Settings → Privacy → Keys for other programs (FEAT-37). A key lets a shortcut or a script on
  // this computer add todos and notes, or read the library — never more (the background service
  // decides that, see tokens.rs). A new key is shown once, here, and then only its name remains.
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { api, del, post } from '../lib/api';
  import { toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { ago, fmtDate, t } from '../lib/i18n';
  import { failed } from '../lib/errors';

  type Scope = 'capture' | 'read';
  type AccessKey = { id: string; name: string; scopes: Scope[]; created: string; lastUsed: string | null };
  const MAX_KEYS = 50;
  const choices = [
    { id: 'capture', scopes: ['capture'] as Scope[] },
    { id: 'read', scopes: ['read'] as Scope[] },
    { id: 'both', scopes: ['capture', 'read'] as Scope[] },
  ];

  let keys = $state<AccessKey[]>([]);
  let phase = $state<'loading' | 'ok' | 'error'>('loading');
  let loadError = $state('');
  // The form for a new key, while it is open.
  let form = $state<{ name: string; choice: string } | null>(null);
  let formError = $state('');
  let busy = $state<Record<string, boolean>>({});
  // The key just made: shown until the user says it was copied, never again.
  let made = $state<{ name: string; secret: string } | null>(null);
  let copied = $state(false);
  let secretBox = $state<HTMLInputElement | null>(null);
  let nameBox = $state<HTMLInputElement | null>(null);

  async function load() {
    try { const r = await api<AccessKey[]>('/access-keys'); keys = Array.isArray(r) ? r : []; phase = 'ok'; }
    catch (e: any) { phase = 'error'; loadError = e?.message ?? String(e); }
  }
  onMount(load);

  const choiceOf = (k: AccessKey) => (k.scopes.includes('capture') && k.scopes.includes('read') ? 'both' : k.scopes.includes('read') ? 'read' : 'capture');

  function openForm() {
    made = null; copied = false; formError = '';
    form = { name: '', choice: 'capture' };
    queueMicrotask(() => nameBox?.focus());
  }

  async function create(e: SubmitEvent) {
    e.preventDefault();
    if (!form || busy.create) return;
    const name = form.name.trim();
    if (!name) { formError = t('secb.keys.nameNeeded'); nameBox?.focus(); return; }
    busy.create = true; formError = '';
    try {
      const r = await post<{ key: AccessKey; secret: string }>('/access-keys', { name, scopes: choices.find((c) => c.id === form!.choice)?.scopes ?? ['capture'] });
      made = { name: r.key.name, secret: r.secret };
      copied = false;
      form = null;
      await load();
      queueMicrotask(() => { secretBox?.focus(); secretBox?.select(); });
    } catch (err: any) {
      formError = err?.message ?? String(err);
    } finally { busy.create = false; }
  }

  async function copy() {
    if (!made) return;
    try { await navigator.clipboard.writeText(made.secret); copied = true; }
    catch { secretBox?.select(); toast(t('secb.keys.copyFailed'), { kind: 'error' }); }
  }

  function done() { made = null; copied = false; }

  async function remove(k: AccessKey) {
    const ok = await confirmDialog({ title: t('secb.keys.removeTitle', { name: k.name }), body: t('secb.keys.removeBody'), confirmLabel: t('secb.keys.removeConfirm'), danger: true });
    if (!ok) return;
    busy['rm:' + k.id] = true;
    try { await del(`/access-keys/${encodeURIComponent(k.id)}`); toast(t('secb.keys.removed'), { kind: 'success' }); }
    catch (e) { failed(t('secb.keys.removeFailed'), e); }
    finally { busy['rm:' + k.id] = false; await load(); }
  }
</script>

<div class="card keys">
  <h3 id="secb-keys-title">{t('secb.keys.title')}</h3>
  <p class="muted sm">{t('secb.keys.intro')}</p>

  {#if made}
    <div class="made" role="region" aria-labelledby="secb-made-title">
      <p class="made-title" id="secb-made-title"><Icon name="lock" size={14} /> <span>{t('secb.keys.made', { name: made.name })}</span></p>
      <div class="row">
        <input bind:this={secretBox} class="grow secret" dir="ltr" readonly value={made.secret} aria-label={t('secb.keys.label')} aria-describedby="secb-once" onfocus={(e) => e.currentTarget.select()} />
        <button type="button" class="btn primary" onclick={copy}><Icon name={copied ? 'check' : 'copy'} size={14} /> {copied ? t('secb.keys.copied') : t('secb.keys.copy')}</button>
      </div>
      <p class="muted sm" id="secb-once">{t('secb.keys.once')}</p>
      <div class="row end"><button type="button" class="btn" onclick={done}>{t('secb.keys.done')}</button></div>
    </div>
  {/if}

  {#if phase === 'loading'}
    <p class="muted sm" role="status">{t('secb.keys.loading')}</p>
  {:else if phase === 'error'}
    <p class="err" role="alert">{t('secb.keys.loadFailed')} <span class="muted">{loadError}</span> <button type="button" class="btn ghost" onclick={load}>{t('secb.keys.retry')}</button></p>
  {:else}
    {#each keys as k (k.id)}
      <div class="row key">
        <div class="grow">
          <b dir="auto">{k.name}</b>
          <div class="muted sm">{t('secb.keys.scope.' + choiceOf(k))} · {t('secb.keys.created', { date: fmtDate(k.created) })} · {k.lastUsed ? t('secb.keys.used', { when: ago(k.lastUsed) }) : t('secb.keys.never')}</div>
        </div>
        <button type="button" class="btn ghost danger" disabled={busy['rm:' + k.id]} onclick={() => remove(k)} aria-label={t('secb.keys.removeNamed', { name: k.name })}><Icon name="trash" size={14} /> {t('secb.keys.remove')}</button>
      </div>
    {:else}
      {#if !form && !made}<p class="muted sm">{t('secb.keys.empty')}</p>{/if}
    {/each}

    {#if form}
      <form class="new" onsubmit={create} aria-labelledby="secb-new-title">
        <h4 id="secb-new-title">{t('secb.keys.new')}</h4>
        <label for="secb-name">{t('secb.keys.name')}</label>
        <input id="secb-name" bind:this={nameBox} dir="auto" maxlength="60" autocomplete="off" bind:value={form.name} oninput={() => (formError = '')} aria-describedby="secb-name-hint" aria-invalid={!!formError} />
        <p class="muted sm" id="secb-name-hint">{t('secb.keys.nameHint')}</p>
        <fieldset class="what">
          <legend>{t('secb.keys.what')}</legend>
          {#each choices as c (c.id)}
            <label class="radio">
              <input type="radio" name="secb-scope" value={c.id} bind:group={form.choice} />
              <span><span class="blk">{t('secb.keys.scope.' + c.id)}</span><span class="muted sm blk">{t('secb.keys.scope.' + c.id + 'Hint')}</span></span>
            </label>
          {/each}
        </fieldset>
        {#if formError}<p class="err" role="alert">{formError}</p>{/if}
        <div class="row end">
          <button type="button" class="btn" onclick={() => { form = null; formError = ''; }}>{t('secb.keys.cancel')}</button>
          <button type="submit" class="btn primary" disabled={busy.create} aria-busy={busy.create}>{busy.create ? t('secb.keys.creating') : t('secb.keys.create')}</button>
        </div>
      </form>
    {:else if keys.length >= MAX_KEYS}
      <p class="muted sm">{t('secb.keys.limit', { n: MAX_KEYS })}</p>
    {:else}
      <div class="row gap-top"><button type="button" class="btn" onclick={openForm}><Icon name="plus" size={14} /> {t('secb.keys.new')}</button></div>
    {/if}
  {/if}
</div>

<style>
  .keys { margin-bottom: 12px; }
  h3 { font-size: 14px; font-weight: 600; margin: 0 0 6px; }
  h4 { font-size: 13px; font-weight: 600; margin: 0 0 2px; }
  .sm { font-size: var(--fs-xs); }
  .err { color: var(--danger); font-size: var(--fs-sm); margin: 4px 0 0; }
  .key { padding: 10px 0; border-bottom: 1px solid var(--hairline); }
  .key:last-of-type { border-bottom: 0; }
  .key b { overflow-wrap: anywhere; }
  .made { margin: 10px 0; padding: 12px; border: 1px solid var(--accent); border-radius: var(--radius, 8px); background: var(--tint); }
  .made-title { display: flex; gap: 6px; align-items: center; font-weight: 600; font-size: 13px; margin: 0 0 8px; color: var(--on-tint, inherit); }
  .made-title span { overflow-wrap: anywhere; }
  .secret { font-family: var(--mono, ui-monospace, SFMono-Regular, Menlo, monospace); font-size: 12px; min-width: 0; }
  .new { margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--hairline); }
  .what { border: 0; padding: 0; margin: 10px 0 0; min-width: 0; }
  .what legend { font-size: var(--fs-xs); font-weight: 500; color: var(--muted); padding: 0; }
  .radio { display: flex; gap: 8px; align-items: flex-start; margin: 8px 0; cursor: pointer; color: var(--fg); font-size: 13px; }
  .radio input { margin-top: 4px; }
  .blk { display: block; font-weight: 400; }
  .end { justify-content: flex-end; margin-top: 10px; }
  .gap-top { margin-top: 10px; }
</style>
