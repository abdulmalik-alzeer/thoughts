<script lang="ts">
  // Customise › Add-ons: every add-on and page, on or off, with what it may do and what it saved.
  // Switching one off, removing it or taking a permission away is one History entry (with Undo).
  import Icon from '../lib/Icon.svelte';
  import { onMount } from 'svelte';
  import { go, toast } from '../lib/store.svelte';
  import { confirmDialog } from '../lib/dialog.svelte';
  import { fmtDate, fmtNum, t } from '../lib/i18n';
  import { failed } from '../lib/errors';
  import { on } from '../lib/api';
  import { permText, revertChange } from '../lib/layer';
  import { addOns, clearData, forgetData, removeAddOn, restoreData, sizeText, switchAddOn, withdrawPermission, type AddOn, type DataInfo, type Leftover } from '../lib/packs';

  let { open }: { open: (tab: string) => void } = $props();

  let items = $state<AddOn[]>([]);
  let leftover = $state<Leftover[]>([]);
  let loading = $state(true);
  let loadError = $state('');
  let busy = $state('');

  async function load() {
    loadError = '';
    try { ({ items, leftover } = await addOns()); }
    catch (e: any) { loadError = e?.message || t('ldc.addons.loadFailed'); }
    finally { loading = false; }
  }
  onMount(() => {
    load();
    // Kept or undone changes elsewhere (History, a review, the Recovery menu) show here at once.
    return on((e: any) => { if (e?.type === 'layer.changed') load(); });
  });

  const when = (iso: string | null | undefined) => (iso ? fmtDate(iso, { dateStyle: 'medium', timeStyle: 'short' }) : '');
  const size = (b: number | null | undefined) => sizeText(b, fmtNum);

  /** Run one action; afterwards the list is read again. `done` gets the new History entry. */
  async function act(key: string, failKey: string, fn: () => Promise<string | void>, done?: (hash: string) => void) {
    if (busy) return;
    busy = key;
    try {
      const hash = await fn();
      done?.(String(hash ?? ''));
    } catch (e) { failed(t(failKey), e); }
    finally { busy = ''; await load(); }
  }

  /** A toast that offers to undo the History entry just made. */
  function withUndo(text: string, hash: string) {
    toast(text, {
      kind: 'success', key: 'addons',
      action: hash ? { label: t('undo'), run: () => { revertChange(hash).then(() => { toast(t('layer.undone'), { kind: 'success', key: 'addons' }); load(); }, (e) => failed(t('layer.undoFailed'), e)); } } : undefined,
    });
  }

  const flip = (a: AddOn) => act('switch:' + a.path, 'ldc.addons.switchFailed', () => switchAddOn(a.path, !a.on),
    (h) => withUndo(t(a.on ? 'ldc.addons.offDone' : 'ldc.addons.onDone', { name: a.label }), h));

  async function remove(a: AddOn) {
    const ok = await confirmDialog({ title: t('ldc.addons.removeTitle', { name: a.label }), body: t('ldc.addons.removeBody'), confirmLabel: t('ldc.addons.removeGo'), danger: true });
    if (ok) await act('remove:' + a.path, 'ldc.addons.removeFailed', () => removeAddOn(a.path), (h) => withUndo(t('ldc.addons.removed', { name: a.label }), h));
  }

  async function take(a: AddOn, perm: string) {
    const ok = await confirmDialog({ title: t('ldc.addons.takeTitle'), body: t('ldc.addons.takeBody', { name: a.label }), list: [permText(perm)], confirmLabel: t('ldc.addons.takeGo'), danger: true });
    if (ok) await act('perm:' + a.path + perm, 'ldc.addons.takeFailed', () => withdrawPermission(a.path, perm), (h) => withUndo(t('ldc.addons.taken', { name: a.label }), h));
  }

  async function putBack(name: string, label: string, v: DataInfo['versions'][number]) {
    const date = when(v.saved);
    const ok = await confirmDialog({ title: t('ldc.data.putBackTitle', { date }), body: t('ldc.data.putBackBody', { name: label }), confirmLabel: t('ldc.data.putBackGo') });
    if (ok) await act('data:' + name, 'ldc.data.failed', () => restoreData(name, v.id), () => toast(t('ldc.data.putBackDone'), { kind: 'success', key: 'addons' }));
  }

  async function clear(name: string, label: string) {
    const ok = await confirmDialog({ title: t('ldc.data.clearTitle', { name: label }), body: t('ldc.data.clearBody'), confirmLabel: t('ldc.data.clearGo'), danger: true });
    if (ok) await act('data:' + name, 'ldc.data.failed', () => clearData(name), () => toast(t('ldc.data.cleared'), { kind: 'success', key: 'addons' }));
  }

  async function forget(l: Leftover) {
    const ok = await confirmDialog({ title: t('ldc.left.forgetTitle', { name: l.label }), body: t('ldc.left.forgetBody'), confirmLabel: t('ldc.left.forgetGo'), danger: true });
    if (ok) await act('data:' + l.name, 'ldc.data.failed', () => forgetData(l.name), () => toast(t('ldc.left.forgotten'), { kind: 'success', key: 'addons' }));
  }
</script>

{#snippet data(name: string, label: string, d: DataInfo, removable: boolean)}
  <div class="data">
    <div class="row wrap">
      <Icon name="inbox" size={14} />
      <span class="grow sm">
        <b>{t('ldc.data.title')}:</b>
        {#if d.bytes != null}{t('ldc.data.now', { size: size(d.bytes), date: when(d.saved) })}{:else}{t('ldc.data.none')}{/if}
      </span>
      {#if removable}
        <button type="button" class="btn ghost sm" disabled={!!busy} onclick={() => forget({ name, label, data: d })}>{t('ldc.left.forget')}</button>
      {:else if d.bytes != null}
        <button type="button" class="btn ghost sm" disabled={!!busy} onclick={() => clear(name, label)}>{t('ldc.data.clear')}</button>
      {/if}
    </div>
    {#if d.versions.length}
      <details>
        <summary class="sm">{t('ldc.data.copies', { n: fmtNum(d.versions.length) })}</summary>
        <p class="muted sm">{t('ldc.data.copiesHint')}</p>
        <ul class="copies">
          {#each d.versions as v (v.id)}
            <li class="row">
              <span class="grow sm">{t('ldc.data.copy', { date: when(v.saved), size: size(v.bytes) })}</span>
              <button type="button" class="btn sm" disabled={!!busy} aria-label={t('ldc.data.putBackLabel', { date: when(v.saved) })} onclick={() => putBack(name, label, v)}>{t('ldc.data.putBack')}</button>
            </li>
          {/each}
        </ul>
      </details>
    {/if}
  </div>
{/snippet}

<p class="muted sm">{t('ldc.addons.intro')}</p>

{#if loading}
  <div class="card" aria-busy="true"><span class="sr-only">{t('ldc.addons.loading')}</span><div class="skel"></div><div class="skel w70"></div></div>
{:else if loadError}
  <div class="card" role="alert">
    <p>{t('ldc.addons.loadFailed')} <span class="muted sm"><bdi>{loadError}</bdi></span></p>
    <button type="button" class="btn" onclick={() => { loading = true; load(); }}><Icon name="retry" size={14} /> {t('shell.retry')}</button>
  </div>
{:else}
  {#if !items.length}
    <div class="card empty">
      <Icon name="plugin" size={26} />
      <p>{t('ldc.addons.empty')}</p>
      <div class="row wrap center">
        <button type="button" class="btn primary" onclick={() => open('ask')}>{t('ldc.tab.ask')}</button>
        <button type="button" class="btn" onclick={() => open('ready')}>{t('ldc.ask.seeReady')}</button>
      </div>
    </div>
  {:else}
    <ul class="list">
      {#each items as a (a.path)}
        <li class="card item" class:off={!a.on}>
          <div class="row wrap top">
            <span class="pill">{t('layer3.kind.' + a.kind)}</span>
            <b class="grow" dir="auto"><bdi>{a.label}</bdi></b>
            {#if !a.on}<span class="pill warn">{t('ldc.addons.off')}</span>{/if}
            {#if a.unsupported}<span class="pill">{t('layer3.notLoadedTitle')}</span>{/if}
          </div>
          {#if a.description}<p class="sm desc" dir="auto">{a.description}</p>{/if}
          {#if a.unsupported && a.message}<p class="sm" role="note" dir="auto">{a.message}</p>{/if}
          {#if !a.on}<p class="muted sm">{t('ldc.addons.offHint')}</p>{/if}
          {#if a.permissions.length}
            <div class="sm">
              <span class="muted">{t('ldc.addons.mayDo')}</span>
              <ul class="perms">
                {#each a.permissions as p (p)}
                  <li class="row">
                    <span class="grow">{permText(p)}{#if p === 'library:write' && a.write.length}<br /><span class="muted">{t('layer3.writeIn')}</span> {#each a.write as w, i}{i ? ' · ' : ''}<bdi>{w}</bdi>{/each}{/if}{#if a.read?.length && ['library:read', 'todos:read', 'ideas:read'].includes(p)}<br /><span class="muted">{t('lub.readIn')}</span> {#each a.read as w, i}{i ? ' · ' : ''}<bdi>{w}</bdi>{/each}{/if}</span>
                    {#if a.manageable}<button type="button" class="btn ghost sm" disabled={!!busy} aria-label={t('ldc.addons.takeLabel', { perm: permText(p), name: a.label })} onclick={() => take(a, p)}>{t('ldc.addons.take')}</button>{/if}
                  </li>
                {/each}
              </ul>
            </div>
          {:else}
            <p class="muted sm">{t('ldc.addons.nothing')}</p>
          {/if}
          {#if a.data}{@render data(a.name, a.label, a.data, false)}{/if}
          {#if a.manageable}
            <div class="row wrap acts">
              <button type="button" class="btn" class:primary={!a.on} disabled={!!busy} aria-busy={busy === 'switch:' + a.path} onclick={() => flip(a)}>
                <Icon name={a.on ? 'stop' : 'retry'} size={14} /> {t(a.on ? 'ldc.addons.switchOff' : 'ldc.addons.switchOn')}
              </button>
              <button type="button" class="btn ghost" disabled={!!busy} onclick={() => remove(a)}><Icon name="trash" size={14} /> {t('ldc.addons.remove')}</button>
            </div>
          {:else}
            <p class="muted sm">{t('ldc.addons.notManaged')}</p>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if leftover.length}
    <h3 class="gap">{t('ldc.left.title')}</h3>
    <p class="muted sm">{t('ldc.left.hint')}</p>
    <ul class="list">
      {#each leftover as l (l.name)}
        <li class="card item">
          <b dir="auto"><bdi>{l.label}</bdi></b>
          {@render data(l.name, l.label, l.data, true)}
        </li>
      {/each}
    </ul>
  {/if}

  {#if items.some((a) => a.data) || leftover.length}
    <p class="muted sm gap">{t('ldc.data.moveHint')} <button type="button" class="link" onclick={() => go({ kind: 'settings', id: 'maintenance' })}>{t('ldc.data.openMove')}</button></p>
  {/if}
{/if}

<style>
  .list { list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; }
  .item { display: grid; gap: 6px; }
  .item.off { border-style: dashed; }
  .top { align-items: center; gap: 8px; }
  .desc { margin: 0; }
  .item p { margin: 0; }
  .sm { font-size: var(--fs-xs); }
  .wrap { flex-wrap: wrap; }
  .perms { list-style: none; margin: 4px 0 0; padding: 0; display: grid; gap: 2px; }
  .perms li { gap: 8px; align-items: center; }
  .acts { gap: 6px; margin-top: 4px; }
  .data { border-top: 1px solid var(--hairline); padding-top: 6px; display: grid; gap: 4px; }
  .data .row { gap: 8px; align-items: center; }
  .data :global(.icon) { color: var(--muted); flex: none; }
  .copies { list-style: none; margin: 4px 0 0; padding: 0; display: grid; gap: 4px; }
  .copies li { gap: 8px; align-items: center; }
  summary { cursor: pointer; }
  .pill.warn { background: var(--tint); color: var(--on-tint); }
  .empty { display: grid; justify-items: center; gap: 8px; text-align: center; padding: 22px; }
  .empty :global(.icon) { color: var(--muted); }
  .center { justify-content: center; gap: 8px; }
  .gap { margin-top: 14px; }
  h3 { font-size: 14px; font-weight: 600; margin: 14px 0 4px; }
  .link { background: none; border: 0; padding: 0; color: var(--accent); cursor: pointer; font: inherit; text-decoration: underline; }
</style>
