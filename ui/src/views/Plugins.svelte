<script lang="ts">
  import HelpTip from '../lib/HelpTip.svelte';
  // Customise › Plugins: everything the user customised, as a list. Named plugins (from a file, a
  // ready-made pack, or a change kept "as one plugin") first, then what stands on its own: each
  // add-on, page, automatic task and theme, Home, the sidebar, the Todos page, the app pages, your
  // look and the keyboard shortcuts. Every switch, removal and naming is one History entry.
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { go, refreshChats, toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { on } from '../lib/api';
  import { revertChange } from '../lib/layer';
  import { customisePlugin, exportAllPlugins, kindIcon, kindLabel, loadPlugins, originText, partText, removePlugin, switchPlugin, type PluginItem, type Saved } from '../lib/plugins';
  import PackImport from './PackImport.svelte';
  import PluginDetails from './PluginDetails.svelte';
  import PluginExport from './PluginExport.svelte';
  import AddOns from './AddOns.svelte';

  let { open }: { open: (tab: string) => void } = $props();

  let items = $state<PluginItem[]>([]);
  let problem = $state('');
  let loading = $state(true);
  let loadError = $state('');
  let busy = $state('');
  let menu = $state('');
  let details = $state<Record<string, boolean>>({});
  let exporting = $state<PluginItem | null>(null);
  let result = $state<null | { copied: boolean; fp: string; name: string }>(null);

  const named = $derived(items.filter((p) => p.named));
  const loose = $derived(items.filter((p) => !p.named));

  async function load() {
    loadError = '';
    try { ({ plugins: items, problem } = await loadPlugins()); }
    catch (e: any) { loadError = e?.message || t('plg.loadFailed'); }
    finally { loading = false; }
  }
  onMount(() => {
    load();
    // Kept or undone changes elsewhere (History, a review, the Recovery menu) show here at once.
    return on((e: any) => { if (e?.type === 'layer.changed' || e?.type === 'workflows.changed') load(); });
  });

  async function act(key: string, failKey: string, fn: () => Promise<string | void>, done?: (hash: string) => void) {
    if (busy) return;
    busy = key;
    menu = '';
    try { done?.(String((await fn()) ?? '')); }
    catch (e) { failed(t(failKey), e); }
    finally { busy = ''; await load(); }
  }

  /** A toast that offers to undo the History entry just made. */
  function withUndo(text: string, hash: string) {
    toast(text, {
      kind: 'success', key: 'plugins',
      action: hash ? { label: t('undo'), run: () => { revertChange(hash).then(() => { toast(t('layer.undone'), { kind: 'success', key: 'plugins' }); load(); }, (e) => failed(t('layer.undoFailed'), e)); } } : undefined,
    });
  }

  const flip = (p: PluginItem) => act('switch:' + p.id, 'plg.switchFailed', () => switchPlugin(p.id, !p.on), (h) => withUndo(t(p.on ? 'plg.offDone' : 'plg.onDone', { name: p.name }), h));

  async function remove(p: PluginItem) {
    menu = '';
    const ok = await confirmDialog({ title: t('plg.removeTitle', { name: p.name }), body: t('plg.removeBody'), list: p.contains.slice(0, 20).map(partText), confirmLabel: t('plg.removeGo'), danger: true });
    if (ok) await act('remove:' + p.id, 'plg.removeFailed', () => removePlugin(p.id), (h) => withUndo(t('plg.removed', { name: p.name }), h));
  }

  async function change(p: PluginItem) {
    menu = '';
    // Only a named plugin can be changed as one thing: a loose item is named first.
    if (!p.named) { toast(t('plg.changeLoose')); exporting = null; return; }
    await act('change:' + p.id, 'plg.changeFailed', async () => {
      const chat = await customisePlugin(p.id);
      await refreshChats().catch(() => {});
      if (chat) go({ kind: 'chat', id: chat });
    });
  }

  function exported(saved: Saved, name: string) {
    exporting = null;
    if (saved.result !== 'cancelled') result = { copied: saved.result === 'copied', fp: saved.fingerprint, name };
    load();
  }

  const exportAll = () => act('all', 'plg.exp.failed', async () => {
    const saved = await exportAllPlugins();
    if (saved.result !== 'cancelled') result = { copied: saved.result === 'copied', fp: saved.fingerprint, name: t('plg.all.fileName') };
  });

  async function copyFp() {
    if (!result) return;
    try { await navigator.clipboard.writeText(result.fp); toast(t('ldc.share.fpCopied'), { kind: 'success' }); } catch { /* the code is on screen */ }
  }

  const stateText = (p: PluginItem) => p.state === 'needsNewerApp' ? t('plg.needsNewer', { version: p.minApp }) : p.state === 'unsupported' ? t('plg.unsupported') : p.state === 'empty' ? t('plg.emptyState') : '';

  /** The menu closes on Escape, and when something outside it is pressed or takes the focus. (Not on
   *  the focus leaving alone: in the app's own window a pressed button does not take the focus.) */
  function menuKey(e: KeyboardEvent, id: string) {
    if (e.key !== 'Escape' || menu !== id) return;
    e.stopPropagation();
    menu = '';
    document.getElementById('plg-more-' + id)?.focus();
  }
  function outside(e: Event) {
    if (menu && !(e.target as Element | null)?.closest?.('.menuwrap')) menu = '';
  }
</script>

{#snippet row(p: PluginItem)}
  <li class="card item" class:off={!p.on}>
    <div class="row top">
      <span class="kind" title={kindLabel(p.kind)}><Icon name={kindIcon(p.kind)} size={18} /></span>
      <div class="grow who">
        <b><bdi>{p.name}</bdi></b>
        <span class="muted sm">
          {kindLabel(p.kind)}{#if p.version}{' · '}<bdi>{t('plg.version', { version: p.version })}</bdi>{/if}{#if originText(p)}{' · '}{originText(p)}{/if}
        </span>
      </div>
      {#if stateText(p)}<span class="pill warn">{stateText(p)}</span>{/if}
      {#if p.canSwitch}
        <button type="button" class="sw" role="switch" aria-checked={p.on} aria-label={t('plg.switchLabel', { name: p.name })} disabled={!!busy} aria-busy={busy === 'switch:' + p.id} onclick={() => flip(p)}>
          <span class="knob" aria-hidden="true"></span>
        </button>
        <span class="sm state" aria-hidden="true">{t(p.on ? 'plg.on' : 'plg.off')}</span>
      {/if}
      <div class="menuwrap" role="presentation" onkeydown={(e) => menuKey(e, p.id)}>
        <button type="button" class="btn ghost sm" id={'plg-more-' + p.id} aria-haspopup="menu" aria-expanded={menu === p.id} aria-label={t('plg.menu', { name: p.name })} onclick={() => (menu = menu === p.id ? '' : p.id)}>⋯</button>
        {#if menu === p.id}
          <div class="menu" role="menu" aria-label={t('plg.menu', { name: p.name })}>
            {#if p.canExport}<button type="button" role="menuitem" onclick={() => { menu = ''; exporting = p; }}><Icon name="download" size={14} /> {t('plg.menu.export')}</button>{/if}
            {#if p.named && p.canSwitch}<button type="button" role="menuitem" disabled={!!busy} onclick={() => change(p)}><Icon name="palette" size={14} /> {t('plg.menu.change')}</button>{/if}
            <button type="button" role="menuitem" onclick={() => { details[p.id] = !details[p.id]; menu = ''; }}><Icon name="info" size={14} /> {t(details[p.id] ? 'plg.menu.hide' : 'plg.menu.details')}</button>
            {#if p.canRemove}<button type="button" role="menuitem" class="danger" disabled={!!busy} onclick={() => remove(p)}><Icon name="trash" size={14} /> {t('plg.menu.remove')}</button>{/if}
          </div>
        {/if}
      </div>
    </div>
    {#if p.description}<p class="sm" dir="auto">{p.description}</p>{/if}
    {#if p.author}<p class="muted sm" dir="auto">{t('plg.madeBy', { author: p.author })}</p>{/if}
    {#if p.message}<p class="sm" role="note" dir="auto">{p.message}</p>{/if}
    {#if !p.on && p.state === 'off'}<p class="muted sm">{t('plg.offHint')}</p>{/if}
    {#if p.named && p.contains.length}
      <ul class="chips" aria-label={t('plg.contains')}>
        {#each p.contains.slice(0, 12) as c, i (i)}<li class="pill"><Icon name={kindIcon(c.kind)} size={12} /> <span dir="auto">{partText(c)}</span></li>{/each}
        {#if p.contains.length > 12}<li class="pill">+{p.contains.length - 12}</li>{/if}
      </ul>
    {/if}
    {#if p.permissionWords.length}
      <p class="sm"><span class="muted">{t('plg.mayDo')}</span> {p.permissionWords.join(' · ')}</p>
    {:else if p.kind === 'bundle' || p.kind === 'addon' || p.kind === 'page'}
      <p class="muted sm">{t('plg.mayNothing')}</p>
    {/if}
    {#if p.tasks.length}
      <p class="sm">
        <span class="muted">{t('plg.tasks')}</span>
        {#each p.tasks as k, i (k.file)}{i ? ' · ' : ''}<bdi>{k.name}</bdi> ({t(k.approved ? 'plg.taskAllowed' : 'plg.taskNotAllowed')}){/each}
        <button type="button" class="link" onclick={() => open('tasks')}>{t('plg.taskOpen')}</button>
      </p>
    {/if}
    {#if details[p.id]}<PluginDetails plugin={p} {open} />{/if}
  </li>
{/snippet}

<svelte:document onpointerdown={outside} onfocusin={outside} />

<p class="muted sm">{t('plg.intro')}</p>
<div class="row wrap tools">
  <PackImport label="plg.import" checking="plg.importChecking" />
  <button type="button" class="btn" disabled={!!busy || !items.length} aria-busy={busy === 'all'} title={t('plg.exportAllTip')} onclick={exportAll}><Icon name="download" size={15} /> {t('plg.exportAll')}</button>
</div>

{#if result}
  <div class="done" role="status">
    <p>{t(result.copied ? 'plg.exp.copied' : 'plg.exp.saved')} <bdi class="code" dir="ltr">{result.fp}</bdi></p>
    <p class="muted sm">{t('plg.exp.fpHint')}</p>
    <p class="muted sm">{t('pkf.isZip')}</p>
    <div class="row wrap">
      <button type="button" class="btn sm" onclick={copyFp}><Icon name="copy" size={13} /> {t('ldc.share.copyFp')}</button>
      <button type="button" class="btn ghost sm" onclick={() => (result = null)}>{t('close')}</button>
    </div>
  </div>
{/if}

{#if loading}
  <div class="card" aria-busy="true"><span class="sr-only">{t('plg.loading')}</span><div class="skel"></div><div class="skel w70"></div></div>
{:else if loadError}
  <div class="card" role="alert">
    <p>{t('plg.loadFailed')} <span class="muted sm"><bdi>{loadError}</bdi></span></p>
    <button type="button" class="btn" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('shell.retry')}</button>
  </div>
{:else if problem}
  <div class="card problem" role="alert">
    <p><Icon name="warning" size={16} /> <span dir="auto">{problem}</span></p>
    <button type="button" class="btn primary" onclick={() => open('history')}><Icon name="rollback" size={14} /> {t('ldc.tab.history')}</button>
  </div>
{:else if !items.length}
  <div class="card empty">
    <Icon name="modules" size={26} />
    <p>{t('plg.empty')}</p>
    <div class="row wrap center">
      <button type="button" class="btn primary" onclick={() => open('ask')}>{t('ldc.tab.ask')}</button>
      <button type="button" class="btn" onclick={() => open('ready')}>{t('ldc.ask.seeReady')}</button>
    </div>
  </div>
{:else}
  {#if named.length}
    <h3>{t('plg.named')} <HelpTip text={t('plg.namedHint')} /></h3>
    <ul class="list">{#each named as p (p.id)}{@render row(p)}{/each}</ul>
  {/if}
  {#if loose.length}
    <h3>{t('plg.loose')} <HelpTip text={t('plg.looseHint')} /></h3>
    <ul class="list">{#each loose as p (p.id)}{@render row(p)}{/each}</ul>
  {/if}
{/if}

<!-- What add-ons that are no longer in the app saved (kept until the user deletes it). -->
{#if !loading && !loadError && !problem}<AddOns {open} only={[]} />{/if}

{#if exporting}<PluginExport plugin={exporting} close={() => (exporting = null)} done={exported} />{/if}

<style>
  .list { list-style: none; margin: 0 0 6px; padding: 0; display: grid; gap: 10px; }
  .item { display: grid; gap: 6px; }
  .item.off { border-style: dashed; }
  .item p { margin: 0; }
  .top { align-items: center; gap: 10px; }
  .kind { color: var(--accent-2); display: inline-flex; flex: none; }
  .who { display: grid; gap: 1px; min-width: 0; }
  .who b { overflow-wrap: anywhere; }
  .sm { font-size: var(--fs-xs); }
  .wrap { flex-wrap: wrap; }
  .tools { gap: 8px; margin: 8px 0 12px; }
  .state { min-width: 2.6em; color: var(--muted); }
  .sw { position: relative; flex: none; width: 38px; height: 22px; padding: 0; border-radius: 999px; border: var(--border-w, 1px) solid var(--hairline); background: var(--fill); cursor: pointer; }
  .sw .knob { position: absolute; top: 2px; inset-inline-start: 2px; width: 16px; height: 16px; border-radius: 50%; background: var(--muted); transition: inset-inline-start .15s; }
  .sw[aria-checked='true'] { background: var(--accent); border-color: var(--accent); }
  .sw[aria-checked='true'] .knob { inset-inline-start: 18px; background: var(--accent-fg); }
  .sw:disabled { opacity: .6; cursor: default; }
  @media (prefers-reduced-motion: reduce) { .sw .knob { transition: none; } }
  .menuwrap { position: relative; flex: none; }
  .menu { position: absolute; top: calc(100% + 4px); inset-inline-end: 0; z-index: 8; min-width: 220px; padding: 5px; display: grid; gap: 1px; background: var(--panel); border: var(--border-w, 1px) solid var(--hairline); border-radius: var(--radius-lg, 12px); box-shadow: var(--shadow-lg); }
  .menu button { display: flex; align-items: center; gap: 8px; padding: 7px 10px; border: 0; background: none; color: var(--fg); font: inherit; font-size: 13px; text-align: start; border-radius: var(--radius); cursor: pointer; }
  .menu button:hover:not(:disabled), .menu button:focus-visible { background: var(--fill); }
  .menu button:disabled { opacity: .55; cursor: default; }
  .menu .danger { color: var(--danger); }
  .chips { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: 5px; }
  .chips .pill { display: inline-flex; align-items: center; gap: 4px; }
  .pill.warn { background: var(--tint); color: var(--on-tint); }
  .done { margin: 0 0 12px; padding: 10px 12px; border-radius: var(--radius); background: var(--fill); display: grid; gap: 6px; }
  .done p { margin: 0; }
  .code { font-family: var(--mono); font-weight: 600; overflow-wrap: anywhere; }
  .problem { display: grid; gap: 10px; justify-items: start; }
  .problem p { margin: 0; display: flex; gap: 8px; align-items: flex-start; }
  .problem :global(.icon) { color: var(--danger); flex: none; margin-top: 2px; }
  .empty { display: grid; justify-items: center; gap: 8px; text-align: center; padding: 22px; }
  .empty :global(.icon) { color: var(--muted); }
  .center { justify-content: center; gap: 8px; }
  h3 { font-size: 14px; font-weight: 600; margin: 14px 0 2px; }
  .link { background: none; border: 0; padding: 0; color: var(--accent); cursor: pointer; font: inherit; text-decoration: underline; }
</style>
