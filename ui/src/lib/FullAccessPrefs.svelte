<script lang="ts">
  // Settings › Privacy & cost › Full access (lib/fullAccess.ts). Applied at once, never part of the
  // page's unsaved draft: on only through the desktop app's own two-step warning, off in one click.
  import Icon from './Icon.svelte';
  import { app, toast } from './store.svelte';
  import { fmtDate, t } from './i18n';
  import { failed } from './errors';
  import { inTauri } from './api';
  import { fullAccessOn, setFullAccess, setSourceDir } from './fullAccess';

  let busy = $state(false);
  let source = $state<string>(app.settings?.fullAccess?.sourceDir ?? '');
  let sourceNote = $state('');
  const on = $derived(fullAccessOn());
  const since = $derived(app.settings?.fullAccess?.since as string | null | undefined);

  async function flip(want: boolean) {
    if (busy) return;
    busy = true;
    try {
      const r = await setFullAccess(want);
      if (r === 'on') toast(t('fullaccess.nowOn'), { kind: 'info' });
      else if (r === 'off') toast(t('fullaccess.nowOff'), { kind: 'success' });
      else if (r === 'unavailable') toast(t('fullaccess.desktopOnly'), { kind: 'error' });
    } catch (e) { failed(t('fullaccess.failed'), e); }
    finally { busy = false; }
  }

  async function saveSource() {
    sourceNote = '';
    try { await setSourceDir(source); sourceNote = t('fullaccess.sourceSaved'); }
    catch (e) { failed(t('fullaccess.failed'), e); }
  }
</script>

<div class="card fa" class:live={on} id="st-full-access">
  <h3><Icon name="warning" size={16} /> {t('fullaccess.title')}</h3>
  <p class="sm">{t('fullaccess.intro')}</p>
  <p class="sm risk">{t('fullaccess.risks')}</p>
  <div class="row wrap">
    <span class="grow sm" role="status">
      {#if on}<b class="danger-text">{since ? t('fullaccess.on', { when: fmtDate(since, { dateStyle: 'medium', timeStyle: 'short' }) }) : t('fullaccess.onNoTime')}</b>
      {:else}{t('fullaccess.off')}{/if}
    </span>
    {#if on}
      <button type="button" class="btn primary" disabled={busy} aria-busy={busy} onclick={() => flip(false)}>{t('fullaccess.turnOff')}</button>
    {:else}
      <button type="button" class="btn danger" disabled={busy || !inTauri()} aria-busy={busy} onclick={() => flip(true)}>{t('fullaccess.turnOn')}</button>
    {/if}
  </div>
  <p class="muted sm">{on ? t('fullaccess.offNote') : inTauri() ? t('fullaccess.confirmNote') : t('fullaccess.desktopOnly')}</p>
  <label for="st-fa-source">{t('fullaccess.source')}</label>
  <div class="row wrap">
    <input id="st-fa-source" class="grow" dir="ltr" type="text" bind:value={source} placeholder="/Users/…/thoughts" aria-describedby="st-fa-source-hint" />
    <button type="button" class="btn" onclick={saveSource}>{t('fullaccess.sourceSave')}</button>
  </div>
  <p class="muted sm" id="st-fa-source-hint">{t('fullaccess.sourceHint')}{#if sourceNote} <span role="status">{sourceNote}</span>{/if}</p>
</div>

<style>
  .fa h3 { display: flex; align-items: center; gap: 6px; }
  .fa.live { border: 2px solid var(--danger); }
  .risk { color: var(--fg); }
  .danger-text { color: var(--danger); }
  .btn.danger { color: var(--danger); }
  .sm { font-size: var(--fs-xs); }
  .row { display: flex; gap: 8px; align-items: center; margin-top: 6px; }
  .wrap { flex-wrap: wrap; }
  .grow { flex: 1; min-width: 0; }
</style>
