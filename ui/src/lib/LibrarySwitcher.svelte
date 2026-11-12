<script lang="ts">
  // The library switcher at the top of the sidebar (FEAT-32): which library is open, the others to
  // open, and adding, naming and taking libraries off the list. Adding reuses the setup screens.
  import { onMount, tick } from 'svelte';
  import Icon from './Icon.svelte';
  import { t, tn } from './i18n';
  import { app } from './store.svelte';
  import { libs, openLib, startLibraries, switchLibrary, renameLibrary, removeLibrary, startAdding, type Lib } from './libraries.svelte';

  let open = $state(false);
  let btn = $state<HTMLButtonElement | null>(null);
  let menu = $state<HTMLElement | null>(null);
  const here = $derived(openLib());
  const name = $derived(here?.displayName || app.status?.libraryName || '');

  onMount(() => { startLibraries(); });

  async function toggle() {
    open = !open;
    if (open) { await tick(); menu?.querySelector<HTMLElement>('button')?.focus(); }
  }
  function close(refocus = true) { open = false; if (refocus) btn?.focus(); }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); return; }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const items = [...(menu?.querySelectorAll<HTMLElement>('button:not([disabled])') ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    const next = items[(i + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length];
    if (next) { e.preventDefault(); next.focus(); }
  }
  function outside(e: MouseEvent) {
    if (!open) return;
    const n = e.target as Node;
    if (menu?.contains(n) || btn?.contains(n)) return;
    close(false);
  }
  const pick = (l: Lib) => { close(false); switchLibrary(l.id); };
  const add = () => { close(false); startAdding(); };
  const rename = () => { close(false); renameLibrary(); };
  const remove = (l: Lib) => { close(false); removeLibrary(l); };
</script>

<svelte:window onclick={outside} />

{#if name}
  <div class="libs">
    <button bind:this={btn} type="button" class="current" aria-haspopup="true" aria-expanded={open} aria-label={t('libs.button', { name })} title={here?.path ?? ''} onclick={toggle}>
      <Icon name="book" size={15} />
      <span class="grow ell" dir="auto">{name}</span>
      {#if libs.list.some((l) => !l.open && l.running > 0)}<span class="dot" aria-hidden="true"></span>{/if}
      <Icon name="down" size={14} />
    </button>
    {#if open}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="menu" bind:this={menu} role="group" aria-label={t('libs.menuLabel')} onkeydown={onKey}>
        <p class="head">{t('libs.menuLabel')}</p>
        {#each libs.list as l (l.id)}
          <div class="item" class:on={l.open}>
            <button type="button" class="pick" aria-current={l.open ? 'true' : undefined} disabled={libs.busy} onclick={() => (l.open ? close() : pick(l))}>
              <span class="mark" aria-hidden="true">{#if l.open}<Icon name="check" size={14} />{/if}</span>
              <span class="grow">
                <span class="blk ell" dir="auto">{l.displayName}</span>
                <span class="blk sm muted ell">
                  {#if l.open}{t('libs.openNow')}{:else if l.missing}{t('libs.missing')}{:else if l.running}{tn('libs.working', l.running)}{:else}<bdi dir="ltr">{l.path}</bdi>{/if}
                </span>
              </span>
              {#if !l.open && l.running}<span class="dot" aria-hidden="true"></span>{/if}
            </button>
            {#if !l.open}
              <button type="button" class="iconbtn rm" aria-label={t('libs.removeOne', { name: l.displayName })} title={t('libs.removeOne', { name: l.displayName })} onclick={() => remove(l)}><Icon name="close" size={13} /></button>
            {/if}
          </div>
        {/each}
        <div class="sep" role="separator"></div>
        <button type="button" class="act" onclick={add}><Icon name="plus" size={14} /> {t('libs.add')}</button>
        {#if here}<button type="button" class="act" onclick={rename}><Icon name="edit" size={14} /> {t('libs.rename')}</button>{/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .libs { position: relative; margin: 0 0 10px; }
  .current { width: 100%; display: flex; align-items: center; gap: 8px; font: inherit; font-size: 13px; font-weight: 600; color: var(--fg);
    background: transparent; border: var(--border-w, 1px) solid var(--hairline); border-radius: var(--radius); padding: 6px 8px; cursor: pointer; text-align: start; }
  .current:hover, .current[aria-expanded='true'] { background: color-mix(in srgb, var(--fg) 6%, transparent); }
  .current:focus-visible, .menu button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .grow { flex: 1; min-width: 0; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .blk { display: block; }
  .sm { font-size: var(--fs-xs, 12px); font-weight: 400; }
  .muted { color: var(--muted); }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); flex: none; }
  .menu { position: absolute; inset-inline: 0; top: calc(100% + 4px); z-index: 40; background: var(--panel); color: var(--fg);
    border: var(--border-w, 1px) solid var(--border, var(--hairline)); border-radius: var(--radius); box-shadow: 0 10px 30px rgba(0,0,0,.22); padding: 6px; max-height: 60vh; overflow-y: auto; }
  .head { margin: 2px 6px 6px; font-size: var(--fs-xs, 12px); color: var(--muted); }
  .item { display: flex; align-items: center; gap: 2px; border-radius: calc(var(--radius) - 2px); }
  .item.on { background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .pick, .act { flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; font: inherit; font-size: 13px; color: inherit; background: transparent;
    border: 0; border-radius: calc(var(--radius) - 2px); padding: 6px; cursor: pointer; text-align: start; }
  .pick:hover:not([disabled]), .act:hover { background: color-mix(in srgb, var(--fg) 7%, transparent); }
  .pick[disabled] { cursor: progress; opacity: .7; }
  .mark { width: 14px; flex: none; color: var(--accent); }
  .rm { flex: none; opacity: .7; }
  .rm:hover, .rm:focus-visible { opacity: 1; }
  .sep { height: 1px; background: var(--hairline); margin: 6px 2px; }
  .act { width: 100%; }
  @media (prefers-reduced-motion: no-preference) { .menu { animation: libs-in .12s ease-out; } }
  :global(html[data-motion='reduce']) .menu { animation: none; }
  @keyframes libs-in { from { opacity: 0; transform: translateY(-3px); } to { opacity: 1; transform: none; } }
</style>
