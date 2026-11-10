<script lang="ts" module>
  export type PaletteItem = {
    id: string;
    title: string;
    /** Section label shown on the trailing side (Go to, Chats, Ideas…). */
    group?: string;
    icon?: string;
    /** Shortcut text, already formatted. */
    keys?: string;
    /** Extra words to match on. */
    keywords?: string;
    run: () => void;
  };

  /** Fuzzy match: 0 = no match; higher is better. Substring beats scattered letters. */
  export function fuzzy(query: string, text: string): number {
    const q = query.trim().toLowerCase();
    const s = text.toLowerCase();
    if (!q) return 1;
    const at = s.indexOf(q);
    if (at >= 0) return 1000 - Math.min(at, 500) + (at === 0 || /\s/.test(s[at - 1]) ? 200 : 0);
    let from = 0, score = 0, last = -2;
    for (const ch of q) {
      if (ch === ' ') continue;
      const j = s.indexOf(ch, from);
      if (j < 0) return 0;
      score += j === last + 1 ? 6 : 1;
      last = j;
      from = j + 1;
    }
    return score;
  }
</script>

<script lang="ts">
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';
  import { t } from './i18n';
  import { trapFocus } from './focus';

  let { items, close }: { items: PaletteItem[]; close: () => void } = $props();

  let q = $state('');
  let active = $state(0);
  let listEl = $state<HTMLElement | null>(null);

  const shown = $derived.by(() => {
    if (!q.trim()) return items.slice(0, 60);
    return items
      .map((it, i) => ({ it, i, s: Math.max(fuzzy(q, it.title), fuzzy(q, it.keywords ?? '') * 0.6, fuzzy(q, it.group ?? '') * 0.3) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s || a.i - b.i)
      .slice(0, 60)
      .map((x) => x.it);
  });

  $effect(() => { q; active = 0; });

  async function move(d: number) {
    if (!shown.length) return;
    active = (active + d + shown.length) % shown.length;
    await tick();
    listEl?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  }

  function run(it: PaletteItem | undefined) {
    if (!it) return;
    close();
    it.run();
  }

  function onKey(e: KeyboardEvent) {
    if (e.isComposing) return;
    if (e.key === 'ArrowDown') { e.preventDefault(); move(1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); move(-1); }
    else if (e.key === 'Home' && !q) { e.preventDefault(); active = 0; }
    else if (e.key === 'Enter') { e.preventDefault(); run(shown[active]); }
  }
</script>

<div class="modal-bg pal-bg no-print" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
  <div class="modal pal" role="dialog" aria-modal="true" aria-label={t('shell.palette')} tabindex="-1" use:trapFocus={{ onEscape: close }}>
    <div class="field">
      <Icon name="search" size={16} />
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={q} onkeydown={onKey} autofocus dir="auto" placeholder={t('shell.palettePlaceholder')}
        role="combobox" aria-expanded="true" aria-controls="pal-list" aria-autocomplete="list"
        aria-activedescendant={shown.length ? 'pal-opt-' + active : undefined} aria-label={t('shell.palette')}
        autocomplete="off" spellcheck="false" />
    </div>
    <div class="list scroll" id="pal-list" role="listbox" aria-label={t('shell.palette')} bind:this={listEl}>
      {#each shown as it, i (it.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="opt" id={'pal-opt-' + i} role="option" tabindex="-1" aria-selected={i === active}
          onclick={() => run(it)} onmousemove={() => (active = i)}>
          <Icon name={it.icon ?? 'right'} size={15} class={it.icon ? '' : 'flip-rtl'} />
          <span class="grow ell" dir="auto">{it.title}</span>
          {#if it.group}<span class="grp">{it.group}</span>{/if}
          {#if it.keys}<kbd dir="ltr">{it.keys}</kbd>{/if}
        </div>
      {:else}
        <p class="muted empty">{t('shell.paletteEmpty')}</p>
      {/each}
    </div>
    <div class="hint muted" aria-hidden="true">
      <span><kbd dir="ltr">↑</kbd> <kbd dir="ltr">↓</kbd> {t('shell.paletteMove')}</span>
      <span><kbd dir="ltr">↵</kbd> {t('shell.paletteRun')}</span>
      <span><kbd dir="ltr">Esc</kbd> {t('close')}</span>
    </div>
  </div>
</div>

<style>
  .pal-bg { place-items: start center; padding-top: 14vh; z-index: 70; }
  .pal { width: min(600px, 92vw); padding: 0; display: flex; flex-direction: column; max-height: 62vh; overflow: hidden; }
  .field { display: flex; align-items: center; gap: 10px; padding: 12px 16px; border-bottom: 1px solid var(--hairline); color: var(--muted); }
  .field input { background: transparent; border: 0; padding: 2px 0; font-size: 16px; box-shadow: none; outline-offset: 4px; }
  .list { flex: 1; min-height: 0; padding: 6px; }
  .opt { display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-radius: 8px; cursor: pointer; font-size: 14px; }
  .opt :global(.icon) { flex: none; color: var(--muted); }
  .opt[aria-selected="true"] { background: var(--tint); }
  :global([data-theme="contrast"]) .opt[aria-selected="true"] { background: var(--accent); color: var(--accent-fg); }
  :global([data-theme="contrast"]) .opt[aria-selected="true"] :global(.icon),
  :global([data-theme="contrast"]) .opt[aria-selected="true"] .grp { color: inherit; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .grp { font-size: 12px; color: var(--muted); flex: none; }
  .empty { padding: 18px 10px; margin: 0; text-align: center; }
  .hint { display: flex; gap: 16px; padding: 8px 16px; border-top: 1px solid var(--hairline); font-size: 12px; }
</style>
