<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { app, applyTheme, saveSettings, toast } from '../lib/store.svelte';
  import { t } from '../lib/i18n';
  import { themeSwatches } from '../lib/themeSwatches';

  let { theme = $bindable(), mode = $bindable() }: { theme: string; mode: string } = $props();

  // Every card shows its theme's three highlight colours, read from the theme's own tokens for the
  // Day / Night being chosen, so themes added through Customise show theirs like the built-in ones.
  const dark = matchMedia('(prefers-color-scheme: dark)');
  let sysDark = $state(dark.matches);
  $effect(() => {
    const on = () => (sysDark = dark.matches);
    dark.addEventListener('change', on);
    return () => dark.removeEventListener('change', on);
  });
  const shown = $derived(mode === 'dark' || (mode === 'system' && sysDark) ? 'dark' : 'light');
  const themes = $derived.by(() => {
    const list = [
      { id: 'saudi', name: t('saudi') },
      { id: 'contrast', name: t('contrast') },
      { id: 'tech', name: t('tech') },
      ...app.customThemes.map((c) => ({ id: c.id, name: c.name })),
    ];
    const sw = themeSwatches(list.map((th) => th.id), shown);
    return list.map((th) => ({ ...th, sw: sw[th.id] ?? [] }));
  });

  // Choosing a look takes effect — and is saved — straight away, so what you see is never an
  // unsaved preview. (During first-run setup nothing is saved yet; setup saves it at the end.)
  let saving = 0;
  async function choose(next: { theme?: string; mode?: string }) {
    const before = { theme: app.settings?.theme, mode: app.settings?.mode };
    if (next.theme) theme = next.theme;
    if (next.mode) mode = next.mode;
    if (!app.settings) return;
    app.settings.theme = theme; app.settings.mode = mode;
    applyTheme();
    if (!app.settings.onboarded) return;
    const mine = ++saving;
    try { await saveSettings({ theme, mode }); }
    catch (e: any) {
      if (mine !== saving) return;
      theme = before.theme; mode = before.mode;
      app.settings.theme = before.theme; app.settings.mode = before.mode;
      applyTheme();
      toast(t('shell.themeNotSaved'), { kind: 'error', detail: e.message });
    }
  }
</script>

<div class="lbl" id="tp-theme">{t('theme')}</div>
<div class="themes" role="group" aria-labelledby="tp-theme">
  {#each themes as th (th.id)}
    <button type="button" class="th" class:active={theme === th.id} aria-pressed={theme === th.id} onclick={() => choose({ theme: th.id })}>
      <span class="sw" aria-hidden="true">{#each th.sw as c}<i style="background:{c}"></i>{/each}</span><span dir="auto">{th.name}</span>
    </button>
  {/each}
</div>
<div class="lbl" id="tp-mode">{t('mode')}</div>
<div class="row" role="group" aria-labelledby="tp-mode">
  {#each [['light', 'sun'], ['dark', 'moon'], ['system', 'system']] as [m, icon]}
    <button type="button" class="btn" class:primary={mode === m} aria-pressed={mode === m} onclick={() => choose({ mode: m })}><Icon name={icon} size={15} /> {t(m as any)}</button>
  {/each}
</div>

<style>
  .lbl { font-size: var(--fs-xs); color: var(--muted); margin: 10px 0 4px; }
  .themes { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  .th { font: inherit; color: inherit; text-align: start; cursor: pointer; border: 1px solid var(--hairline); background: var(--panel); border-radius: var(--radius-lg); padding: 10px; display: flex; flex-direction: column; gap: 6px; }
  .th.active { box-shadow: 0 0 0 2px var(--accent); }
  .sw { display: flex; gap: 3px; }
  .sw i { width: 22px; height: 14px; border-radius: 3px; border: 1px solid rgba(128,128,128,.4); }
</style>
