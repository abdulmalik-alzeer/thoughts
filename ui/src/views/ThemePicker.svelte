<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { app, applyTheme } from '../lib/store.svelte';
  import { t } from '../lib/i18n';

  let { theme = $bindable(), mode = $bindable() }: { theme: string; mode: string } = $props();

  const themes = $derived([
    { id: 'saudi', name: t('saudi'), sw: ['#006c35', '#faf7f0', '#d4af37'] },
    { id: 'contrast', name: t('contrast'), sw: ['#000000', '#ffffff', '#ffd400'] },
    { id: 'tech', name: t('tech'), sw: ['#ff2bd6', '#07040f', '#00f0ff'] },
    ...app.customThemes.map((c) => ({ id: c.id, name: c.name, sw: [] as string[] })),
  ]);

  function preview() {
    if (app.settings) { app.settings.theme = theme; app.settings.mode = mode; applyTheme(); }
  }
</script>

<div class="lbl">{t('theme')}</div>
<div class="themes">
  {#each themes as th}
    <button class="th" class:active={theme === th.id} onclick={() => { theme = th.id; preview(); }}>
      <span class="sw">{#each th.sw as c}<i style="background:{c}"></i>{/each}</span>{th.name}
    </button>
  {/each}
</div>
<div class="lbl">{t('mode')}</div>
<div class="row">
  {#each [['light', 'sun'], ['dark', 'moon'], ['system', 'system']] as [m, icon]}
    <button class="btn" class:primary={mode === m} onclick={() => { mode = m; preview(); }}><Icon name={icon} size={15} /> {t(m as any)}</button>
  {/each}
</div>

<style>
  .lbl { font-size: 12px; color: var(--muted); margin: 10px 0 4px; }
  .themes { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  .th { all: unset; cursor: pointer; border: 1px solid var(--hairline); background: var(--panel); border-radius: var(--radius-lg); padding: 10px; display: flex; flex-direction: column; gap: 6px; }
  .th.active { box-shadow: 0 0 0 2px var(--accent); }
  .sw { display: flex; gap: 3px; }
  .sw i { width: 22px; height: 14px; border-radius: 3px; border: 1px solid rgba(128,128,128,.4); }
</style>
