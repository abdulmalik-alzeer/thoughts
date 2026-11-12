<script lang="ts">
  // Customise › Ready-made: packs that come with the app. Choosing one stages it for the usual review.
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { on } from '../lib/api';
  import { permText } from '../lib/layer';
  import { readyPacks, stageReady, type ReadyPack } from '../lib/packs';

  let packs = $state<ReadyPack[]>([]);
  let loading = $state(true);
  let loadError = $state('');
  let busy = $state('');

  async function load() {
    loadError = '';
    try { packs = await readyPacks(); }
    catch (e: any) { loadError = e?.message || t('ldc.ready.loadFailed'); }
    finally { loading = false; }
  }
  onMount(() => {
    load();
    return on((e: any) => { if (e?.type === 'layer.changed') load(); });
  });

  async function choose(p: ReadyPack) {
    if (busy) return;
    busy = p.id;
    try { await stageReady(p.id); }
    catch (e) { failed(t('ldc.ready.failed'), e); }
    finally { busy = ''; }
  }
</script>

<p class="muted sm">{t('ldc.ready.intro')}</p>

{#if loading}
  <div class="card" aria-busy="true"><span class="sr-only">{t('ldc.ready.loading')}</span><div class="skel"></div><div class="skel w70"></div></div>
{:else if loadError}
  <div class="card" role="alert">
    <p>{t('ldc.ready.loadFailed')} <span class="muted sm"><bdi>{loadError}</bdi></span></p>
    <button type="button" class="btn" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('shell.retry')}</button>
  </div>
{:else}
  <ul class="grid">
    {#each packs as p (p.id)}
      <li class="card pack">
        <div class="row top">
          <h3 class="grow" dir="auto">{p.name}</h3>
          {#if p.added}<span class="pill accent"><Icon name="check" size={12} /> {t('ldc.ready.added')}</span>{/if}
        </div>
        <p class="sm" dir="auto">{p.about}</p>
        {#if p.permissions.length}
          <div class="sm">
            <span class="muted">{t('ldc.ready.mayDo')}</span>
            <ul class="perms">
              {#each p.permissions as perm (perm)}<li>{permText(perm)}{#if perm === 'library:write' && p.write.length}<br /><span class="muted">{t('layer3.writeIn')}</span> {#each p.write as w, i}{i ? ' · ' : ''}<bdi>{w}</bdi>{/each}{/if}</li>{/each}
            </ul>
          </div>
        {:else if !p.workflow}
          <!-- (An automatic task asks for nothing but runs the assistant in the library: its description says what, and when it may run.) -->
          <p class="muted sm">{t('ldc.ready.nothing')}</p>
        {/if}
        <p class="muted xs"><bdi class="code">{t('ldc.ready.from', { fp: p.code })}</bdi></p>
        {#if !p.added}
          <div class="row end">
            <button type="button" class="btn primary" disabled={!!busy} aria-busy={busy === p.id} aria-label={t('ldc.ready.tryLabel', { name: p.name })} onclick={() => choose(p)}>
              <Icon name="eye" size={14} /> {t('ldc.ready.try')}
            </button>
          </div>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .grid { list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); }
  .pack { display: grid; gap: 6px; align-content: start; }
  .pack p { margin: 0; }
  h3 { font-size: 14px; font-weight: 600; margin: 0; }
  .top { align-items: center; gap: 8px; }
  .sm { font-size: var(--fs-xs); }
  .xs { font-size: 11px; }
  .code { font-family: var(--mono); overflow-wrap: anywhere; }
  .perms { margin: 2px 0 0; padding-inline-start: 18px; }
  .end { justify-content: flex-end; }
</style>
