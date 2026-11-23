<script lang="ts">
  // Customise, a page of its own (sidebar): start a new customisation (a chat of its own), the add-ons one at a time, ready-made
  // packs, automatic tasks and routines, sharing, and the app history. Recovery and Rollback are
  // core code: nothing here is part of the customisation layer.
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { app } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { checkPending, openReview, newCustomise } from '../lib/layer';
  import AutomaticTasks from './AutomaticTasks.svelte';
  import Routines from './Routines.svelte';
  import WaitingChanges from './WaitingChanges.svelte';
  import UpdateChoices from './UpdateChoices.svelte';
  import { checkAppUpdate } from '../lib/updates';
  import { check } from '../lib/updateProgress.svelte';
  import Plugins from './Plugins.svelte';
  import ReadyPacks from './ReadyPacks.svelte';
  import SharePacks from './SharePacks.svelte';
  import Rollback from './Rollback.svelte';
  import Automations from './Automations.svelte';

  const TABS = [
    { id: 'ask', icon: 'wrap' },
    { id: 'automate', icon: 'ai' },
    // (Since 0.5.0 this tab is Plugins; its id stays `addons`, so links to it keep working.)
    { id: 'addons', icon: 'plugin' },
    { id: 'ready', icon: 'modules' },
    { id: 'tasks', icon: 'clock' },
    { id: 'share', icon: 'upload' },
    { id: 'history', icon: 'rollback' },
    { id: 'updates', icon: 'sync' },
  ] as const;
  type Tab = (typeof TABS)[number]['id'];
  const known = (v: string): v is Tab => TABS.some((x) => x.id === v);

  let tab = $state<Tab>('ask');
  // A link to a tab (`go({kind: 'customise', id: 'addons'})`) opens that tab, also when this page is
  // already open on another one (every `go` sets a new view). Switching tabs here stays on the page,
  // so the heading does not take the focus away from the tab list.
  $effect(() => { const v = app.view; if (v.kind === 'customise' && known(v.id ?? '')) tab = v.id as Tab; });
  const open = (id: string) => { if (known(id)) tab = id; };

  // A change is asked for in a customisation chat of its own, opened by New customisation.
  let starting = $state(false);
  async function start() { if (starting) return; starting = true; try { await newCustomise(); } finally { starting = false; } }

  onMount(() => { checkPending(false); });

  /** Arrow keys move between tabs (the tab list pattern). */
  function onTabKey(e: KeyboardEvent) {
    const i = TABS.findIndex((x) => x.id === tab);
    const rtl = document.documentElement.dir === 'rtl';
    const step = e.key === 'ArrowRight' ? (rtl ? -1 : 1) : e.key === 'ArrowLeft' ? (rtl ? 1 : -1) : e.key === 'Home' ? -i : e.key === 'End' ? TABS.length - 1 - i : 0;
    if (!step) return;
    e.preventDefault();
    const next = TABS[(i + step + TABS.length) % TABS.length].id;
    open(next);
    queueMicrotask(() => document.getElementById('cz-tab-' + next)?.focus());
  }
</script>

<div class="page cz">
  <div class="row head">
    <h1 class="grow">{t('layer.customise')}</h1>
    {#if tab !== 'ask'}
      <button type="button" class="btn primary" onclick={start} disabled={starting} title={t('r10-cust-start.newTip')}><Icon name="palette" size={15} /> {t('r10-cust-start.new')}</button>
    {/if}
  </div>
  <p class="muted intro">{t('ldc.intro')}</p>

  {#if app.pending?.exists}
    <div class="row pend" role="status">
      <Icon name={app.pending.running ? 'clock' : 'info'} size={15} />
      <span class="grow">{t(app.pending.running ? 'layer.running' : 'layer.pendingWaiting')}</span>
      <button type="button" class="btn" onclick={openReview}>{t('layer.review')}</button>
    </div>
  {/if}
  <WaitingChanges />

  <div class="seg tabs" role="tablist" aria-label={t('layer.customise')} tabindex="-1" onkeydown={onTabKey}>
    {#each TABS as x (x.id)}
      <button type="button" class="segb" role="tab" id={'cz-tab-' + x.id} aria-selected={tab === x.id} aria-controls="cz-panel" tabindex={tab === x.id ? 0 : -1} onclick={() => open(x.id)}>
        <Icon name={x.icon} size={14} /> {t(x.id === 'automate' ? 'auto-ui.tab' : x.id === 'addons' ? 'plg.tab' : 'ldc.tab.' + x.id)}
      </button>
    {/each}
  </div>

  <div id="cz-panel" class="panel" role="tabpanel" aria-labelledby={'cz-tab-' + tab}>
    {#if tab === 'ask'}
      <div class="card start">
        <p class="lead">{t('r10-cust-start.lead')}</p>
        <ol class="steps muted sm">
          <li>{t('r10-cust-start.step1')}</li>
          <li>{t('r10-cust-start.step2')}</li>
          <li>{t('r10-cust-start.step3')}</li>
        </ol>
        <button type="button" class="btn primary go" onclick={start} disabled={starting} title={t('r10-cust-start.newTip')}><Icon name="palette" size={16} /> {starting ? t('set.starting') : t('r10-cust-start.new')}</button>
        <p class="muted sm gap">{t('ldc.ask.historyHint')}</p>
      </div>
      <div class="card row wrap">
        <Icon name="modules" size={18} />
        <span class="grow sm">{t('ldc.ask.ideas')}</span>
        <button type="button" class="btn" onclick={() => open('ready')}>{t('ldc.ask.seeReady')}</button>
      </div>
      <div class="card row wrap">
        <Icon name="ai" size={18} />
        <span class="grow sm">{t('auto-ui.askCard')}</span>
        <button type="button" class="btn" onclick={() => open('automate')}>{t('auto-ui.askCardGo')}</button>
      </div>
    {:else if tab === 'automate'}
      <Automations />
    {:else if tab === 'addons'}
      <Plugins {open} />
    {:else if tab === 'ready'}
      <ReadyPacks />
    {:else if tab === 'tasks'}
      <div class="card">
        <h3>{t('layer.workflows')}</h3>
        <p class="muted sm">{t('layer.workflowsHint')}</p>
        <AutomaticTasks />
      </div>
      <div class="card">
        <h3>{t('auto.routines')}</h3>
        <p class="muted sm">{t('auto.routinesHint')}</p>
        <Routines />
      </div>
    {:else if tab === 'share'}
      <SharePacks {open} />
    {:else if tab === 'history'}
      <div class="card"><Rollback embedded /></div>
    {:else}
      <div class="card">
        <h3>{t('layer.appUpdates')}</h3>
        <p class="muted sm">{t('layer.appUpdatesHint')}</p>
        <button type="button" class="btn" class:checking={check.busy} disabled={check.busy} aria-busy={check.busy} onclick={() => checkAppUpdate(true)}><Icon name="sync" size={15} /> {t('layer.upd.check')}</button>
        <UpdateChoices />
      </div>
    {/if}
  </div>
</div>

<style>
  .cz { max-width: 860px; }
  .intro { margin: 0 0 12px; }
  .tabs { flex-wrap: wrap; margin-bottom: 14px; border-radius: var(--radius-lg, 14px); }
  .panel { display: grid; gap: 0; }
  h3 { font-size: 14px; font-weight: 600; margin: 0 0 6px; }
  .card { margin-bottom: 12px; }
  .card.row { gap: 10px; align-items: center; }
  .card.row :global(.icon) { color: var(--accent-2); flex: none; }
  .wrap { flex-wrap: wrap; }
  .gap { margin: 10px 0 0; }
  .sm { font-size: var(--fs-xs); }
  .head { gap: 12px; align-items: center; flex-wrap: wrap; margin-bottom: 8px; }
  .head h1 { margin-bottom: 0; }
  .lead { margin: 0 0 8px; }
  .steps { margin: 0 0 14px; padding-inline-start: 20px; display: grid; gap: 4px; }
  .go { padding-block: 9px; padding-inline: 16px; }
  .pend { margin: 0 0 12px; padding: 8px 10px; border-radius: var(--radius); background: var(--fill); font-size: 13px; gap: 8px; align-items: center; }
  .pend :global(.icon) { flex: none; color: var(--accent-2); }
</style>
